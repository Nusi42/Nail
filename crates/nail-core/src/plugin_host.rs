//! The plug-in host inside the core (M2d WP64, ADR 0055).
//!
//! [`PluginsCtl`] owns the one [`PluginHost`] of a running core (created
//! on first use inside the Tokio runtime), the installed packages
//! (`<data-dir>/plugins/<id>.nailplugin`, [`PackageStore`]), the plug-ins'
//! own catalogs (for `%fluent-id%` manifest texts) and the HTTP client of
//! `nail.http.fetch`.
//!
//! [`CoreServices`] is the core's side of the host
//! ([`nail_plugins::PluginServices`]): mail reads over the store and the
//! search index (the host's permission gate already narrowed the accounts
//! and filters every result again), per-plug-in storage, HTTP through
//! [`crate::plugin_http`], log lines (stored in order, then broadcast) and
//! events. It holds the service weakly (the service owns the host), never
//! decrypts end-to-end encrypted mail for a plug-in, and does its store
//! work on the Tokio runtime so that `emit` / `log` never block a plug-in
//! thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use nail_api::DraftId;
use nail_api::mcp::tools::McpWriteResult;
use nail_api::plugins::PluginPermission;
use nail_api::plugins::admin::MAX_PACKAGE_BYTES;
use nail_api::plugins::{
    PluginAttachment, PluginManifest, PluginMessage, PluginMessageBody, PluginPackageSource,
    PluginStorageUsage, message_uri,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, LocalizedMessage, MessageArg, MessageId, MessageSummary,
    PluginId, Request, Response, SearchQuery, ThreadId,
};
use nail_plugins::gate::Denied;
use nail_plugins::services::{
    AttachmentData as PluginAttachmentData, HookCallRecord, JobRunRecord, PluginCollection,
    PluginContact, PluginWriteRequest, StoredJobState,
};
use nail_plugins::{
    AccountScope, BoxFuture, HostLimits, HostLogLine, HttpRequest, HttpResponse, PackageFiles,
    PackageStore, PluginCatalogs, PluginHost, PluginServices, PluginSpec, ServiceResult,
};
use nail_search::query::{Query, Term, TermKind, TextField};
use nail_store::{PluginLogLine, PluginRecord};
use nail_triage::Clock as _;
use serde_json::Value;

use crate::error::Result;
use crate::plugin_http::PluginHttp;
use crate::service::{Inner, Service, lock};

/// Most messages one `nail.mail.search` returns.
const MAX_SEARCH: u32 = 500;

/// Most index hits one `nail.mail.search` looks at per account while it
/// leaves out encrypted messages.
const MAX_SEARCH_SCAN: u32 = 5000;

/// Log lines waiting for the writer; more are dropped and counted.
const LOG_QUEUE: usize = 4096;

/// The query with free text limited to header fields (sender,
/// recipients, subject), for accounts whose message text the plug-in may
/// not read. `None` when it names message text itself (`body:`,
/// `filename:`).
fn header_only(query: &Query) -> Option<Query> {
    Some(match query {
        Query::All => Query::All,
        Query::Term(term) => match &term.kind {
            TermKind::Text {
                field: TextField::Any,
                value,
                quoted,
            } => Query::Or(
                [
                    TextField::From,
                    TextField::To,
                    TextField::Cc,
                    TextField::Subject,
                ]
                .into_iter()
                .map(|field| {
                    Query::Term(Term {
                        kind: TermKind::Text {
                            field,
                            value: value.clone(),
                            quoted: *quoted,
                        },
                        span: term.span,
                    })
                })
                .collect(),
            ),
            TermKind::Text {
                field: TextField::Body | TextField::Filename,
                ..
            } => return None,
            _ => query.clone(),
        },
        Query::Not(inner) => Query::Not(Box::new(header_only(inner)?)),
        Query::And(items) => Query::And(items.iter().map(header_only).collect::<Option<_>>()?),
        Query::Or(items) => Query::Or(items.iter().map(header_only).collect::<Option<_>>()?),
    })
}

/// Up to `limit` hits of `text` in `account` (or everywhere), leaving out
/// end-to-end encrypted messages when `hide_encrypted` (scanning further
/// pages so that hidden hits do not shorten the answer).
async fn search_part(
    service: &Service,
    text: &str,
    account: Option<AccountId>,
    limit: u32,
    hide_encrypted: bool,
) -> ServiceResult<Vec<MessageSummary>> {
    let mut found = Vec::new();
    let mut offset = 0u32;
    loop {
        let query = SearchQuery {
            text: text.to_owned(),
            account_id: account.clone(),
            folder_id: None,
            offset,
            limit,
            sort: nail_api::search::SearchSort::Date,
        };
        let (total, hits) = match service
            .search_with(query, crate::search::ServerSearchMode::Off)
            .await
            .map_err(api)?
        {
            Response::SearchResults { total, hits, .. } => (total, hits),
            Response::InvalidQuery { error } => {
                return Err(ApiError::invalid_request(format!(
                    "invalid search: {}",
                    error.message
                )));
            }
            _ => return Err(ApiError::internal("unexpected answer to Search")),
        };
        let returned = u32::try_from(hits.len()).unwrap_or(u32::MAX);
        let messages: Vec<MessageSummary> = hits.into_iter().map(|h| h.message).collect();
        if hide_encrypted {
            let visible = service
                .blocking(move |s| {
                    let mut visible = Vec::with_capacity(messages.len());
                    for m in messages {
                        if !s.message_is_encrypted(&m.id)? {
                            visible.push(m);
                        }
                    }
                    Ok(visible)
                })
                .await
                .map_err(api)?;
            found.extend(visible);
        } else {
            found.extend(messages);
        }
        offset = offset.saturating_add(returned);
        if !hide_encrypted
            || found.len() >= limit as usize
            || returned == 0
            || offset >= total
            || offset >= MAX_SEARCH_SCAN
        {
            break;
        }
    }
    found.truncate(limit as usize);
    Ok(found)
}

/// A plug-in's catalogs with what they were loaded from (package hash or
/// dev folder).
type CachedCatalogs = (Option<String>, Arc<PluginCatalogs>);

/// The plug-in host and what belongs to it (one per service).
pub(crate) struct PluginsCtl {
    host: OnceLock<PluginHost>,
    packages: Option<PackageStore>,
    pub(crate) http: PluginHttp,
    /// The plug-ins' own catalogs by id (package hash or dev folder).
    catalogs: Mutex<HashMap<PluginId, CachedCatalogs>>,
    limits: HostLimits,
}

