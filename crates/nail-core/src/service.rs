//! The [`Service`]: one per running core, shared by every transport.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use nail_api::{
    Account, AccountId, ApiError, ErrorCode, Event, MessageBody, MessageId, PROTOCOL_VERSION,
    Request, Response,
};
use nail_config::{ConfigManager, ConfigWatcher, DEFAULT_DEBOUNCE};
use nail_store::{BlobHash, BlobStore, Store};
use tokio::sync::broadcast;

use crate::error::{CoreError, Result};
use crate::sync::{ImapAccount, SyncEngine, SyncReport};

/// Capacity of the event channel per subscriber. A subscriber that falls
/// further behind misses events; transports then send it
/// [`Service::resync_events`] so it refetches.
pub const EVENT_CAPACITY: usize = 256;

/// Largest page [`Request::ListMessages`] returns; larger limits are capped.
pub const MAX_PAGE_SIZE: u32 = 1000;

/// Where a [`Service`] keeps its data and reads its configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOptions {
    /// Data directory: `nail.sqlite3` and `blobs/`.
    pub data_dir: PathBuf,
    /// Config directory: `theme.kdl` and `layout.kdl` (both optional).
    pub config_dir: PathBuf,
    /// UI language override (BCP 47), `None` to follow the OS / browser.
    pub locale: Option<String>,
    /// Watch the config directory and broadcast changes.
    pub watch_config: bool,
}

impl ServiceOptions {
    /// Options with the given directories, no locale override and watching on.
    pub fn new(data_dir: impl Into<PathBuf>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            config_dir: config_dir.into(),
            locale: None,
            watch_config: true,
        }
    }
}

/// The Nail core service: answers every [`Request`] and broadcasts
/// [`Event`]s. Cheap to clone; all clones share one core.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    blobs: BlobStore,
    config: ConfigManager,
    locale: Option<String>,
    events: broadcast::Sender<Event>,
    sync: SyncEngine,
    watcher: Mutex<Option<ConfigWatcher>>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("store", &self.inner.store)
            .field("config_dir", &self.inner.config.dir())
            .field("locale", &self.inner.locale)
            .finish_non_exhaustive()
    }
}

impl Service {
    /// Opens the data directory, loads the configuration (invalid or missing
    /// files fall back to the defaults, see [`Service::config_error_events`])
    /// and, if requested, starts watching the config directory.
    pub fn open(options: ServiceOptions) -> Result<Self> {
        std::fs::create_dir_all(&options.data_dir)?;
        let (store, blobs) = nail_store::open_data_dir(&options.data_dir)?;
        let config = ConfigManager::load(&options.config_dir);
        let service = Self::from_parts(store, blobs, config, options.locale);
        if options.watch_config {
            service.watch_config()?;
        }
        Ok(service)
    }

    /// Builds a service from already opened parts, without a config watcher.
    pub fn from_parts(
        store: Store,
        blobs: BlobStore,
        config: ConfigManager,
        locale: Option<String>,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let sync = SyncEngine::new(store.clone(), blobs.clone(), events.clone());
        for err in config.errors() {
            tracing::warn!(%err, "invalid configuration file; using the default");
        }
        Self {
            inner: Arc::new(Inner {
                store,
                blobs,
                config,
                locale,
                events,
                sync,
                watcher: Mutex::new(None),
            }),
        }
    }

    /// Starts watching the config directory: valid changes are broadcast as
    /// [`Event::ConfigChanged`], invalid ones as [`Event::ConfigError`].
    /// Watching lasts as long as the service. Calling it again restarts it.
    pub fn watch_config(&self) -> Result<()> {
        let events = self.inner.events.clone();
        let watcher = self.inner.config.watch(DEFAULT_DEBOUNCE, move |update| {
            let _ = events.send(update.to_event());
        })?;
        *self
            .inner
            .watcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(watcher);
        Ok(())
    }

