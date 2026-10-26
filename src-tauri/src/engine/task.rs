//! Runs one download from probe to finalized file.

use std::future::Future;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, IF_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::EngineConfig;
use super::error::{EngineError, EngineResult};
use super::events::{EventSink, Progress, SegmentView, SpeedMeter, eta_seconds};
use super::files;
use super::limiter::Limits;
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
/// Room left in front of a connection whose range is split: more than it can
/// have buffered or in flight, so the part handed over is never one it's
/// still writing.
const SPLIT_GUARD: u64 = 2 * WRITE_BUFFER as u64;

pub(super) struct TaskContext {
    pub db: Arc<Database>,
    pub client: Client,
    pub sink: Arc<dyn EventSink>,
    pub cfg: Arc<EngineConfig>,
    pub segments_per_download: u32,
    /// Retries within one attempt.
    pub max_retries: u32,
    pub limits: Limits,
    pub cancel: CancellationToken,
    pub tools: Arc<ToolManager>,
    pub guard: Arc<crate::net_guard::NetGuard>,
    pub cookies: Arc<crate::cookies::CookieJar>,
    pub headers: Arc<crate::request_headers::HeaderStore>,
    /// Set (with the cancel token) when the disk watcher stops the download.
    pub stop_reason: Arc<std::sync::atomic::AtomicU8>,
}

/// Pauses the download (via its stop reason and cancel token) if the drive
/// holding `dir` gets nearly full, for downloads that don't reserve their
/// whole size up front. Stops watching when dropped.
pub(super) struct DiskWatch(JoinHandle<()>);

impl DiskWatch {
    pub(super) fn start(ctx: &TaskContext, dir: &Path) -> Self {
        let (free, reserve, every) = (ctx.cfg.free_space.clone(), ctx.cfg.disk_reserve, ctx.cfg.disk_check_interval);
        let (cancel, reason, dir) = (ctx.cancel.clone(), ctx.stop_reason.clone(), dir.to_path_buf());
        Self(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(every) => {}
                }
                if free.of(&dir).is_some_and(|n| n < reserve) {
                    reason.store(super::StopReason::NoSpace as u8, Ordering::Release);
                    cancel.cancel();
                    return;
                }
            }
        }))
    }
}

impl Drop for DiskWatch {
    fn drop(&mut self) {
        self.0.abort();
    }
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

    // DNS names are checked as they resolve; a literal IP never goes through DNS.
    if let Ok(url) = url::Url::parse(&download.url) {
        ctx.guard.check_literal(&url).map_err(|b| EngineError::Blocked(b.to_string()))?;
    }
    let cookie = ctx.cookies.header_for(&download.url);
    let mut remote =
        with_retries(ctx, || http::probe(&ctx.client, &download.url, download.referrer.as_deref(), cookie.as_deref())).await?;
    remote.cookie = ctx.cookies.header_for(&remote.final_url);

    let temp_exists = download.temp_path.as_deref().is_some_and(|p| Path::new(p).exists());
    let has_progress = download.downloaded_bytes > 0 || temp_exists;
    let download = if has_progress && !same_remote(&download, &remote) {
        discard_progress(&ctx.db, id)?;
        downloads::require(&*ctx.db.lock()?, id)?
    } else {
        download
    };

    // What's still to be written must fit, leaving the reserve free. A
    // segmented download's file already holds its full size once created.
    if let Some(total) = remote.total_bytes {
        let on_disk = download.temp_path.as_deref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
        if let Some(message) = super::disk::shortfall(&dest_dir, total.saturating_sub(on_disk), ctx.cfg.disk_reserve, &ctx.cfg.free_space) {
            return Err(EngineError::NoSpace(message));
        }
    }

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
    segs.sort_by_key(|s| s.start);
    let file_ok = std::fs::metadata(temp).is_ok_and(|m| m.len() == total);
    if !(file_ok && covers(&segs, total)) {
        segs = plan_segments(total, ctx.segments_per_download, ctx.cfg.min_segment_size);
        let file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(temp)?;
        file.set_len(total)?;
        downloads::replace_segments(&*ctx.db.lock()?, id, &segs)?;
    }