impl PluginsCtl {
    /// Packages go to `<data_dir>/plugins`; `None` (a service built from
    /// parts) cannot install packages.
    pub(crate) fn new(data_dir: Option<std::path::PathBuf>, limits: HostLimits) -> Self {
        Self {
            host: OnceLock::new(),
            packages: data_dir.map(|d| PackageStore::new(d.join("plugins"))),
            http: PluginHttp::default(),
            catalogs: Mutex::new(HashMap::new()),
            limits,
        }
    }

    /// The host when it was created.
    pub(crate) fn existing_host(&self) -> Option<&PluginHost> {
        self.host.get()
    }

    /// The package store, or `unavailable`.
    pub(crate) fn packages(&self) -> Result<&PackageStore> {
        self.packages.as_ref().ok_or_else(|| {
            ApiError::new(
                ErrorCode::Unavailable,
                "this core has no data directory for plug-in packages",
            )
            .into()
        })
    }

    /// Remembers a plug-in's catalogs (`key` = package hash or folder).
    pub(crate) fn set_catalogs(
        &self,
        id: &PluginId,
        key: Option<String>,
        catalogs: PluginCatalogs,
    ) {
        lock(&self.catalogs).insert(id.clone(), (key, Arc::new(catalogs)));
    }

    fn cached_catalogs(&self, id: &PluginId, key: Option<&str>) -> Option<Arc<PluginCatalogs>> {
        lock(&self.catalogs)
            .get(id)
            .filter(|(k, _)| k.as_deref() == key)
            .map(|(_, c)| Arc::clone(c))
    }

    pub(crate) fn forget(&self, id: &PluginId) {
        lock(&self.catalogs).remove(id);
    }

    /// What a plug-in's catalogs are cached by: its package hash, or its
    /// dev folder with the host's start generation, so that every dev
    /// reload (a changed locale file with the same manifest, too) reads
    /// the catalogs again.
    fn catalogs_key(&self, record: &PluginRecord) -> Option<String> {
        match &record.source {
            PluginPackageSource::Dev { folder } => Some(self.dev_key(&record.id, folder)),
            _ => record.package_sha256.clone(),
        }
    }

    fn dev_key(&self, id: &PluginId, folder: &str) -> String {
        let generation = self
            .existing_host()
            .and_then(|host| host.generation(id))
            .unwrap_or_default();
        format!("{folder}#{generation}")
    }

    /// Whether the cached catalogs of a plug-in were loaded for `key`.
    fn catalogs_current(&self, id: &PluginId, key: &str) -> bool {
        lock(&self.catalogs)
            .get(id)
            .is_some_and(|(k, _)| k.as_deref() == Some(key))
    }

    /// Resolves a `%fluent-id%` plug-in name in a host message's `plugin`
    /// argument (the host names plug-ins by their manifest's raw name).
    pub(crate) fn localize_plugin_arg(
        &self,
        id: &PluginId,
        language: Option<&str>,
        message: &mut LocalizedMessage,
    ) {
        let Some(MessageArg::Text(name)) = message.args.get("plugin") else {
            return;
        };
        if !(name.len() > 2 && name.starts_with('%') && name.ends_with('%')) {
            return;
        }
        let catalogs = lock(&self.catalogs).get(id).map(|(_, c)| Arc::clone(c));
        if let Some(catalogs) = catalogs {
            let resolved = catalogs.text(language, name);
            message
                .args
                .insert("plugin".to_owned(), MessageArg::Text(resolved));
        }
    }
}

impl Service {
    /// The core's clock was moved (the `clock-set` / `clock-advance` test
    /// controls): plug-in jobs due at the new time run now.
    pub fn clock_moved(&self) {
        if let Some(host) = self.inner.plugins.existing_host() {
            host.run_due_jobs();
        }
    }

    /// The plug-in host, created on first use (inside the Tokio runtime).
    pub(crate) fn plugin_host(&self) -> &PluginHost {
        self.inner.plugins.host.get_or_init(|| {
            let handle = tokio::runtime::Handle::current();
            let services = CoreServices::start(self, handle.clone());
            PluginHost::new(
                Arc::new(services),
                handle,
                self.inner.plugins.limits.clone(),
            )
        })
    }

    /// Starts every enabled plug-in (at start-up).
    pub(crate) async fn start_enabled_plugins(&self) {
        let records = match self.store().call(|s| s.plugins()).await {
            Ok(records) => records,
            Err(err) => {
                tracing::warn!(%err, "cannot list plug-ins");
                return;
            }
        };
        self.refresh_installed_plugins().await;
        for record in records.into_iter().filter(|r| r.enabled) {
            if let Err(err) = self.start_plugin(&record).await {
                tracing::warn!(plugin = %record.id, %err, "plug-in did not start");
            }
        }
    }

    /// Starts (or restarts) an installed plug-in with its stored grants
    /// and settings. Failures to load its package are recorded as its
    /// error; failures of the plug-in itself are recorded by the host.
    pub(crate) async fn start_plugin(&self, record: &PluginRecord) -> Result<()> {
        let id = record.id.clone();
        let (grants, settings) = {
            let id = id.clone();
            self.store()
                .call(move |s| Ok((s.plugin_grants(&id)?, s.plugin_settings(&id)?)))
                .await?
        };
        let host = self.plugin_host().clone();
        let result = match &record.source {
            PluginPackageSource::Dev { folder } => {
                let folder = std::path::PathBuf::from(folder);
                match host.load_dev(&folder, grants, settings).await {
                    Ok(manifest) => {
                        self.remember_dev_catalogs(&id, &folder, &manifest).await;
                        Ok(())
                    }
                    Err(err) => Err(err.to_api_error()),
                }
            }
            _ => {
                let packages = self.inner.plugins.packages()?.clone();
                let sha = record.package_sha256.clone();
                let load_id = id.clone();
                let loaded =
                    tokio::task::spawn_blocking(move || packages.load(&load_id, sha.as_deref()))
                        .await?;
                match loaded {
                    Ok(package) => match package.valid_manifest().cloned() {
                        Some(manifest) => {
                            self.inner.plugins.set_catalogs(
                                &id,
                                Some(package.sha256.clone()),
                                PluginCatalogs::load(&manifest, &package.files),
                            );
                            host.start(PluginSpec {
                                manifest,
                                files: package.files,
                                grants,
                                settings,
                                dev_folder: None,
                            })
                            .await
                        }
                        None => Err(ApiError::invalid_request(format!(
                            "the installed package of {id} has manifest problems"
                        ))),
                    },
                    Err(err) => {
                        let error = err.to_api_error();
                        self.record_plugin_error(&id, Some(error.clone())).await;
                        Err(error)
                    }
                }
            }
        };
        result.map_err(Into::into)
    }

