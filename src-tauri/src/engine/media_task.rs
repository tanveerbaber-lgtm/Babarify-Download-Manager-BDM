//! Runs one media download through yt-dlp.
//!
//! yt-dlp works inside a private temp folder in the destination; when it
//! finishes, the result is moved into place with the same never-overwrite
//! finalization as file downloads. Pausing kills the process tree and keeps
//! the folder so yt-dlp can continue its partial files on resume.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;

use super::error::{EngineError, EngineResult};
use super::events::{Progress, SpeedMeter, eta_seconds};
use super::files;
use super::task::TaskContext;
use crate::db::downloads;
use crate::error::AppError;
use crate::media::output::{Line, friendly_error, parse_line, stage_label};
use crate::media::{args, process};
use crate::models::DownloadStatus;

const EMIT_INTERVAL: Duration = Duration::from_millis(300);
const PERSIST_INTERVAL: Duration = Duration::from_secs(2);
const STDERR_LINES_KEPT: usize = 40;

pub(super) fn temp_dir_for(dest_dir: &Path, id: &str) -> PathBuf {
    let short: String = id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    dest_dir.join(format!(".bdm-media-{short}"))
}

pub(super) async fn run(ctx: &TaskContext, id: &str) -> EngineResult<()> {
    let result = run_inner(ctx, id).await;
    if let Err(e) = &result
        && !matches!(e, EngineError::Stopped)
    {
        // Don't leave an empty hidden folder behind after a failure; a
        // folder with partial data is kept so a retry can continue it.
        // Read the path first: the lock must be released before locking again.
        let temp = downloads::require(&*ctx.db.lock()?, id).ok().and_then(|d| d.temp_path);
        if let Some(dir) = temp.map(PathBuf::from)
            && std::fs::read_dir(&dir).is_ok_and(|mut e| e.next().is_none())
        {
            let _ = std::fs::remove_dir(&dir);
            let conn = ctx.db.lock()?;
            let _ = conn.execute("UPDATE downloads SET temp_path = NULL WHERE id = ?1", [id]);
        }
    }
    result
}

/// Which pipe a line came from.
enum Pipe {
    Out,
    Err,
}

/// Forwards each line of `reader` to `tx`.
fn forward_lines<R: AsyncRead + Unpin + Send + 'static>(reader: R, pipe: fn() -> Pipe, tx: mpsc::UnboundedSender<(Pipe, String)>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send((pipe(), line)).is_err() {
                break;
            }
        }
    });
}

