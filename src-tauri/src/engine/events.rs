//! Events the engine reports to the UI, behind a trait so tests can capture them.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::models::Download;

pub const EVENT_PROGRESS: &str = "download:progress";
pub const EVENT_UPDATED: &str = "download:updated";
pub const EVENT_REMOVED: &str = "download:removed";
pub const EVENT_TOOL_PROGRESS: &str = "tools:progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// Bytes per second, averaged over the last few seconds.
    pub speed: f64,
    pub eta_seconds: Option<f64>,
    pub connections: u32,
    /// What a media download is doing, e.g. "Merging video and audio".
    pub stage: Option<String>,
    /// Per-connection progress of a segmented download (empty otherwise).
    pub segments: Vec<SegmentView>,
}

/// One connection's byte range and how much of it has arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    pub start: u64,
    pub len: u64,
    pub done: u64,
}

/// Progress of installing or updating a media tool.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    pub tool: crate::media::tools::ToolKind,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub stage: String,
}

pub trait EventSink: Send + Sync + 'static {
    fn progress(&self, progress: &Progress);
    fn updated(&self, download: &Download);
    fn removed(&self, id: &str);
    fn tool_progress(&self, _progress: &ToolProgress) {}
    /// A download finished successfully.
    fn completed(&self, _download: &Download) {}
    /// A download failed and no automatic retry is planned.
    fn failed(&self, _download: &Download) {}
    /// The virus scan of a finished download found a threat.
    fn threat(&self, _download: &Download) {}
    /// Every download has finished (nothing queued, running or waiting to retry).
    fn all_done(&self) {}
    /// A queue was started or stopped by its schedule.
    fn queues_changed(&self) {}
}

/// Sliding-window throughput meter.
pub struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
    window: Duration,
}

impl SpeedMeter {
    pub fn new(window: Duration) -> Self {
        Self { samples: VecDeque::new(), window }
    }

    /// Records the cumulative byte count and returns bytes/second.
    pub fn sample(&mut self, now: Instant, total: u64) -> f64 {
        self.samples.push_back((now, total));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > self.window && self.samples.len() > 2 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        let (t0, b0) = *self.samples.front().expect("just pushed");
        let elapsed = now.duration_since(t0).as_secs_f64();
        if elapsed <= 0.0 {
            0.0
        } else {
            total.saturating_sub(b0) as f64 / elapsed
        }
    }
}

pub fn eta_seconds(downloaded: u64, total: Option<u64>, speed: f64) -> Option<f64> {
    let total = total?;
    if speed <= 0.0 {
        return None;
    }
    Some(total.saturating_sub(downloaded) as f64 / speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_over_window() {
        let mut m = SpeedMeter::new(Duration::from_secs(5));
        let t0 = Instant::now();
        assert_eq!(m.sample(t0, 0), 0.0);
        let s = m.sample(t0 + Duration::from_secs(2), 2_000);
        assert!((s - 1_000.0).abs() < 1e-6);
    }

    #[test]
    fn eta_needs_total_and_speed() {
        assert_eq!(eta_seconds(50, Some(150), 10.0), Some(10.0));
        assert_eq!(eta_seconds(50, None, 10.0), None);
        assert_eq!(eta_seconds(50, Some(150), 0.0), None);
    }
}
