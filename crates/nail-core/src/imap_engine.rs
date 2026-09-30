//! The engine of one IMAP account.
//!
//! ```text
//! loop:
//!   connect (status connecting → online, or offline / auth-error and a
//!            backoff that doubles up to EngineTiming::reconnect_max)
//!   session on the connection (plus a second one for replaying writes,
//!   opened lazily; M2c):
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
            Some((code, err.to_string(), err.localized())),
        );
        service.broadcast(nail_api::Event::SyncFailed {
            account_id: account.id.clone(),
            code,
            message: err.to_string(),
            localized: err.localized(),
        });
        if !pause(backoff, &signals, &mut stop).await {
            break;
        }
        backoff = (backoff * 2).min(timing.reconnect_max);
    }
    tracing::info!(account = %account.id, "engine stopped");
}

/// The connections of a session: the main one (sync passes) and the
/// replay connection for queued writes (M2c), so replaying never changes
/// the main connection's selected mailbox. When the replay connection
/// cannot be opened, writes replay on the main one.
struct Conns<'a> {
    main: tokio::sync::Mutex<ImapClient>,
    replay: std::sync::Mutex<Option<Arc<tokio::sync::Mutex<ImapClient>>>>,
    settings: &'a ImapSettings,
}

impl Conns<'_> {
    /// The replay connection, (re)opened when missing or broken.
    async fn replay_client(&self) -> Option<Arc<tokio::sync::Mutex<ImapClient>>> {
        let current = crate::service::lock(&self.replay).clone();
        if let Some(client) = current
            && !client.lock().await.is_broken()
        {
            return Some(client);
        }
        match connect(self.settings).await {
            Ok(client) => {
                let client = Arc::new(tokio::sync::Mutex::new(client));
                *crate::service::lock(&self.replay) = Some(Arc::clone(&client));
                Some(client)
            }
            Err(err) => {
                tracing::debug!(%err, "no replay connection; replaying on the main one");
                *crate::service::lock(&self.replay) = None;
                None
            }
        }
    }
}

/// Replays queued writes, then runs a full pass.
async fn full_cycle(service: &Service, account: &AccountConfig, conns: &Conns<'_>) -> Result<()> {
    replay(service, account, conns).await?;
    let client = &conns.main;
    let result = {
        let mut guard = client.lock().await;
        run_pass(&service.sync_ctx(), &account.id, &mut guard, &account.sync).await
    };
    report_pass(&service.sync_ctx(), &account.id, &result);
    // The folders are known now: the Sieve script can name them (uploaded
    // only when its inputs changed).
    if result.is_ok() {
        service.queue_sieve(Some(&account.id));
    }
    result.map(drop)
}

async fn replay(service: &Service, account: &AccountConfig, conns: &Conns<'_>) -> Result<()> {
    let pending = {
        let id = account.id.clone();
        service
            .store()
            .call(move |s| Ok(s.accounts_with_pending_ops()?.contains(&id)))
            .await?
    };
    if !pending {
        return Ok(());
    }
    if let Some(client) = conns.replay_client().await {
        let executor = ImapExecutor {
            service,
            client: &client,
        };
        replay_account(service, &account.id, &executor).await?;
        if client.lock().await.is_broken() {
            // Deferred writes retry after the session reconnects (as when
            // they replayed on the main connection).
            return Err(nail_proto_imap::ImapError::ConnectionLost.into());
        }
        return Ok(());
    }
    let client = &conns.main;
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
    conns: &Conns<'_>,
    path: &str,
) -> Result<()> {
    let client = &conns.main;
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
            let bodies = account.sync.effective(&folder.path, folder.role).bodies;
            let mut guard = client.lock().await;
            sync_folder(&service.sync_ctx(), &mut guard, &folder, &bodies).await?;
        }
        None => return full_cycle(service, account, conns).await,
    }
    replay(service, account, conns).await
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
    let conns = Conns {
        main: tokio::sync::Mutex::new(client),
        replay: std::sync::Mutex::new(None),
        settings,
    };
    let end = session_loop(service, account, settings, &conns, signals, stop, poll).await;
    conns.close_and(end).await
}

impl Conns<'_> {
    /// Closes the replay connection; a stopped session logs out of the
    /// main one too.
    async fn close_and(self, end: SessionEnd) -> SessionEnd {
        let main = {
            let replay = crate::service::lock(&self.replay).take();
            if let Some(replay) = replay
                && let Ok(client) = Arc::try_unwrap(replay)
            {
                let _ = tokio::time::timeout(Duration::from_secs(2), client.into_inner().logout())
                    .await;
            }
            self.main
        };
        if matches!(end, SessionEnd::Stopped) {
            let client = main.into_inner();
            let _ = tokio::time::timeout(Duration::from_secs(2), client.logout()).await;
        }
        end
    }
}

async fn session_loop(
    service: &Service,
    account: &AccountConfig,
    settings: &ImapSettings,
    conns: &Conns<'_>,
    signals: &Signals,
    stop: &mut watch::Receiver<bool>,
    poll: Duration,
) -> SessionEnd {
    let client = &conns.main;
    if let Err(err) = full_cycle(service, account, conns).await {
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
                    folder_changed(service, account, conns, &path).await
                }
                WatchEvent::Disconnected { error, .. } => {
                    // The server may be gone: check the main connection so
                    // the account shows offline (and reconnects) at once.
                    tracing::debug!(account = %account.id, %path, %error, "watcher disconnected");
                    client.lock().await.noop().await.map_err(CoreError::from)
                }
            },
            () = signals.replay.notified() => replay(service, account, conns).await,
            () = signals.sync.notified() => full_cycle(service, account, conns).await,
            _ = timer.tick() => full_cycle(service, account, conns).await,
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