async fn run_inner(ctx: &TaskContext, id: &str) -> EngineResult<()> {
    let download = {
        let conn = ctx.db.lock()?;
        downloads::set_status(&conn, id, DownloadStatus::Downloading, None)?;
        downloads::require(&conn, id)?
    };
    ctx.sink.updated(&download);

    let options = download
        .media_options
        .clone()
        .ok_or_else(|| AppError::Internal("media download without options".into()))?;
    args::validate(&options)?;
    let tools = ctx.tools.require()?;
    // yt-dlp makes its own connections, so the address is checked up front.
    if let Ok(url) = url::Url::parse(&download.url) {
        ctx.guard.check_url(&url).await.map_err(|b| EngineError::Blocked(b.to_string()))?;
    }
    let dest_dir = PathBuf::from(&download.destination_dir);
    if !dest_dir.is_dir() {
        return Err(AppError::Validation("The destination folder no longer exists.".into()).into());
    }

    let temp_dir = match &download.temp_path {
        Some(p) => PathBuf::from(p),
        None => {
            let p = temp_dir_for(&dest_dir, id);
            downloads::set_temp_path(&*ctx.db.lock()?, id, &p.to_string_lossy())?;
            p
        }
    };
    std::fs::create_dir_all(&temp_dir)?;
    hide(&temp_dir);

    let mut argv = args::download_args(
        &tools,
        &options,
        &temp_dir,
        ctx.segments_per_download,
        download.referrer.as_deref(),
        &ctx.headers.for_url(&download.url),
        &download.url,
    );
    // Limits go before the "--" that ends the options.
    let at = argv.len() - 2;
    argv.splice(at..at, args::limit_args(ctx.limits.effective(), ctx.max_retries));
    // Deleted when this function returns, after yt-dlp has exited.
    let cookies = ctx.cookies.file_for(&download.url)?;
    if let Some(file) = &cookies {
        let at = argv.len() - 2;
        argv.splice(at..at, ["--cookies".to_string(), file.path().to_string_lossy().into_owned()]);
    }
    let mut cmd = process::command(&tools.yt_dlp);
    cmd.args(&argv);
    let mut child = cmd.spawn().map_err(|e| AppError::Io(format!("couldn't start yt-dlp: {e}")))?;
    let pid = child.id().unwrap_or(0);
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    // yt-dlp prints download progress on stdout but post-processing
    // progress and errors on stderr, so both are read and parsed.
    let (tx, mut rx) = mpsc::unbounded_channel();
    forward_lines(stdout, || Pipe::Out, tx.clone());
    forward_lines(stderr, || Pipe::Err, tx);
    let mut stderr_tail: Vec<String> = Vec::new();

    let mut state = MediaProgress::new(ctx.cfg.speed_window);
    let mut final_file: Option<PathBuf> = None;
    let mut last_emit = Instant::now() - EMIT_INTERVAL;
    let mut last_persist = Instant::now();

    loop {
        let next = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                process::kill_tree(pid).await;
                let _ = child.wait().await;
                persist(ctx, id, &state);
                emit(ctx, id, &mut state);
                return Err(EngineError::Stopped);
            }
            l = rx.recv() => l,
        };
        let Some((pipe, line)) = next else { break };
        if matches!(pipe, Pipe::Err) && !line.starts_with("BDMPP|") {
            if stderr_tail.len() == STDERR_LINES_KEPT {
                stderr_tail.remove(0);
            }
            stderr_tail.push(line.clone());
        }
        match parse_line(&line) {
            Line::Info { format_id, expected_size, title } => {
                state.set_streams(&format_id);
                state.expected = expected_size;
                if !title.is_empty() {
                    let conn = ctx.db.lock()?;
                    conn.execute("UPDATE downloads SET title = ?2 WHERE id = ?1", rusqlite::params![id, title])
                        .map_err(AppError::from)?;
                }
            }
            Line::Progress { finished, downloaded, total, format_id, .. } => {
                state.update(&format_id, downloaded, total, finished);
            }
            Line::PostProcess { started: true, name } => {
                state.stage = Some(stage_label(&name));
                last_emit = Instant::now() - EMIT_INTERVAL;
            }
            Line::File(path) => final_file = Some(PathBuf::from(path)),
            _ => {}
        }
        if last_emit.elapsed() >= EMIT_INTERVAL {
            emit(ctx, id, &mut state);
            last_emit = Instant::now();
        }
        if last_persist.elapsed() >= PERSIST_INTERVAL {
            persist(ctx, id, &state);
            last_persist = Instant::now();
        }
    }

    let status = child.wait().await?;
    persist(ctx, id, &state);
    if ctx.cancel.is_cancelled() {
        return Err(EngineError::Stopped);
    }
    if !status.success() {
        return Err(EngineError::Media(friendly_error(&stderr_tail)));
    }

    let produced = match final_file.filter(|p| p.is_file()) {
        Some(p) => p,
        None => newest_file(&temp_dir).ok_or_else(|| EngineError::Media("yt-dlp finished but produced no file.".into()))?,
    };
    // Only accept output from our own temp folder.
    if produced.parent() != Some(temp_dir.as_path()) {
        return Err(EngineError::Media("yt-dlp wrote its output to an unexpected place.".into()));
    }
    let name = produced.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| download.file_name.clone());
    let (src, dir, final_name) = (produced.clone(), dest_dir.clone(), name.clone());
    let saved = tokio::task::spawn_blocking(move || files::finalize(&src, &dir, &final_name))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    files::mark_from_internet(&saved, &download.url, download.referrer.as_deref());
    let size = std::fs::metadata(&saved)?.len();
    let _ = std::fs::remove_dir_all(&temp_dir);

    let updated = {
        let conn = ctx.db.lock()?;
        let saved_name = saved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
        downloads::set_file_name(&conn, id, &saved_name)?;
        downloads::mark_completed(&conn, id, &saved.to_string_lossy(), size)?;
        downloads::require(&conn, id)?
    };
    ctx.sink.updated(&updated);
    Ok(())
}

/// Tracks byte progress across the streams yt-dlp downloads one after another.
struct MediaProgress {
    streams: Vec<String>,
    per_stream: HashMap<String, (u64, Option<u64>)>,
    stage: Option<String>,
    meter: SpeedMeter,
    /// yt-dlp's (approximate) size of all streams, known before they start.
    expected: Option<u64>,
}

impl MediaProgress {
    fn new(window: Duration) -> Self {
        Self {
            streams: Vec::new(),
            per_stream: HashMap::new(),
            stage: None,
            meter: SpeedMeter::new(window),
            expected: None,
        }
    }

    /// "299+140" → two streams.
    fn set_streams(&mut self, format_id: &str) {
        self.streams = format_id.split('+').map(str::to_owned).collect();
    }