    async fn remember_dev_catalogs(
        &self,
        id: &PluginId,
        folder: &std::path::Path,
        manifest: &PluginManifest,
    ) {
        let files = match folder.canonicalize() {
            Ok(root) => PackageFiles::Folder(root),
            Err(_) => return,
        };
        let manifest = manifest.clone();
        let catalogs = tokio::task::spawn_blocking(move || PluginCatalogs::load(&manifest, &files))
            .await
            .ok();
        if let Some(catalogs) = catalogs {
            let key = self
                .inner
                .plugins
                .dev_key(id, &folder.display().to_string());
            self.inner.plugins.set_catalogs(id, Some(key), catalogs);
        }
    }

    /// Stores (or clears) the error a plug-in stopped with.
    pub(crate) async fn record_plugin_error(&self, id: &PluginId, error: Option<ApiError>) {
        let id = id.clone();
        let result = self
            .store()
            .call(move |s| match s.plugin(&id) {
                Ok(_) => s.set_plugin_error(&id, error.as_ref()),
                Err(e) if e.is_not_found() => Ok(()),
                Err(e) => Err(e),
            })
            .await;
        if let Err(err) = result {
            tracing::warn!(%err, "cannot record a plug-in error");
        }
    }

    /// The plug-in's catalogs for its manifest texts (loaded from its
    /// package or folder when not cached).
    pub(crate) async fn plugin_catalogs(&self, record: &PluginRecord) -> Arc<PluginCatalogs> {
        let key = self.inner.plugins.catalogs_key(record);
        if let Some(catalogs) = self
            .inner
            .plugins
            .cached_catalogs(&record.id, key.as_deref())
        {
            return catalogs;
        }
        let manifest = record.manifest.clone();
        let files = match &record.source {
            PluginPackageSource::Dev { folder } => std::path::Path::new(folder)
                .canonicalize()
                .ok()
                .map(PackageFiles::Folder),
            _ => match self.inner.plugins.packages() {
                Ok(packages) => {
                    let packages = packages.clone();
                    let id = record.id.clone();
                    let sha = record.package_sha256.clone();
                    tokio::task::spawn_blocking(move || packages.load(&id, sha.as_deref()))
                        .await
                        .ok()
                        .and_then(std::result::Result::ok)
                        .map(|p| p.files)
                }
                Err(_) => None,
            },
        };
        let catalogs = match files {
            Some(files) => {
                tokio::task::spawn_blocking(move || PluginCatalogs::load(&manifest, &files))
                    .await
                    .unwrap_or_else(|_| PluginCatalogs::from_sources([]))
            }
            None => PluginCatalogs::from_sources([]),
        };
        let catalogs = Arc::new(catalogs);
        lock(&self.inner.plugins.catalogs).insert(record.id.clone(), (key, Arc::clone(&catalogs)));
        catalogs
    }

    /// Tells the merge preview which plug-in commands and panels exist.
    pub(crate) async fn refresh_installed_plugins(&self) {
        let Ok(records) = self.store().call(|s| s.plugins()).await else {
            return;
        };
        let mut installed = nail_config::InstalledPlugins::default();
        for record in &records {
            installed.add(&record.id, &record.manifest);
        }
        self.config().set_installed_plugins(installed);
    }

    /// After a dev reload the host may run a changed manifest: the store
    /// follows it (grants of permissions it no longer declares go). The
    /// plug-in's catalogs are read again on every dev reload, also when
    /// only a locale file changed.
    pub(crate) async fn sync_dev_manifests(&self) {
        let Some(host) = self.inner.plugins.existing_host().cloned() else {
            return;
        };
        let Ok(records) = self.store().call(|s| s.plugins()).await else {
            return;
        };
        for record in records {
            let PluginPackageSource::Dev { folder } = &record.source else {
                continue;
            };
            let Some(running) = host.manifest(&record.id) else {
                continue;
            };
            let changed = running != record.manifest;
            let key = self.inner.plugins.dev_key(&record.id, folder);
            if changed || !self.inner.plugins.catalogs_current(&record.id, &key) {
                self.remember_dev_catalogs(&record.id, std::path::Path::new(folder), &running)
                    .await;
            }
            if !changed {
                continue;
            }
            let result = self
                .store()
                .call(move |s| {
                    let grants: Vec<_> = s
                        .plugin_grants(&record.id)?
                        .into_iter()
                        .filter(|g| running.permissions.contains(&g.permission))
                        .collect();
                    s.install_plugin(&nail_store::PluginInstall {
                        manifest: &running,
                        source: &record.source,
                        package_sha256: None,
                        grants: &grants,
                        enabled: record.enabled,
                    })
                })
                .await;
            match result {
                Ok(_) => self.broadcast(Event::PluginsChanged),
                Err(err) => tracing::warn!(%err, "cannot update a dev plug-in's manifest"),
            }
        }
        self.refresh_installed_plugins().await;
    }
}

/// A message's headers view for plug-ins.
pub(crate) fn plugin_message(summary: &MessageSummary, encrypted: bool) -> PluginMessage {
    PluginMessage {
        id: summary.id.clone(),
        uri: message_uri(&summary.id),
        account_id: summary.account_id.clone(),
        folder_id: summary.folder_id.clone(),
        thread_id: summary.thread_id.clone(),
        subject: summary.subject.clone(),
        from: summary.from.clone(),
        to: summary.to.clone(),
        cc: Vec::new(),
        date: summary.date.clone(),
        flags: summary.flags,
        tags: summary.tags.clone(),
        has_attachments: summary.has_attachments,
        encrypted,
    }
}

/// The core's side of the plug-in host (see the module docs).
pub(crate) struct CoreServices {
    service: Weak<Inner>,
    handle: tokio::runtime::Handle,
    /// Log lines in order to the writer task (bounded: a flood of lines
    /// is dropped and counted instead of growing without limit).
    logs: tokio::sync::mpsc::Sender<HostLogLine>,
    /// Lines dropped because the queue was full, per plug-in.
    dropped: Arc<Mutex<HashMap<PluginId, u64>>>,
}

