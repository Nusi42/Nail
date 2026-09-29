//! The engine of one IMAP account.
//!
//! ```text
//! loop:
//!   connect (status connecting → online, or offline / auth-error and a
//!            backoff that doubles up to EngineTiming::reconnect_max)
//!   session on the connection:
//!     replay queued writes, full pass (sync.rs)
//!     a MailboxWatcher (own connection, IDLE or NOOP polling) per folder
//!       whose sync policy asks for it (INBOX by default)
//!     until stopped or the connection breaks:
//!       watcher reports a change   → sync that folder, replay
//!       writes queued              → replay
//!       "sync now" / poll interval → replay, full pass
//! ```
//!
//! Errors that break the connection end the session and lead back to the
//! reconnect loop; everything the user did meanwhile stays queued.

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::ImapOptions;
use nail_api::{AccountConfig, AccountState, IncomingServer};
use nail_proto_imap::{ImapClient, MailboxWatcher, WatchConfig, WatchEvent};
use tokio::sync::{mpsc, watch};

use crate::engine::{Signals, pause, state_for_error, stopped};
use crate::error::{CoreError, Result};
use crate::replay::{ImapExecutor, replay_account};
use crate::service::Service;
use crate::sync::{ImapSettings, connect, report_pass, run_pass, sync_folder};

enum SessionEnd {
    Stopped,
    Failed(CoreError),
}

/// Runs the engine until `stop` is set.
pub(crate) async fn run(
    service: Service,
    account: AccountConfig,
    signals: Arc<Signals>,
    mut stop: watch::Receiver<bool>,
) {
    let IncomingServer::Imap { server, options } = &account.incoming else {
        return;
    };
    let settings = ImapSettings {
        config: crate::creds::imap_config(server, options),
        credentials: Arc::new(service.endpoint_credentials(&account.id, &server.auth)),
    };
    let timing = service.inner.timing;
    let poll = timing.poll_override.unwrap_or_else(|| {
        Duration::from_secs(u64::from(
            options
                .poll_interval_secs
                .unwrap_or(ImapOptions::DEFAULT_POLL_INTERVAL_SECS),
        ))
    });
    let mut backoff = timing.reconnect_min;
    loop {
        if *stop.borrow() {
            break;
        }
        service.set_status(&account.id, AccountState::Connecting, None);
        let connected = tokio::select! {
            result = connect(&settings) => result,
            () = stopped(&mut stop) => break,
        };
        let err = match connected {
            Ok(client) => {
                service.set_status(&account.id, AccountState::Online, None);
                let since = tokio::time::Instant::now();
                match session(
                    &service, &account, &settings, client, &signals, &mut stop, poll,
                )
                .await
                {
                    SessionEnd::Stopped => break,
                    SessionEnd::Failed(err) => {
                        // A session that failed right after connecting does
                        // not reset the backoff (no reconnect storm).
                        if since.elapsed() >= timing.healthy_after {
                            backoff = timing.reconnect_min;
                        }
                        err
                    }
                }
            }
            Err(err) => CoreError::from(err),
        };
        let code = err.code();
        tracing::info!(account = %account.id, %err, retry_in = ?backoff, "IMAP connection failed");
        service.set_status(
            &account.id,
            state_for_error(code),
            Some((code, err.to_string())),
        );
        service.broadcast(nail_api::Event::SyncFailed {
            account_id: account.id.clone(),
            code,
            message: err.to_string(),
            localized: None,
        });
        if !pause(backoff, &signals, &mut stop).await {
            break;
        }
        backoff = (backoff * 2).min(timing.reconnect_max);
    }
    tracing::info!(account = %account.id, "engine stopped");
}

/// Replays queued writes, then runs a full pass.
async fn full_cycle(
    service: &Service,
    account: &AccountConfig,
    client: &tokio::sync::Mutex<ImapClient>,
) -> Result<()> {
    replay(service, account, client).await?;
    let result = {
        let mut guard = client.lock().await;
        run_pass(&service.sync_ctx(), &account.id, &mut guard, &account.sync).await
    };
    report_pass(&service.sync_ctx(), &account.id, &result);
    result.map(drop)
}

async fn replay(
    service: &Service,
    account: &AccountConfig,
    client: &tokio::sync::Mutex<ImapClient>,
) -> Result<()> {
    let executor = ImapExecutor { service, client };
    replay_account(service, &account.id, &executor).await?;
    if client.lock().await.is_broken() {
        return Err(nail_proto_imap::ImapError::ConnectionLost.into());
    }
    Ok(())
}