    fn update(&mut self, format_id: &str, downloaded: Option<u64>, total: Option<u64>, finished: bool) {
        let entry = self.per_stream.entry(format_id.to_string()).or_insert((0, None));
        if let Some(d) = downloaded {
            entry.0 = d;
        }
        if total.is_some() {
            entry.1 = total;
        }
        if finished && let Some(t) = entry.1 {
            entry.0 = t;
        }
        let count = self.streams.len().max(self.per_stream.len());
        self.stage = if count > 1 {
            let index = self.streams.iter().position(|s| s == format_id).map_or(self.per_stream.len(), |i| i + 1);
            Some(format!("Downloading stream {index} of {count}"))
        } else {
            None
        };
    }

    fn downloaded(&self) -> u64 {
        self.per_stream.values().map(|(d, _)| *d).sum()
    }

    /// Total of all streams: exact once every stream's size is known, until
    /// then yt-dlp's estimate (never less than what has already arrived).
    fn total(&self) -> Option<u64> {
        let streams = self.streams.len().max(1);
        let exact = if self.per_stream.len() >= streams { self.per_stream.values().map(|(_, t)| *t).sum() } else { None };
        exact.or_else(|| self.expected.map(|e| e.max(self.downloaded())))
    }
}

fn emit(ctx: &TaskContext, id: &str, state: &mut MediaProgress) {
    let done = state.downloaded();
    let total = state.total();
    let speed = state.meter.sample(Instant::now(), done);
    let processing = state.stage.as_deref().is_some_and(|s| !s.starts_with("Downloading"));
    ctx.sink.progress(&Progress {
        id: id.to_string(),
        downloaded_bytes: done,
        total_bytes: total,
        speed: if processing { 0.0 } else { speed },
        eta_seconds: if processing { None } else { eta_seconds(done, total, speed) },
        connections: 1,
        stage: state.stage.clone(),
        segments: Vec::new(),
    });
}

fn persist(ctx: &TaskContext, id: &str, state: &MediaProgress) {
    if let Ok(conn) = ctx.db.lock() {
        let _ = downloads::set_media_progress(&conn, id, state.downloaded(), state.total());
    }
}

/// Most recently modified finished file in `dir` (ignores yt-dlp's partials).
fn newest_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            !n.ends_with(".part") && !n.ends_with(".ytdl") && !n.contains(".part-frag") && !n.ends_with(".temp")
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// Marks the temp folder hidden so it doesn't clutter the destination.
fn hide(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        unsafe extern "system" {
            fn SetFileAttributesW(name: *const u16, attrs: u32) -> i32;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a valid NUL-terminated UTF-16 path that outlives the call.
        unsafe {
            SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN);
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_progress_of_two_streams() {
        let mut p = MediaProgress::new(Duration::from_secs(5));
        p.set_streams("299+140");
        p.update("299", Some(50), Some(100), false);
        assert_eq!(p.stage.as_deref(), Some("Downloading stream 1 of 2"));
        assert_eq!(p.total(), None, "audio size not known yet");
        p.update("299", Some(100), Some(100), true);
        p.update("140", Some(10), Some(40), false);
        assert_eq!(p.stage.as_deref(), Some("Downloading stream 2 of 2"));
        assert_eq!(p.downloaded(), 110);
        assert_eq!(p.total(), Some(140));
    }

    #[test]
    fn estimate_fills_in_until_every_stream_is_known() {
        let mut p = MediaProgress::new(Duration::from_secs(5));
        p.set_streams("299+140");
        p.expected = Some(1000);
        p.update("299", Some(100), Some(900), false);
        assert_eq!(p.total(), Some(1000), "audio size unknown: use yt-dlp's estimate");
        p.update("299", Some(900), Some(900), true);
        p.update("140", Some(10), Some(120), false);
        assert_eq!(p.total(), Some(1020), "both known: exact sum wins");
        let mut q = MediaProgress::new(Duration::from_secs(5));
        q.set_streams("299+140");
        q.expected = Some(50);
        q.update("299", Some(80), None, false);
        assert_eq!(q.total(), Some(80), "an estimate never shows less than what arrived");
    }

    #[test]
    fn single_stream_has_no_stage_label() {
        let mut p = MediaProgress::new(Duration::from_secs(5));
        p.set_streams("18");
        p.update("18", Some(5), Some(10), false);
        assert_eq!(p.stage, None);
        assert_eq!(p.total(), Some(10));
    }

    #[test]
    fn newest_file_skips_partials() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mp4.part"), b"x").unwrap();
        std::fs::write(dir.path().join("a.f140.m4a.ytdl"), b"x").unwrap();
        std::fs::write(dir.path().join("a.mp4"), b"x").unwrap();
        assert_eq!(newest_file(dir.path()).unwrap().file_name().unwrap(), "a.mp4");
    }
}