    let source = Arc::new(ProgressSource::segmented(&segs));
    let reporter = Reporter::spawn(ctx, id, source.clone(), Some(total));

    // As many connections as a fresh plan would use, so a resumed download
    // with one big range left is spread out again.
    let planned = plan_segments(total, ctx.segments_per_download, ctx.cfg.min_segment_size).len();
    let unfinished = segs.iter().filter(|s| s.remaining() > 0).count();
    let stop = ctx.cancel.child_token();
    let workers = (0..planned.max(unfinished)).map(|_| {
        let source = source.clone();
        let stop = stop.clone();
        async move {
            let result = segment_worker(ctx, remote, temp, &source, &stop).await;
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

/// Whether saved segments cover `0..total` exactly once, in order of start.
fn covers(segs: &[Segment], total: u64) -> bool {
    segs.first().is_some_and(|s| s.start == 0)
        && segs.last().is_some_and(|s| s.end + 1 == total)
        && segs.windows(2).all(|w| w[0].end + 1 == w[1].start)
        && segs.iter().all(|s| s.start <= s.end && s.downloaded <= s.len())
}

/// One connection: downloads its range, then takes more work until none is left.
async fn segment_worker(
    ctx: &TaskContext,
    remote: &RemoteFile,
    temp: &Path,
    source: &ProgressSource,
    stop: &CancellationToken,
) -> EngineResult<()> {
    let Some(mut slot) = source.take_work(ctx.cfg.min_segment_size) else { return Ok(()) };
    let mut failures = 0;
    loop {
        if slot.remaining() == 0 {
            match source.take_work(ctx.cfg.min_segment_size) {
                Some(next) => {
                    slot = next;
                    failures = 0;
                }
                None => return Ok(()),
            }
            continue;
        }
        let before = slot.done();
        match fetch_range(ctx, remote, temp, &slot, stop).await {
            Ok(()) => failures = 0,
            Err(e) if e.is_retryable() && failures < ctx.max_retries => {
                if slot.done() > before {
                    failures = 0;
                }
                failures += 1;
                backoff(ctx, stop, failures, e.retry_after()).await?;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Downloads the rest of `slot`. Its end is read again for every chunk, so
/// the request stops early if another connection took over its back part.
async fn fetch_range(
    ctx: &TaskContext,
    remote: &RemoteFile,
    temp: &Path,
    slot: &Slot,
    stop: &CancellationToken,
) -> EngineResult<()> {
    let from = slot.start + slot.done();
    let end = slot.end();
    let mut req = remote.get(&ctx.client).header(RANGE, format!("bytes={from}-{end}"));
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
    copy_body(resp, &mut file, &slot.done, || Some((slot.end() + 1).saturating_sub(from)), &ctx.limits, stop).await?;
    if slot.remaining() > 0 {
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
    let source = Arc::new(ProgressSource::single());
    let reporter = Reporter::spawn(ctx, id, source.clone(), remote.total_bytes);
    // The file grows as it downloads (its size may not even be known).
    let _watch = DiskWatch::start(ctx, temp.parent().unwrap_or(temp));
    let result = single_loop(ctx, download, remote, temp, &source.single_counter()).await;
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
            copy_body(resp, &mut file, counter, || limit, &ctx.limits, &ctx.cancel).await?;
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
            Err(e) if e.is_retryable() && failures < ctx.max_retries => {
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
/// Stops after `limit()` bytes if given (read again for every chunk, as it
/// can shrink) and keeps under the speed limits. Returns the bytes written.
async fn copy_body(
    resp: Response,
    file: &mut tokio::fs::File,
    counter: &AtomicU64,
    limit: impl Fn() -> Option<u64>,
    speed: &Limits,
    stop: &CancellationToken,
) -> EngineResult<u64> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUFFER);
    let mut written = 0u64;
    let mut last_flush = Instant::now();

    loop {
        if limit().is_some_and(|l| written + buf.len() as u64 >= l) {
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
        let take = match limit() {
            Some(l) => chunk.len().min(l.saturating_sub(written + buf.len() as u64) as usize),
            None => chunk.len(),
        };
        buf.extend_from_slice(&chunk[..take]);
        tokio::select! {
            _ = stop.cancelled() => return Err(EngineError::Stopped),
            _ = speed.take(take as u64) => {}
        }
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
            Err(e) if e.is_retryable() && failures < ctx.max_retries => {
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
    files::mark_from_internet(&saved, &download.url, download.referrer.as_deref());

    let updated = {
        let conn = ctx.db.lock()?;
        downloads::mark_completed(&conn, &download.id, &saved.to_string_lossy(), size)?;
        downloads::require(&conn, &download.id)?
    };
    ctx.sink.updated(&updated);
    Ok(())
}

// ---------------------------------------------------------------- progress

/// One connection's byte range. `end` only ever moves earlier, when an idle
/// connection takes over the back part of the range.
struct Slot {
    idx: u32,
    start: u64,
    end: AtomicU64,
    /// Bytes written from `start`; can briefly pass `end` after a split, by
    /// bytes the other connection also writes (the same data).
    done: AtomicU64,
    /// A connection is working on this range.
    claimed: AtomicBool,
}

impl Slot {
    fn new(idx: u32, start: u64, end: u64, done: u64) -> Self {
        Self { idx, start, end: AtomicU64::new(end), done: AtomicU64::new(done), claimed: AtomicBool::new(false) }
    }

    fn end(&self) -> u64 {
        self.end.load(Ordering::Acquire)
    }

    fn len(&self) -> u64 {
        self.end() - self.start + 1
    }

    fn done(&self) -> u64 {
        self.done.load(Ordering::Acquire).min(self.len())
    }

    fn remaining(&self) -> u64 {
        self.len() - self.done()
    }
}

struct ProgressSource {
    slots: Mutex<Vec<Arc<Slot>>>,
    /// Single-stream downloads have one counter and no byte ranges.
    segmented: bool,
}

impl ProgressSource {
    fn segmented(segs: &[Segment]) -> Self {
        let slots = segs.iter().map(|s| Arc::new(Slot::new(s.idx, s.start, s.end, s.downloaded.min(s.len())))).collect();
        Self { slots: Mutex::new(slots), segmented: true }
    }

    fn single() -> Self {
        Self { slots: Mutex::new(vec![Arc::new(Slot::new(0, 0, u64::MAX - 1, 0))]), segmented: false }
    }

    fn slots(&self) -> Vec<Arc<Slot>> {
        self.slots.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn single_counter(&self) -> Arc<SingleCounter> {
        Arc::new(SingleCounter(self.slots()[0].clone()))
    }

    fn total(&self) -> u64 {
        let slots = self.slots();
        if self.segmented { slots.iter().map(|s| s.done()).sum() } else { slots[0].done.load(Ordering::Acquire) }
    }

    /// Connections downloading right now.
    fn connections(&self) -> u32 {
        if !self.segmented {
            return 1;
        }
        self.slots().iter().filter(|s| s.claimed.load(Ordering::Acquire) && s.remaining() > 0).count() as u32
    }

    fn views(&self) -> Vec<SegmentView> {
        if !self.segmented {
            return Vec::new();
        }
        let mut slots = self.slots();
        slots.sort_by_key(|s| s.start);
        slots.iter().map(|s| SegmentView { start: s.start, len: s.len(), done: s.done() }).collect()
    }

    /// The current layout, for saving.
    fn snapshot(&self) -> Vec<Segment> {
        self.slots().iter().map(|s| Segment { idx: s.idx, start: s.start, end: s.end(), downloaded: s.done() }).collect()
    }

    /// The next range for an idle connection: an unclaimed unfinished one, or
    /// else the back half of the range with the most left (dynamic
    /// segmentation). `None` when no split would leave both parts at least
    /// `min_split` bytes.
    fn take_work(&self, min_split: u64) -> Option<Arc<Slot>> {
        let mut slots = self.slots.lock().ok()?;
        if let Some(free) = slots.iter().find(|s| s.remaining() > 0 && !s.claimed.load(Ordering::Acquire)) {
            free.claimed.store(true, Ordering::Release);
            return Some(free.clone());
        }
        let victim = slots.iter().filter(|s| s.claimed.load(Ordering::Acquire)).max_by_key(|s| s.remaining())?.clone();
        let reserve = victim.start + victim.done.load(Ordering::Acquire) + SPLIT_GUARD;
        let end = victim.end();
        if end < reserve || end - reserve < 2 * min_split.max(1) {
            return None;
        }
        let split = reserve + (end - reserve) / 2;
        victim.end.store(split, Ordering::Release);
        let idx = slots.iter().map(|s| s.idx).max().unwrap_or(0) + 1;
        let taken = Arc::new(Slot::new(idx, split + 1, end, 0));
        taken.claimed.store(true, Ordering::Release);
        slots.push(taken.clone());
        Some(taken)
    }
}

/// The single-stream counter, borrowed as an `AtomicU64`.
struct SingleCounter(Arc<Slot>);

impl std::ops::Deref for SingleCounter {
    type Target = AtomicU64;
    fn deref(&self) -> &AtomicU64 {
        &self.0.done
    }
}

fn persist(db: &Database, id: &str, source: &ProgressSource) -> EngineResult<()> {
    let conn = db.lock()?;
    // The whole layout is saved, since splits change the ranges.
    if source.segmented {
        downloads::replace_segments(&conn, id, &source.snapshot())?;
    }
    downloads::save_progress(&conn, id, &[], source.total())?;
    Ok(())
}

/// Emits progress events and periodically persists progress while a
/// transfer runs.
struct Reporter {
    stop: CancellationToken,
    handle: JoinHandle<()>,
}

impl Reporter {
    fn spawn(ctx: &TaskContext, id: &str, source: Arc<ProgressSource>, total: Option<u64>) -> Self {
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
                    connections: source.connections(),
                    stage: None,
                    segments: source.views(),
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
    fn idle_connections_take_over_the_largest_range() {
        let mb = 1024 * 1024;
        let source = ProgressSource::segmented(&plan_segments(8 * mb, 2, mb));
        let a = source.take_work(mb).unwrap();
        let b = source.take_work(mb).unwrap();
        assert_eq!((a.start, b.start), (0, 4 * mb), "unclaimed ranges first");
        a.done.store(4 * mb, Ordering::Release);
        b.done.store(mb, Ordering::Release);

        // A finished connection takes the back half of what b has left.
        let c = source.take_work(mb).unwrap();
        let reserve = 5 * mb + SPLIT_GUARD;
        assert_eq!(b.end(), reserve + (8 * mb - 1 - reserve) / 2);
        assert_eq!((c.start, c.end()), (b.end() + 1, 8 * mb - 1));
        let mut layout = source.snapshot();
        layout.sort_by_key(|s| s.start);
        assert!(covers(&layout, 8 * mb), "{layout:?}");
        assert_eq!(source.total(), 5 * mb);

        // Nothing is split into pieces smaller than the minimum.
        b.done.store(b.len() - 1, Ordering::Release);
        c.done.store(c.len() - mb / 2, Ordering::Release);
        assert!(source.take_work(mb).is_none());
    }

    #[test]
    fn bytes_past_a_moved_end_are_not_counted_twice() {
        let source = ProgressSource::segmented(&[Segment { idx: 0, start: 0, end: 99, downloaded: 0 }]);
        let s = source.take_work(1).unwrap();
        s.done.store(150, Ordering::Release);
        assert_eq!(s.done(), 100);
        assert_eq!(source.total(), 100);
    }

    #[test]
    fn saved_layouts_must_cover_the_file() {
        let seg = |start, end| Segment { idx: 0, start, end, downloaded: 0 };
        assert!(covers(&[seg(0, 9), seg(10, 19)], 20));
        assert!(!covers(&[seg(0, 9), seg(11, 19)], 20), "gap");
        assert!(!covers(&[seg(0, 10), seg(10, 19)], 20), "overlap");
        assert!(!covers(&[seg(0, 9)], 20), "short");
    }

    #[test]
    fn small_files_get_fewer_segments() {
        assert_eq!(plan_segments(1_000_000, 8, 300_000).len(), 3);
        assert_eq!(plan_segments(100, 8, 1024 * 1024).len(), 1);
    }
}
