//! Clock injection: the scheduler and the notification batcher read the
//! time only through [`Clock`], so tests (and the Playwright scenarios,
//! through the core's test controls) decide what "now" is.

use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Duration, Utc};

/// A source of the current time.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> DateTime<Utc>;

    /// Now in Unix seconds.
    fn unix(&self) -> i64 {
        self.now().timestamp()
    }
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<DateTime<Utc>>,
}

impl ManualClock {
    /// A clock standing at `now`.
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// A clock standing at a Unix time (seconds; out-of-range values
    /// stand at the epoch).
    pub fn at_unix(secs: i64) -> Self {
        Self::new(DateTime::from_timestamp(secs, 0).unwrap_or_default())
    }

    /// Sets the time (it may go backwards: tests of clock changes).
    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }

    /// Moves the time forward.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> DateTime<Utc> {
        (**self).now()
    }
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now(&self) -> DateTime<Utc> {
        (**self).now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = Arc::new(ManualClock::at_unix(1_000));
        assert_eq!(clock.unix(), 1_000);
        clock.advance(Duration::seconds(90));
        assert_eq!(clock.unix(), 1_090);
        clock.set(DateTime::from_timestamp(5, 0).unwrap());
        assert_eq!(Clock::unix(&&*clock), 5);
        let system = SystemClock;
        assert!(system.unix() > 1_700_000_000);
    }
}
