//! Adaptive polling of the API engines (Graph and Gmail have no push a
//! desktop client can receive; plan M3 scope decisions): [`RoundTimer`]
//! wraps the contract's [`AdaptivePoll`] with an account's poll range and
//! reads the delta calls of each round — changes (or a resync) poll again
//! soon, a quiet round backs off, a failed one waits for its longest
//! `Retry-After`, and more pages waiting continue at once.

use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::engine::{AdaptivePoll, DeltaOutcome, EngineError};

/// When to poll an account next, from the delta calls of each round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundTimer {
    poll: AdaptivePoll,
    continue_now: bool,
    round_changes: bool,
    round_more: bool,
    round_error: Option<RoundFailure>,
}

/// A failed delta call in the current round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RoundFailure {
    /// The longest `Retry-After` (seconds) any failure of the round asked for.
    retry_after: Option<u32>,
}

impl RoundTimer {
    /// A timer for a poll range (minimum and maximum interval, seconds).
    pub fn new((min, max): (u32, u32)) -> Self {
        Self {
            poll: AdaptivePoll::new(min, max),
            continue_now: false,
            round_changes: false,
            round_more: false,
            round_error: None,
        }
    }

    /// Records the outcome of one delta call in the current round.
    pub fn observe(&mut self, outcome: &Result<DeltaOutcome, EngineError>) {
        match outcome {
            Ok(DeltaOutcome::CursorExpired) => self.round_changes = true,
            Ok(DeltaOutcome::Delta(delta)) => {
                self.round_more |= delta.more;
                if !(delta.added.is_empty() && delta.changed.is_empty() && delta.removed.is_empty())
                {
                    self.round_changes = true;
                }
            }
            Err(err) => {
                let previous = self.round_error.and_then(|f| f.retry_after);
                self.round_error = Some(RoundFailure {
                    retry_after: previous.max(err.retry_after_secs),
                });
            }
        }
    }

    /// Records changes found outside a delta call (hierarchy, PIM).
    pub fn observe_changes(&mut self, changed: bool) {
        self.round_changes |= changed;
    }

    /// Ends the round: decides the next delay from what it saw.
    pub fn end_round(&mut self) {
        self.continue_now = self.round_more;
        if let Some(failure) = self.round_error {
            let mut err = EngineError::temporary(ErrorCode::Network, "poll failed");
            err.retry_after_secs = failure.retry_after;
            self.poll.failed(&err);
        } else if self.round_changes {
            self.poll.activity();
        } else if !self.round_more {
            self.poll.quiet();
        }
        self.round_changes = false;
        self.round_more = false;
        self.round_error = None;
    }

    /// The user did something (opened the account, pressed "Sync", wrote):
    /// poll soon.
    pub fn user_activity(&mut self) {
        self.poll.activity();
    }

    /// The wait before the next round (zero while pages are still being
    /// read).
    pub fn next_delay(&self) -> Duration {
        if self.continue_now {
            Duration::ZERO
        } else {
            Duration::from_secs(u64::from(self.poll.next_delay_secs()))
        }
    }

    /// The underlying interval state.
    pub fn poll(&self) -> AdaptivePoll {
        self.poll
    }
}

/// Defines `$name`, an engine's poll timer: a [`RoundTimer`] (reached
/// through `Deref`) made from an account's options type `$options` with
/// `poll_range()`.
#[macro_export]
macro_rules! options_timer {
    ($(#[$meta:meta])* $name:ident, $options:ty) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name($crate::poll::RoundTimer);

        impl $name {
            /// A timer for an account's options.
            pub fn new(options: &$options) -> Self {
                Self($crate::poll::RoundTimer::new(options.poll_range()))
            }
        }

        impl ::core::ops::Deref for $name {
            type Target = $crate::poll::RoundTimer;

            fn deref(&self) -> &$crate::poll::RoundTimer {
                &self.0
            }
        }

        impl ::core::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut $crate::poll::RoundTimer {
                &mut self.0
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::SyncDelta;

    fn delta(changes: usize, more: bool) -> DeltaOutcome {
        DeltaOutcome::Delta(SyncDelta {
            cursor: "c".into(),
            removed: (0..changes).map(|i| format!("m{i}")).collect(),
            more,
            ..SyncDelta::default()
        })
    }

    #[test]
    fn rounds_drive_the_interval() {
        let mut timer = RoundTimer::new((30, 300));
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        timer.observe(&Ok(delta(0, false)));
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::from_mins(1));
        timer.observe(&Ok(delta(0, false)));
        timer.observe(&Ok(delta(2, false)));
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        timer.observe(&Ok(delta(0, true)));
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::ZERO);
        timer.observe(&Ok(DeltaOutcome::CursorExpired));
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        timer.observe(&Err(EngineError::rate_limited("slow", 120)));
        timer.observe(&Err(EngineError::temporary(ErrorCode::Network, "down")));
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::from_mins(2));
        timer.end_round();
        assert_eq!(
            timer.next_delay(),
            Duration::from_mins(2),
            "backed off further"
        );
        timer.observe_changes(true);
        timer.end_round();
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        for _ in 0..10 {
            timer.end_round();
        }
        assert_eq!(timer.next_delay(), Duration::from_mins(5));
        timer.user_activity();
        assert_eq!(timer.poll().next_delay_secs(), 30);
    }

    struct Options {
        range: (u32, u32),
    }

    impl Options {
        fn poll_range(&self) -> (u32, u32) {
            self.range
        }
    }

    #[expect(
        unreachable_pub,
        reason = "the macro defines a public timer for the engine crates"
    )]
    mod defined {
        crate::options_timer!(
            /// A test timer.
            TestTimer,
            super::Options
        );
    }

    #[test]
    fn options_timers_use_the_poll_range() {
        let mut timer = defined::TestTimer::new(&Options { range: (5, 20) });
        assert_eq!(timer.next_delay(), Duration::from_secs(5));
        timer.end_round();
        assert_eq!(timer.poll().next_delay_secs(), 10);
    }
}
