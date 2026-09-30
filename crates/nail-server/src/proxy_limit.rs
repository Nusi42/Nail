//! A rate limit for the remote-content proxy routes (M2b follow-up, ADR
//! 0048): each remote session — or, where no session cookie arrives (mail
//! documents on the sandbox origin have an opaque origin and send none),
//! each client address — may fetch at most [`PROXY_REQUESTS_PER_MINUTE`]
//! resources per minute. More are answered `429 Too Many Requests`, so a
//! message (or a client) cannot turn Nail into a fetch amplifier.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Proxy fetches one session or client may make per minute.
pub const PROXY_REQUESTS_PER_MINUTE: u32 = 300;

/// A fixed-window counter per key.
#[derive(Debug)]
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    counters: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(PROXY_REQUESTS_PER_MINUTE, Duration::from_secs(60))
    }
}

impl RateLimiter {
    /// At most `limit` requests per `window` and key.
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            counters: Mutex::new(HashMap::new()),
        }
    }

    /// Counts a request of `key` at `now`; `false` when over the limit.
    pub fn allow_at(&self, key: &str, now: Instant) -> bool {
        let mut counters = self
            .counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if counters.len() > 10_000 {
            let window = self.window;
            counters.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let entry = counters.entry(key.to_owned()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit {
            return false;
        }
        entry.1 += 1;
        true
    }

    /// [`Self::allow_at`] now.
    pub fn allow(&self, key: &str) -> bool {
        self.allow_at(key, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_key_and_window() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        let t0 = Instant::now();
        assert!(limiter.allow_at("session:a", t0));
        assert!(limiter.allow_at("session:a", t0));
        assert!(!limiter.allow_at("session:a", t0), "third in the window");
        assert!(limiter.allow_at("session:b", t0), "other keys count apart");
        assert!(limiter.allow_at("session:a", t0 + Duration::from_secs(61)));
    }
}