impl CoreServices {
    /// The services of `service` and the task that stores log lines.
    fn start(service: &Service, handle: tokio::runtime::Handle) -> Self {
        let (logs, mut rx) = tokio::sync::mpsc::channel::<HostLogLine>(LOG_QUEUE);
        let dropped: Arc<Mutex<HashMap<PluginId, u64>>> = Arc::default();
        let counted = Arc::clone(&dropped);
        let weak = Arc::downgrade(&service.inner);
        handle.spawn(async move {
            let mut pending: std::collections::VecDeque<HostLogLine> =
                std::collections::VecDeque::new();
            loop {
                // What was dropped is reported (a host line per plug-in)
                // as soon as the writer has room again.
                if pending.is_empty() {
                    pending.extend(dropped_lines(&counted));
                }
                let line = match pending.pop_front() {
                    Some(line) => line,
                    None => match rx.recv().await {
                        Some(line) => line,
                        None => break,
                    },
                };
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let service = Service { inner };
                let stored = service
                    .store()
                    .call(move |s| {
                        s.append_plugin_log(&PluginLogLine {
                            plugin_id: &line.plugin_id,
                            at_ms: line.at_ms,
                            level: line.level,
                            source: line.source,
                            message: &line.message,
                            localized: line.localized.as_ref(),
                        })
                    })
                    .await;
                match stored {
                    Ok(entry) => service.broadcast(Event::PluginLogLine {
                        entry: Box::new(entry),
                    }),
                    // Lines of a plug-in that was just uninstalled.
                    Err(err) if err.is_not_found() => {}
                    Err(err) => tracing::warn!(%err, "cannot store a plug-in log line"),
                }
            }
        });
        Self {
            service: Arc::downgrade(&service.inner),
            handle,
            logs,
            dropped,
        }
    }

    fn service(&self) -> ServiceResult<Service> {
        self.service
            .upgrade()
            .map(|inner| Service { inner })
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "Nail is shutting down"))
    }
}

/// Queues a log line for the writer without blocking; a full queue drops
/// it and counts it for its plug-in (M2d review: an unbounded queue let a
/// flood of lines grow the core's memory).
fn enqueue_log(
    logs: &tokio::sync::mpsc::Sender<HostLogLine>,
    dropped: &Mutex<HashMap<PluginId, u64>>,
    line: HostLogLine,
) {
    use tokio::sync::mpsc::error::TrySendError;
    if let Err(TrySendError::Full(line)) = logs.try_send(line) {
        *lock(dropped).entry(line.plugin_id).or_default() += 1;
    }
}

/// Host lines for log lines dropped because the writer queue was full.
fn dropped_lines(dropped: &Mutex<HashMap<PluginId, u64>>) -> Vec<HostLogLine> {
    let taken = std::mem::take(&mut *lock(dropped));
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default();
    taken
        .into_iter()
        .map(|(plugin_id, count)| HostLogLine {
            plugin_id,
            at_ms,
            level: nail_api::plugins::PluginLogLevel::Warn,
            source: nail_api::plugins::PluginLogSource::Host,
            message: format!("{count} log lines dropped"),
            localized: Some(
                LocalizedMessage::new(nail_plugins::l10n::LOG_DROPPED).arg("count", count),
            ),
        })
        .collect()
}

fn api(err: crate::error::CoreError) -> ApiError {
    match err {
        crate::error::CoreError::Store(store) => store.into(),
        other => other.into(),
    }
}

/// Whether the reader treats a message as end-to-end encrypted, cheaply
/// (cached status or the raw structure; no decryption): for lists.
fn encrypted_cheap(service: &Service, id: &MessageId) -> bool {
    let store = service.store();
    if store
        .message_security(id)
        .ok()
        .flatten()
        .and_then(|r| r.status)
        .is_some_and(|s| s.is_encrypted())
    {
        return true;
    }
    store
        .get_message(id)
        .ok()
        .and_then(|m| m.blob_hash)
        .and_then(|hash| service.blobs().get(&hash).ok())
        .is_some_and(|raw| crate::crypto::is_encrypted_raw(&raw))
}

impl PluginServices for CoreServices {
    fn expand_accounts(&self, accounts: &[AccountId]) -> Vec<AccountId> {
        let Ok(service) = self.service() else {
            return Vec::new();
        };
        let stored = service.store().list_accounts().unwrap_or_default();
        let config = service.inner.accounts.config();
        let mut out: Vec<AccountId> = Vec::new();
        for id in accounts {
            let expanded = if id.as_str() == AccountId::ALL {
                crate::virtuals::all_members(&config, &stored)
            } else if let Some(va) = crate::virtuals::virtual_config(&config, &stored, id) {
                va.members
            } else if config.account(id).is_some() || stored.iter().any(|a| &a.id == id) {
                vec![id.clone()]
            } else {
                Vec::new()
            };
            for account in expanded {
                if !out.contains(&account) {
                    out.push(account);
                }
            }
        }
        out
    }

