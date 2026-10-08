//! Adaptive history polling (no Cloud Pub/Sub push on a desktop client;
//! plan M3 scope decisions). [`PollTimer`] wraps the contract's
//! [`AdaptivePoll`](nail_api::engine::AdaptivePoll) with the account's `poll-interval` /
//! `max-poll-interval` and reads the outcome of each
//! [`nail_api::engine::MailApiEngine::changes`] call: changes or a resync
//! → poll soon, nothing → back off, an error → its `Retry-After` or back
//! off, more history waiting → continue at once.

use nail_api::engine::{DeltaOutcome, EngineError};
use nail_api::gmail::GmailOptions;
pub use nail_net::poll::RoundTimer;

nail_net::options_timer!(
    /// When to poll a Gmail account next: [`RoundTimer`] with one `changes`
    /// call per round.
    PollTimer,
    GmailOptions
);

impl PollTimer {
    /// Records the outcome of a `changes` call (a round of its own).
    pub fn observe(&mut self, outcome: &Result<DeltaOutcome, EngineError>) {
        self.0.observe(outcome);
        self.0.end_round();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use nail_api::ErrorCode;
    use nail_api::engine::SyncDelta;

    fn delta(changes: usize, more: bool) -> DeltaOutcome {
        DeltaOutcome::Delta(SyncDelta {
            cursor: "5".into(),
            removed: (0..changes).map(|i| i.to_string()).collect(),
            more,
            ..SyncDelta::default()
        })
    }

    #[test]
    fn backs_off_when_quiet_and_resets_on_changes() {
        let mut timer = PollTimer::new(&GmailOptions::default());
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        timer.observe(&Ok(delta(0, false)));
        assert_eq!(timer.next_delay(), Duration::from_mins(1));
        for _ in 0..10 {
            timer.observe(&Ok(delta(0, false)));
        }
        assert_eq!(timer.next_delay(), Duration::from_mins(5));
        timer.observe(&Ok(delta(2, false)));
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
        timer.observe(&Ok(delta(0, false)));
        timer.user_activity();
        assert_eq!(timer.next_delay(), Duration::from_secs(30));
    }

    #[test]
    fn continues_at_once_while_history_remains() {
        let mut timer = PollTimer::new(&GmailOptions {
            poll_interval_secs: Some(20),
            max_poll_interval_secs: Some(40),
        });
        timer.observe(&Ok(delta(1, true)));
        assert_eq!(timer.next_delay(), Duration::ZERO);
        timer.observe(&Ok(delta(0, true)));
        assert_eq!(timer.next_delay(), Duration::ZERO);
        timer.observe(&Ok(delta(0, false)));
        assert_eq!(timer.next_delay(), Duration::from_secs(40));
        timer.observe(&Ok(DeltaOutcome::CursorExpired));
        assert_eq!(timer.next_delay(), Duration::from_secs(20));
    }

    #[test]
    fn honours_retry_after_once() {
        let mut timer = PollTimer::new(&GmailOptions::default());
        timer.observe(&Err(EngineError::rate_limited("quota", 900)));
        assert_eq!(timer.next_delay(), Duration::from_mins(15));
        timer.observe(&Ok(delta(0, false)));
        assert_eq!(timer.next_delay(), Duration::from_mins(2));
        timer.observe(&Err(EngineError::temporary(ErrorCode::Network, "down")));
        assert_eq!(timer.next_delay(), Duration::from_mins(4));
        assert_eq!(timer.poll().next_delay_secs(), 240);
    }
}
