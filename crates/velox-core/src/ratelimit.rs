//! Lock-light async token bucket.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct RateLimiter {
    /// bytes per second, 0 = unlimited
    rate: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl RateLimiter {
    pub fn new(rate: u64) -> Self {
        Self { rate: AtomicU64::new(rate), state: Mutex::new((0.0, Instant::now())) }
    }

    pub fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Relaxed);
        let mut s = self.state.lock();
        s.0 = 0.0;
        s.1 = Instant::now();
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Consume `n` bytes worth of tokens, sleeping if the bucket is in debt.
    pub async fn acquire(&self, n: usize) {
        let rate = self.rate();
        if rate == 0 {
            return;
        }
        let wait = {
            let mut s = self.state.lock();
            let now = Instant::now();
            let elapsed = now.duration_since(s.1).as_secs_f64();
            // Burst capacity: 1/4 second worth of data keeps it smooth.
            let cap = (rate as f64 / 4.0).max(16_384.0);
            s.0 = (s.0 + elapsed * rate as f64).min(cap);
            s.1 = now;
            s.0 -= n as f64;
            if s.0 < 0.0 {
                -s.0 / rate as f64
            } else {
                0.0
            }
        };
        if wait > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(wait.min(5.0))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn limits() {
        let rl = RateLimiter::new(200_000);
        let t = Instant::now();
        for _ in 0..20 {
            rl.acquire(20_000).await; // 400 KB total at 200 KB/s
        }
        assert!(t.elapsed() >= Duration::from_millis(1500), "{:?}", t.elapsed());
    }
}
