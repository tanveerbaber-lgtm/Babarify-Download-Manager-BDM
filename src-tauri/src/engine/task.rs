//! Runs one download from probe to finalized file.

use std::future::Future;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, IF_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::EngineConfig;
use super::error::{EngineError, EngineResult};
use super::events::{EventSink, Progress, SpeedMeter, eta_seconds};
use super::files;
use super::http::{self, RemoteFile};
use crate::db::{Database, downloads};
use crate::error::AppError;
use crate::media::tools::ToolManager;
use crate::models::{Download, DownloadKind, DownloadStatus, Segment};

/// Bytes are only counted as downloaded once they have been handed to the
/// OS, so persisted progress never runs ahead of the file contents.
const WRITE_BUFFER: usize = 128 * 1024;
/// Slow connections still flush at least this often so progress stays live.
const MAX_FLUSH_INTERVAL: Duration = Duration::from_millis(200);

pub(super) struct TaskContext {
    pub db: Arc<Database>,
    pub client: Client,
    pub sink: Arc<dyn EventSink>,
    pub cfg: Arc<EngineConfig>,
    pub segments_per_download: u32,
    pub cancel: CancellationToken,
    pub tools: Arc<ToolManager>,
}

/// Downloads `id` to completion. Returns `Err(Stopped)` when cancelled.
pub(super) async fn run(ctx: &TaskContext, id: &str) -> EngineResult<()> {
    if downloads::require(&*ctx.db.lock()?, id)?.kind == DownloadKind::Media {
        return super::media_task::run(ctx, id).await;
    }
    let mut restarted = false;
    loop {
        match attempt(ctx, id).await {
            // The remote file changed under us: start over once from byte zero.
            Err(EngineError::Changed) if !restarted => {
                restarted = true;
                discard_progress(&ctx.db, id)?;
            }
            other => return other,
        }
    }
}

async fn attempt(ctx: &TaskContext, id: &str) -> EngineResult<()> {
    let download = {
        let conn = ctx.db.lock()?;
        downloads::set_status(&conn, id, DownloadStatus::Downloading, None)?;
        downloads::require(&conn, id)?
    };
    ctx.sink.updated(&download);

    let dest_dir = PathBuf::from(&download.destination_dir);
    if !dest_dir.is_dir() {
        return Err(AppError::Validation("The destination folder no longer exists.".into()).into());
    }

    let remote = with_retries(ctx, || http::probe(&ctx.client, &download.url, download.referrer.as_deref())).await?;

    let temp_exists = download.temp_path.as_deref().is_some_and(|p| Path::new(p).exists());
    let has_progress = download.downloaded_bytes > 0 || temp_exists;
    let download = if has_progress && !same_remote(&download, &remote) {
        discard_progress(&ctx.db, id)?;
        downloads::require(&*ctx.db.lock()?, id)?
    } else {
        download
    };

    let temp = {
        let conn = ctx.db.lock()?;
        downloads::set_remote_info(
            &conn,
            id,
            &downloads::RemoteInfo {
                final_url: &remote.final_url,
                total_bytes: remote.total_bytes,
                resumable: remote.resumable,
                etag: remote.etag.as_deref(),
                last_modified: remote.last_modified.as_deref(),
            },
        )?;
        match &download.temp_path {
            Some(p) => PathBuf::from(p),
            None => {
                let p = files::temp_path_for(&dest_dir, &download.file_name, id);
                downloads::set_temp_path(&conn, id, &p.to_string_lossy())?;
                p
            }
        }
    };

    let size = match (remote.resumable, remote.total_bytes) {
        (true, Some(total)) if total > 0 => segmented(ctx, id, &remote, &temp, total).await?,
        _ => single(ctx, id, &download, &remote, &temp).await?,
    };
    finish(ctx, &download, &temp, &dest_dir, size).await
}

/// Resuming is only safe if the server still describes the same file.
fn same_remote(download: &Download, remote: &RemoteFile) -> bool {
    remote.resumable
        && remote.total_bytes.is_some()
        && download.total_bytes.map(|n| n as u64) == remote.total_bytes
        && download.etag == remote.etag
        && download.last_modified == remote.last_modified
}