    /// A new receiver of all events broadcast from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Broadcasts an event to all subscribers.
    pub fn broadcast(&self, event: Event) {
        let _ = self.inner.events.send(event);
    }

    /// [`Event::ConfigError`]s for configuration files that are currently
    /// invalid. Transports send these to a client when it connects, so
    /// errors found at start-up are not lost.
    pub fn config_error_events(&self) -> Vec<Event> {
        self.inner
            .config
            .errors()
            .iter()
            .map(nail_config::ConfigError::to_event)
            .collect()
    }

    /// Events that bring a client that missed events back in sync: the
    /// current configuration ([`Event::ConfigChanged`] followed by any
    /// [`Event::ConfigError`]s) and an [`Event::MailChanged`] for every
    /// folder. Transports send them when a subscriber lagged behind the
    /// event channel ([`broadcast::error::RecvError::Lagged`]).
    pub async fn resync_events(&self) -> Vec<Event> {
        let config = &self.inner.config;
        let mut events = vec![
            nail_config::ConfigUpdate::Changed {
                theme: config.theme(),
                layout: config.layout(),
            }
            .to_event(),
        ];
        events.extend(self.config_error_events());
        let folders = self
            .inner
            .store
            .call(|s| {
                let mut ids = Vec::new();
                for account in s.list_accounts()? {
                    ids.extend(s.list_folders(&account.id)?.into_iter().map(|f| f.id));
                }
                Ok(ids)
            })
            .await;
        match folders {
            Ok(folder_ids) if !folder_ids.is_empty() => {
                events.push(Event::MailChanged { folder_ids });
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(%err, "cannot list folders for a resync"),
        }
        events
    }

    /// The metadata store.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// The blob store.
    pub fn blobs(&self) -> &BlobStore {
        &self.inner.blobs
    }

    /// The configuration manager.
    pub fn config(&self) -> &ConfigManager {
        &self.inner.config
    }

    /// Adds (or updates) an IMAP account and makes it syncable.
    pub async fn add_imap_account(&self, account: Account, settings: ImapAccount) -> Result<()> {
        let id = account.id.clone();
        self.inner
            .store
            .call(move |s| s.upsert_account(&account))
            .await?;
        self.inner.sync.register(id.clone(), settings);
        tracing::info!(account = %id, "IMAP account registered");
        Ok(())
    }

    /// Runs one sync pass over `account` and waits for it.
    pub async fn sync_account(&self, account: &AccountId) -> Result<SyncReport> {
        self.inner.sync.sync_account(account).await
    }

    /// Starts sync passes in the background: for `account`, or for every
    /// syncable account when `None`. An account without server settings
    /// (e.g. a local account) reports an immediately finished pass.
    pub async fn start_sync(&self, account: Option<AccountId>) -> Result<()> {
        let accounts = match account {
            Some(id) => {
                let check = id.clone();
                self.inner
                    .store
                    .call(move |s| s.get_account(&check))
                    .await?;
                vec![id]
            }
            None => self.inner.sync.syncable_accounts(),
        };
        for id in accounts {
            let service = self.clone();
            if self.inner.sync.is_syncable(&id) {
                tokio::spawn(async move {
                    // Failures are broadcast as `SyncFailed` by the engine.
                    let _ = service.sync_account(&id).await;
                });
            } else {
                self.broadcast(Event::SyncProgress {
                    account_id: id,
                    done: 1,
                    total: 1,
                });
            }
        }
        Ok(())
    }

    /// The raw bytes of a blob by its hex hash (the `/api/blob` endpoint).
    /// Blocking.
    pub fn blob(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = BlobHash::parse(hash)?;
        Ok(self.inner.blobs.get(&hash)?)
    }

    /// Answers one request. Never fails: errors become [`Response::Error`].
    pub async fn handle(&self, request: Request) -> Response {
        match self.dispatch(request).await {
            Ok(response) => response,
            Err(err) => {
                let err = ApiError::from(err);
                if err.code == ErrorCode::Internal {
                    tracing::error!(%err, "request failed");
                } else {
                    tracing::debug!(%err, "request failed");
                }
                err.into()
            }
        }
    }

    async fn dispatch(&self, request: Request) -> Result<Response> {
        let store = self.inner.store.clone();
        Ok(match request {
            Request::Ping => Response::Pong {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                protocol: PROTOCOL_VERSION,
            },
            Request::GetConfig => Response::Config {
                theme: Box::new((*self.inner.config.theme()).clone()),
                layout: Box::new((*self.inner.config.layout()).clone()),
                locale: self.inner.locale.clone(),
            },
            Request::ListAccounts => Response::Accounts {
                accounts: store.call(|s| s.list_accounts()).await?,
            },
            Request::ListFolders { account_id } => Response::Folders {
                folders: store.call(move |s| s.list_folders(&account_id)).await?,
            },
            Request::ListMessages {
                folder_id,
                offset,
                limit,
            } => {
                let limit = limit.min(MAX_PAGE_SIZE);
                let id = folder_id.clone();
                let page = store
                    .call(move |s| s.list_messages(&id, offset, limit))
                    .await?;
                Response::Messages {
                    folder_id,
                    offset,
                    total: page.total,
                    messages: page.messages,
                }
            }
            Request::GetMessage { id } => Response::Message {
                message: Box::new(self.message_body(id).await?),
            },
            Request::SyncNow { account_id } => {
                self.start_sync(account_id).await?;
                Response::SyncStarted
            }
            // Defined by the M1a contract (WP10); wired by the integration
            // package (WP14). Until then they fail cleanly.
            request @ (Request::DiscoverAccount { .. }
            | Request::TestAccount { .. }
            | Request::StartOAuth { .. }
            | Request::CancelOAuth { .. }
            | Request::GetAccountsConfig
            | Request::AddAccount { .. }
            | Request::UpdateAccount { .. }
            | Request::RemoveAccount { .. }
            | Request::ListIdentities { .. }
            | Request::ListVirtualAccounts
            | Request::AddVirtualAccount { .. }
            | Request::UpdateVirtualAccount { .. }
            | Request::RemoveVirtualAccount { .. }
            | Request::ListAccountStatuses
            | Request::UpdateFlags { .. }
            | Request::MoveMessages { .. }
            | Request::DeleteMessages { .. }
            | Request::ListThreads { .. }
            | Request::GetThread { .. }
            | Request::Search { .. }
            | Request::CheckQuery { .. }
            | Request::SaveDraft { .. }
            | Request::GetDraft { .. }
            | Request::DeleteDraft { .. }
            | Request::SendDraft { .. }
            | Request::CancelSend { .. }
            | Request::ListOutbox) => {
                return Err(ApiError::unsupported(format!(
                    "{} is not implemented by this core yet",
                    request_name(&request)
                ))
                .into());
            }
        })
    }

    async fn message_body(&self, id: MessageId) -> Result<MessageBody> {
        let store = self.inner.store.clone();
        let blobs = self.inner.blobs.clone();
        tokio::task::spawn_blocking(move || -> Result<MessageBody> {
            let stored = store.get_message(&id)?;
            let hash = stored.blob_hash.ok_or_else(|| {
                CoreError::from(ApiError::new(
                    ErrorCode::Unavailable,
                    format!("the body of message {id} is not downloaded yet"),
                ))
            })?;
            let raw = blobs.get(&hash)?;
            Ok(nail_mime::parse_message(&raw).into_body(id))
        })
        .await?
    }
}

/// The variant name of a request (its wire `type`), for diagnostics.
fn request_name(request: &Request) -> String {
    let debug = format!("{request:?}");
    debug
        .split([' ', '{', '('])
        .next()
        .unwrap_or_default()
        .to_owned()
}