    fn mail_get(&self, id: MessageId) -> BoxFuture<'static, ServiceResult<Option<PluginMessage>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            service
                .blocking(move |s| {
                    let stored = match s.store().get_message(&id) {
                        Ok(stored) => stored,
                        Err(e) if e.is_not_found() => return Ok(None),
                        Err(e) => return Err(e.into()),
                    };
                    let encrypted = s.message_is_encrypted(&id)?;
                    let mut message = plugin_message(&stored.summary, encrypted);
                    if let Some(hash) = &stored.blob_hash
                        && let Ok(raw) = s.blobs().get(hash)
                    {
                        // Header fields only; never decrypted.
                        message.cc = nail_mime::parse_message(&raw).headers.cc;
                    }
                    Ok(Some(message))
                })
                .await
                .map_err(api)
        })
    }

    fn mail_body(
        &self,
        id: MessageId,
    ) -> BoxFuture<'static, ServiceResult<Option<PluginMessageBody>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let forbidden = || {
                ApiError::new(
                    ErrorCode::Forbidden,
                    "end-to-end encrypted content is not given to plug-ins",
                )
            };
            let check = id.clone();
            if service
                .blocking(move |s| s.message_is_encrypted(&check))
                .await
                .map_err(api)?
            {
                return Err(forbidden());
            }
            let message = match service.handle(Request::GetMessage { id: id.clone() }).await {
                Response::Message { message } => message,
                Response::Error {
                    code: ErrorCode::NotFound,
                    ..
                } => return Ok(None),
                Response::Error {
                    code,
                    message,
                    localized,
                } => {
                    return Err(ApiError {
                        code,
                        message,
                        localized,
                    });
                }
                _ => return Err(ApiError::internal("unexpected answer to GetMessage")),
            };
            // The body may have been downloaded just now: check again, and
            // never pass on what the reader decrypted.
            let check = id.clone();
            let encrypted_now = service
                .blocking(move |s| s.message_is_encrypted(&check))
                .await
                .map_err(api)?;
            if encrypted_now || message.security.as_ref().is_some_and(|s| s.is_encrypted()) {
                return Err(forbidden());
            }
            Ok(Some(PluginMessageBody {
                id: message.id.clone(),
                text: message.text.clone(),
                html: message.sanitized_html.clone(),
                attachments: message
                    .attachments
                    .iter()
                    .filter(|a| !a.inline)
                    .map(|a| PluginAttachment {
                        name: a.filename.clone().unwrap_or_default(),
                        mime_type: a.mime.clone(),
                        size: a.size,
                    })
                    .collect(),
            }))
        })
    }

    fn mail_search(
        &self,
        query: String,
        scope: AccountScope,
        body_scope: AccountScope,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let limit = limit.clamp(1, MAX_SEARCH);
            let parsed = nail_search::parse_query(&query)
                .map_err(|e| ApiError::invalid_request(format!("invalid search: {}", e.message)))?;
            // Message text only where `mail.read.body` is granted; header
            // fields elsewhere (M2d review: `body:` probing).
            let headers = header_only(&parsed).map(|q| q.to_string());
            let parts: Vec<(Option<AccountId>, String)> = match (&scope, &body_scope) {
                (AccountScope::All, AccountScope::All) => vec![(None, query.clone())],
                _ => {
                    let accounts = match &scope {
                        AccountScope::All => service.all_members().await.map_err(api)?,
                        AccountScope::Only(accounts) => accounts.clone(),
                    };
                    accounts
                        .into_iter()
                        .filter_map(|account| {
                            let text = if body_scope.contains(&account) {
                                query.clone()
                            } else {
                                headers.clone()?
                            };
                            Some((Some(account), text))
                        })
                        .collect()
                }
            };
            if parts.is_empty() && headers.is_none() {
                return Err(
                    Denied::new("nail.mail.search", PluginPermission::MailReadBody).to_api_error(),
                );
            }
            // While the index holds decrypted text of encrypted mail, such
            // mail is never found (like remote sessions without
            // `encrypted`; ADR 0055: decrypted text never reaches a
            // plug-in).
            let hide_encrypted = !crate::security::security_settings(&service)
                .await
                .map_err(api)?
                .index_decrypted
                .is_empty();
            let mut summaries: Vec<MessageSummary> = Vec::new();
            for (account, text) in parts {
                summaries
                    .extend(search_part(&service, &text, account, limit, hide_encrypted).await?);
            }
            summaries.retain(|m| scope.contains(&m.account_id));
            summaries.sort_by(|a, b| b.date.cmp(&a.date));
            summaries.dedup_by(|a, b| a.id == b.id);
            summaries.truncate(limit as usize);
            service
                .blocking(move |s| {
                    Ok(summaries
                        .iter()
                        .map(|m| plugin_message(m, encrypted_cheap(s, &m.id)))
                        .collect())
                })
                .await
                .map_err(api)
        })
    }

    fn mail_thread(
        &self,
        thread: ThreadId,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let messages = match service
                .handle(Request::GetThread { thread_id: thread })
                .await
            {
                Response::Thread { messages, .. } => messages,
                Response::Error {
                    code: ErrorCode::NotFound,
                    ..
                } => {
                    return Ok(Vec::new());
                }
                Response::Error {
                    code,
                    message,
                    localized,
                } => {
                    return Err(ApiError {
                        code,
                        message,
                        localized,
                    });
                }
                _ => return Err(ApiError::internal("unexpected answer to GetThread")),
            };
            service
                .blocking(move |s| {
                    let mut out = Vec::with_capacity(messages.len());
                    for m in &messages {
                        out.push(plugin_message(m, s.message_is_encrypted(&m.id)?));
                    }
                    Ok(out)
                })
                .await
                .map_err(api)
        })
    }

    fn calendar_events(
        &self,
        from: String,
        to: String,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<nail_api::plugins::PluginCalendarEvent>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let request = Request::ListEventInstances {
                from,
                to,
                calendar_ids: None,
                tz: crate::pim::local_zone_name(),
            };
            match service.handle(request).await {
                Response::EventInstances { instances, .. } => Ok(instances
                    .into_iter()
                    .take(limit as usize)
                    .map(|i| nail_api::plugins::PluginCalendarEvent {
                        event_id: i.event_id,
                        calendar_id: i.calendar_id,
                        title: i.title,
                        start: i.start_utc,
                        end: i.end_utc,
                        all_day: i.all_day,
                        location: i.location,
                        recurring: i.recurring,
                    })
                    .collect()),
                Response::Error {
                    code,
                    message,
                    localized,
                } => Err(ApiError {
                    code,
                    message,
                    localized,
                }),
                _ => Err(ApiError::internal(
                    "unexpected answer to ListEventInstances",
                )),
            }
        })
    }

    fn storage_get(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<Option<Value>>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_get(&plugin, &key))
                .await?)
        })
    }

    fn storage_set(
        &self,
        plugin: PluginId,
        key: String,
        value: Value,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_set(&plugin, &key, &value))
                .await
                .map(|_| ())?)
        })
    }

    fn storage_delete(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<bool>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_delete(&plugin, &key))
                .await?)
        })
    }

    fn storage_keys(
        &self,
        plugin: PluginId,
        prefix: Option<String>,
    ) -> BoxFuture<'static, ServiceResult<Vec<String>>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_keys(&plugin, prefix.as_deref()))
                .await?)
        })
    }

    fn storage_clear(&self, plugin: PluginId) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_clear(&plugin))
                .await?)
        })
    }

    fn storage_usage(
        &self,
        plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<PluginStorageUsage>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.plugin_storage_usage(&plugin))
                .await?)
        })
    }

    fn http_fetch(
        &self,
        _plugin: PluginId,
        request: HttpRequest,
    ) -> BoxFuture<'static, ServiceResult<HttpResponse>> {
        let service = self.service();
        Box::pin(async move {
            let http = service?.inner.plugins.http.clone();
            http.fetch(request).await
        })
    }

    fn emit(&self, event: Event) {
        let Ok(service) = self.service() else {
            return;
        };
        match &event {
            Event::PluginNotification { notification } if notification.spec.system => {
                // OS notifications follow the notification switch and the
                // quiet hours; the toast is always shown.
                let service = service.clone();
                let mut notification = notification.clone();
                self.handle.spawn(async move {
                    let quiet = service
                        .store()
                        .call(|s| s.notification_settings())
                        .await
                        .map(|settings| {
                            let local = service.inner.clock.now().with_timezone(&chrono::Local);
                            !settings.enabled
                                || settings
                                    .quiet_hours
                                    .as_ref()
                                    .is_some_and(|q| nail_triage::notify::in_quiet_hours(q, &local))
                        })
                        .unwrap_or(false);
                    if quiet {
                        notification.spec.system = false;
                    }
                    service.broadcast(Event::PluginNotification { notification });
                });
                return;
            }
            Event::PluginStopped {
                plugin_id,
                reason,
                error,
                localized,
            } => {
                let mut localized = localized.clone();
                if let Some(message) = localized.as_deref_mut() {
                    service.inner.plugins.localize_plugin_arg(
                        plugin_id,
                        service.inner.locale.as_deref(),
                        message,
                    );
                }
                if reason.is_crash() {
                    let service = service.clone();
                    let id = plugin_id.clone();
                    let mut stored = ApiError::internal(error.clone().unwrap_or_default());
                    if let Some(localized) = &localized {
                        stored = stored.with_localized((**localized).clone());
                    }
                    self.handle.spawn(async move {
                        service.record_plugin_error(&id, Some(stored)).await;
                    });
                }
                service.broadcast(Event::PluginStopped {
                    plugin_id: plugin_id.clone(),
                    reason: *reason,
                    error: error.clone(),
                    localized,
                });
                return;
            }
            Event::PluginsChanged => {
                let service = service.clone();
                self.handle
                    .spawn(async move { service.sync_dev_manifests().await });
            }
            _ => {}
        }
        service.broadcast(event);
    }

    fn log(&self, mut line: HostLogLine) {
        // The host names plug-ins by their manifest's raw name
        // (`%plugin-name%`); the stored line carries the resolved one.
        if let (Ok(service), Some(message)) = (self.service(), line.localized.as_mut()) {
            service.inner.plugins.localize_plugin_arg(
                &line.plugin_id,
                service.inner.locale.as_deref(),
                message,
            );
        }
        enqueue_log(&self.logs, &self.dropped, line);
    }

    // --- M4a (ADR 0084) ---

    fn write(
        &self,
        request: PluginWriteRequest,
    ) -> BoxFuture<'static, ServiceResult<McpWriteResult>> {
        let service = self.service();
        let logs = self.logs.clone();
        let dropped = Arc::clone(&self.dropped);
        Box::pin(async move {
            let service = service?;
            let plugin = request.plugin_id().cloned();
            let operation = request.operation.clone();
            let accounts = match request.scope {
                AccountScope::All => None,
                AccountScope::Only(accounts) => Some(accounts),
            };
            let writes = service.actor_writes();
            let result = Box::pin(writes.run(nail_mcp::ActorWriteRequest {
                by: request.attribution,
                operation: request.operation,
                write: crate::agents::actor_write(request.write),
                accounts,
            }))
            .await;
            // Every plug-in write leaves a line in its log (the audit of
            // plug-ins); no mailbox content, only what ran and how it ended.
            if let Some(plugin_id) = plugin {
                let outcome = match &result {
                    Ok(McpWriteResult::Done { .. }) => "done",
                    Ok(McpWriteResult::Denied { .. }) => "denied",
                    Ok(McpWriteResult::Expired { .. }) => "expired",
                    Ok(McpWriteResult::Vetoed { .. }) => "vetoed",
                    Err(_) => "failed",
                };
                enqueue_log(
                    &logs,
                    &dropped,
                    HostLogLine {
                        plugin_id,
                        at_ms: chrono::Utc::now().timestamp_millis(),
                        level: if result.is_ok() {
                            nail_api::plugins::PluginLogLevel::Info
                        } else {
                            nail_api::plugins::PluginLogLevel::Warn
                        },
                        source: nail_api::plugins::PluginLogSource::Host,
                        message: format!("write {operation}: {outcome}"),
                        localized: Some(
                            LocalizedMessage::new("plugin-write-log")
                                .arg("operation", operation.as_str())
                                .arg("outcome", outcome),
                        ),
                    },
                );
            }
            result
        })
    }

    fn calendars(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        let service = self.service();
        Box::pin(async move {
            let calendars = match service?
                .handle(Request::ListCalendars { source_id: None })
                .await
            {
                Response::Calendars { calendars } => calendars,
                other => return Err(answer_error(other)),
            };
            Ok(collections(
                calendars
                    .into_iter()
                    .filter(nail_api::pim::Calendar::has_events),
            ))
        })
    }

    fn address_books(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        let service = self.service();
        Box::pin(async move {
            let books = match service?
                .handle(Request::ListAddressBooks { source_id: None })
                .await
            {
                Response::AddressBooks { address_books } => address_books,
                other => return Err(answer_error(other)),
            };
            let mut out: Vec<PluginCollection> = books
                .into_iter()
                .map(|b| PluginCollection {
                    id: b.id.as_str().to_owned(),
                    name: b.settings.name.clone(),
                    default: false,
                    read_only: b.read_only
                        || b.role == Some(nail_api::pim::contact::AddressBookRole::Collected),
                })
                .collect();
            if let Some(first) = out.iter_mut().find(|b| !b.read_only) {
                first.default = true;
            }
            Ok(out)
        })
    }

    fn task_lists(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        let service = self.service();
        Box::pin(async move {
            let calendars = match service?
                .handle(Request::ListCalendars { source_id: None })
                .await
            {
                Response::Calendars { calendars } => calendars,
                other => return Err(answer_error(other)),
            };
            Ok(collections(
                calendars
                    .into_iter()
                    .filter(nail_api::pim::Calendar::has_tasks),
            ))
        })
    }

    fn find_contacts(
        &self,
        email: String,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginContact>>> {
        let service = self.service();
        Box::pin(async move {
            let wanted = email.trim().to_lowercase();
            if wanted.is_empty() {
                return Ok(Vec::new());
            }
            let contacts = match service?
                .handle(Request::SearchContacts {
                    query: wanted.clone(),
                    address_book_ids: None,
                    limit: limit.clamp(1, 100).saturating_mul(4),
                })
                .await
            {
                Response::Contacts { contacts, .. } => contacts,
                other => return Err(answer_error(other)),
            };
            Ok(contacts
                .into_iter()
                .filter(|c| c.emails.iter().any(|e| e.trim().to_lowercase() == wanted))
                .take(limit.clamp(1, 100) as usize)
                .map(|c| PluginContact {
                    id: c.id.as_str().to_owned(),
                    address_book_id: c.address_book_id.as_str().to_owned(),
                    full_name: Some(c.display_name.clone()).filter(|n| !n.is_empty()),
                    emails: c.emails.clone(),
                    organization: c.organization.clone().filter(|n| !n.is_empty()),
                })
                .collect())
        })
    }

    fn attachment(
        &self,
        message: MessageId,
        index: u32,
        max_bytes: u64,
    ) -> BoxFuture<'static, ServiceResult<Option<PluginAttachmentData>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let check = message.clone();
            match service
                .blocking(move |s| s.message_is_encrypted(&check))
                .await
            {
                Ok(true) => {
                    return Err(ApiError::new(
                        ErrorCode::Forbidden,
                        "end-to-end encrypted content is not given to plug-ins",
                    ));
                }
                Ok(false) => {}
                Err(err) => {
                    let err = api(err);
                    return if err.code == ErrorCode::NotFound {
                        Ok(None)
                    } else {
                        Err(err)
                    };
                }
            }
            let data = nail_mcp::CoreData(crate::mcp::McpCore(service));
            match nail_mcp::McpData::attachment(&data, &message, index).await {
                Ok(found) => {
                    let size = found.data.len() as u64;
                    Ok(Some(PluginAttachmentData {
                        name: found.filename.unwrap_or_default(),
                        mime_type: found.mime,
                        size,
                        bytes: if size <= max_bytes {
                            found.data
                        } else {
                            Vec::new()
                        },
                    }))
                }
                Err(err) if err.code == ErrorCode::NotFound => Ok(None),
                Err(err) => Err(err),
            }
        })
    }

    fn draft_mode(&self, draft: DraftId) -> BoxFuture<'static, ServiceResult<Option<String>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            match service.store().call(move |s| s.get_draft(&draft)).await {
                Ok(draft) => Ok(Some(
                    match draft.mode {
                        nail_api::accounts::ComposerMode::Plain => "plain",
                        nail_api::accounts::ComposerMode::Html => "html",
                        nail_api::accounts::ComposerMode::Markdown => "markdown",
                    }
                    .to_owned(),
                )),
                Err(err) if err.is_not_found() => Ok(None),
                Err(err) => Err(err.into()),
            }
        })
    }

    fn message_matches(
        &self,
        message: MessageId,
        query: String,
    ) -> BoxFuture<'static, ServiceResult<bool>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let id = message.clone();
            let summary = match service.store().call(move |s| s.get_message(&id)).await {
                Ok(stored) => stored.summary,
                Err(err) if err.is_not_found() => return Ok(false),
                Err(err) => return Err(err.into()),
            };
            // The filter within the message's folder, newest first: a
            // message the hook sees is new, so it is on the first pages.
            let mut offset = 0;
            loop {
                let request = SearchQuery {
                    text: query.clone(),
                    account_id: Some(summary.account_id.clone()),
                    folder_id: Some(summary.folder_id.clone()),
                    offset,
                    limit: FILTER_PAGE,
                    sort: nail_api::search::SearchSort::Date,
                };
                match service
                    .search_with(request, crate::search::ServerSearchMode::Off)
                    .await
                    .map_err(api)?
                {
                    Response::SearchResults { hits, total, .. } => {
                        if hits.iter().any(|h| h.message.id == message) {
                            return Ok(true);
                        }
                        offset += FILTER_PAGE;
                        if hits.is_empty() || offset >= total || offset >= MAX_FILTER_SCAN {
                            return Ok(false);
                        }
                    }
                    Response::InvalidQuery { error } => {
                        return Err(ApiError::invalid_request(format!(
                            "invalid hook filter: {}",
                            error.message
                        )));
                    }
                    other => return Err(answer_error(other)),
                }
            }
        })
    }

    fn locale(&self) -> Option<String> {
        self.service().ok().and_then(|s| s.inner.locale.clone())
    }

    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        match self.service() {
            Ok(service) => service.inner.clock.now(),
            Err(_) => chrono::Utc::now(),
        }
    }

    fn job_states(
        &self,
        plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<Vec<StoredJobState>>> {
        let service = self.service();
        Box::pin(async move {
            let states = service?
                .store()
                .call(move |s| s.plugin_job_states(&plugin))
                .await?;
            Ok(states
                .into_iter()
                .map(|s| StoredJobState {
                    job: s.job,
                    next_run_at: s.next_run_at,
                })
                .collect())
        })
    }

    fn set_job_next_run(
        &self,
        plugin: PluginId,
        job: String,
        next_run_at: Option<i64>,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.set_plugin_job_next_run(&plugin, &job, next_run_at))
                .await?)
        })
    }

    fn record_job_run(&self, record: JobRunRecord) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| {
                    s.record_plugin_job_run(
                        &record.plugin_id,
                        &record.job,
                        record.at,
                        record.outcome,
                        record.error.as_deref(),
                        record.next_run_at,
                    )
                })
                .await?)
        })
    }

    fn record_hook_call(&self, record: HookCallRecord) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| {
                    s.record_plugin_hook_call(
                        &record.plugin_id,
                        record.hook,
                        record.at,
                        record.ms,
                        record.outcome,
                        record.error.as_deref(),
                    )
                })
                .await?)
        })
    }

    fn record_cpu(
        &self,
        plugin: PluginId,
        at: i64,
        cpu_ms: u64,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        let service = self.service();
        Box::pin(async move {
            Ok(service?
                .store()
                .call(move |s| s.add_plugin_cpu(&plugin, at, cpu_ms))
                .await?)
        })
    }
}