/// Syncs one folder a watcher reported (by path), then replays.
async fn folder_changed(
    service: &Service,
    account: &AccountConfig,
    client: &tokio::sync::Mutex<ImapClient>,
    path: &str,
) -> Result<()> {
    let found = {
        let account = account.id.clone();
        let path = path.to_owned();
        service
            .store()
            .call(move |s| s.find_folder_by_path(&account, &path))
            .await?
    };
    match found {
        Some(folder) => {
            let mut guard = client.lock().await;
            sync_folder(&service.sync_ctx(), &mut guard, &folder).await?;
        }
        None => return full_cycle(service, account, client).await,
    }
    replay(service, account, client).await
}

async fn session(
    service: &Service,
    account: &AccountConfig,
    settings: &ImapSettings,
    client: ImapClient,
    signals: &Signals,
    stop: &mut watch::Receiver<bool>,
    poll: Duration,
) -> SessionEnd {
    let client = tokio::sync::Mutex::new(client);
    if let Err(err) = full_cycle(service, account, &client).await {
        return SessionEnd::Failed(err);
    }
    let (events_tx, mut events) = mpsc::channel(64);
    let watchers = match WatcherSet::start(service, account, settings, &events_tx).await {
        Ok(watchers) => watchers,
        Err(err) => return SessionEnd::Failed(err),
    };
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + poll, poll);
    let end = loop {
        let step = tokio::select! {
            () = stopped(stop) => break SessionEnd::Stopped,
            Some((path, event)) = events.recv() => match event {
                WatchEvent::Connected { .. } | WatchEvent::Changed(_) => {
                    folder_changed(service, account, &client, &path).await
                }
                WatchEvent::Disconnected { error, .. } => {
                    // The server may be gone: check the main connection so
                    // the account shows offline (and reconnects) at once.
                    tracing::debug!(account = %account.id, %path, %error, "watcher disconnected");
                    client.lock().await.noop().await.map_err(CoreError::from)
                }
            },
            () = signals.replay.notified() => replay(service, account, &client).await,
            () = signals.sync.notified() => full_cycle(service, account, &client).await,
            _ = timer.tick() => full_cycle(service, account, &client).await,
        };
        if let Err(err) = step {
            let broken = client.lock().await.is_broken();
            if broken
                || err.is_transient()
                || !matches!(err, CoreError::Api(_) | CoreError::Imap(_))
            {
                break SessionEnd::Failed(err);
            }
            tracing::warn!(account = %account.id, %err, "sync step failed; continuing");
        }
    };
    watchers.stop().await;
    if matches!(end, SessionEnd::Stopped) {
        let client = client.into_inner();
        let _ = tokio::time::timeout(Duration::from_secs(2), client.logout()).await;
    }
    end
}

/// The watchers of a session.
struct WatcherSet {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    stops: Vec<tokio::sync::oneshot::Sender<()>>,
}

impl WatcherSet {
    async fn start(
        service: &Service,
        account: &AccountConfig,
        settings: &ImapSettings,
        events: &mpsc::Sender<(String, WatchEvent)>,
    ) -> Result<Self> {
        let folders = {
            let id = account.id.clone();
            service.store().call(move |s| s.list_folders(&id)).await?
        };
        let timing = service.inner.timing;
        let mut set = Self {
            tasks: Vec::new(),
            stops: Vec::new(),
        };
        for folder in folders {
            let policy = account.sync.effective(&folder.path, folder.role);
            if !policy.enabled || !policy.idle {
                continue;
            }
            let config = WatchConfig::new(folder.path.clone())
                .with_reconnect(timing.reconnect_min, timing.reconnect_max)
                .with_healthy_after(timing.healthy_after);
            let mut watcher = MailboxWatcher::spawn(
                settings.config.clone(),
                Arc::clone(&settings.credentials),
                config,
            );
            let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
            let path = folder.path.clone();
            let out = events.clone();
            set.tasks.push(tokio::spawn(async move {
                loop {
                    tokio::select! {
                        event = watcher.next_event() => {
                            let Some(event) = event else { break };
                            if out.send((path.clone(), event)).await.is_err() {
                                break;
                            }
                        }
                        _ = &mut stop_rx => break,
                    }
                }
                watcher.stop().await;
            }));
            set.stops.push(stop_tx);
        }
        Ok(set)
    }

    async fn stop(self) {
        for stop in self.stops {
            let _ = stop.send(());
        }
        for task in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
        }
    }
}
