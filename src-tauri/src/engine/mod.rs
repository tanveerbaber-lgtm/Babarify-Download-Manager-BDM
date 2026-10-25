//! Download engine: runs up to N downloads at once, each split into
//! byte-range segments when the server allows it.
//!
//! The database is the queue: queued downloads start in priority order,
//! then queue position, from queues that are running. A periodic tick
//! starts and stops scheduled queues and fires automatic retries.
//!
//! State changes are written to the database first and then reported through
//! an [`EventSink`], so the UI and the database never disagree for long.

pub mod error;
pub mod events;
pub mod files;
pub mod http;
pub mod limiter;
mod media_task;
pub mod schedule;
mod task;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::NaiveDateTime;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

use crate::db::{Database, downloads, now_ms, queues, settings};
use crate::error::{AppError, AppResult};
use crate::media::tools::ToolManager;
use crate::models::{Download, DownloadStatus, Settings};
use crate::net_guard::NetGuard;
use error::EngineError;
use events::EventSink;
use limiter::{Limits, RateLimiter};
use schedule::Transition;
use task::TaskContext;

/// Tunables; tests shorten the timeouts and delays.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub user_agent: String,
    pub connect_timeout: Duration,
    /// Maximum silence on an open connection before it counts as stalled.
    pub read_timeout: Duration,
    pub retry_base_delay: Duration,
    pub max_retry_delay: Duration,
    /// Files are not split into segments smaller than this.
    pub min_segment_size: u64,
    pub progress_interval: Duration,
    pub persist_every_ticks: u32,
    pub speed_window: Duration,
    /// How often schedules and automatic retries are checked.
    pub tick_interval: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("BabarifyDownloadManager/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: Duration::from_secs(20),
            read_timeout: Duration::from_secs(30),
            retry_base_delay: Duration::from_secs(1),
            max_retry_delay: Duration::from_secs(30),
            min_segment_size: 1024 * 1024,
            progress_interval: Duration::from_millis(500),
            persist_every_ticks: 4,
            speed_window: Duration::from_secs(5),
            tick_interval: Duration::from_secs(15),
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
    /// Its queue stopped: back to queued, progress kept.
    Requeue = 4,
}

impl StopReason {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Pause,
            2 => Self::Cancel,
            3 => Self::Remove,
            4 => Self::Requeue,
            _ => Self::None,
        }
    }
}

struct Active {
    token: CancellationToken,
    reason: Arc<AtomicU8>,
    queue_id: String,
    limit: Arc<RateLimiter>,
}

impl Active {
    fn stop(&self, reason: StopReason) {
        self.reason.store(reason as u8, Ordering::Release);
        self.token.cancel();
    }
}

/// Retry policy from the settings.
#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    /// Retries within one attempt (dropped connections, timeouts, 5xx).
    max_retries: u32,
    /// Automatic retries of a failed download.
    auto_count: u32,
    auto_delay: Duration,
}

struct Scheduler {
    active: HashMap<String, Active>,
    max_concurrent: usize,
    segments_per_download: u32,
    retry: RetryPolicy,
    /// Local time of the previous tick; `None` before the first.
    last_tick: Option<NaiveDateTime>,
    /// A download completed since everything was last done.
    completed_since_idle: bool,
}

