//! The [`Service`]: one per running core, shared by every transport.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::{
    Account, AccountId, ApiError, ErrorCode, Event, PROTOCOL_VERSION, Request, Response,
};
use nail_config::{ConfigManager, ConfigWatcher, DEFAULT_DEBOUNCE};
use nail_discovery::Discoverer;
use nail_search::{CommitPolicy, SearchIndex};
use nail_store::{BlobHash, BlobStore, Store};
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;

use crate::accounts::AccountManager;
use crate::engine::Engines;
use crate::error::Result;
use crate::oauth::{OAuthFlows, Tokens};
use crate::sync::{
    ImapAccount, ImapSettings, RunningPasses, SyncCtx, SyncEngine, SyncReport, sync_with_connection,
};

/// Capacity of the event channel per subscriber. A subscriber that falls
/// further behind misses events; transports then send it
/// [`Service::resync_events`] so it refetches.
pub const EVENT_CAPACITY: usize = 256;

/// Largest page [`Request::ListMessages`] returns; larger limits are capped.
pub const MAX_PAGE_SIZE: u32 = 1000;

/// Days the MCP activity log and the rule-run history are kept; older
/// entries are removed when the engines start.
pub const HISTORY_RETENTION_DAYS: i64 = 90;

/// Timing of the background engines (reconnects, send retries). The
/// defaults suit real servers; tests and demos shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineTiming {
    /// First reconnect delay after a failed connection; doubles up to
    /// `reconnect_max`.
    pub reconnect_min: Duration,
    /// Longest reconnect delay.
    pub reconnect_max: Duration,
    /// Delay before the first retry of a send that failed temporarily
    /// (doubles per attempt, at most an hour).
    pub send_retry: Duration,
    /// Overrides every account's poll interval (full passes, POP3 fetches).
    pub poll_override: Option<Duration>,
    /// An IMAP session (or IDLE watcher connection) that lasted this long
    /// resets the reconnect backoff when it fails; one that fails sooner
    /// keeps the backoff growing.
    pub healthy_after: Duration,
    /// Delay between saving a draft and uploading its copy to the server's
    /// Drafts folder; a later save within it replaces the upload.
    pub draft_upload: Duration,
    /// How often scheduled rules (`schedule`, `older-than`) are checked.
    pub rule_tick: Duration,
}

impl Default for EngineTiming {
    fn default() -> Self {
        Self {
            reconnect_min: Duration::from_secs(2),
            reconnect_max: Duration::from_secs(300),
            send_retry: Duration::from_secs(60),
            poll_override: None,
            healthy_after: Duration::from_secs(60),
            draft_upload: Duration::from_secs(5),
            rule_tick: Duration::from_secs(60),
        }
    }
}

impl EngineTiming {
    /// Short delays for tests and demos (reconnect within a second).
    pub fn fast() -> Self {
        Self {
            reconnect_min: Duration::from_millis(200),
            reconnect_max: Duration::from_secs(1),
            send_retry: Duration::from_secs(1),
            poll_override: None,
            healthy_after: Duration::from_secs(5),
            draft_upload: Duration::from_millis(300),
            rule_tick: Duration::from_secs(1),
        }
    }
}

