//! Quick retries of a deferred replay pass (ADR 0245).
//!
//! A retryable failure (a timeout, a busy server) ends an account's replay
//! pass with its writes still queued. Without more, they waited for the
//! account's next poll or the next action. Now a deferred pass schedules a
//! quick retry: the waits double from [`crate::EngineTiming::replay_retry`]
//! (2 s by default) up to [`LONGEST_WAIT`], with a little jitter, or follow
//! the server's `Retry-After`; after [`QUICK_RETRIES`] retries the account
//! waits for its next poll again. A pass that defers nothing resets the
//! count. The values are fixed on purpose: there is no setting for them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nail_api::AccountId;
use nail_net::http::RetryPolicy;
use tokio::task::JoinHandle;

use crate::service::lock;

/// Quick retries after deferred passes before the account waits for its
/// next poll.
pub(crate) const QUICK_RETRIES: u32 = 5;

/// The longest wait between quick retries.
pub(crate) const LONGEST_WAIT: Duration = Duration::from_mins(1);

/// The longest server-requested wait (`Retry-After`) a quick retry
/// follows; after a longer one the account waits for its next poll.
pub(crate) const LONGEST_SERVER_WAIT: Duration = Duration::from_mins(5);

/// The waits between quick retries, starting at `first`.
pub(crate) fn policy(first: Duration) -> RetryPolicy {
    RetryPolicy {
        max_retries: QUICK_RETRIES,
        base_delay: first,
        max_delay: LONGEST_WAIT.max(first),
        max_inline_wait: LONGEST_SERVER_WAIT,
    }
}

/// The quick retries of every account.
#[derive(Default, Clone)]
pub(crate) struct ReplayRetries(Arc<RetriesInner>);

#[derive(Default)]
struct RetriesInner {
    accounts: Mutex<HashMap<AccountId, Retry>>,
    generation: AtomicU64,
}

/// One account's quick retries.
#[derive(Default)]
struct Retry {
    /// Quick retries scheduled since the last pass that deferred nothing.
    scheduled: u32,
    /// The retry waiting for its time, if any.
    waiting: Option<Waiting>,
}

/// A retry waiting for its time.
struct Waiting {
    generation: u64,
    task: JoinHandle<()>,
}

impl ReplayRetries {
    /// A pass deferred nothing: the next deferred pass starts with the
    /// shortest wait again. A waiting retry is dropped (nothing is left
    /// for it).
    pub(crate) fn passed(&self, account: &AccountId) {
        let retry = lock(&self.0.accounts).remove(account);
        if let Some(waiting) = retry.and_then(|r| r.waiting) {
            waiting.task.abort();
        }
    }

    /// A pass deferred writes: runs `trigger` after the next wait of
    /// `policy` (or the server's `retry_after_secs`) and answers that wait.
    /// `None` when a retry is already waiting, the quick retries are used
    /// up, or the server asked for a longer wait than a quick retry
    /// follows: the account then waits for its next poll.
    pub(crate) fn deferred<F>(
        &self,
        account: &AccountId,
        policy: RetryPolicy,
        retry_after_secs: Option<u32>,
        trigger: F,
    ) -> Option<Duration>
    where
        F: FnOnce() + Send + 'static,
    {
        // Held while spawning, so the task cannot claim itself before it
        // is registered.
        let mut accounts = lock(&self.0.accounts);
        let retry = accounts.entry(account.clone()).or_default();
        if retry.waiting.is_some() || retry.scheduled >= policy.max_retries {
            return None;
        }
        let wait = policy.wait(retry.scheduled, retry_after_secs)?;
        retry.scheduled += 1;
        let generation = self.0.generation.fetch_add(1, Ordering::Relaxed);
        let this = self.clone();
        let id = account.clone();
        // The wait starts now, not when the task first runs.
        let sleep = tokio::time::sleep(wait);
        let task = tokio::spawn(async move {
            sleep.await;
            if this.claim(&id, generation) {
                trigger();
            }
        });
        retry.waiting = Some(Waiting { generation, task });
        Some(wait)
    }

    /// Whether the retry `generation` of `account` is still the waiting one
    /// (then it is no longer waiting).
    fn claim(&self, account: &AccountId, generation: u64) -> bool {
        let mut accounts = lock(&self.0.accounts);
        let Some(retry) = accounts.get_mut(account) else {
            return false;
        };
        if retry.waiting.as_ref().map(|w| w.generation) != Some(generation) {
            return false;
        }
        retry.waiting = None;
        true
    }

