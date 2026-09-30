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

use nail_api::plugins::admin::MAX_PACKAGE_BYTES;
use nail_api::plugins::{
    PluginAttachment, PluginManifest, PluginMessage, PluginMessageBody, PluginPackageSource,
    PluginStorageUsage, message_uri,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, LocalizedMessage, MessageArg, MessageId, MessageSummary,
    PluginId, Request, Response, SearchQuery, ThreadId,
};
use nail_plugins::{
    AccountScope, BoxFuture, HostLimits, HostLogLine, HttpRequest, HttpResponse, PackageFiles,
    PackageStore, PluginCatalogs, PluginHost, PluginServices, PluginSpec, ServiceResult,
};
use nail_store::{PluginLogLine, PluginRecord};
use serde_json::Value;

use crate::error::Result;
use crate::plugin_http::PluginHttp;
use crate::service::{Inner, Service, lock};

/// Most messages one `nail.mail.search` returns.
const MAX_SEARCH: u32 = 500;

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
            self.inner
                .plugins
                .set_catalogs(id, Some(folder.display().to_string()), catalogs);
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
        let key = record_key(record);
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
    /// follows it (grants of permissions it no longer declares go).
    pub(crate) async fn sync_dev_manifests(&self) {
        let Some(host) = self.inner.plugins.existing_host().cloned() else {
            return;
        };
        let Ok(records) = self.store().call(|s| s.plugins()).await else {
            return;
        };
        for record in records {
            if !matches!(record.source, PluginPackageSource::Dev { .. }) {
                continue;
            }
            let Some(running) = host.manifest(&record.id) else {
                continue;
            };
            if running == record.manifest {
                continue;
            }
            let id = record.id.clone();
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
                Ok(_) => {
                    self.inner.plugins.forget(&id);
                    self.broadcast(Event::PluginsChanged);
                }
                Err(err) => tracing::warn!(%err, "cannot update a dev plug-in's manifest"),
            }
        }
        self.refresh_installed_plugins().await;
    }
}

fn record_key(record: &PluginRecord) -> Option<String> {
    match &record.source {
        PluginPackageSource::Dev { folder } => Some(folder.clone()),
        _ => record.package_sha256.clone(),
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
    /// Log lines in order to the writer task.
    logs: tokio::sync::mpsc::UnboundedSender<HostLogLine>,
}

impl CoreServices {
    /// The services of `service` and the task that stores log lines.
    fn start(service: &Service, handle: tokio::runtime::Handle) -> Self {
        let (logs, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostLogLine>();
        let weak = Arc::downgrade(&service.inner);
        handle.spawn(async move {
            while let Some(line) = rx.recv().await {
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
        }
    }

    fn service(&self) -> ServiceResult<Service> {
        self.service
            .upgrade()
            .map(|inner| Service { inner })
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "Nail is shutting down"))
    }
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
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>> {
        let service = self.service();
        Box::pin(async move {
            let service = service?;
            let limit = limit.clamp(1, MAX_SEARCH);
            let accounts: Vec<Option<AccountId>> = match &scope {
                AccountScope::All => vec![None],
                AccountScope::Only(accounts) => accounts.iter().cloned().map(Some).collect(),
            };
            let mut summaries: Vec<MessageSummary> = Vec::new();
            for account in accounts {
                let query = SearchQuery {
                    text: query.clone(),
                    account_id: account,
                    folder_id: None,
                    offset: 0,
                    limit,
                    sort: nail_api::search::SearchSort::Date,
                };
                match service
                    .search_with(query, crate::search::ServerSearchMode::Off)
                    .await
                    .map_err(api)?
                {
                    Response::SearchResults { hits, .. } => {
                        summaries.extend(hits.into_iter().map(|h| h.message));
                    }
                    Response::InvalidQuery { error } => {
                        return Err(ApiError::invalid_request(format!(
                            "invalid search: {}",
                            error.message
                        )));
                    }
                    _ => return Err(ApiError::internal("unexpected answer to Search")),
                }
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

    fn log(&self, line: HostLogLine) {
        let _ = self.logs.send(line);
    }
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
    /// notification (desktop shell): its texts, and the body names the
    /// plug-in ("from Follow-up"), so it cannot pose as Nail's own.
    pub fn plugin_notification_texts(
        &self,
        notification: &nail_api::plugins::PluginNotification,
    ) -> (String, String) {
        let from = self.render_localized(
            &LocalizedMessage::new("plugin-dialog-from")
                .arg("plugin", self.plugin_display_name(&notification.plugin_id)),
        );
        let body = match &notification.spec.body {
            Some(body) if !body.is_empty() => format!("{body}\n{from}"),
            _ => from,
        };
        (notification.spec.title.clone(), body)
    }
}