/// Where a [`Service`] keeps its data, reads its configuration, and how it
/// reaches the outside world.
#[derive(Clone)]
pub struct ServiceOptions {
    /// Data directory: `nail.sqlite3`, `blobs/` and `search/`.
    pub data_dir: PathBuf,
    /// Config directory: `theme.kdl`, `layout.kdl`, `accounts.kdl` (all
    /// optional).
    pub config_dir: PathBuf,
    /// UI language override (BCP 47), `None` to follow the OS / browser.
    pub locale: Option<String>,
    /// Watch the config directory and broadcast changes.
    pub watch_config: bool,
    /// Start the account engines, the outbox and the index committer when
    /// opening (needs a tokio runtime; otherwise call [`Service::start`]).
    pub start_engines: bool,
    /// Secret storage; `None` = the OS keychain (`nail-auth`).
    pub secrets: Option<Arc<dyn SecretStore>>,
    /// Account discovery; `None` = the real network (built on first use).
    pub discoverer: Option<Arc<Discoverer>>,
    /// Open the system browser for OAuth2 sign-ins.
    pub open_browser: bool,
    /// Allow `OpenConfigFile` to start a text editor on this machine (off
    /// for seeded demo and test runs, which must never open windows).
    pub open_editor: bool,
    /// Engine timing.
    pub timing: EngineTiming,
    /// S/MIME trust anchors; `None` = the Windows certificate stores
    /// (current user's Root / CA / Disallowed) on Windows, none elsewhere.
    pub trust_anchors: Option<Arc<dyn nail_smime::TrustAnchors>>,
    /// HTTPS for online key lookups (WKD, keys.openpgp.org); `None` = the
    /// real network (rustls, HTTPS only).
    pub key_lookup: Option<Arc<dyn nail_pgp::discovery::HttpClient>>,
    /// How the remote-content proxy fetches; `None` = the real network
    /// (rustls, platform verifier, system DNS). Seeded runs pass one that
    /// reaches only their mock servers.
    pub proxy_fetcher: Option<nail_proxy::Fetcher>,
}

impl std::fmt::Debug for ServiceOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceOptions")
            .field("data_dir", &self.data_dir)
            .field("config_dir", &self.config_dir)
            .field("locale", &self.locale)
            .field("watch_config", &self.watch_config)
            .field("start_engines", &self.start_engines)
            .field("keychain", &self.secrets.is_none())
            .field("open_browser", &self.open_browser)
            .field("open_editor", &self.open_editor)
            .field("timing", &self.timing)
            .field("fixed_trust_anchors", &self.trust_anchors.is_some())
            .field("key_lookup_mock", &self.key_lookup.is_some())
            .field("proxy_fetcher", &self.proxy_fetcher.is_some())
            .finish_non_exhaustive()
    }
}

impl ServiceOptions {
    /// Options with the given directories, no locale override, watching
    /// and engines on, the OS keychain, real discovery and no browser.
    pub fn new(data_dir: impl Into<PathBuf>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            config_dir: config_dir.into(),
            locale: None,
            watch_config: true,
            start_engines: true,
            secrets: None,
            discoverer: None,
            open_browser: false,
            open_editor: false,
            timing: EngineTiming::default(),
            trust_anchors: None,
            key_lookup: None,
            proxy_fetcher: None,
        }
    }
}

