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
/// entries are removed when the engines start and then daily. (The remote
/// security log follows its own setting,
/// `RemoteUiSettings::audit_retention_days`.)
pub const HISTORY_RETENTION_DAYS: i64 = 90;

/// How often old history is purged while the core runs.
pub const HISTORY_PURGE_INTERVAL: Duration = Duration::from_hours(24);

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
    /// Longest sleep of the snooze wake task (so clock changes are
    /// noticed).
    pub snooze_poll: Duration,
    /// A burst of new mail becomes one notification this long after its
    /// last message…
    pub notification_quiet: Duration,
    /// …and at the latest this long after its first.
    pub notification_max_delay: Duration,
}

impl Default for EngineTiming {
    fn default() -> Self {
        Self {
            reconnect_min: Duration::from_secs(2),
            reconnect_max: Duration::from_mins(5),
            send_retry: Duration::from_mins(1),
            poll_override: None,
            healthy_after: Duration::from_mins(1),
            draft_upload: Duration::from_secs(5),
            rule_tick: Duration::from_mins(1),
            snooze_poll: Duration::from_mins(1),
            notification_quiet: Duration::from_secs(3),
            notification_max_delay: Duration::from_secs(15),
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
            snooze_poll: Duration::from_secs(1),
            notification_quiet: Duration::from_secs(1),
            notification_max_delay: Duration::from_secs(3),
        }
    }
}

