//! Time source. Every decision about expiry, idle time, backoff and TOTP
//! steps reads the clock through [`Clock`], so tests can move time (and
//! simulate clock skew) without sleeping.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unix seconds (UTC).
pub trait Clock: Send + Sync + fmt::Debug {
    /// The current time in Unix seconds.
    fn now(&self) -> i64;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default()
    }
}

/// A clock that only moves when told to (tests, demos).
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    /// A clock standing at `now` (Unix seconds).
    pub fn new(now: i64) -> Self {
        Self(Arc::new(AtomicI64::new(now)))
    }

    /// Moves the clock forward (or back, with a negative value).
    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }

    /// Sets the clock.
    pub fn set(&self, now: i64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Parses an RFC 3339 time from the store into Unix seconds.
pub(crate) fn parse_rfc3339(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.timestamp())
}

/// Formats Unix seconds as RFC 3339 (UTC, seconds), like the store.
pub(crate) fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now(), 1_000);
        clock.advance(30);
        assert_eq!(clock.now(), 1_030);
        clock.advance(-60);
        assert_eq!(clock.now(), 970);
        clock.set(5);
        assert_eq!(clock.clone().now(), 5);
    }

    #[test]
    fn system_clock_is_after_2020() {
        assert!(SystemClock.now() > 1_577_836_800);
    }

    #[test]
    fn rfc3339_round_trip() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_790_000_000), "2026-09-21T14:13:20Z");
        assert_eq!(parse_rfc3339("2026-09-21T14:13:20Z"), Some(1_790_000_000));
        assert_eq!(parse_rfc3339("not a time"), None);
    }
}