struct Inner {
    db: Arc<Database>,
    client: reqwest::Client,
    sink: Arc<dyn EventSink>,
    cfg: Arc<EngineConfig>,
    runtime: Handle,
    sched: Mutex<Scheduler>,
    tools: Arc<ToolManager>,
    global_limit: Arc<RateLimiter>,
    guard: Arc<NetGuard>,
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
        let s = settings::load(&*db.lock()?, "")?;
        let guard = NetGuard::new(s.allow_local_network);
        let client = http::build_client(&cfg, guard.clone())
            .map_err(|e| AppError::Internal(format!("couldn't create HTTP client: {e}")))?;
        let engine = Self {
            inner: Arc::new(Inner {
                db,
                client,
                sink,
                cfg: Arc::new(cfg),
                runtime,
                tools,
                global_limit: Arc::new(RateLimiter::new(0)),
                guard,
                sched: Mutex::new(Scheduler {
                    active: HashMap::new(),
                    max_concurrent: 1,
                    segments_per_download: 1,
                    retry: RetryPolicy { max_retries: 5, auto_count: 0, auto_delay: Duration::from_secs(300) },
                    last_tick: None,
                    completed_since_idle: false,
                }),
            }),
        };
        engine.apply_policy(&s)?;
        Ok(engine)
    }

    /// Restores state after launch (interrupted downloads become paused,
    /// queued ones start again in order) and starts the schedule ticker.
    pub fn start_up(&self) -> AppResult<()> {
        downloads::recover_interrupted(&*self.inner.db.lock()?)?;
        self.tick()?;
        // The ticker holds only a weak reference so a dropped engine stops it.
        let weak = Arc::downgrade(&self.inner);
        let every = self.inner.cfg.tick_interval;
        self.inner.runtime.spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(inner) = weak.upgrade() else { break };
                if let Err(e) = (Engine { inner }).tick() {
                    eprintln!("[bdm] scheduler tick failed: {e}");
                }
            }
        });
        Ok(())
    }

    /// Applies limits and the retry policy from the settings.
    pub fn apply_policy(&self, s: &Settings) -> AppResult<()> {
        self.inner.global_limit.set_rate(u64::from(s.speed_limit_kbps) * 1024);
        self.inner.guard.set_allow_local(s.allow_local_network);
        {
            let mut sched = self.sched()?;
            sched.retry = RetryPolicy {
                max_retries: s.max_retries,
                auto_count: s.auto_retry_count,
                auto_delay: Duration::from_secs(u64::from(s.auto_retry_delay_minutes) * 60),
            };
        }
        self.set_limits(s.max_concurrent_downloads, s.segments_per_download)
    }

    pub fn set_limits(&self, max_concurrent: u32, segments_per_download: u32) -> AppResult<()> {
        {
            let mut s = self.sched()?;
            s.max_concurrent = max_concurrent.max(1) as usize;
            s.segments_per_download = segments_per_download.max(1);
        }
        self.schedule()
    }

    #[cfg(test)]
    fn set_auto_retry(&self, count: u32, delay: Duration) {
        let mut s = self.sched().unwrap();
        s.retry.auto_count = count;
        s.retry.auto_delay = delay;
    }

    /// Queues a pending, paused, failed or cancelled download. It starts when
    /// a slot is free and its queue is running.
    pub fn enqueue(&self, id: &str) -> AppResult<()> {
        {
            let sched = self.sched()?;
            if sched.active.contains_key(id) {
                return Ok(());
            }
            let conn = self.inner.db.lock()?;
            let d = downloads::require(&conn, id)?;
            if d.status == DownloadStatus::Completed {
                return Err(AppError::Validation("This download has already finished.".into()));
            }
            // A manual start resets the automatic-retry budget.
            downloads::clear_retry(&conn, id)?;
            if d.status != DownloadStatus::Queued {
                downloads::set_status(&conn, id, DownloadStatus::Queued, None)?;
            }
            self.inner.sink.updated(&downloads::require(&conn, id)?);
        }
        self.schedule()
    }

    /// Pauses a running or queued download, keeping its progress.
    pub fn pause(&self, id: &str) -> AppResult<()> {
        let sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Pause);
            return Ok(());
        }
        let conn = self.inner.db.lock()?;
        let d = downloads::require(&conn, id)?;
        let waiting_retry = d.status == DownloadStatus::Failed && d.retry_at.is_some();
        if d.status != DownloadStatus::Queued && !waiting_retry {
            return Err(AppError::Validation("This download isn't running.".into()));
        }
        downloads::clear_retry(&conn, id)?;
        downloads::set_status(&conn, id, DownloadStatus::Paused, None)?;
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }

    /// Stops a download and discards its partial data.
    pub fn cancel(&self, id: &str) -> AppResult<()> {
        let sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Cancel);
            return Ok(());
        }
        let conn = self.inner.db.lock()?;
        let d = downloads::require(&conn, id)?;
        if d.status == DownloadStatus::Completed {
            return Err(AppError::Validation("This download has already finished.".into()));
        }
        discard_partial(&conn, id)?;
        downloads::clear_retry(&conn, id)?;
        downloads::set_status(&conn, id, DownloadStatus::Cancelled, None)?;
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }

    /// Removes a download from the list. Partial files are deleted; finished
    /// files are never touched.
    pub fn remove(&self, id: &str) -> AppResult<()> {
        let sched = self.sched()?;
        if let Some(active) = sched.active.get(id) {
            active.stop(StopReason::Remove);
            return Ok(());
        }
        let conn = self.inner.db.lock()?;
        if downloads::require(&conn, id)?.status != DownloadStatus::Completed {
            discard_partial(&conn, id)?;
        }
        downloads::delete(&conn, id)?;
        self.inner.sink.removed(id);
        Ok(())
    }

    /// Starts a queue and queues everything in it that hasn't finished.
    pub fn start_queue(&self, queue_id: &str) -> AppResult<()> {
        {
            let _sched = self.sched()?;
            let conn = self.inner.db.lock()?;
            queues::set_running(&conn, queue_id, true)?;
            let ids: Vec<String> = conn
                .prepare(
                    "SELECT id FROM downloads WHERE queue_id = ?1 AND status IN ('pending', 'paused', 'failed')
                     ORDER BY priority DESC, position",
                )?
                .query_map([queue_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for id in ids {
                downloads::clear_retry(&conn, &id)?;
                downloads::set_status(&conn, &id, DownloadStatus::Queued, None)?;
                self.inner.sink.updated(&downloads::require(&conn, &id)?);
            }
        }
        self.schedule()
    }

    /// Stops a queue; its running downloads go back to waiting with their progress.
    pub fn stop_queue(&self, queue_id: &str) -> AppResult<()> {
        let sched = self.sched()?;
        queues::set_running(&*self.inner.db.lock()?, queue_id, false)?;
        for a in sched.active.values().filter(|a| a.queue_id == queue_id) {
            a.stop(StopReason::Requeue);
        }
        Ok(())
    }

    /// Moves a download to another queue (a running download keeps running).
    pub fn move_to_queue(&self, id: &str, queue_id: &str) -> AppResult<()> {
        {
            let mut sched = self.sched()?;
            let conn = self.inner.db.lock()?;
            queues::require(&conn, queue_id)?;
            downloads::set_queue(&conn, id, queue_id)?;
            if let Some(a) = sched.active.get_mut(id) {
                a.queue_id = queue_id.to_string();
            }
            self.inner.sink.updated(&downloads::require(&conn, id)?);
        }
        self.schedule()
    }

    /// Sets a download's own speed limit (bytes/s); applies at once if it's running.
    pub fn set_speed_limit(&self, id: &str, limit: Option<i64>) -> AppResult<()> {
        let sched = self.sched()?;
        let conn = self.inner.db.lock()?;
        downloads::set_speed_limit(&conn, id, limit)?;
        if let Some(a) = sched.active.get(id) {
            a.limit.set_rate(limit.filter(|n| *n > 0).unwrap_or(0) as u64);
        }
        self.inner.sink.updated(&downloads::require(&conn, id)?);
        Ok(())
    }

    /// Decides which addresses downloads (and yt-dlp) may reach.
    pub fn net_guard(&self) -> &Arc<NetGuard> {
        &self.inner.guard
    }

    /// Re-runs selection after a priority or order change.
    pub fn reschedule(&self) -> AppResult<()> {
        self.schedule()
    }

    fn sched(&self) -> AppResult<MutexGuard<'_, Scheduler>> {
        self.inner
            .sched
            .lock()
            .map_err(|_| AppError::Internal("scheduler lock poisoned".into()))
    }

    fn tick(&self) -> AppResult<()> {
        self.tick_at(chrono::Local::now().naive_local(), now_ms())
    }

    /// Applies schedule starts and stops between the last tick and `now`,
    /// fires due automatic retries and fills free slots.
    fn tick_at(&self, now: NaiveDateTime, now_ms: i64) -> AppResult<()> {
        let to_stop = {
            let mut sched = self.sched()?;
            let last = sched.last_tick.replace(now);
            let conn = self.inner.db.lock()?;
            let mut to_stop = Vec::new();
            let mut changed = false;
            for q in queues::list(&conn)? {
                let transition = match last {
                    Some(last) => schedule::last_transition(&q.schedule, last, now),
                    // At launch, a queue whose window is open starts.
                    None => schedule::inside_window(&q.schedule, now).then_some(Transition::Start),
                };
                match transition {
                    Some(Transition::Start) if !q.running => {
                        queues::set_running(&conn, &q.id, true)?;
                        changed = true;
                    }
                    Some(Transition::Stop) if q.running => {
                        queues::set_running(&conn, &q.id, false)?;
                        to_stop.push(q.id);
                        changed = true;
                    }
                    _ => {}
                }
            }
            for id in downloads::due_retries(&conn, now_ms)? {
                conn.execute(
                    "UPDATE downloads SET retry_at = NULL, status = 'queued', error_message = NULL WHERE id = ?1",
                    [&id],
                )?;
                self.inner.sink.updated(&downloads::require(&conn, &id)?);
            }
            changed |= stop_idle_scheduled_queues(&conn, &sched)?;
            if changed {
                self.inner.sink.queues_changed();
            }
            to_stop
        };
        for q in to_stop {
            let sched = self.sched()?;
            for a in sched.active.values().filter(|a| a.queue_id == q) {
                a.stop(StopReason::Requeue);
            }
        }
        self.schedule()
    }

    /// Starts queued downloads from running queues while there are free slots.
    fn schedule(&self) -> AppResult<()> {
        let mut sched = self.sched()?;
        let free = sched.max_concurrent.saturating_sub(sched.active.len());
        if free == 0 {
            return Ok(());
        }
        let conn = self.inner.db.lock()?;
        let running: HashSet<String> = queues::list(&conn)?.into_iter().filter(|q| q.running).map(|q| q.id).collect();
        let picks: Vec<(String, String)> = downloads::queued_in_order(&conn)?
            .into_iter()
            .filter(|(id, queue)| running.contains(queue) && !sched.active.contains_key(id))
            .take(free)
            .collect();
        for (id, queue_id) in picks {
            let own_rate = downloads::require(&conn, &id)?.speed_limit.filter(|n| *n > 0).unwrap_or(0) as u64;
            let active = Active {
                token: CancellationToken::new(),
                reason: Arc::new(AtomicU8::new(StopReason::None as u8)),
                queue_id,
                limit: Arc::new(RateLimiter::new(own_rate)),
            };
            let ctx = TaskContext {
                db: self.inner.db.clone(),
                client: self.inner.client.clone(),
                sink: self.inner.sink.clone(),
                cfg: self.inner.cfg.clone(),
                segments_per_download: sched.segments_per_download,
                max_retries: sched.retry.max_retries,
                limits: Limits { global: self.inner.global_limit.clone(), own: active.limit.clone() },
                cancel: active.token.clone(),
                tools: self.inner.tools.clone(),
                guard: self.inner.guard.clone(),
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
        let Ok(policy) = self.sched().map(|s| s.retry) else { return };
        let outcome = self.record_outcome(id, result, reason, policy);
        // Leave the active set only once the outcome is recorded, and in the
        // same step as the completion flag, so two downloads finishing
        // together can't both report "all done".
        if let Ok(mut s) = self.sched() {
            s.active.remove(id);
            if matches!(&outcome, Ok(Some(d)) if d.status == DownloadStatus::Completed) {
                s.completed_since_idle = true;
            }
        }
        match outcome {
            Ok(Some(d)) if d.status == DownloadStatus::Completed => self.inner.sink.completed(&d),
            Ok(Some(d)) if d.status == DownloadStatus::Failed && d.retry_at.is_none() => self.inner.sink.failed(&d),
            Ok(_) => {}
            Err(e) => eprintln!("[bdm] couldn't record outcome of download {id}: {e}"),
        }
        let _ = self.schedule();
        if let Err(e) = self.check_all_done() {
            eprintln!("[bdm] couldn't check for finished queues: {e}");
        }
    }

    /// Stops scheduled queues that ran dry and reports when everything is done.
    fn check_all_done(&self) -> AppResult<()> {
        let mut sched = self.sched()?;
        let conn = self.inner.db.lock()?;
        if stop_idle_scheduled_queues(&conn, &sched)? {
            self.inner.sink.queues_changed();
        }
        if sched.completed_since_idle && sched.active.is_empty() && downloads::unfinished_count(&conn)? == 0 {
            sched.completed_since_idle = false;
            drop(conn);
            drop(sched);
            self.inner.sink.all_done();
        }
        Ok(())
    }

    /// Writes the result of a finished task; returns the final record unless it was removed.
    fn record_outcome(
        &self,
        id: &str,
        result: Result<(), EngineError>,
        reason: StopReason,
        policy: RetryPolicy,
    ) -> AppResult<Option<Download>> {
        let conn = self.inner.db.lock()?;
        match (result, reason) {
            // Completed: the task already recorded and reported it.
            (Ok(()), _) => return Ok(Some(downloads::require(&conn, id)?)),
            (Err(EngineError::Stopped), StopReason::Remove) => {
                discard_partial(&conn, id)?;
                downloads::delete(&conn, id)?;
                self.inner.sink.removed(id);
                return Ok(None);
            }
            (Err(EngineError::Stopped), StopReason::Cancel) => {
                discard_partial(&conn, id)?;
                downloads::set_status(&conn, id, DownloadStatus::Cancelled, None)?;
            }
            (Err(EngineError::Stopped), StopReason::Requeue) => {
                downloads::set_status(&conn, id, DownloadStatus::Queued, None)?;
            }
            (Err(EngineError::Stopped), _) => {
                downloads::set_status(&conn, id, DownloadStatus::Paused, None)?;
            }
            (Err(e), _) => {
                downloads::set_status(&conn, id, DownloadStatus::Failed, Some(&e.user_message()))?;
                let used = downloads::require(&conn, id)?.auto_retries;
                if e.worth_auto_retry() && used < i64::from(policy.auto_count) {
                    let at = now_ms() + policy.auto_delay.as_millis() as i64;
                    downloads::schedule_retry(&conn, id, Some(at))?;
                }
            }
        }
        let d = downloads::require(&conn, id)?;
        self.inner.sink.updated(&d);
        Ok(Some(d))
    }
}

/// Scheduled queues with nothing left to do stop until their next start.
fn stop_idle_scheduled_queues(conn: &rusqlite::Connection, sched: &Scheduler) -> AppResult<bool> {
    let mut changed = false;
    for q in queues::list(conn)?.into_iter().filter(|q| q.running && q.schedule.enabled) {
        let waiting: i64 = conn.query_row(
            "SELECT COUNT(*) FROM downloads WHERE queue_id = ?1 AND status IN ('queued', 'downloading')",
            [&q.id],
            |r| r.get(0),
        )?;
        if waiting == 0 && !sched.active.values().any(|a| a.queue_id == q.id) {
            queues::set_running(conn, &q.id, false)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Deletes the partial file and resets progress.
fn discard_partial(conn: &rusqlite::Connection, id: &str) -> AppResult<()> {
    if let Some(temp) = downloads::require(conn, id)?.temp_path {
        files::remove_if_exists(Path::new(&temp))?;
    }
    downloads::reset_progress(conn, id)
}