    /// Drops every waiting retry (the service shuts down).
    pub(crate) fn stop_all(&self) {
        let accounts: Vec<Retry> = lock(&self.0.accounts).drain().map(|(_, r)| r).collect();
        for waiting in accounts.into_iter().filter_map(|r| r.waiting) {
            waiting.task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn account() -> AccountId {
        AccountId::new("acct-1")
    }

    /// A trigger that counts its runs.
    fn counter() -> (Arc<AtomicUsize>, impl Fn() -> Box<dyn FnOnce() + Send>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let make = {
            let runs = Arc::clone(&runs);
            move || -> Box<dyn FnOnce() + Send> {
                let runs = Arc::clone(&runs);
                Box::new(move || {
                    runs.fetch_add(1, Ordering::SeqCst);
                })
            }
        };
        (runs, make)
    }

    /// Lets the timer tasks run up to now.
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn waits_double_from_the_first_up_to_a_minute() {
        let policy = policy(Duration::from_secs(2));
        let waits: Vec<Duration> = (0..QUICK_RETRIES).map(|n| policy.backoff(n)).collect();
        for (n, wait) in waits.iter().enumerate() {
            let base = Duration::from_secs(2 << n).min(LONGEST_WAIT);
            assert!(*wait >= base, "retry {n}: {wait:?} < {base:?}");
            assert!(*wait <= (base + base / 4).min(LONGEST_WAIT.max(base)));
        }
        assert!(policy.backoff(10) <= LONGEST_WAIT);
    }

    #[test]
    fn a_server_wait_wins_up_to_the_limit() {
        let policy = policy(Duration::from_secs(2));
        assert_eq!(policy.wait(0, Some(30)), Some(Duration::from_secs(30)));
        let limit = u32::try_from(LONGEST_SERVER_WAIT.as_secs()).unwrap();
        assert_eq!(policy.wait(0, Some(limit + 1)), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_deferred_pass_runs_the_trigger_after_the_wait() {
        let retries = ReplayRetries::default();
        let (runs, trigger) = counter();
        let wait = retries
            .deferred(&account(), policy(Duration::from_secs(2)), None, trigger())
            .unwrap();
        assert!(wait >= Duration::from_secs(2));
        tokio::time::advance(wait.saturating_sub(Duration::from_millis(1))).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn one_retry_waits_at_a_time() {
        let retries = ReplayRetries::default();
        let (runs, trigger) = counter();
        let policy = policy(Duration::from_secs(2));
        assert!(
            retries
                .deferred(&account(), policy, None, trigger())
                .is_some()
        );
        assert!(
            retries
                .deferred(&account(), policy, None, trigger())
                .is_none()
        );
        // Another account has its own.
        let other = AccountId::new("acct-2");
        assert!(retries.deferred(&other, policy, None, trigger()).is_some());
        tokio::time::advance(LONGEST_WAIT).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn the_quick_retries_run_out_and_a_clean_pass_resets_them() {
        let retries = ReplayRetries::default();
        let (runs, trigger) = counter();
        let policy = policy(Duration::from_secs(2));
        let mut waits = Vec::new();
        for _ in 0..QUICK_RETRIES {
            let wait = retries
                .deferred(&account(), policy, None, trigger())
                .unwrap();
            waits.push(wait);
            tokio::time::advance(wait).await;
            settle().await;
        }
        assert_eq!(runs.load(Ordering::SeqCst), 5);
        assert!(
            waits
                .windows(2)
                .all(|w| w[0] < w[1] || w[1] == LONGEST_WAIT)
        );
        // Used up: the next deferred pass waits for the poll.
        assert!(
            retries
                .deferred(&account(), policy, None, trigger())
                .is_none()
        );
        retries.passed(&account());
        let wait = retries
            .deferred(&account(), policy, None, trigger())
            .unwrap();
        assert!(wait < Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_clean_pass_and_shutdown_drop_a_waiting_retry() {
        let retries = ReplayRetries::default();
        let (runs, trigger) = counter();
        let policy = policy(Duration::from_secs(2));
        retries.deferred(&account(), policy, None, trigger());
        retries.passed(&account());
        retries.deferred(&AccountId::new("acct-2"), policy, None, trigger());
        retries.stop_all();
        tokio::time::advance(LONGEST_WAIT).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_server_wait_is_followed() {
        let retries = ReplayRetries::default();
        let (runs, trigger) = counter();
        let policy = policy(Duration::from_secs(2));
        let wait = retries.deferred(&account(), policy, Some(45), trigger());
        assert_eq!(wait, Some(Duration::from_secs(45)));
        tokio::time::advance(Duration::from_secs(44)).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        tokio::time::advance(Duration::from_secs(1)).await;
        settle().await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        // Longer than a quick retry follows: wait for the poll.
        retries.passed(&account());
        assert!(
            retries
                .deferred(&account(), policy, Some(3600), trigger())
                .is_none()
        );
    }
}