/// Where a [`Service`] keeps its data, reads its configuration, and how it
/// reaches the outside world.
#[derive(Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent on/off switches of an options struct (tests and embedders turn them off one by one), not a state"
)]
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
    /// The time snooze, quiet hours and notification batching see; `None`
    /// = the system clock. Seeded demo runs pass an
    /// [`AdjustableClock`](crate::AdjustableClock) the tests move.
    pub clock: Option<Arc<dyn nail_triage::Clock>>,
    /// Limits of the plug-in sandbox (M2d): CPU budget per call, memory,
    /// pending calls, … (`HostLimits::default()`).
    pub plugin_limits: nail_plugins::HostLimits,
    /// ICS subscriptions may fetch plain `http:` from loopback hosts (tests
    /// and seeded demos with their mock servers; set by
    /// [`crate::demo::demo_options`]). Off otherwise: a `webcal://localhost`
    /// link must not make Nail poll local services (ADR 0064).
    pub loopback_feeds: bool,
    /// How Exchange accounts reach their servers (M3c): Autodiscover's
    /// HTTP and Windows sign-in; the defaults use the real network and
    /// SSPI. Seeded demos and tests point them at mock servers.
    pub exchange: crate::exchange::ExchangeHooks,
    /// The desktop shell's side of the M4b requests (installation facts,
    /// updater, Run key, dev endpoint, default apps; ADR 0087). `None`
    /// (`nail serve`, tests): those requests answer `unsupported`.
    pub desktop: Option<Arc<dyn nail_api::DesktopHost>>,
    /// PKCS#11 modules to use instead of the settings' module files (M4c):
    /// tests and demos pass the soft token; `None` = load the configured
    /// DLLs.
    pub smartcard_modules: Option<Vec<Arc<dyn nail_pkcs11::Module>>>,
    /// Where the OS light / dark and high-contrast state comes from (M5a,
    /// Follow Windows). `None` = the platform default (WP152: the Windows
    /// registry watcher); tests and seeded runs pass a
    /// [`nail_api::appearance::ManualSystemAppearance`].
    pub system_appearance: Option<Arc<dyn nail_api::appearance::SystemAppearanceSource>>,
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
            .field("custom_clock", &self.clock.is_some())
            .field("plugin_limits", &self.plugin_limits)
            .field("loopback_feeds", &self.loopback_feeds)
            .field("exchange", &self.exchange)
            .field("desktop", &self.desktop.is_some())
            .field(
                "smartcard_modules",
                &self.smartcard_modules.as_ref().map(Vec::len),
            )
            .field("system_appearance", &self.system_appearance)
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
            clock: None,
            plugin_limits: nail_plugins::HostLimits::default(),
            loopback_feeds: false,
            exchange: crate::exchange::ExchangeHooks::default(),
            desktop: None,
            smartcard_modules: None,
            system_appearance: None,
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
    /// The time the triage logic sees (M2c).
    pub(crate) clock: Arc<dyn nail_triage::Clock>,
    /// Sieve uploads (M2c).
    pub(crate) sieve: crate::sieve::SieveCtl,
    /// The snooze scheduler (M2c).
    pub(crate) snooze: crate::snooze::SnoozeCtl,
    /// New-mail notification batching (M2c).
    pub(crate) notify: crate::notify::NotifyCtl,
    /// The plug-in host, packages and catalogs (M2d).
    pub(crate) plugins: crate::plugin_host::PluginsCtl,
    /// Calendars, contacts and tasks: source engines, reminders, the PIM
    /// search index (M3a).
    pub(crate) pim: crate::pim::PimCtl,
    /// Exchange accounts: running engines, Autodiscover / sign-in hooks
    /// (M3c).
    pub(crate) exchange: crate::exchange::ExchangeCtl,
    /// Confirmations broker, before-send vetoes and headers (M4a).
    pub(crate) agents: crate::agents::AgentsCtl,
    /// Print jobs of this run (M4c).
    pub(crate) print: crate::print::PrintCtl,
    /// Import / export / backup / restore jobs (M4c).
    pub(crate) portability: crate::portability::PortabilityCtl,
    /// The last diagnostics preview (the bundle writes exactly it).
    pub(crate) diagnostics: crate::portability::DiagnosticsCache,
    /// PKCS#11 smartcards and PIN prompts (M4c).
    pub(crate) smartcard: crate::smartcard::SmartcardCtl,
    /// The desktop shell's side of the M4b requests (ADR 0087).
    pub(crate) desktop: Option<Arc<dyn nail_api::DesktopHost>>,
    watcher: Mutex<Option<ConfigWatcher>>,
    /// Follow Windows: the subscription to the OS light / dark state (M5a;
    /// `None` without a source). While it is set, the UI's
    /// `ReportSystemAppearance` is ignored.
    pub(crate) appearance_watch: Mutex<Option<crate::appearance::SystemAppearanceWatch>>,
    /// M5b (ADR 0109): the light / dark state each remote session's browser
    /// reported, by session id; such a session follows it instead of the
    /// host OS ([`crate::appearance::SESSION_APPEARANCE_LIMIT`] at most).
    pub(crate) session_appearance:
        Mutex<std::collections::HashMap<String, nail_api::appearance::SystemAppearance>>,
    /// M5c (ADR 0116): the open pop-out windows of the desktop app, in
    /// opening order.
    pub(crate) pop_outs: Mutex<Vec<nail_api::windows::PopOut>>,
    /// The connection logs (ADR 0190).
    pub(crate) connlog: crate::connlog::ConnLogCtl,
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
        let secrets = resolve_secrets(&options);
        let index_dir = options.data_dir.join(nail_search::INDEX_DIR_NAME);
        // Encryption at rest first: the database file is opened with its
        // key, the store and blob store get their cipher before anything
        // reads messages or drafts (ADRs 0032, 0095). When the encrypted
        // database cannot be opened (its key is gone from the keychain),
        // the service runs on an empty in-memory store and offers only the
        // recovery key (M4c, ADR 0099).
        let opened = nail_vault::open_data_dir(
            &options.data_dir,
            Some(index_dir.clone()),
            Arc::clone(&secrets),
        )?;
        let (store, blobs, vault) = match opened {
            nail_vault::OpenedDataDir::Ready(ready) => {
                let vault = crate::vault::VaultCtl::from_vault(
                    ready.vault,
                    ready.open_error,
                    Some(index_dir.clone()),
                );
                (ready.store, ready.blobs, vault)
            }
            nail_vault::OpenedDataDir::Locked(locked) => {
                let store = Store::open_in_memory()?;
                let blob_dir =
                    std::env::temp_dir().join(format!("nail-locked-{}", std::process::id()));
                let blobs = BlobStore::open(&blob_dir)?;
                let stand_in = nail_vault::Vault::new(
                    store.clone(),
                    blobs.clone(),
                    None,
                    Arc::clone(&secrets),
                );
                let vault = crate::vault::VaultCtl::locked(locked, stand_in);
                (store, blobs, vault)
            }
        };
        let search = vault.open_index(&index_dir)?;
        let config = ConfigManager::load(&options.config_dir);
        let service = Self::build(store, blobs, config, search, vault, secrets, &options)?;
        // Follow Windows (M5a): the given source, else the platform's (the
        // registry on Windows); applies the OS state before anyone asks.
        let watch = crate::appearance::watch_system_appearance(&service, options.system_appearance);
        *lock(&service.inner.appearance_watch) = watch;
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
    ) -> Result<Self> {
        let mut options = ServiceOptions::new(PathBuf::new(), config.dir().to_owned());
        options.locale = locale;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        options.secrets = Some(Arc::clone(&secrets));
        options.trust_anchors = Some(Arc::new(nail_smime::FixedTrustAnchors::new(Vec::new())));
        let search = SearchIndex::open_in_ram(CommitPolicy::default())?;
        let vault =
            crate::vault::VaultCtl::open(store.clone(), blobs.clone(), None, Arc::clone(&secrets));
        Self::build(store, blobs, config, search, vault, secrets, &options)
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
        let service = Self {
            inner: Arc::new(Inner {
                store,
                blobs,
                config,
                locale: options.locale.clone(),
                events: events.clone(),
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
                clock: options
                    .clock
                    .clone()
                    .unwrap_or_else(|| Arc::new(nail_triage::SystemClock)),
                sieve: crate::sieve::SieveCtl::default(),
                snooze: crate::snooze::SnoozeCtl::default(),
                notify: crate::notify::NotifyCtl::new(&options.timing),
                plugins: crate::plugin_host::PluginsCtl::new(
                    (!options.data_dir.as_os_str().is_empty()).then(|| options.data_dir.clone()),
                    options.plugin_limits.clone(),
                ),
                pim: crate::pim::PimCtl::new(options.loopback_feeds),
                exchange: crate::exchange::ExchangeCtl::new(options.exchange.clone()),
                agents: crate::agents::AgentsCtl::default(),
                print: crate::print::PrintCtl::default(),
                portability: crate::portability::PortabilityCtl::default(),
                diagnostics: crate::portability::DiagnosticsCache::default(),
                smartcard: crate::smartcard::SmartcardCtl::new(
                    events.clone(),
                    options.smartcard_modules.clone(),
                ),
                desktop: options.desktop.clone(),
                watcher: Mutex::new(None),
                appearance_watch: Mutex::new(None),
                session_appearance: Mutex::new(std::collections::HashMap::new()),
                pop_outs: Mutex::new(Vec::new()),
                connlog: crate::connlog::ConnLogCtl::new(
                    (!options.data_dir.as_os_str().is_empty()).then(|| options.data_dir.clone()),
                ),
                accounts_watcher: Mutex::new(None),
                background: Mutex::new(Vec::new()),
            }),
        };
        // M4b: the desktop shell reports updater / Run-key changes.
        crate::desktop::attach(&service);
        Ok(service)
    }

    /// Starts the background work: an engine per enabled account, the
    /// outbox sender, the search index committer and (when needed) a
    /// rebuild of the search index. Must be called within a tokio runtime.
    /// Calling it again only reconciles the engines.
    pub fn start(&self) {
        if self.inner.vault.is_locked() {
            // M4c: the data could not be opened; only the recovery key
            // works until a restart (nothing syncs into the stand-in).
            tracing::warn!("the encrypted data is locked: engines stay off");
            return;
        }
        self.inner
            .started
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.reconcile_engines();
        let mut background = lock(&self.inner.background);
        if !background.is_empty() {
            return;
        }
        // M4a review: nothing of this run waits for earlier confirmations.
        self.cancel_orphaned_confirmations();
        // ADR 0190: the connection log's disk mirror and change notices.
        let connlog = self.clone();
        background.push(tokio::spawn(async move {
            connlog.start_connlog().await;
            crate::connlog::run_notices(connlog).await;
        }));
        background.push(tokio::spawn(crate::outbox::run_sender(self.clone())));
        background.push(tokio::spawn(crate::search::run_committer(self.clone())));
        background.push(tokio::spawn(crate::search::run_maintenance(self.clone())));
        background.push(tokio::spawn(crate::rules::run_rules(self.clone())));
        // M2c: snooze wake-ups, notification batches, Sieve uploads.
        background.push(tokio::spawn(crate::snooze::run_snoozes(self.clone())));
        background.push(tokio::spawn(crate::notify::run_notifications(self.clone())));
        background.push(tokio::spawn(crate::sieve::run_sieve(self.clone())));
        // An interrupted switch of encryption at rest resumes (ADR 0032);
        // a vault encrypted before M4c gets its database file encrypted.
        background.push(tokio::spawn(crate::vault::resume_migration(self.clone())));
        // M2d: the enabled plug-ins.
        let plugins = self.clone();
        background.push(tokio::spawn(async move {
            plugins.start_enabled_plugins().await;
        }));
        // M3a: local calendars, source engines, reminders.
        background.extend(crate::pim::start(self));
        // M4c: inserted / removed smartcards, PIN-cache logins.
        background.push(tokio::spawn(crate::smartcard::run_poller(self.clone())));
        let service = self.clone();
        background.push(tokio::spawn(async move {
            // At start, then daily (M2b follow-up: the remote security log
            // is kept as long as its setting says).
            loop {
                if let Err(err) = service.purge_history(nail_util::time::unix_now()).await {
                    tracing::warn!(%err, "cannot purge old history");
                }
                tokio::time::sleep(HISTORY_PURGE_INTERVAL).await;
            }
        }));
    }

    /// Removes MCP audit entries and finished rule runs older than
    /// [`HISTORY_RETENTION_DAYS`] before `now` (Unix seconds), remote-UI
    /// audit entries older than the remote settings'
    /// `audit_retention_days`, and remote-content allows of messages that
    /// no longer exist. Returns how many MCP entries and rule runs were
    /// removed.
    pub async fn purge_history(&self, now: i64) -> Result<(usize, usize)> {
        let before = now - HISTORY_RETENTION_DAYS * 86_400;
        let remote_before =
            now - i64::from(self.remote_ui_settings().audit_retention_days.max(1)) * 86_400;
        Ok(self
            .inner
            .store
            .call(move |s| {
                let purged = (s.purge_mcp_audit(before)?, s.purge_rule_runs(before)?);
                s.purge_remote_audit(remote_before)?;
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
        crate::plugin_host::shutdown(self);
        crate::pim::stop(self);
        self.inner.smartcard.logout_all();
        self.inner.engines.stop_all().await;
    }

    /// Starts watching the config directory: valid changes of `theme.kdl`
    /// and `layout.kdl` are broadcast as [`Event::ConfigChanged`], of
    /// `accounts.kdl` as [`Event::AccountsChanged`]; invalid ones as
    /// [`Event::ConfigError`]. Watching lasts as long as the service.
    /// Calling it again restarts it.
    pub fn watch_config(&self) -> Result<()> {
        let events = self.inner.events.clone();
        let config = self.inner.config.clone();
        let watcher = self.inner.config.watch(DEFAULT_DEBOUNCE, move |update| {
            drop(events.send(update.to_event()));
            // M5d: a file changed on disk (after its `ConfigChanged`, or its
            // errors); the editors' histories carry the new revisions (undo /
            // redo then report conflicts).
            if matches!(
                update,
                nail_config::ConfigUpdate::Changed { .. }
                    | nail_config::ConfigUpdate::Errors { .. }
            ) {
                for scope in nail_api::editing::HistoryScope::ALL {
                    drop(events.send(Event::EditHistoryChanged {
                        history: config.edit_history(*scope),
                    }));
                }
            }
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

    /// Broadcasts the events of config writes.
    pub(crate) fn broadcast_config(&self, updates: &[nail_config::ConfigUpdate]) {
        for update in updates {
            self.broadcast(update.to_event());
        }
    }

    /// Broadcasts an event to all subscribers.
    pub fn broadcast(&self, event: Event) {
        drop(self.inner.events.send(event));
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
            // The theme list and selection (M5a), before the resolved theme.
            Event::ThemeChanged {
                appearance: Box::new(config.appearance()),
            },
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

    /// A localized message in the service's language (English fallback
    /// per key): for texts the shells show themselves, such as OS
    /// notifications.
    pub fn render_localized(&self, message: &nail_api::LocalizedMessage) -> String {
        crate::strings::render(self.inner.locale.as_deref(), message)
    }

    /// The MCP server port the settings ask for (0 = any free port).
    pub async fn mcp_port(&self) -> u16 {
        self.inner
            .store
            .call(Store::mcp_settings)
            .await
            .ok()
            .and_then(|s| s.port)
            .unwrap_or(0)
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
            connlog: Arc::clone(&self.inner.connlog.logs),
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
        let mut settings = ImapSettings::from(settings);
        // ADR 0190: registered accounts log into their scope too.
        settings.config = settings.config.with_tap(self.account_tap(account));
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
        let accounts = if let Some(id) = account {
            self.expand_account(&id).await?
        } else {
            let mut all = self.inner.engines.accounts();
            all.extend(self.inner.manual.accounts());
            all
        };
        for id in accounts {
            if self.inner.engines.request_sync(&id) {
                continue;
            }
            if self.inner.manual.is_registered(&id) {
                let service = self.clone();
                tokio::spawn(async move {
                    // Failures are broadcast as `SyncFailed`.
                    drop(service.sync_account(&id).await);
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

    /// The configuration of account `id` (`not_found` without one).
    pub(crate) fn account_config(&self, id: &AccountId) -> Result<nail_api::AccountConfig> {
        self.inner
            .accounts
            .config()
            .account(id)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("account {id}")).into())
    }

    pub(crate) async fn stored_accounts(&self) -> Result<Vec<Account>> {
        Ok(self.inner.store.call(Store::list_accounts).await?)
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
                platform: nail_api::platform::PlatformInfo::current(self.inner.desktop.is_some()),
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
            Request::DiscoverAccount {
                email,
                trusted_hosts,
                windows_sign_in,
            } => {
                let (candidates, exchange) = self
                    .discover_all(&email, &trusted_hosts, windows_sign_in)
                    .await?;
                Response::Discovered {
                    candidates,
                    email,
                    exchange: exchange.map(Box::new),
                }
            }
            Request::TestAccount { config, secrets } => Response::AccountTested {
                result: Box::new(self.test_account(&config, &secrets).await?),
            },
            Request::StartOAuth {
                provider,
                login_hint,
                protocol,
            } => self.start_oauth(provider, login_hint, protocol).await?,
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
            // P3 (ADR 0164): Gmail-like actions on conversations.
            Request::ApplyMailAction { targets, action } => Response::MailActionQueued {
                result: self
                    .apply_mail_action(&targets, action, None, crate::mail_actions::USER_ACTOR)
                    .await?,
            },
            Request::ResolveMailTargets { targets, action } => Response::MailTargets {
                resolution: self.resolve_mail_targets(&targets, &action, None).await?,
            },
            Request::UndoMailAction { batch_id } => Response::Queued {
                op_ids: self
                    .undo_mail_action(&batch_id, crate::mail_actions::USER_ACTOR)
                    .await?,
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
            Request::GetConversation {
                thread_id,
                folder_id,
            } => {
                let (thread, messages) = self
                    .get_conversation(&thread_id, folder_id.as_ref(), None)
                    .await?;
                Response::Conversation {
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
            Request::GetDraftAttachment { draft_id, blob } => {
                self.draft_attachment(draft_id, blob).await?
            }
            Request::ListDrafts => Response::Drafts {
                drafts: self.list_drafts().await?,
            },
            Request::DeleteDraft { draft_id } => {
                self.delete_draft(&draft_id).await?;
                Response::Done
            }
            // M4a: the plug-ins' before-send hooks may veto (`SendVetoed`).
            Request::SendDraft {
                draft_id,
                delay_secs,
                send_at,
                crypto,
                expected_revision,
            } => {
                Box::pin(self.send_draft_checked(
                    draft_id,
                    delay_secs,
                    send_at,
                    crypto,
                    expected_revision,
                ))
                .await?
            }
            Request::CancelSend { outbox_id } => Response::SendCancelled {
                draft_id: self.cancel_send(outbox_id).await?,
            },
            Request::ListOutbox => Response::Outbox {
                entries: self.inner.store.call(Store::list_outbox).await?,
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
                searches: self.inner.store.call(Store::list_saved_searches).await?,
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
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::mcp_admin_requests!()
            | Request::GetMcpSettings
            | Request::UpdateMcpSettings { .. }) => crate::mcp::handle(self, request).await?,
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::config_merge_requests!() | Request::UpdateLayout { .. }) => {
                crate::customize::handle(self, request).await?
            }

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
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::remote_admin_requests!()
            | Request::AllowRemoteContent { .. }
            | Request::ListRemoteContentAllows
            | Request::RemoveRemoteContentAllow { .. }) => {
                crate::remote::handle(self, request).await?
            }
            request @ (Request::TakeLaunchRequests | Request::ImportMessageFile { .. }) => {
                crate::launch::handle(self, request).await?
            }
            // ADR 0206: links clicked in a message.
            Request::OpenLink { url } => crate::links::open(self, &url)?,
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
            request @ Request::RuleFromMessage { .. } => {
                crate::rules::handle(self, request).await?
            }
            // M2d plug-ins (ADR 0050): one arm, the host is wired by WP64.
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::plugin_admin_requests!()
            | Request::ListPlugins
            | Request::GetPlugin { .. }
            | Request::PreviewPluginInstall { .. }
            | Request::InstallPlugin { .. }
            | Request::LoadDevPlugin { .. }
            | Request::RunPluginCommand { .. }
            | Request::RespondPluginDialog { .. }
            | Request::PluginUiAction { .. }
            | Request::GetPluginPanel { .. }) => crate::plugins::handle(self, request).await?,
            // M3a PIM (ADR 0058): one arm, wired by WP75.
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::pim_source_requests!()
            | nail_api::pim_event_task_requests!()
            | nail_api::pim_contact_requests!()
            | Request::EventFromMessage { .. }
            | Request::TaskFromMessage { .. }
            | Request::SearchPim { .. }
            | Request::PreviewPimImport { .. }
            | Request::ImportPim { .. }
            | Request::ListReminders { .. }
            | Request::SnoozeReminders { .. }
            | Request::DismissReminders { .. }) => crate::pim::handle(self, request).await?,
            // M3b invitations and scheduling (ADRs 0065, 0070); boxed: the
            // scheduling futures are large.
            request @ (Request::GetInvitation { .. }
            | Request::RespondToInvitation { .. }
            | Request::ProposeNewTime { .. }
            | Request::RefreshInvitation { .. }
            | Request::ForwardInvitation { .. }
            | Request::ApplyInvitation { .. }
            | Request::SendInvitations { .. }
            | Request::CancelMeeting { .. }
            | Request::RespondToCounter { .. }
            | Request::QueryFreeBusy { .. }
            | Request::GetSchedulingSettings
            | Request::SetSchedulingSettings { .. }) => {
                Box::pin(crate::scheduling::handle(self, request)).await?
            }
            // M3b Gmail labels (ADRs 0065, 0070).
            request @ (Request::ListGmailLabels { .. }
            | Request::CreateGmailLabel { .. }
            | Request::UpdateGmailLabel { .. }
            | Request::DeleteGmailLabel { .. }) => crate::gmail::handle(self, request).await?,
            // M3c Exchange (ADR 0072): one arm, wired by WP95.
            request @ (Request::GetExchangeStatus { .. }
            | Request::ListExchangeMailboxes { .. }
            | Request::AddExchangeMailbox { .. }
            | Request::UpdateExchangeMailbox { .. }
            | Request::RemoveExchangeMailbox { .. }
            | Request::ListExchangeCategories { .. }
            | Request::CreateExchangeCategory { .. }
            | Request::UpdateExchangeCategory { .. }
            | Request::DeleteExchangeCategory { .. }
            | Request::ClassifyFocused { .. }
            | Request::GetAutomaticReplies { .. }
            | Request::SetAutomaticReplies { .. }
            | Request::GetExchangeRules { .. }
            | Request::SyncExchangeRules { .. }
            | Request::SetExchangeInboxRuleEnabled { .. }
            | Request::DeleteExchangeInboxRule { .. }
            | Request::ListRoomLists { .. }
            | Request::FindRooms { .. }) => crate::exchange::handle(self, request).await?,
            // M4a confirmations, policies and plug-in extensions (ADR 0079):
            // one arm; WP105 wires the host and the requesters.
            request @ (Request::ListConfirmations { .. }
            | Request::DecideConfirmation { .. }
            | Request::ListActionPolicies { .. }
            | Request::SetActionPolicy { .. }
            | Request::GetPluginActivity { .. }
            | Request::RunPluginJob { .. }
            | Request::GetMessagePluginAnnotations { .. }
            | Request::RenderAttachmentPreview { .. }
            | Request::RunPluginAction { .. }
            | Request::OverrideSendVeto { .. }) => crate::agents::handle(self, request).await?,
            // M4b desktop requests (ADR 0087): store-backed settings plus
            // the shell's DesktopHost.
            request @ (nail_api::desktop_requests!()) => {
                crate::desktop::handle(self, request).await?
            }
            // M4c (ADR 0094): the palette's recent actions and print jobs are
            // served; smartcard settings are stored; imports / exports /
            // backups / diagnostics, the recovery key and smartcard use
            // answer `unsupported` (`m4c-not-ready`) until WP125.
            request @ (Request::GetRecentActions
            | Request::RecordActionUsed { .. }
            | Request::ClearRecentActions) => crate::palette::handle(self, request).await?,
            request @ (Request::CreatePrintJob { .. }
            | Request::GetPrintJob { .. }
            | Request::PrintViewReady { .. }
            | Request::SavePrintPdf { .. }) => crate::print::handle(self, request).await?,
            request @ (Request::GetSmartcardSettings
            | Request::SetSmartcardSettings { .. }
            | Request::ListSmartcards
            | Request::UseSmartcardKey { .. }
            | Request::AnswerPinPrompt { .. }
            | Request::CreateRecoveryKey { .. }
            | Request::VerifyRecoveryKey { .. }
            | Request::RestoreVaultKey { .. }
            | Request::RemoveRecoveryKey) => crate::smartcard::handle(self, request).await?,
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::portability_requests!()
            | Request::PreviewDiagnostics { .. }
            | Request::CreateDiagnosticsBundle { .. }
            | Request::PickPath { .. }) => crate::portability::handle(self, request).await?,
            // M5a (ADR 0102): themes, selection, Follow Windows (WP152).
            request @ (Request::ListThemes
            | Request::SetTheme { .. }
            | Request::SetThemePreferences { .. }
            | Request::ReportSystemAppearance { .. }) => {
                crate::appearance::handle(self, request).await?
            }
            // M5a WP155: attachments of received mail.
            request @ (Request::GetMessageAttachment { .. } | Request::SaveAttachment { .. }) => {
                crate::attachments::handle(self, request).await?
            }
            // ADR 0196: folder management.
            request @ (Request::CreateFolder { .. }
            | Request::RenameFolder { .. }
            | Request::MoveFolder { .. }
            | Request::DeleteFolder { .. }) => crate::folders::handle(self, request).await?,
            // ADR 0194: "Show source".
            Request::GetMessageSource { message_id } => {
                crate::message_source::handle(self, message_id).await?
            }
            // M5b (ADR 0109): UI state kept across restarts.
            request @ (Request::GetUiState | Request::SetUiState { .. }) => {
                crate::ui_state::handle(self, request).await?
            }
            // M5c (ADR 0116): remembered window state and desktop pop-outs.
            request @ (Request::GetWindowStates
            | Request::SetWindowState { .. }
            | Request::ResetWindowStates
            | Request::ListPopOuts
            | Request::PopOutWindow { .. }
            | Request::ReturnPopOut { .. }
            | Request::ClosePopOut { .. }
            | Request::FocusPopOut { .. }) => crate::windows::handle(self, request).await?,
            // M5d (ADR 0131): the undo history and the theme editor.
            request @ (Request::GetEditHistory { .. }
            | Request::UndoEdit { .. }
            | Request::RedoEdit { .. }
            | Request::GetThemeDetails { .. }
            | Request::EditTheme { .. }
            | Request::DuplicateTheme { .. }
            | Request::RenameTheme { .. }
            | Request::DeleteTheme { .. }) => crate::editing::handle(self, request).await?,
            // ADR 0190: the connection log, stored errors, the app log.
            #[expect(
                clippy::unnested_or_patterns,
                reason = "a request group (nail_api::request_groups) expands to an or-pattern of its own"
            )]
            request @ (nail_api::connlog_admin_requests!() | Request::ListSyncErrors { .. }) => {
                crate::connlog::handle(self, request).await?
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
