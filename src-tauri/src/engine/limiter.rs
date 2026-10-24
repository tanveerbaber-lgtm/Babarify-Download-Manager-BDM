//! Token-bucket speed limits: one shared by every download, one per download.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// At most this much unused allowance carries over, so a limit can't be
/// beaten by a burst after a quiet spell.
const BURST: Duration = Duration::from_millis(500);

pub struct RateLimiter {
    /// Bytes per second; 0 = unlimited.
    rate: AtomicU64,
    bucket: tokio::sync::Mutex<Bucket>,
}

struct Bucket {
    /// May go negative: the debt is paid off by waiting.
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(rate: u64) -> Self {
        Self { rate: AtomicU64::new(rate), bucket: tokio::sync::Mutex::new(Bucket { tokens: 0.0, last: Instant::now() }) }
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Changes the limit; running transfers pick it up on their next chunk.
    pub fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Relaxed);
    }

    /// Accounts for `n` bytes, sleeping as long as needed to stay under the limit.
    pub async fn take(&self, n: u64) {
        let rate = self.rate();
        if rate == 0 || n == 0 {
            return;
        }
        let wait = {
            let mut b = self.bucket.lock().await;
            let now = Instant::now();
            let r = rate as f64;
            b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * r).min(r * BURST.as_secs_f64());
            b.last = now;
            b.tokens -= n as f64;
            if b.tokens >= 0.0 {
                return;
            }
            Duration::from_secs_f64(-b.tokens / r)
        };
        tokio::time::sleep(wait).await;
    }
}

/// The limits that apply to one download.
#[derive(Clone)]
pub struct Limits {
    pub global: Arc<RateLimiter>,
    pub own: Arc<RateLimiter>,
}

impl Limits {
    pub async fn take(&self, n: u64) {
        self.global.take(n).await;
        self.own.take(n).await;
    }

    /// The tighter of the two limits in bytes/s, if any (for yt-dlp's `-r`).
    pub fn effective(&self) -> Option<u64> {
        [self.global.rate(), self.own.rate()].into_iter().filter(|r| *r > 0).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_never_waits() {
        let l = RateLimiter::new(0);
        let t = Instant::now();
        for _ in 0..1000 {
            l.take(1 << 20).await;
        }
        assert!(t.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn holds_throughput_near_the_limit() {
        let l = RateLimiter::new(1_000_000);
        let t = Instant::now();
        for _ in 0..50 {
            l.take(16_000).await; // 800 KB in total
        }
        let secs = t.elapsed().as_secs_f64();
        assert!((0.7..1.2).contains(&secs), "took {secs:.2}s");
    }

    #[tokio::test]
    async fn limit_changes_apply_immediately() {
        let l = RateLimiter::new(100);
        l.set_rate(0);
        let t = Instant::now();
        l.take(1_000_000).await;
        assert!(t.elapsed() < Duration::from_millis(50));
        let limits = Limits { global: Arc::new(RateLimiter::new(500)), own: Arc::new(RateLimiter::new(200)) };
        assert_eq!(limits.effective(), Some(200));
        limits.own.set_rate(0);
        assert_eq!(limits.effective(), Some(500));
    }
}
