//! Adaptive polling (Graph change notifications need a public webhook,
//! which a desktop client does not have; plan M3 scope decisions).
//!
//! * [`PollTimer`] wraps the contract's [`AdaptivePoll`] with the
//!   account's `poll-interval` / `max-poll-interval`
//!   ([`ExchangeOptions::poll_range`]): a round with changes (or a resync)
//!   polls again soon, a quiet round backs off, a failed one waits for its
//!   `Retry-After`, and a folder with more pages waiting continues at once.
//! * [`FolderPoll`] spreads the folders over rounds: the Inbox (and every
//!   folder marked as priority) is asked every round, a folder that stayed
//!   quiet is asked every 2nd, 4th … up to every [`FolderPoll::MAX_SKIP`]th
//!   round, and a change brings it back to every round. That keeps the
//!   request count of large mailboxes well below Graph's per-mailbox
//!   limits.

use std::collections::BTreeMap;
use std::time::Duration;

use nail_api::engine::{AdaptivePoll, DeltaOutcome, EngineError};
use nail_api::exchange::ExchangeOptions;

/// When to poll an account next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollTimer {
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

impl PollTimer {
    /// A timer for an account's options.
    pub fn new(options: &ExchangeOptions) -> Self {
        let (min, max) = options.poll_range();
        Self {
            poll: AdaptivePoll::new(min, max),
            continue_now: false,
            round_changes: false,
            round_more: false,
            round_error: None,
        }
    }

    /// Records the outcome of one folder's (or the hierarchy's) delta call
    /// in the current round.
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
            let mut err = EngineError::temporary(nail_api::ErrorCode::Network, "poll failed");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct FolderState {
    quiet_rounds: u32,
    skipped: u32,
    priority: bool,
}

/// Which folders to ask in a round (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderPoll {
    folders: BTreeMap<String, FolderState>,
}

impl FolderPoll {
    /// A quiet folder is asked at least every this many rounds.
    pub const MAX_SKIP: u32 = 8;

    /// An empty schedule.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a folder (or changes its priority); a priority folder (the
    /// Inbox) is asked every round.
    pub fn track(&mut self, folder: &str, priority: bool) {
        self.folders.entry(folder.to_owned()).or_default().priority = priority;
    }

    /// Forgets a folder.
    pub fn untrack(&mut self, folder: &str) {
        self.folders.remove(folder);
    }

    /// Asks every folder in the next round (the user pressed "Sync").
    pub fn wake_all(&mut self) {
        for state in self.folders.values_mut() {
            state.quiet_rounds = 0;
            state.skipped = u32::MAX / 2;
        }
    }

    /// The folders to ask this round (and counts the round for the rest).
    pub fn due(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for (folder, state) in &mut self.folders {
            let every = 1u32
                .checked_shl(state.quiet_rounds.min(16))
                .unwrap_or(Self::MAX_SKIP)
                .min(Self::MAX_SKIP);
            if state.priority || state.skipped + 1 >= every {
                state.skipped = 0;
                out.push(folder.clone());
            } else {
                state.skipped += 1;
            }
        }
        out
    }

    /// Records whether a folder asked this round had changes.
    pub fn observe(&mut self, folder: &str, changed: bool) {
        if let Some(state) = self.folders.get_mut(folder) {
            if changed {
                state.quiet_rounds = 0;
            } else {
                state.quiet_rounds = state.quiet_rounds.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;
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
        let mut timer = PollTimer::new(&ExchangeOptions::default());
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
        let custom = ExchangeOptions {
            poll_interval_secs: Some(5),
            max_poll_interval_secs: Some(20),
            ..ExchangeOptions::default()
        };
        assert_eq!(
            PollTimer::new(&custom).next_delay(),
            Duration::from_secs(10),
            "at least the minimum interval"
        );
    }

    #[test]
    fn quiet_folders_are_asked_less_often() {
        let mut poll = FolderPoll::new();
        poll.track("inbox", true);
        poll.track("archive", false);
        assert_eq!(poll.due(), ["archive", "inbox"]);
        let mut archive_rounds = Vec::new();
        for round in 0..40 {
            let due = poll.due();
            assert!(due.contains(&"inbox".to_owned()));
            if due.contains(&"archive".to_owned()) {
                archive_rounds.push(round);
                poll.observe("archive", false);
            }
            poll.observe("inbox", false);
        }
        // Every round, every 2nd, every 4th, then every 8th round.
        assert_eq!(archive_rounds[..6], [0, 2, 6, 14, 22, 30]);
        assert!(
            archive_rounds
                .windows(2)
                .all(|w| w[1] - w[0] <= FolderPoll::MAX_SKIP)
        );
        poll.observe("archive", true);
        let next: Vec<bool> = (0..2)
            .map(|_| poll.due().contains(&"archive".to_owned()))
            .collect();
        assert!(next.contains(&true));
        poll.wake_all();
        assert_eq!(poll.due().len(), 2);
        poll.untrack("archive");
        assert_eq!(poll.due(), ["inbox"]);
        poll.observe("unknown", true);
    }
}
