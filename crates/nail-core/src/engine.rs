//! Per-account engines: one background task per enabled account in
//! `accounts.kdl` (IMAP: `imap_engine.rs`, POP3: `pop3_engine.rs`), their
//! connection states (`AccountStatus` events) and the signals that wake
//! them (queued writes, "sync now").
//!
//! [`Service::reconcile_engines`] starts, restarts (changed configuration)
//! and stops engines to match the configuration; it runs at start-up and
//! after every account change.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nail_api::{
    AccountConfig, AccountId, AccountState, AccountStatus, ErrorCode, Event, IncomingServer,
};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use crate::service::{Service, lock};

/// Wakes an engine.
#[derive(Debug, Default)]
pub(crate) struct Signals {
    /// Writes were queued: replay them.
    pub replay: Notify,
    /// The user asked to sync now.
    pub sync: Notify,
}

struct Running {
    config: AccountConfig,
    signals: Arc<Signals>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// The running engines and every account's connection state.
#[derive(Default)]
pub(crate) struct Engines {
    running: Mutex<HashMap<AccountId, Running>>,
    statuses: Mutex<HashMap<AccountId, AccountStatus>>,
}

impl std::fmt::Debug for Engines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engines")
            .field("running", &lock(&self.running).keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Engines {
    /// Accounts with a running engine, sorted.
    pub(crate) fn accounts(&self) -> Vec<AccountId> {
        let mut ids: Vec<_> = lock(&self.running).keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Asks an account's engine for a full sync; `false` without an engine.
    pub(crate) fn request_sync(&self, account: &AccountId) -> bool {
        lock(&self.running).get(account).is_some_and(|r| {
            r.signals.sync.notify_one();
            true
        })
    }

    /// Asks an account's engine to replay queued writes; `false` without
    /// an engine.
    pub(crate) fn request_replay(&self, account: &AccountId) -> bool {
        lock(&self.running).get(account).is_some_and(|r| {
            r.signals.replay.notify_one();
            true
        })
    }

    /// Stops one account's engine (if it runs) and waits for it.
    pub(crate) async fn stop_account(&self, account: &AccountId) {
        let engine = lock(&self.running).remove(account);
        if let Some(engine) = engine {
            stop(engine).await;
        }
    }

    /// Stops every engine and waits (briefly) for them to log out.
    pub(crate) async fn stop_all(&self) {
        let running: Vec<Running> = lock(&self.running).drain().map(|(_, r)| r).collect();
        for engine in running {
            stop(engine).await;
        }
    }
}

/// Stops one engine: asks it to finish, gives it a moment to log out, then
/// aborts it.
async fn stop(engine: Running) {
    let _ = engine.stop.send(true);
    let mut task = engine.task;
    if tokio::time::timeout(Duration::from_secs(3), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}

/// Now as RFC 3339 (UTC).
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Now as Unix seconds.
pub(crate) fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The error class of a failed connection as an account state.
pub(crate) fn state_for_error(code: ErrorCode) -> AccountState {
    if code == ErrorCode::AuthFailed {
        AccountState::AuthError
    } else {
        AccountState::Offline
    }
}

/// Waits until `stop` is set (or its sender is gone).
pub(crate) async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// Sleeps for `delay`, or less when the user asks to sync now or the
/// engine is stopped. Returns `false` when stopped.
pub(crate) async fn pause(
    delay: Duration,
    signals: &Signals,
    stop: &mut watch::Receiver<bool>,
) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => true,
        () = signals.sync.notified() => true,
        () = signals.replay.notified() => true,
        () = stopped(stop) => false,
    }
}

impl Service {
    /// Records an account's connection state; broadcasts
    /// [`Event::AccountStatus`] when it changed.
    pub(crate) fn set_status(
        &self,
        account: &AccountId,
        state: AccountState,
        error: Option<(ErrorCode, String, Option<nail_api::LocalizedMessage>)>,
    ) {
        let (error, message, localized) = match error {
            Some((code, message, localized)) => (Some(code), Some(message), localized),
            None => (None, None, None),
        };
        let status = {
            let mut statuses = lock(&self.inner.engines.statuses);
            if let Some(current) = statuses.get(account)
                && current.state == state
                && current.error == error
            {
                return;
            }
            let status = AccountStatus {
                account_id: account.clone(),
                state,
                since: now_rfc3339(),
                error,
                message,
                localized,
            };
            statuses.insert(account.clone(), status.clone());
            status
        };
        tracing::info!(account = %status.account_id, state = status.state.as_str(), "account status");
        self.broadcast(Event::AccountStatus { status });
    }

    /// `ListAccountStatuses`: one entry per configured account.
    pub(crate) fn account_statuses(&self) -> Vec<AccountStatus> {
        let config = self.inner.accounts.config();
        let statuses = lock(&self.inner.engines.statuses);
        config
            .accounts
            .iter()
            .map(|account| {
                statuses
                    .get(&account.id)
                    .cloned()
                    .unwrap_or_else(|| AccountStatus {
                        account_id: account.id.clone(),
                        state: if account.enabled {
                            AccountState::Offline
                        } else {
                            AccountState::Disabled
                        },
                        since: now_rfc3339(),
                        error: None,
                        message: None,
                        localized: None,
                    })
            })
            .collect()
    }

    /// Starts, restarts and stops engines to match `accounts.kdl`. Needs a
    /// tokio runtime.
    pub(crate) fn reconcile_engines(&self) {
        let config = self.inner.accounts.config();
        let mut to_stop = Vec::new();
        {
            let mut running = lock(&self.inner.engines.running);
            let wanted: HashMap<&AccountId, &AccountConfig> = config
                .accounts
                .iter()
                .filter(|a| a.enabled)
                .map(|a| (&a.id, a))
                .collect();
            let current: Vec<AccountId> = running.keys().cloned().collect();
            for id in current {
                let keep = wanted.get(&id).is_some_and(|c| running[&id].config == **c);
                if !keep && let Some(engine) = running.remove(&id) {
                    to_stop.push(engine);
                }
            }
            for account in config.accounts.iter().filter(|a| a.enabled) {
                if running.contains_key(&account.id) {
                    continue;
                }
                let signals = Arc::new(Signals::default());
                let (stop_tx, stop_rx) = watch::channel(false);
                let service = self.clone();
                let config = account.clone();
                let task_signals = Arc::clone(&signals);
                let task = match account.incoming {
                    IncomingServer::Imap { .. } => tokio::spawn(crate::imap_engine::run(
                        service,
                        config,
                        task_signals,
                        stop_rx,
                    )),
                    IncomingServer::Pop3 { .. } => tokio::spawn(crate::pop3_engine::run(
                        service,
                        config,
                        task_signals,
                        stop_rx,
                    )),
                };
                tracing::info!(account = %account.id, "engine started");
                running.insert(
                    account.id.clone(),
                    Running {
                        config: account.clone(),
                        signals,
                        stop: stop_tx,
                        task,
                    },
                );
            }
        }
        for account in config.accounts.iter().filter(|a| !a.enabled) {
            self.set_status(&account.id, AccountState::Disabled, None);
        }
        {
            let mut statuses = lock(&self.inner.engines.statuses);
            statuses.retain(|id, _| config.account(id).is_some());
        }
        if !to_stop.is_empty() {
            tokio::spawn(async move {
                for engine in to_stop {
                    stop(engine).await;
                }
            });
        }
    }

    /// Replays an account's queued writes: through its engine, with a
    /// fresh connection for a registered account, or locally for an
    /// account without a server.
    pub(crate) fn request_replay(&self, account: &AccountId) {
        if self.inner.engines.request_replay(account) {
            return;
        }
        let service = self.clone();
        let account = account.clone();
        if let Some(settings) = self.inner.manual.settings(&account) {
            tokio::spawn(async move {
                let settings = crate::sync::ImapSettings::from(settings);
                crate::replay::replay_with_connection(&service, &account, &settings).await;
            });
            return;
        }
        let configured = self.inner.accounts.config().account(&account).is_some();
        if !configured {
            tokio::spawn(async move {
                crate::replay::replay_locally(&service, &account).await;
            });
        }
        // A configured account without a running engine (disabled) keeps
        // its writes queued until it is enabled again.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_states() {
        assert_eq!(
            state_for_error(ErrorCode::AuthFailed),
            AccountState::AuthError
        );
        assert_eq!(state_for_error(ErrorCode::Network), AccountState::Offline);
        assert!(now_rfc3339().ends_with('Z'));
        assert!(unix_now() > 1_700_000_000);
    }

    #[tokio::test]
    async fn pause_ends_early_on_signals() {
        let signals = Signals::default();
        let (tx, mut rx) = watch::channel(false);
        signals.sync.notify_one();
        assert!(pause(Duration::from_secs(60), &signals, &mut rx).await);
        tx.send(true).unwrap();
        assert!(!pause(Duration::from_secs(60), &signals, &mut rx).await);
    }
}