fn discard_progress(db: &Database, id: &str) -> EngineResult<()> {
    let conn = db.lock()?;
    if let Some(temp) = downloads::require(&conn, id)?.temp_path {
        files::remove_if_exists(Path::new(&temp))?;
    }
    downloads::reset_progress(&conn, id)?;
    Ok(())
}

// ---------------------------------------------------------------- segmented

/// Splits `total` bytes into at most `wanted` segments of at least `min_size`.
pub(super) fn plan_segments(total: u64, wanted: u32, min_size: u64) -> Vec<Segment> {
    let by_size = (total / min_size.max(1)).max(1);
    let count = u64::from(wanted.max(1)).min(by_size);
    let base = total / count;
    let extra = total % count;
    let mut start = 0;
    (0..count)
        .map(|i| {
            let len = base + u64::from(i < extra);
            let seg = Segment { idx: i as u32, start, end: start + len - 1, downloaded: 0 };
            start += len;
            seg
        })
        .collect()
}

async fn segmented(
    ctx: &TaskContext,
    id: &str,
    remote: &RemoteFile,
    temp: &Path,
    total: u64,
) -> EngineResult<u64> {
    let mut segs = downloads::segments(&*ctx.db.lock()?, id)?;
    let file_ok = std::fs::metadata(temp).is_ok_and(|m| m.len() == total);
    let layout_ok = file_ok
        && segs.first().is_some_and(|s| s.start == 0)
        && segs.last().is_some_and(|s| s.end + 1 == total);
    if !layout_ok {
        segs = plan_segments(total, ctx.segments_per_download, ctx.cfg.min_segment_size);
        let file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(temp)?;
        file.set_len(total)?;
        downloads::replace_segments(&*ctx.db.lock()?, id, &segs)?;
    }

    let source = Arc::new(ProgressSource {
        counters: segs.iter().map(|s| AtomicU64::new(s.downloaded.min(s.len()))).collect(),
        segment_idx: segs.iter().map(|s| s.idx).collect(),
    });
    let connections = segs.iter().filter(|s| s.remaining() > 0).count() as u32;
    let reporter = Reporter::spawn(ctx, id, source.clone(), Some(total), connections);

    let stop = ctx.cancel.child_token();
    let workers = segs.iter().enumerate().filter(|(_, s)| s.remaining() > 0).map(|(i, seg)| {
        let source = source.clone();
        let stop = stop.clone();
        async move {
            let result = segment_worker(ctx, remote, temp, *seg, &source.counters[i], &stop).await;
            if result.is_err() {
                // One fatal failure stops the other connections.
                stop.cancel();
            }
            result
        }
    });
    let results = futures_util::future::join_all(workers).await;

    reporter.stop().await;
    persist(&ctx.db, id, &source)?;

    if let Some(err) = results.into_iter().filter_map(Result::err).find(|e| !matches!(e, EngineError::Stopped)) {
        return Err(err);
    }
    if ctx.cancel.is_cancelled() {
        return Err(EngineError::Stopped);
    }
    let done = source.total();
    if done != total {
        return Err(EngineError::SizeMismatch { expected: total, actual: done });
    }
    Ok(total)
}