/// The Nail core service: answers every [`Request`] and broadcasts
/// [`Event`]s. Cheap to clone; all clones share one core.
#[derive(Clone)]
pub struct Service {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) store: Store,
    pub(crate) blobs: BlobStore,
    pub(crate) config: ConfigManager,
    pub(crate) locale: Option<String>,
    pub(crate) events: broadcast::Sender<Event>,
    pub(crate) search: Arc<crate::search::SearchSlot>,
    pub(crate) manual: SyncEngine,
    pub(crate) running: RunningPasses,
    pub(crate) accounts: AccountManager,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) tokens: Arc<Tokens>,
    pub(crate) flows: OAuthFlows,
    pub(crate) engines: Engines,
    pub(crate) outbox_wake: Notify,
    pub(crate) discoverer: Mutex<Option<Arc<Discoverer>>>,
    pub(crate) open_browser: bool,
    pub(crate) open_editor: bool,
    pub(crate) timing: EngineTiming,
    /// Whether [`Service::start`] ran (engines follow account changes).
    pub(crate) started: std::sync::atomic::AtomicBool,
    /// The last UI context a client reported (M1b, for MCP).
    pub(crate) ui_context: Mutex<nail_api::UiContext>,
    /// Wakes MCP subscribers of `nail://ui/context` after a report.
    pub(crate) ui_context_changed: tokio::sync::watch::Sender<u64>,
    /// `rules.kdl` (ADR 0024).
    pub(crate) rules: crate::rules::RulesManager,
    /// Wakes the rules task (new mail waiting in the rule inbox).
    pub(crate) rules_wake: Notify,
    /// Pending uploads of drafts to the server's Drafts folder.
    pub(crate) draft_uploads: crate::composer::DraftUploads,
    /// The running MCP server's address, once started (ADR 0025).
    pub(crate) mcp_info: Mutex<Option<nail_api::mcp::McpServerInfo>>,
    /// End-to-end encryption state (M2a): unlock caches, trust anchors,
    /// the key lookup client.
    pub(crate) crypto: crate::crypto::CryptoState,
    /// Encryption at rest (M2a).
    pub(crate) vault: crate::vault::VaultCtl,
    /// Remote UI and remote content (M2b).
    pub(crate) remote: crate::remote::RemoteState,
    /// Launch requests waiting for the UI (M2b).
    pub(crate) launches: Mutex<Vec<nail_api::LaunchRequest>>,
    watcher: Mutex<Option<ConfigWatcher>>,
    accounts_watcher: Mutex<Option<crate::watch::AccountsWatcher>>,
    background: Mutex<Vec<JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for task in self
            .background
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }
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

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Service {
    /// Opens the data directory, loads the configuration (invalid or missing
    /// files fall back to the defaults, see [`Service::config_error_events`])
    /// and, if requested, starts watching the config directory and the
    /// engines.
    pub fn open(options: ServiceOptions) -> Result<Self> {
        std::fs::create_dir_all(&options.data_dir)?;
        let (store, blobs) = nail_store::open_data_dir(&options.data_dir)?;
        let secrets = resolve_secrets(&options);
        let index_dir = options.data_dir.join(nail_search::INDEX_DIR_NAME);
        // Encryption at rest first: the store and blob store get their
        // cipher before anything reads messages or drafts (ADR 0032).
        let vault = crate::vault::VaultCtl::open(
            store.clone(),
            blobs.clone(),
            Some(index_dir.clone()),
            Arc::clone(&secrets),
        );
        let search = vault.open_index(&index_dir)?;
        let config = ConfigManager::load(&options.config_dir);
        let service = Self::build(store, blobs, config, search, vault, secrets, &options)?;
        if options.watch_config {
            service.watch_config()?;
        }
        service.sync_store_accounts_blocking()?;
        if options.start_engines && tokio::runtime::Handle::try_current().is_ok() {
            service.start();
        }
        Ok(service)
    }

    /// Builds a service from already opened parts: no config watcher, no
    /// engines, an in-memory search index and secret store. For tests and
    /// embedding.
    pub fn from_parts(
        store: Store,
        blobs: BlobStore,
        config: ConfigManager,
        locale: Option<String>,
    ) -> Self {
        let mut options = ServiceOptions::new(PathBuf::new(), config.dir().to_owned());
        options.locale = locale;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        options.secrets = Some(Arc::clone(&secrets));
        options.trust_anchors = Some(Arc::new(nail_smime::FixedTrustAnchors::new(Vec::new())));
        let search = SearchIndex::open_in_ram(CommitPolicy::default())
            .expect("an in-memory search index always opens");
        let vault =
            crate::vault::VaultCtl::open(store.clone(), blobs.clone(), None, Arc::clone(&secrets));
        Self::build(store, blobs, config, search, vault, secrets, &options)
            .expect("building a service from parts does not fail")
    }

    fn build(
        store: Store,
        blobs: BlobStore,
        config: ConfigManager,
        search: SearchIndex,
        vault: crate::vault::VaultCtl,
        secrets: Arc<dyn SecretStore>,
        options: &ServiceOptions,
    ) -> Result<Self> {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        for err in config.errors() {
            tracing::warn!(%err, "invalid configuration file; using the default");
        }
        let accounts = AccountManager::load(config.dir());
        let rules = crate::rules::RulesManager::load(config.dir());
        let crypto = crate::crypto::CryptoState::new(options)?;
        let remote = crate::remote::RemoteState::new(
            store.clone(),
            Arc::clone(&secrets),
            events.clone(),
            options.proxy_fetcher.clone(),
        );
        let http = nail_auth::http_client()
            .map_err(|err| ApiError::internal(format!("cannot build the HTTP client: {err}")))?;
        let tokens = Arc::new(Tokens::new(
            &accounts.config().oauth_providers,
            Arc::clone(&secrets),
            http,
        ));
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                blobs,
                config,
                locale: options.locale.clone(),
                events,
                search: Arc::new(crate::search::SearchSlot::new(search)),
                manual: SyncEngine::default(),
                running: RunningPasses::default(),
                accounts,
                secrets,
                tokens,
                flows: OAuthFlows::default(),
                engines: Engines::default(),
                outbox_wake: Notify::new(),
                discoverer: Mutex::new(options.discoverer.clone()),
                open_browser: options.open_browser,
                open_editor: options.open_editor,
                timing: options.timing,
                started: std::sync::atomic::AtomicBool::new(false),
                ui_context: Mutex::new(nail_api::UiContext::default()),
                ui_context_changed: tokio::sync::watch::channel(0).0,
                rules,
                rules_wake: Notify::new(),
                draft_uploads: crate::composer::DraftUploads::default(),
                mcp_info: Mutex::new(None),
                crypto,
                vault,
                remote,
                launches: Mutex::new(Vec::new()),
                watcher: Mutex::new(None),
                accounts_watcher: Mutex::new(None),
                background: Mutex::new(Vec::new()),
            }),
        })
    }

    /// Starts the background work: an engine per enabled account, the
    /// outbox sender, the search index committer and (when needed) a
    /// rebuild of the search index. Must be called within a tokio runtime.
    /// Calling it again only reconciles the engines.
    pub fn start(&self) {
        self.inner
            .started
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.reconcile_engines();
        let mut background = lock(&self.inner.background);
        if !background.is_empty() {
            return;
        }
        background.push(tokio::spawn(crate::outbox::run_sender(self.clone())));
        background.push(tokio::spawn(crate::search::run_committer(self.clone())));
        background.push(tokio::spawn(crate::search::run_maintenance(self.clone())));
        background.push(tokio::spawn(crate::rules::run_rules(self.clone())));
        // An interrupted switch of encryption at rest resumes (ADR 0032).
        background.push(tokio::spawn(crate::vault::resume_migration(self.clone())));
        let service = self.clone();
        background.push(tokio::spawn(async move {
            if let Err(err) = service.purge_history(crate::engine::unix_now()).await {
                tracing::warn!(%err, "cannot purge old history");
            }
        }));
    }

    /// Removes MCP audit entries, finished rule runs and remote-UI audit
    /// entries older than [`HISTORY_RETENTION_DAYS`] before `now` (Unix
    /// seconds), and remote-content allows of messages that no longer
    /// exist. Returns how many MCP entries and rule runs were removed.
    pub async fn purge_history(&self, now: i64) -> Result<(usize, usize)> {
        let before = now - HISTORY_RETENTION_DAYS * 86_400;
        Ok(self
            .inner
            .store
            .call(move |s| {
                let purged = (s.purge_mcp_audit(before)?, s.purge_rule_runs(before)?);
                s.purge_remote_audit(before)?;
                s.purge_stale_remote_content_allows()?;
                Ok(purged)
            })
            .await?)
    }

    /// Stops the engines and background tasks (the service stays usable
    /// for requests).
    pub async fn shutdown(&self) {
        let tasks: Vec<_> = lock(&self.inner.background).drain(..).collect();
        for task in tasks {
            task.abort();
        }
        self.inner.engines.stop_all().await;
    }

    /// Starts watching the config directory: valid changes of `theme.kdl`
    /// and `layout.kdl` are broadcast as [`Event::ConfigChanged`], of
    /// `accounts.kdl` as [`Event::AccountsChanged`]; invalid ones as
    /// [`Event::ConfigError`]. Watching lasts as long as the service.
    /// Calling it again restarts it.
    pub fn watch_config(&self) -> Result<()> {
        let events = self.inner.events.clone();
        let watcher = self.inner.config.watch(DEFAULT_DEBOUNCE, move |update| {
            let _ = events.send(update.to_event());
        })?;
        *lock(&self.inner.watcher) = Some(watcher);
        let accounts = crate::watch::AccountsWatcher::start(self)?;
        *lock(&self.inner.accounts_watcher) = Some(accounts);
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

    /// Error events for the configuration files: the M1a
    /// [`Event::ConfigError`] (first error) of every invalid file, then one
    /// [`Event::ConfigErrors`] per file (theme, layout, accounts, rules)
    /// with all of its errors (empty when valid). Transports send these to
    /// a client when it connects, so errors found at start-up are not lost.
    pub fn config_error_events(&self) -> Vec<Event> {
        let mut events: Vec<Event> = self
            .inner
            .config
            .errors()
            .iter()
            .map(nail_config::ConfigError::to_event)
            .collect();
        if let Some(err) = self.inner.accounts.error() {
            events.push(err.to_event());
        }
        if let Some(err) = self.inner.rules.errors().first() {
            events.push(err.to_event());
        }
        events.extend(self.inner.config.error_events());
        events.push(nail_config::issues_event(
            crate::accounts::ACCOUNTS_FILE,
            &self.inner.accounts.all_errors(),
        ));
        events.push(nail_config::issues_event(
            nail_config::rules::RULES_FILE,
            &self.inner.rules.errors(),
        ));
        events
    }

    /// Events that bring a client that missed events back in sync: the
    /// current configuration ([`Event::ConfigChanged`] followed by any
    /// [`Event::ConfigError`]s), [`Event::AccountsChanged`] and an
    /// [`Event::MailChanged`] for every folder. Transports send them when a
    /// subscriber lagged behind the event channel
    /// ([`broadcast::error::RecvError::Lagged`]).
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
        events.push(Event::AccountsChanged);
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

    /// The search index (replaced while encryption at rest converts it).
    pub fn search_index(&self) -> Arc<SearchIndex> {
        self.inner.search.get()
    }

    /// The secret storage (keychain, or the store given in the options).
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.inner.secrets
    }

    pub(crate) fn sync_ctx(&self) -> SyncCtx {
        SyncCtx {
            store: self.inner.store.clone(),
            blobs: self.inner.blobs.clone(),
            events: self.inner.events.clone(),
            index: Arc::clone(&self.inner.search),
        }
    }

    /// Adds (or updates) an IMAP account with in-memory credentials and
    /// makes it syncable on demand ([`Service::sync_account`]). Configured
    /// accounts (`accounts.kdl`) are synced by their engines instead.
    pub async fn add_imap_account(&self, account: Account, settings: ImapAccount) -> Result<()> {
        let id = account.id.clone();
        self.inner
            .store
            .call(move |s| s.upsert_account(&account))
            .await?;
        self.inner.manual.register(id.clone(), settings);
        tracing::info!(account = %id, "IMAP account registered");
        Ok(())
    }

    /// Runs one sync pass over an account registered with
    /// [`Service::add_imap_account`] and waits for it. Fails with
    /// [`ErrorCode::Unavailable`] while a pass for the account runs.
    pub async fn sync_account(&self, account: &AccountId) -> Result<SyncReport> {
        let settings = self.inner.manual.settings(account).ok_or_else(|| {
            ApiError::not_found(format!("account {account} has no server to sync"))
        })?;
        let _guard = self.inner.running.claim(account)?;
        let settings = ImapSettings::from(settings);
        let report = sync_with_connection(
            &self.sync_ctx(),
            account,
            &settings,
            &nail_api::accounts::SyncPolicy::default(),
        )
        .await?;
        // Writes queued for a manual account replay after its pass.
        crate::replay::replay_with_connection(self, account, &settings).await;
        Ok(report)
    }

    /// Starts synchronizing in the background: `account` (a virtual account
    /// means its members), or every account when `None`. Accounts with an
    /// engine sync on it; registered accounts get a pass; accounts without
    /// a server (local) report a finished pass at once.
    pub async fn start_sync(&self, account: Option<AccountId>) -> Result<()> {
        let accounts = match account {
            Some(id) => self.expand_account(&id).await?,
            None => {
                let mut all = self.inner.engines.accounts();
                all.extend(self.inner.manual.accounts());
                all
            }
        };
        for id in accounts {
            if self.inner.engines.request_sync(&id) {
                continue;
            }
            if self.inner.manual.is_registered(&id) {
                let service = self.clone();
                tokio::spawn(async move {
                    // Failures are broadcast as `SyncFailed`.
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

    /// The real accounts behind an account id (itself, or a virtual
    /// account's members). Fails with `not_found` for unknown ids.
    pub(crate) async fn expand_account(&self, id: &AccountId) -> Result<Vec<AccountId>> {
        if id.as_str() == AccountId::ALL {
            return self.all_members().await;
        }
        let stored = self.stored_accounts().await?;
        let config = self.inner.accounts.config();
        if let Some(va) = crate::virtuals::virtual_config(&config, &stored, id) {
            return Ok(va.members);
        }
        if config.account(id).is_some() || stored.iter().any(|a| &a.id == id) {
            return Ok(vec![id.clone()]);
        }
        Err(ApiError::not_found(format!("account {id}")).into())
    }

    pub(crate) async fn stored_accounts(&self) -> Result<Vec<Account>> {
        Ok(self.inner.store.call(|s| s.list_accounts()).await?)
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
                accounts: self.list_accounts().await?,
            },
            Request::ListFolders { account_id } => Response::Folders {
                folders: self.list_folders(&account_id).await?,
            },
            Request::ListMessages {
                folder_id,
                offset,
                limit,
            } => {
                let limit = limit.min(MAX_PAGE_SIZE);
                let page = self.list_messages(&folder_id, offset, limit).await?;
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
            Request::DiscoverAccount { email } => Response::Discovered {
                candidates: self.discover(&email).await?,
                email,
            },
            Request::TestAccount { config, secrets } => Response::AccountTested {
                result: Box::new(self.test_account(&config, &secrets).await?),
            },
            Request::StartOAuth {
                provider,
                login_hint,
            } => self.start_oauth(provider, login_hint).await?,
            Request::CancelOAuth { flow_id } => {
                self.inner.flows.cancel(&flow_id)?;
                Response::Done
            }
            Request::GetAccountsConfig => Response::AccountsConfig {
                config: self.inner.accounts.config(),
            },
            Request::AddAccount { config, secrets } => Response::AccountSaved {
                config: Box::new(self.save_account(*config, &secrets, true).await?),
            },
            Request::UpdateAccount { config, secrets } => Response::AccountSaved {
                config: Box::new(self.save_account(*config, &secrets, false).await?),
            },
            Request::RemoveAccount { account_id } => {
                self.remove_account(&account_id).await?;
                Response::Done
            }
            Request::ListIdentities { account_id } => Response::Identities {
                identities: self.list_identities(account_id.as_ref()).await?,
            },
            Request::ChooseIdentity {
                account_id,
                recipients,
                reply_to,
            } => Response::IdentityChosen {
                identity_id: self
                    .choose_identity(account_id.as_ref(), &recipients, reply_to)
                    .await?,
            },
            Request::ListVirtualAccounts => Response::VirtualAccounts {
                virtual_accounts: self.inner.accounts.config().virtual_accounts,
            },
            Request::AddVirtualAccount { config } => Response::VirtualAccountSaved {
                config: Box::new(self.save_virtual_account(*config, true)?),
            },
            Request::UpdateVirtualAccount { config } => Response::VirtualAccountSaved {
                config: Box::new(self.save_virtual_account(*config, false)?),
            },
            Request::RemoveVirtualAccount { account_id } => {
                self.remove_virtual_account(&account_id)?;
                Response::Done
            }
            Request::ListAccountStatuses => Response::AccountStatuses {
                statuses: self.account_statuses(),
            },
            Request::UpdateFlags {
                message_ids,
                change,
            } => Response::Queued {
                op_ids: self.update_flags(&message_ids, change).await?,
            },
            Request::MoveMessages {
                message_ids,
                target,
            } => Response::Queued {
                op_ids: self.move_messages(&message_ids, &target).await?,
            },
            Request::DeleteMessages {
                message_ids,
                permanent,
            } => Response::Queued {
                op_ids: self.delete_messages(&message_ids, permanent).await?,
            },
            Request::ListThreads {
                folder_id,
                offset,
                limit,
            } => {
                let page = self
                    .list_threads(&folder_id, offset, limit.min(MAX_PAGE_SIZE))
                    .await?;
                Response::Threads {
                    folder_id,
                    offset,
                    total: page.total,
                    threads: page.threads,
                }
            }
            Request::GetThread { thread_id } => {
                let (thread, messages) = self.get_thread(&thread_id).await?;
                Response::Thread {
                    thread: Box::new(thread),
                    messages,
                }
            }
            Request::Search { query } => self.search(query).await?,
            Request::CheckQuery { text } => Response::QueryChecked {
                error: nail_search::parse_query(&text).err(),
            },
            Request::SaveDraft { draft } => Response::DraftSaved {
                draft_id: self.save_draft(draft).await?,
            },
            Request::GetDraft { draft_id } => {
                let draft = self
                    .inner
                    .store
                    .call(move |s| s.get_draft(&draft_id))
                    .await?;
                Response::Draft {
                    draft: Box::new(draft),
                }
            }
            Request::ListDrafts => Response::Drafts {
                drafts: self.list_drafts().await?,
            },
            Request::DeleteDraft { draft_id } => {
                self.delete_draft(&draft_id).await?;
                Response::Done
            }
            Request::SendDraft {
                draft_id,
                delay_secs,
                send_at,
                crypto,
            } => Response::SendQueued {
                entry: Box::new(
                    self.send_draft(draft_id, delay_secs, send_at, crypto)
                        .await?,
                ),
            },
            Request::CancelSend { outbox_id } => Response::SendCancelled {
                draft_id: self.cancel_send(outbox_id).await?,
            },
            Request::ListOutbox => Response::Outbox {
                entries: self.inner.store.call(|s| s.list_outbox()).await?,
            },
            Request::RetryOp { op_id } => Response::Queued {
                op_ids: self.retry_op(&op_id).await?,
            },

            // --- M1b: UI context and saved searches (core) ---
            Request::ReportUiContext { context } => {
                self.report_ui_context(context);
                Response::Done
            }
            Request::GetUiContext => Response::UiContext {
                context: self.ui_context(),
            },
            Request::ListSavedSearches => Response::SavedSearches {
                searches: self.inner.store.call(|s| s.list_saved_searches()).await?,
            },
            Request::SaveSavedSearch { search } => Response::SavedSearchSaved {
                search: self.save_saved_search(search).await?,
            },
            Request::DeleteSavedSearch { search_id } => {
                self.delete_saved_search(search_id).await?;
                Response::Done
            }

            // --- M1b feature packages: one arm per module (ADR 0022) ---
            request @ (Request::NewDraft { .. }
            | Request::RenderPreview { .. }
            | Request::ConvertDraft { .. }
            | Request::UploadAttachment { .. }
            | Request::SuggestAddresses { .. }) => crate::composer::handle(self, request).await?,
            request @ (Request::ListRules
            | Request::SaveRule { .. }
            | Request::DeleteRule { .. }
            | Request::ReorderRules { .. }
            | Request::TestRule { .. }
            | Request::RunRule { .. }
            | Request::ListRuleRuns { .. }
            | Request::UndoRuleRun { .. }
            | Request::RuleToText { .. }
            | Request::RuleFromText { .. }) => crate::rules::handle(self, request).await?,
            request @ (Request::ListMcpClients
            | Request::CreateMcpClient { .. }
            | Request::UpdateMcpGrants { .. }
            | Request::RevokeMcpClient { .. }
            | Request::ListMcpAudit { .. }
            | Request::GetMcpServerInfo) => crate::mcp::handle(self, request).await?,
            request @ (Request::UpdateLayout { .. }
            | Request::PreviewConfigMerge { .. }
            | Request::ResolveConfigMerge { .. }
            | Request::ApplyConfigMerge { .. }
            | Request::ListConfigBackups { .. }
            | Request::RevertConfig { .. }
            | Request::OpenConfigFile { .. }) => crate::customize::handle(self, request).await?,

            // --- M2a security (ADR 0029): one arm, wired by WP35 ---
            request @ (Request::ListKeys { .. }
            | Request::GetKey { .. }
            | Request::ImportKeys { .. }
            | Request::ImportPkcs12 { .. }
            | Request::ExportPublicKey { .. }
            | Request::GenerateKey { .. }
            | Request::DeleteKey { .. }
            | Request::SetKeyTrust { .. }
            | Request::UnlockKey { .. }
            | Request::LookupKeys { .. }
            | Request::ListWindowsCertificates { .. }
            | Request::ImportWindowsCertificate { .. }
            | Request::CheckRecipientKeys { .. }
            | Request::SendAutocryptSetup { .. }
            | Request::ImportAutocryptSetup { .. }
            | Request::GetSecuritySettings
            | Request::UpdateSecuritySettings { .. }
            | Request::SuggestAuthservId { .. }
            | Request::GetVaultStatus
            | Request::SetVaultEncryption { .. }) => crate::security::handle(self, request).await?,

            // --- M2b remote UI and remote content (ADR 0036): one arm, wired by WP45 ---
            request @ (Request::GetRemoteUiSettings
            | Request::UpdateRemoteUiSettings { .. }
            | Request::RegenerateRemoteCertificate
            | Request::ListRemoteUsers
            | Request::CreateRemoteUser { .. }
            | Request::UpdateRemoteUser { .. }
            | Request::DeleteRemoteUser { .. }
            | Request::SetRemotePassword { .. }
            | Request::StartTotpEnrollment { .. }
            | Request::ConfirmTotpEnrollment { .. }
            | Request::DisableTotp { .. }
            | Request::UnlockRemoteUser { .. }
            | Request::RenamePasskey { .. }
            | Request::DeletePasskey { .. }
            | Request::ListRemoteDevices { .. }
            | Request::UpdateRemoteDevice { .. }
            | Request::RevokeRemoteDevice { .. }
            | Request::ListRemoteSessions { .. }
            | Request::RevokeRemoteSession { .. }
            | Request::RevokeAllRemoteSessions { .. }
            | Request::CreatePairingCode { .. }
            | Request::ListPairingCodes
            | Request::CancelPairingCode { .. }
            | Request::ListRemoteAudit { .. }
            | Request::AllowRemoteContent { .. }
            | Request::ListRemoteContentAllows
            | Request::RemoveRemoteContentAllow { .. }) => {
                crate::remote::handle(self, request).await?
            }
            request @ (Request::TakeLaunchRequests | Request::ImportMessageFile { .. }) => {
                crate::launch::handle(self, request).await?
            }
            // M2c (ADR 0043).
            request @ (Request::GetSieveStatus { .. }
            | Request::SyncSieve { .. }
            | Request::PreviewSieveScript { .. }
            | Request::GetVacation { .. }
            | Request::UpdateVacation { .. }) => crate::sieve::handle(self, request).await?,
            request @ (Request::MarkJunk { .. }
            | Request::GetJunkStatus { .. }
            | Request::UpdateJunkSettings { .. }
            | Request::ResetJunkClassifier { .. }
            | Request::SnoozeMessages { .. }
            | Request::UnsnoozeMessages { .. }
            | Request::ListSnoozed { .. }
            | Request::GetNotificationSettings
            | Request::UpdateNotificationSettings { .. }
            | Request::TestNotification) => crate::triage::handle(self, request).await?,
            request @ (Request::GetFolderSyncPolicy { .. }
            | Request::SetFolderSyncPolicy { .. }
            | Request::FetchMessageBodies { .. }) => {
                crate::sync_policy::handle(self, request).await?
            }
            request @ (Request::GetMcpSettings | Request::UpdateMcpSettings { .. }) => {
                crate::mcp::handle(self, request).await?
            }
            request @ Request::RuleFromMessage { .. } => {
                crate::rules::handle(self, request).await?
            }
        })
    }
}

/// The secret store of the options: the given one, else the OS keychain.
fn resolve_secrets(options: &ServiceOptions) -> Arc<dyn SecretStore> {
    match &options.secrets {
        Some(secrets) => Arc::clone(secrets),
        None => Arc::new(nail_auth::KeychainSecretStore::new()),
    }
}

/// The wire name of a request (its `type`), for messages and logs.
pub(crate) fn request_name(request: &Request) -> String {
    serde_json::to_value(request)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str().map(str::to_owned)))
        .unwrap_or_else(|| "request".to_owned())
}