/// Hits per page when a hook filter is checked against one message.
const FILTER_PAGE: u32 = 200;

/// Most hits a hook filter check looks at.
const MAX_FILTER_SCAN: u32 = 2000;

/// The error of an unexpected answer.
fn answer_error(response: Response) -> ApiError {
    match response {
        Response::Error {
            code,
            message,
            localized,
        } => ApiError {
            code,
            message,
            localized,
        },
        _ => ApiError::internal("unexpected answer"),
    }
}

/// Calendars or task lists as plug-ins see them; the first writable one
/// (in the user's order) is the default.
fn collections(calendars: impl Iterator<Item = nail_api::pim::Calendar>) -> Vec<PluginCollection> {
    let mut list: Vec<nail_api::pim::Calendar> = calendars.collect();
    list.sort_by_key(|c| c.settings.order);
    let mut out: Vec<PluginCollection> = list
        .into_iter()
        .map(|c| PluginCollection {
            id: c.id.as_str().to_owned(),
            name: c.settings.name.clone(),
            default: false,
            read_only: c.read_only,
        })
        .collect();
    if let Some(first) = out.iter_mut().find(|c| !c.read_only) {
        first.default = true;
    }
    out
}

/// Stops every plug-in (the service shuts down).
pub(crate) fn shutdown(service: &Service) {
    if let Some(host) = service.inner.plugins.existing_host() {
        host.shutdown();
    }
}

