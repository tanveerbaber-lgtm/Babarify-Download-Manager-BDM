//! Download engine: a queue that runs up to N downloads at once, each split
//! into byte-range segments when the server allows it.
//!
//! State changes are written to the database first and then reported through
//! an [`EventSink`], so the UI and the database never disagree for long.

pub mod error;
pub mod events;
pub mod files;
pub mod http;
mod media_task;
mod task;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

use crate::db::{Database, downloads, settings};
use crate::error::{AppError, AppResult};
use crate::media::tools::ToolManager;
use crate::models::DownloadStatus;
use error::EngineError;
use events::EventSink;
use task::TaskContext;

/// Tunables; tests shorten the timeouts and delays.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub user_agent: String,
    pub connect_timeout: Duration,
    /// Maximum silence on an open connection before it counts as stalled.
    pub read_timeout: Duration,
    pub max_retries: u32,
    pub retry_base_delay: Duration,
    pub max_retry_delay: Duration,
    /// Files are not split into segments smaller than this.
    pub min_segment_size: u64,
    pub progress_interval: Duration,
    pub persist_every_ticks: u32,
    pub speed_window: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("BabarifyDownloadManager/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: Duration::from_secs(20),
            read_timeout: Duration::from_secs(30),
            max_retries: 5,
            retry_base_delay: Duration::from_secs(1),
            max_retry_delay: Duration::from_secs(30),
            min_segment_size: 1024 * 1024,
            progress_interval: Duration::from_millis(500),
            persist_every_ticks: 4,
            speed_window: Duration::from_secs(5),
        }
    }
}

/// Why a running download was told to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum StopReason {
    None = 0,
    Pause = 1,
    Cancel = 2,
    Remove = 3,
}

impl StopReason {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Pause,
            2 => Self::Cancel,
            3 => Self::Remove,
            _ => Self::None,
        }
    }
}

struct Active {
    token: CancellationToken,
    reason: Arc<AtomicU8>,
}

impl Active {
    fn stop(&self, reason: StopReason) {
        self.reason.store(reason as u8, Ordering::Release);
        self.token.cancel();
    }
}

struct Scheduler {
    queue: VecDeque<String>,
    active: HashMap<String, Active>,
    max_concurrent: usize,
    segments_per_download: u32,
}

