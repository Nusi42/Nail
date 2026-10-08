//! The time the triage logic sees (snooze wake times, quiet hours,
//! notification batching; ADR 0048). Normal runs use the system clock;
//! `--seed demo-multi` uses an [`AdjustableClock`] that the end-to-end
//! tests move through the test controls (`clock-set`, `clock-advance`).

use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};

/// The system clock plus an offset that can be changed at run time.
/// Time keeps running; setting it jumps (forwards or backwards).
#[derive(Debug, Default)]
pub struct AdjustableClock {
    offset_secs: AtomicI64,
}

impl AdjustableClock {
    /// A clock showing the system time.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock by `secs` (negative: back).
    pub fn advance(&self, secs: i64) {
        self.offset_secs.fetch_add(secs, Ordering::SeqCst);
    }

    /// Sets the clock to `unix` seconds now.
    pub fn set(&self, unix: i64) {
        let real = Utc::now().timestamp();
        self.offset_secs.store(unix - real, Ordering::SeqCst);
    }

    /// The current offset from the system clock in seconds.
    pub fn offset(&self) -> i64 {
        self.offset_secs.load(Ordering::SeqCst)
    }
}

impl nail_triage::Clock for AdjustableClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(self.offset())
    }
}

/// How long a timer task sleeps until `next` (Unix seconds; `None` =
/// nothing due): at least 50 ms, at most `cap`.
pub(crate) fn timer_wait(
    next: Option<i64>,
    now: i64,
    cap: std::time::Duration,
) -> std::time::Duration {
    next.map_or(cap, |at| {
        let secs = (at - now).max(0);
        std::time::Duration::from_secs(u64::try_from(secs).unwrap_or(0))
            .clamp(std::time::Duration::from_millis(50), cap)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_triage::Clock;

    #[test]
    fn moves_and_jumps() {
        let clock = AdjustableClock::new();
        let real = Utc::now().timestamp();
        assert!((clock.unix() - real).abs() <= 1);
        clock.advance(3600);
        assert!((clock.unix() - real - 3600).abs() <= 1);
        clock.set(1_000_000_000);
        assert!((clock.unix() - 1_000_000_000).abs() <= 1);
        clock.advance(-10);
        assert!((clock.unix() - 999_999_990).abs() <= 1);
    }
}
