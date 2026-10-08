//! Adaptive polling (Graph change notifications need a public webhook,
//! which a desktop client does not have; plan M3 scope decisions).
//!
//! * [`PollTimer`] wraps the contract's [`AdaptivePoll`](nail_api::engine::AdaptivePoll) with the
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

use nail_api::exchange::ExchangeOptions;
pub use nail_net::poll::RoundTimer;

nail_net::options_timer!(
    /// When to poll an account next: [`RoundTimer`] over the account's
    /// options (one round = the folders asked together).
    PollTimer,
    ExchangeOptions
);

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
    use std::time::Duration;

    use super::*;

    #[test]
    fn timers_follow_the_account_options() {
        assert_eq!(
            PollTimer::new(&ExchangeOptions::default()).next_delay(),
            Duration::from_secs(30)
        );
        let custom = ExchangeOptions {
            poll_interval_secs: Some(5),
            max_poll_interval_secs: Some(20),
            ..ExchangeOptions::default()
        };
        let mut timer = PollTimer::new(&custom);
        assert_eq!(
            timer.next_delay(),
            Duration::from_secs(10),
            "at least the minimum interval"
        );
        timer.user_activity();
        timer.end_round();
        assert_eq!(timer.poll().next_delay_secs(), 20);
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