async fn segment_worker(
    ctx: &TaskContext,
    remote: &RemoteFile,
    temp: &Path,
    seg: Segment,
    counter: &AtomicU64,
    stop: &CancellationToken,
) -> EngineResult<()> {
    let mut failures = 0;
    loop {
        let before = counter.load(Ordering::Acquire);
        if before >= seg.len() {
            return Ok(());
        }
        let from = seg.start + before;
        match fetch_range(&ctx.client, remote, temp, from, seg.end, counter, stop).await {
            Ok(()) => failures = 0,
            Err(e) if e.is_retryable() && failures < ctx.cfg.max_retries => {
                if counter.load(Ordering::Acquire) > before {
                    failures = 0;
                }
                failures += 1;
                backoff(ctx, stop, failures, e.retry_after()).await?;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn fetch_range(
    client: &Client,
    remote: &RemoteFile,
    temp: &Path,
    from: u64,
    end: u64,
    counter: &AtomicU64,
    stop: &CancellationToken,
) -> EngineResult<()> {
    let mut req = remote.get(client).header(RANGE, format!("bytes={from}-{end}"));
    if let Some(validator) = remote.if_range() {
        req = req.header(IF_RANGE, validator);
    }
    let resp = send(req, stop).await?;
    match resp.status() {
        StatusCode::PARTIAL_CONTENT => {
            let range = http::header_str(resp.headers(), CONTENT_RANGE).and_then(|v| http::parse_content_range(&v));
            if range.map(|r| r.start) != Some(from) {
                return Err(EngineError::Changed);
            }
        }
        // A full response to a ranged request means the If-Range validator
        // failed (the file changed) or ranges were switched off.
        StatusCode::OK => return Err(EngineError::Changed),
        _ => http::ensure_success(&resp)?,
    }

    let mut file = tokio::fs::OpenOptions::new().write(true).open(temp).await?;
    file.seek(SeekFrom::Start(from)).await?;
    let wanted = end - from + 1;
    let written = copy_body(resp, &mut file, counter, Some(wanted), stop).await?;
    if written < wanted {
        return Err(EngineError::premature_end());
    }
    Ok(())
}

// ------------------------------------------------------------ single stream

async fn single(
    ctx: &TaskContext,
    id: &str,
    download: &Download,
    remote: &RemoteFile,
    temp: &Path,
) -> EngineResult<u64> {
    let source = Arc::new(ProgressSource { counters: vec![AtomicU64::new(0)], segment_idx: Vec::new() });
    let reporter = Reporter::spawn(ctx, id, source.clone(), remote.total_bytes, 1);
    let result = single_loop(ctx, download, remote, temp, &source.counters[0]).await;
    reporter.stop().await;
    persist(&ctx.db, id, &source)?;
    result?;
    Ok(source.total())
}

async fn single_loop(
    ctx: &TaskContext,
    download: &Download,
    remote: &RemoteFile,
    temp: &Path,
    counter: &AtomicU64,
) -> EngineResult<()> {
    let on_disk = std::fs::metadata(temp).map(|m| m.len()).unwrap_or(0);
    let saved = download.downloaded_bytes.max(0) as u64;
    let mut from = if remote.resumable && saved > 0 && on_disk >= saved { saved } else { 0 };
    let mut failures = 0;

    loop {
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(false).open(temp).await?;
        file.set_len(from).await?;
        file.seek(SeekFrom::Start(from)).await?;
        counter.store(from, Ordering::Release);

        let result: EngineResult<()> = async {
            let mut req = remote.get(&ctx.client);
            if from > 0 {
                req = req.header(RANGE, format!("bytes={from}-"));
                if let Some(validator) = remote.if_range() {
                    req = req.header(IF_RANGE, validator);
                }
            }
            let resp = send(req, &ctx.cancel).await?;
            match resp.status() {
                StatusCode::PARTIAL_CONTENT if from > 0 => {
                    let range = http::header_str(resp.headers(), CONTENT_RANGE).and_then(|v| http::parse_content_range(&v));
                    if range.map(|r| r.start) != Some(from) {
                        return Err(EngineError::Changed);
                    }
                }
                StatusCode::OK if from > 0 => return Err(EngineError::Changed),
                _ => http::ensure_success(&resp)?,
            }
            let limit = remote.total_bytes.map(|t| t.saturating_sub(from));
            copy_body(resp, &mut file, counter, limit, &ctx.cancel).await?;
            if let Some(total) = remote.total_bytes
                && counter.load(Ordering::Acquire) < total
            {
                return Err(EngineError::premature_end());
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable() && failures < ctx.cfg.max_retries => {
                let now = counter.load(Ordering::Acquire);
                if now > from {
                    failures = 0;
                }
                failures += 1;
                // Without range support the only option is to start again.
                from = if remote.resumable { now } else { 0 };
                backoff(ctx, &ctx.cancel, failures, e.retry_after()).await?;
            }
            Err(e) => return Err(e),
        }
    }
}

// ------------------------------------------------------------------ shared

async fn send(req: reqwest::RequestBuilder, stop: &CancellationToken) -> EngineResult<Response> {
    tokio::select! {
        _ = stop.cancelled() => Err(EngineError::Stopped),
        r = req.send() => r.map_err(EngineError::from_reqwest),
    }
}

/// Streams a response body into `file`, adding written bytes to `counter`.
/// Stops after `limit` bytes if given. Returns the number of bytes written.
async fn copy_body(
    resp: Response,
    file: &mut tokio::fs::File,
    counter: &AtomicU64,
    limit: Option<u64>,
    stop: &CancellationToken,
) -> EngineResult<u64> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUFFER);
    let mut written = 0u64;
    let mut last_flush = Instant::now();

    loop {
        if limit.is_some_and(|l| written + buf.len() as u64 >= l) {
            break;
        }
        let next = tokio::select! {
            _ = stop.cancelled() => return Err(EngineError::Stopped),
            n = stream.next() => n,
        };
        let Some(chunk) = next else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(_) => {
                // Keep what already arrived so the retry continues from there.
                flush(file, &mut buf, counter, &mut written).await?;
                // Any failure mid-body is a transport problem worth retrying,
                // whatever category reqwest assigns it.
                return Err(EngineError::Network {
                    message: "the connection was interrupted".into(),
                    retryable: true,
                });
            }
        };
        let take = match limit {
            Some(l) => chunk.len().min((l - written - buf.len() as u64) as usize),
            None => chunk.len(),
        };
        buf.extend_from_slice(&chunk[..take]);
        if buf.len() >= WRITE_BUFFER || last_flush.elapsed() >= MAX_FLUSH_INTERVAL {
            flush(file, &mut buf, counter, &mut written).await?;
            last_flush = Instant::now();
        }
    }
    flush(file, &mut buf, counter, &mut written).await?;
    Ok(written)
}

async fn flush(
    file: &mut tokio::fs::File,
    buf: &mut Vec<u8>,
    counter: &AtomicU64,
    written: &mut u64,
) -> EngineResult<()> {
    if buf.is_empty() {
        return Ok(());
    }
    file.write_all(buf).await?;
    file.flush().await?;
    counter.fetch_add(buf.len() as u64, Ordering::AcqRel);
    *written += buf.len() as u64;
    buf.clear();
    Ok(())
}

async fn with_retries<T, F, Fut>(ctx: &TaskContext, mut op: F) -> EngineResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = EngineResult<T>>,
{
    let mut failures = 0;
    loop {
        let result = tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(EngineError::Stopped),
            r = op() => r,
        };
        match result {
            Err(e) if e.is_retryable() && failures < ctx.cfg.max_retries => {
                failures += 1;
                backoff(ctx, &ctx.cancel, failures, e.retry_after()).await?;
            }
            other => return other,
        }
    }
}

/// Exponential backoff (or the server's `Retry-After`), interrupted by `stop`.
async fn backoff(
    ctx: &TaskContext,
    stop: &CancellationToken,
    failures: u32,
    hint: Option<std::time::Duration>,
) -> EngineResult<()> {
    let exp = ctx.cfg.retry_base_delay.saturating_mul(1 << failures.saturating_sub(1).min(10));
    let delay = hint.unwrap_or(exp).min(ctx.cfg.max_retry_delay);
    tokio::select! {
        _ = stop.cancelled() => Err(EngineError::Stopped),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}

async fn finish(
    ctx: &TaskContext,
    download: &Download,
    temp: &Path,
    dest_dir: &Path,
    size: u64,
) -> EngineResult<()> {
    let actual = tokio::fs::metadata(temp).await?.len();
    if actual != size {
        return Err(EngineError::SizeMismatch { expected: size, actual });
    }

    if let Some(expected) = &download.expected_sha256 {
        let path = temp.to_path_buf();
        let hash = tokio::task::spawn_blocking(move || files::sha256_file(&path))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??;
        if !hash.eq_ignore_ascii_case(expected) {
            discard_progress(&ctx.db, &download.id)?;
            return Err(EngineError::Checksum { expected: expected.clone(), actual: hash });
        }
    }

    let (temp, dir, name) = (temp.to_path_buf(), dest_dir.to_path_buf(), download.file_name.clone());
    let saved = tokio::task::spawn_blocking(move || files::finalize(&temp, &dir, &name))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;

    let updated = {
        let conn = ctx.db.lock()?;
        downloads::mark_completed(&conn, &download.id, &saved.to_string_lossy(), size)?;
        downloads::require(&conn, &download.id)?
    };
    ctx.sink.updated(&updated);
    Ok(())
}

// ---------------------------------------------------------------- progress

struct ProgressSource {
    counters: Vec<AtomicU64>,
    /// Segment index per counter; empty for single-stream downloads.
    segment_idx: Vec<u32>,
}

impl ProgressSource {
    fn total(&self) -> u64 {
        self.counters.iter().map(|c| c.load(Ordering::Acquire)).sum()
    }

    fn segment_progress(&self) -> Vec<(u32, u64)> {
        self.segment_idx
            .iter()
            .zip(&self.counters)
            .map(|(idx, c)| (*idx, c.load(Ordering::Acquire)))
            .collect()
    }
}

fn persist(db: &Database, id: &str, source: &ProgressSource) -> EngineResult<()> {
    downloads::save_progress(&*db.lock()?, id, &source.segment_progress(), source.total())?;
    Ok(())
}

/// Emits progress events and periodically persists progress while a
/// transfer runs.
struct Reporter {
    stop: CancellationToken,
    handle: JoinHandle<()>,
}

impl Reporter {
    fn spawn(
        ctx: &TaskContext,
        id: &str,
        source: Arc<ProgressSource>,
        total: Option<u64>,
        connections: u32,
    ) -> Self {
        let stop = CancellationToken::new();
        let (db, sink, cfg, id) = (ctx.db.clone(), ctx.sink.clone(), ctx.cfg.clone(), id.to_string());
        let token = stop.clone();
        let handle = tokio::spawn(async move {
            let mut meter = SpeedMeter::new(cfg.speed_window);
            meter.sample(Instant::now(), source.total());
            let mut tick = 0u32;
            let emit = |meter: &mut SpeedMeter| {
                let done = source.total();
                let speed = meter.sample(Instant::now(), done);
                sink.progress(&Progress {
                    id: id.clone(),
                    downloaded_bytes: done,
                    total_bytes: total,
                    speed,
                    eta_seconds: eta_seconds(done, total, speed),
                    connections,
                    stage: None,
                });
            };
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(cfg.progress_interval) => {}
                }
                emit(&mut meter);
                tick += 1;
                if tick.is_multiple_of(cfg.persist_every_ticks.max(1)) {
                    let _ = persist(&db, &id, &source);
                }
            }
            // One last update so the UI shows where the transfer stopped.
            emit(&mut meter);
        });
        Self { stop, handle }
    }

    async fn stop(self) {
        self.stop.cancel();
        let _ = self.handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_cover_every_byte_once() {
        for (total, wanted, min) in [(10, 3, 1), (100, 8, 1), (5, 8, 1), (1, 4, 1), (1_000_000, 8, 300_000)] {
            let segs = plan_segments(total, wanted, min);
            assert_eq!(segs[0].start, 0);
            assert_eq!(segs.last().unwrap().end + 1, total);
            for w in segs.windows(2) {
                assert_eq!(w[0].end + 1, w[1].start);
            }
            assert_eq!(segs.iter().map(Segment::len).sum::<u64>(), total);
            assert!(segs.len() as u64 <= u64::from(wanted));
        }
    }

    #[test]
    fn small_files_get_fewer_segments() {
        assert_eq!(plan_segments(1_000_000, 8, 300_000).len(), 3);
        assert_eq!(plan_segments(100, 8, 1024 * 1024).len(), 1);
    }
}