/// Reads a package from a file for preview / install (at most
/// [`MAX_PACKAGE_BYTES`]; larger files are `too_large`).
pub(crate) fn read_package_file(path: &std::path::Path) -> std::result::Result<Vec<u8>, ApiError> {
    use std::io::Read;
    let shown = path.display().to_string();
    let file =
        std::fs::File::open(path).map_err(|e| ApiError::not_found(format!("{shown}: {e}")))?;
    let mut bytes = Vec::new();
    file.take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ApiError::not_found(format!("{shown}: {e}")))?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("{shown} is larger than {MAX_PACKAGE_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}

impl Service {
    /// A plug-in's display name in the core's language (its manifest's
    /// name, `%fluent-id%` resolved), or its id when it is not loaded.
    pub fn plugin_display_name(&self, id: &PluginId) -> String {
        let Some(manifest) = self
            .inner
            .plugins
            .existing_host()
            .and_then(|host| host.manifest(id))
        else {
            return id.to_string();
        };
        let mut message =
            LocalizedMessage::new("plugin-dialog-from").arg("plugin", manifest.name.as_str());
        self.inner
            .plugins
            .localize_plugin_arg(id, self.inner.locale.as_deref(), &mut message);
        match message.args.get("plugin") {
            Some(MessageArg::Text(name)) => name.clone(),
            _ => manifest.name,
        }
    }

    /// Title and body of the OS notification for a plug-in's `system`
    /// notification (desktop shell). The plug-in's name comes first, in
    /// the title ("Follow-up: Due today"), and line breaks of the
    /// plug-in's texts are collapsed, so that its text cannot push the
    /// attribution out of view and pose as Nail's own (M2d review).
    pub fn plugin_notification_texts(
        &self,
        notification: &nail_api::plugins::PluginNotification,
    ) -> (String, String) {
        let one_line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = self.render_localized(
            &LocalizedMessage::new("plugin-notification-os-title")
                .arg("plugin", self.plugin_display_name(&notification.plugin_id))
                .arg("title", one_line(&notification.spec.title)),
        );
        let body = notification
            .spec
            .body
            .as_deref()
            .map(one_line)
            .unwrap_or_default();
        (title, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(text: &str) -> Option<String> {
        header_only(&nail_search::parse_query(text).unwrap()).map(|q| q.to_string())
    }

    #[test]
    fn a_full_log_queue_drops_and_counts_lines() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        let dropped = Mutex::new(HashMap::new());
        let line = |plugin: &str| HostLogLine {
            plugin_id: PluginId::new(plugin),
            at_ms: 1,
            level: nail_api::plugins::PluginLogLevel::Info,
            source: nail_api::plugins::PluginLogSource::Api,
            message: "x".into(),
            localized: None,
        };
        for _ in 0..5 {
            enqueue_log(&tx, &dropped, line("a"));
        }
        enqueue_log(&tx, &dropped, line("b"));
        assert_eq!(rx.try_recv().unwrap().plugin_id.as_str(), "a");
        assert_eq!(rx.try_recv().unwrap().plugin_id.as_str(), "a");
        assert!(rx.try_recv().is_err());
        let mut reported = dropped_lines(&dropped);
        reported.sort_by(|x, y| x.plugin_id.as_str().cmp(y.plugin_id.as_str()));
        let counts: Vec<(String, String)> = reported
            .iter()
            .map(|l| {
                let m = l.localized.as_ref().unwrap();
                assert_eq!(m.id, nail_plugins::l10n::LOG_DROPPED);
                (l.plugin_id.to_string(), m.args["count"].to_string())
            })
            .collect();
        assert_eq!(counts, [("a".into(), "3".into()), ("b".into(), "1".into())]);
        assert!(dropped_lines(&dropped).is_empty(), "counted once");
    }

    #[test]
    fn header_only_queries_never_look_at_message_text() {
        assert_eq!(
            rewrite("invoice").as_deref(),
            Some("from:invoice OR to:invoice OR cc:invoice OR subject:invoice")
        );
        assert_eq!(
            rewrite("from:ada -\"due date\" is:unread").as_deref(),
            Some(
                "from:ada NOT (from:\"due date\" OR to:\"due date\" OR cc:\"due date\" OR subject:\"due date\") is:unread"
            )
        );
        assert_eq!(rewrite("").as_deref(), Some(""));
        assert_eq!(rewrite("body:otp"), None);
        assert_eq!(rewrite("subject:x OR NOT filename:pdf"), None);
        // The rewritten text parses again (OR groups are flattened).
        let text = rewrite("a (b OR subject:c) tag:x").unwrap();
        assert_eq!(
            nail_search::parse_query(&text).unwrap().to_string(),
            "(from:a OR to:a OR cc:a OR subject:a) (from:b OR to:b OR cc:b OR subject:b OR subject:c) tag:x"
        );
    }
}