struct Inner {
    db: Arc<Database>,
    client: reqwest::Client,
    sink: Arc<dyn EventSink>,
    cfg: Arc<EngineConfig>,
    runtime: Handle,
    sched: Mutex<Scheduler>,
    tools: Arc<ToolManager>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    pub fn new(
        db: Arc<Database>,
        sink: Arc<dyn EventSink>,
        cfg: EngineConfig,
        runtime: Handle,
        tools: Arc<ToolManager>,
    ) -> AppResult<Self> {
        let client = http::build_client(&cfg)
            .map_err(|e| AppError::Internal(format!("couldn't create HTTP client: {e}")))?;
        let (max_concurrent, segments_per_download) = {
            let s = settings::load(&*db.lock()?, "")?;
            (s.max_concurrent_downloads as usize, s.segments_per_download)
        };
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                client,
                sink,
                cfg: Arc::new(cfg),
                runtime,
                tools,
                sched: Mutex::new(Scheduler {
                    queue: VecDeque::new(),
                    active: HashMap::new(),
                    max_concurrent,
                    segments_per_download,
                }),
            }),
        })
    }

    /// Restores state after launch: interrupted downloads become paused and
    /// queued ones are started again in their original order.
    pub fn start_up(&self) -> AppResult<()> {
        let queued = {
            let conn = self.inner.db.lock()?;
            downloads::recover_interrupted(&conn)?;
            downloads::ids_with_status(&conn, DownloadStatus::Queued)?
        };
        self.sched()?.queue.extend(queued);
        self.schedule()
    }

    pub fn set_limits(&self, max_concurrent: u32, segments_per_download: u32) -> AppResult<()> {
        {
            let mut s = self.sched()?;
            s.max_concurrent = max_concurrent.max(1) as usize;
            s.segments_per_download = segments_per_download.max(1);
        }
        self.schedule()
    }

    /// Queues a pending, paused, failed or cancelled download.
    pub fn enqueue(&self, id: &str) -> AppResult<()> {
        {
            let mut sched = self.sched()?;
            if sched.active.contains_key(id) || sched.queue.iter().any(|q| q == id) {
                return Ok(());
            }
            let conn = self.inner.db.lock()?;
            let d = downloads::require(&conn, id)?;
            if matches!(d.status, DownloadStatus::Completed) {
                return Err(AppError::Validation("This download has already finished.".into()));
            }
            downloads::set_status(&conn, id, DownloadStatus::Queued, None)?;
            sched.queue.push_back(id.to_string());
            self.inner.sink.updated(&downloads::require(&conn, id)?);
        }
        self.schedule()
    }

    /// Pauses a running or queued download, keeping its progress.
    pub fn pause(&self, id: &str) -> AppResult<()> {
        let mut sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Pause);
            return Ok(());
        }
        if let Some(pos) = sched.queue.iter().position(|q| q == id) {
            sched.queue.remove(pos);
            drop(sched);
            self.set_status_and_notify(id, DownloadStatus::Paused, None)?;
            return Ok(());
        }
        Err(AppError::Validation("This download isn't running.".into()))
    }

    /// Stops a download and discards its partial data.
    pub fn cancel(&self, id: &str) -> AppResult<()> {
        let mut sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Cancel);
            return Ok(());
        }
        sched.queue.retain(|q| q != id);
        drop(sched);
        let conn = self.inner.db.lock()?;
        let d = downloads::require(&conn, id)?;
        if d.status == DownloadStatus::Completed {
            return Err(AppError::Validation("This download has already finished.".into()));
        }
        discard_partial(&conn, id)?;
        downloads::set_status(&conn, id, DownloadStatus::Cancelled, None)?;
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }

    /// Removes a download from the list. Partial files are deleted; finished
    /// files are never touched.
    pub fn remove(&self, id: &str) -> AppResult<()> {
        let mut sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Remove);
            return Ok(());
        }
        sched.queue.retain(|q| q != id);
        drop(sched);
        let conn = self.inner.db.lock()?;
        if downloads::require(&conn, id)?.status != DownloadStatus::Completed {
            discard_partial(&conn, id)?;
        }
        downloads::delete(&conn, id)?;
        self.inner.sink.removed(id);
        Ok(())
    }

    fn sched(&self) -> AppResult<MutexGuard<'_, Scheduler>> {
        self.inner
            .sched
            .lock()
            .map_err(|_| AppError::Internal("scheduler lock poisoned".into()))
    }

    fn set_status_and_notify(
        &self,
        id: &str,
        status: DownloadStatus,
        error: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.inner.db.lock()?;
        downloads::set_status(&conn, id, status, error)?;
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }

    /// Starts queued downloads while there are free slots.
    fn schedule(&self) -> AppResult<()> {
        let mut sched = self.sched()?;
        while sched.active.len() < sched.max_concurrent {
            let Some(id) = sched.queue.pop_front() else { break };
            let active = Active {
                token: CancellationToken::new(),
                reason: Arc::new(AtomicU8::new(StopReason::None as u8)),
            };
            let ctx = TaskContext {
                db: self.inner.db.clone(),
                client: self.inner.client.clone(),
                sink: self.inner.sink.clone(),
                cfg: self.inner.cfg.clone(),
                segments_per_download: sched.segments_per_download,
                cancel: active.token.clone(),
                tools: self.inner.tools.clone(),
            };
            let reason = active.reason.clone();
            sched.active.insert(id.clone(), active);
            let engine = self.clone();
            self.inner.runtime.spawn(async move {
                let result = task::run(&ctx, &id).await;
                let reason = StopReason::from_u8(reason.load(Ordering::Acquire));
                engine.finish(&id, result, reason);
            });
        }
        Ok(())
    }

    fn finish(&self, id: &str, result: Result<(), EngineError>, reason: StopReason) {
        if let Ok(mut sched) = self.sched() {
            sched.active.remove(id);
        }
        if let Err(e) = self.record_outcome(id, result, reason) {
            eprintln!("[bdm] couldn't record outcome of download {id}: {e}");
        }
        let _ = self.schedule();
    }

    fn record_outcome(
        &self,
        id: &str,
        result: Result<(), EngineError>,
        reason: StopReason,
    ) -> AppResult<()> {
        let conn = self.inner.db.lock()?;
        match (result, reason) {
            // Completed: the task already recorded it.
            (Ok(()), _) => return Ok(()),
            (Err(EngineError::Stopped), StopReason::Remove) => {
                discard_partial(&conn, id)?;
                downloads::delete(&conn, id)?;
                self.inner.sink.removed(id);
                return Ok(());
            }
            (Err(EngineError::Stopped), StopReason::Cancel) => {
                discard_partial(&conn, id)?;
                downloads::set_status(&conn, id, DownloadStatus::Cancelled, None)?;
            }
            (Err(EngineError::Stopped), _) => {
                downloads::set_status(&conn, id, DownloadStatus::Paused, None)?;
            }
            (Err(e), _) => {
                downloads::set_status(&conn, id, DownloadStatus::Failed, Some(&e.user_message()))?;
            }
        }
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }
}

/// Deletes the partial file and resets progress.
fn discard_partial(conn: &rusqlite::Connection, id: &str) -> AppResult<()> {
    if let Some(temp) = downloads::require(conn, id)?.temp_path {
        files::remove_if_exists(Path::new(&temp))?;
    }
    downloads::reset_progress(conn, id)
}
