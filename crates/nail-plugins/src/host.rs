//! [`PluginHost`]: starts, stops and talks to plug-in runtimes on behalf of
//! the core.
//!
//! The core creates one host with its [`PluginServices`] and a Tokio
//! runtime handle, starts every enabled plug-in ([`PluginHost::start`]) and
//! forwards the runtime requests (`RunPluginCommand`, `RespondPluginDialog`,
//! `PluginUiAction`, `GetPluginPanel`) to it. The host reports through the
//! services: events ([`PluginServices::emit`]: dialogs, notifications,
//! panels, `PluginStopped`, `PluginsChanged`) and log lines
//! ([`PluginServices::log`]).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak, mpsc};

use nail_api::plugins::{
    PluginCommandContext, PluginCommandSurface, PluginDialog, PluginGrant, PluginLogLevel,
    PluginLogSource, PluginManifest, PluginManifestIssue, PluginRunState, PluginStopReason,
    PluginUiNode, PluginUiTarget, is_supported_api_version, parse_plugin_command_id,
    parse_plugin_panel_id,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, LocalizedMessage, PluginDialogId, PluginId, UiContext,
};
use serde_json::Value;
use tokio::sync::{broadcast, oneshot};

use crate::dev::DevWatcher;
use crate::gate::Gate;
use crate::l10n;
use crate::limits::HostLimits;
use crate::package::{PackageError, PackageFiles, load_folder};
use crate::runtime::{self, Launch, ThreadEnd, ThreadMsg, host_line};
use crate::services::{AccountScope, HostLogLine, PluginServices};

/// What the host needs to start a plug-in.
#[derive(Debug, Clone)]
pub struct PluginSpec {
    /// The (valid) manifest.
    pub manifest: PluginManifest,
    /// The plug-in's files.
    pub files: PackageFiles,
    /// The user's grants (only declared permissions count).
    pub grants: Vec<PluginGrant>,
    /// The stored setting values (defaults fill the rest; invalid ones are
    /// ignored).
    pub settings: BTreeMap<String, Value>,
    /// For dev plug-ins: the folder, re-read on reload.
    pub dev_folder: Option<PathBuf>,
}

impl PluginSpec {
    /// A spec with no grants and default settings.
    pub fn new(manifest: PluginManifest, files: PackageFiles) -> Self {
        Self {
            manifest,
            files,
            grants: Vec::new(),
            settings: BTreeMap::new(),
            dev_folder: None,
        }
    }
}

/// Every setting: the defaults overlaid with the valid stored values.
pub fn effective_settings(
    manifest: &PluginManifest,
    stored: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut values = manifest.default_settings();
    let invalid = manifest.invalid_settings(stored);
    for (key, value) in stored {
        if !invalid.contains(key) {
            values.insert(key.clone(), value.clone());
        }
    }
    values
}

/// Why a dev folder could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DevLoadError {
    /// The folder or its manifest cannot be read.
    #[error(transparent)]
    Package(#[from] PackageError),
    /// The manifest has problems.
    #[error("the manifest has {} problems", .0.len())]
    Manifest(Vec<PluginManifestIssue>),
    /// The plug-in did not start.
    #[error("{}", .0.message)]
    Start(ApiError),
    /// The folder cannot be watched.
    #[error("cannot watch the folder: {0}")]
    Watch(String),
}

impl DevLoadError {
    /// As an API error.
    pub fn to_api_error(&self) -> ApiError {
        match self {
            Self::Package(e) => e.to_api_error(),
            Self::Manifest(issues) => {
                let mut error = ApiError::invalid_request(self.to_string());
                if let Some(first) = issues.first() {
                    error = error.with_localized(first.localized());
                }
                error
            }
            Self::Start(e) => e.clone(),
            Self::Watch(msg) => ApiError::internal(msg.clone()),
        }
    }
}

pub(crate) struct Slot {
    manifest: Arc<PluginManifest>,
    spec: PluginSpec,
    state: PluginRunState,
    error: Option<ApiError>,
    generation: u64,
    tx: Option<mpsc::Sender<ThreadMsg>>,
    alive: Arc<AtomicBool>,
    gate: Arc<RwLock<Gate>>,
    watcher: Option<DevWatcher>,
}

pub(crate) struct DialogEntry {
    pub plugin: PluginId,
    pub generation: u64,
    pub call_id: u64,
}

/// State shared by the host and its plug-in threads.
pub(crate) struct Inner {
    pub services: Arc<dyn PluginServices>,
    pub handle: tokio::runtime::Handle,
    pub limits: HostLimits,
    slots: Mutex<HashMap<PluginId, Slot>>,
    dialogs: Mutex<HashMap<PluginDialogId, DialogEntry>>,
    logs: broadcast::Sender<HostLogLine>,
    generation: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn stopped_message(reason: PluginStopReason, name: &str) -> LocalizedMessage {
    LocalizedMessage::new(reason.message_id()).arg("plugin", name)
}

impl Inner {
    /// Stores and broadcasts a log line.
    pub(crate) fn log(&self, line: HostLogLine) {
        self.services.log(line.clone());
        let _ = self.logs.send(line);
    }

    fn host_log(
        &self,
        id: &PluginId,
        level: PluginLogLevel,
        message: impl Into<String>,
        localized: LocalizedMessage,
    ) {
        self.log(host_line(
            id,
            level,
            PluginLogSource::Host,
            message,
            Some(localized),
        ));
    }

    /// The thread finished activation.
    pub(crate) fn activated(&self, id: &PluginId, generation: u64) {
        let version = {
            let mut slots = lock(&self.slots);
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            if slot.generation != generation || slot.state != PluginRunState::Starting {
                return;
            }
            slot.state = PluginRunState::Running;
            slot.error = None;
            slot.manifest.version.clone()
        };
        self.host_log(
            id,
            PluginLogLevel::Info,
            format!("started {id} {version}"),
            LocalizedMessage::new(l10n::STARTED)
                .arg("plugin", id.as_str())
                .arg("version", version.as_str()),
        );
        self.services.emit(Event::PluginsChanged);
    }

    /// Registers a dialog of a running plug-in; `false` when that
    /// plug-in's runtime is no longer the current one.
    pub(crate) fn register_dialog(&self, dialog: PluginDialogId, entry: DialogEntry) -> bool {
        let current = lock(&self.slots)
            .get(&entry.plugin)
            .is_some_and(|s| s.generation == entry.generation && s.alive.load(Ordering::SeqCst));
        if current {
            lock(&self.dialogs).insert(dialog, entry);
        }
        current
    }

    /// Announces a dialog unless it was closed meanwhile.
    pub(crate) fn emit_dialog_opened(&self, dialog: PluginDialog) {
        let dialogs = lock(&self.dialogs);
        if dialogs.contains_key(&dialog.dialog_id) {
            self.services.emit(Event::PluginDialogOpened {
                dialog: Box::new(dialog),
            });
        }
    }

    fn close_dialogs(&self, plugin: &PluginId) {
        let mut dialogs = lock(&self.dialogs);
        let ids: Vec<PluginDialogId> = dialogs
            .iter()
            .filter(|(_, e)| &e.plugin == plugin)
            .map(|(id, _)| id.clone())
            .collect();
        for dialog_id in ids {
            dialogs.remove(&dialog_id);
            self.services.emit(Event::PluginDialogClosed {
                dialog_id,
                plugin_id: plugin.clone(),
            });
        }
    }

    /// A plug-in thread ended.
    pub(crate) fn thread_ended(&self, id: &PluginId, generation: u64, end: ThreadEnd) {
        let ThreadEnd::Crash(crash) = end else {
            return;
        };
        let name = {
            let mut slots = lock(&self.slots);
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            // A runtime that was stopped (or replaced) meanwhile is not a
            // crash of the plug-in.
            if slot.generation != generation || slot.tx.is_none() {
                return;
            }
            slot.alive.store(false, Ordering::SeqCst);
            slot.tx = None;
            slot.state = PluginRunState::Crashed;
            let localized = stopped_message(crash.reason, &slot.manifest.name);
            slot.error = Some(
                ApiError::new(ErrorCode::Internal, crash.message.clone()).with_localized(localized),
            );
            slot.manifest.name.clone()
        };
        self.close_dialogs(id);
        let localized = stopped_message(crash.reason, &name);
        self.host_log(
            id,
            PluginLogLevel::Error,
            format!("stopped ({:?}): {}", crash.reason, crash.message),
            localized.clone(),
        );
        self.services.emit(Event::PluginStopped {
            plugin_id: id.clone(),
            reason: crash.reason,
            error: Some(crash.message),
            localized: Some(Box::new(localized)),
        });
        self.services.emit(Event::PluginsChanged);
    }

    /// Stops a runtime (not a crash). `true` when one was running.
    fn stop(&self, id: &PluginId, reason: PluginStopReason) -> bool {
        let (was_running, name) = {
            let mut slots = lock(&self.slots);
            let Some(slot) = slots.get_mut(id) else {
                return false;
            };
            let was_running = slot.tx.is_some();
            slot.alive.store(false, Ordering::SeqCst);
            // Nothing the old runtime still does is allowed any more.
            if let Ok(mut gate) = slot.gate.write() {
                *gate = Gate::default();
            }
            if let Some(tx) = slot.tx.take() {
                let _ = tx.send(ThreadMsg::Stop);
            }
            slot.state = match reason {
                PluginStopReason::Disabled => PluginRunState::Disabled,
                _ if slot.state == PluginRunState::Crashed && !was_running => {
                    PluginRunState::Crashed
                }
                _ => PluginRunState::Stopped,
            };
            if reason == PluginStopReason::Disabled {
                slot.error = None;
            }
            let name = slot.manifest.name.clone();
            if reason == PluginStopReason::Uninstalled {
                slots.remove(id);
            }
            (was_running, name)
        };
        self.close_dialogs(id);
        if was_running {
            let localized = stopped_message(reason, &name);
            self.host_log(
                id,
                PluginLogLevel::Info,
                format!("stopped ({reason:?})"),
                localized.clone(),
            );
            self.services.emit(Event::PluginStopped {
                plugin_id: id.clone(),
                reason,
                error: None,
                localized: Some(Box::new(localized)),
            });
        }
        self.services.emit(Event::PluginsChanged);
        was_running
    }

    /// The sender, gate and manifest of a running plug-in.
    fn running(
        &self,
        id: &PluginId,
    ) -> Result<(mpsc::Sender<ThreadMsg>, Gate, Arc<PluginManifest>), ApiError> {
        let slots = lock(&self.slots);
        let slot = slots
            .get(id)
            .ok_or_else(|| ApiError::not_found(format!("plug-in `{id}` is not loaded")))?;
        match (&slot.tx, slot.state) {
            (Some(tx), PluginRunState::Running) => Ok((
                tx.clone(),
                slot.gate.read().map(|g| g.clone()).unwrap_or_default(),
                Arc::clone(&slot.manifest),
            )),
            _ => Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("plug-in `{id}` is not running"),
            )),
        }
    }

    fn expand(&self) -> impl Fn(&[AccountId]) -> Vec<AccountId> + '_ {
        |a: &[AccountId]| self.services.expand_accounts(a)
    }
}

/// The plug-in host (see the crate docs).
#[derive(Clone)]
pub struct PluginHost {
    inner: Arc<Inner>,
    /// Stops every plug-in when the last clone of the host is dropped
    /// (plug-in threads keep `Inner` alive, not the guard).
    _guard: Arc<ShutdownGuard>,
}

struct ShutdownGuard(Weak<Inner>);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.0.upgrade() {
            let ids: Vec<PluginId> = lock(&inner.slots).keys().cloned().collect();
            for id in ids {
                inner.stop(&id, PluginStopReason::Shutdown);
            }
        }
    }
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost")
            .field("plugins", &self.plugins())
            .finish()
    }
}

impl PluginHost {
    /// A host whose API calls run on `handle`.
    pub fn new(
        services: Arc<dyn PluginServices>,
        handle: tokio::runtime::Handle,
        limits: HostLimits,
    ) -> Self {
        let (logs, _) = broadcast::channel(1024);
        let inner = Arc::new(Inner {
            services,
            handle,
            limits,
            slots: Mutex::new(HashMap::new()),
            dialogs: Mutex::new(HashMap::new()),
            logs,
            generation: AtomicU64::new(0),
        });
        Self {
            _guard: Arc::new(ShutdownGuard(Arc::downgrade(&inner))),
            inner,
        }
    }

    /// The limits.
    pub fn limits(&self) -> &HostLimits {
        &self.inner.limits
    }

    /// Log lines as they are written (the `nail plugin dev` console).
    pub fn subscribe_logs(&self) -> broadcast::Receiver<HostLogLine> {
        self.inner.logs.subscribe()
    }

    /// The loaded plug-ins.
    pub fn plugins(&self) -> Vec<PluginId> {
        let mut ids: Vec<PluginId> = lock(&self.inner.slots).keys().cloned().collect();
        ids.sort();
        ids
    }

    /// A plug-in's run state (`None` when the host does not know it).
    pub fn state(&self, id: &PluginId) -> Option<PluginRunState> {
        lock(&self.inner.slots).get(id).map(|s| s.state)
    }

    /// Why a plug-in crashed or failed to start.
    pub fn error(&self, id: &PluginId) -> Option<ApiError> {
        lock(&self.inner.slots)
            .get(id)
            .and_then(|s| s.error.clone())
    }

    /// The manifest the plug-in runs with (a dev reload may change it).
    pub fn manifest(&self, id: &PluginId) -> Option<PluginManifest> {
        lock(&self.inner.slots)
            .get(id)
            .map(|s| s.manifest.as_ref().clone())
    }

    /// Which start of the plug-in its slot holds: every start (and every
    /// dev reload) takes a new, larger number.
    pub fn generation(&self, id: &PluginId) -> Option<u64> {
        lock(&self.inner.slots).get(id).map(|s| s.generation)
    }

    /// Starts a plug-in (replacing a running instance, reason
    /// `reloaded`) and waits until its `activate` settled.
    pub async fn start(&self, spec: PluginSpec) -> Result<(), ApiError> {
        start(&self.inner, spec).await
    }

    /// Stops a plug-in (`disabled`, `uninstalled`, `shutdown`, …); `true`
    /// when it was running. `uninstalled` also forgets it.
    pub fn stop(&self, id: &PluginId, reason: PluginStopReason) -> bool {
        self.inner.stop(id, reason)
    }

    /// Stops every plug-in (`shutdown`).
    pub fn shutdown(&self) {
        for id in self.plugins() {
            self.inner.stop(&id, PluginStopReason::Shutdown);
        }
    }

    /// Replaces a plug-in's grants; they apply to its next API call.
    pub fn update_grants(&self, id: &PluginId, grants: &[PluginGrant]) -> bool {
        let mut slots = lock(&self.inner.slots);
        let Some(slot) = slots.get_mut(id) else {
            return false;
        };
        let gate = Gate::new(&slot.manifest, grants);
        if let Ok(mut g) = slot.gate.write() {
            *g = gate;
        }
        slot.spec.grants = grants.to_vec();
        true
    }

    /// New stored setting values; the plug-in sees the effective values
    /// (`nail.settings`, `onChange`).
    pub fn update_settings(&self, id: &PluginId, stored: BTreeMap<String, Value>) -> bool {
        let mut slots = lock(&self.inner.slots);
        let Some(slot) = slots.get_mut(id) else {
            return false;
        };
        let values = effective_settings(&slot.manifest, &stored);
        slot.spec.settings = stored;
        if let Some(tx) = &slot.tx {
            let _ = tx.send(ThreadMsg::Settings(values));
        }
        true
    }

    /// Restarts a plug-in with what it was started with (dev plug-ins
    /// re-read their folder).
    pub async fn reload(&self, id: &PluginId) -> Result<(), ApiError> {
        reload(&self.inner, id).await
    }

    /// Runs `plugin.<id>.<command>` with the UI's context. Returns once the
    /// plug-in took the call (its synchronous part ran).
    pub async fn run_command(
        &self,
        command: &str,
        surface: PluginCommandSurface,
        context: &UiContext,
        client_id: Option<String>,
    ) -> Result<(), ApiError> {
        let (plugin, name) = parse_plugin_command_id(command)
            .ok_or_else(|| ApiError::not_found(format!("`{command}` is not a plug-in command")))?;
        let (tx, gate, manifest) = self.inner.running(&plugin)?;
        if manifest.command(&name).is_none() {
            return Err(ApiError::not_found(format!(
                "`{command}` is not declared by the plug-in"
            )));
        }
        let context = self
            .filter_context(&gate, PluginCommandContext::from_ui(surface, context))
            .await;
        let (reply, rx) = oneshot::channel();
        send(
            &tx,
            ThreadMsg::Command {
                name,
                context,
                client_id,
                reply,
            },
        )?;
        self.wait(rx).await
    }

    /// Closes a dialog with a button (or dismisses it with `None`) and
    /// resolves the plug-in's `showDialog` promise with the values of its
    /// inputs (values of other ids or of the wrong shape are dropped).
    pub fn respond_dialog(
        &self,
        dialog_id: &PluginDialogId,
        button: Option<String>,
        values: BTreeMap<String, Value>,
    ) -> Result<(), ApiError> {
        let entry = lock(&self.inner.dialogs)
            .remove(dialog_id)
            .ok_or_else(|| ApiError::not_found("the dialog is no longer open"))?;
        self.inner.services.emit(Event::PluginDialogClosed {
            dialog_id: dialog_id.clone(),
            plugin_id: entry.plugin.clone(),
        });
        let slots = lock(&self.inner.slots);
        if let Some(slot) = slots.get(&entry.plugin)
            && slot.generation == entry.generation
            && let Some(tx) = &slot.tx
        {
            let _ = tx.send(ThreadMsg::DialogResponse {
                call_id: entry.call_id,
                button,
                values,
            });
        }
        Ok(())
    }

    /// A button inside a dialog body or panel was pressed.
    pub async fn ui_action(
        &self,
        target: &PluginUiTarget,
        component_id: &str,
        values: BTreeMap<String, Value>,
        client_id: Option<String>,
    ) -> Result<(), ApiError> {
        let (reply, rx) = oneshot::channel();
        match target {
            PluginUiTarget::Dialog { dialog_id } => {
                let (plugin, generation, call_id) = {
                    let dialogs = lock(&self.inner.dialogs);
                    let entry = dialogs
                        .get(dialog_id)
                        .ok_or_else(|| ApiError::not_found("the dialog is no longer open"))?;
                    (entry.plugin.clone(), entry.generation, entry.call_id)
                };
                let tx = {
                    let slots = lock(&self.inner.slots);
                    slots
                        .get(&plugin)
                        .filter(|s| s.generation == generation)
                        .and_then(|s| s.tx.clone())
                        .ok_or_else(|| ApiError::not_found("the dialog is no longer open"))?
                };
                send(
                    &tx,
                    ThreadMsg::DialogAction {
                        call_id,
                        component_id: component_id.to_owned(),
                        values,
                        client_id,
                        reply,
                    },
                )?;
            }
            PluginUiTarget::Panel { panel } => {
                let (plugin, name) = parse_plugin_panel_id(panel)
                    .ok_or_else(|| ApiError::not_found(format!("`{panel}` is not a panel")))?;
                let (tx, _, _) = self.inner.running(&plugin)?;
                send(
                    &tx,
                    ThreadMsg::PanelAction {
                        name,
                        component_id: component_id.to_owned(),
                        values,
                        client_id,
                        reply,
                    },
                )?;
            }
            // Banner buttons of open hooks (M4a, ADR 0079): WP102.
            PluginUiTarget::Banner { .. } => {
                return Err(ApiError::unsupported("banner actions are not wired yet"));
            }
        }
        self.wait(rx).await
    }

    /// Renders a panel (`<plugin>.<panel>`) for the UI's context.
    pub async fn panel(
        &self,
        panel: &str,
        context: &UiContext,
        client_id: Option<String>,
    ) -> Result<Vec<PluginUiNode>, ApiError> {
        let (plugin, name) = parse_plugin_panel_id(panel)
            .ok_or_else(|| ApiError::not_found(format!("`{panel}` is not a panel")))?;
        let (tx, gate, manifest) = self.inner.running(&plugin)?;
        if manifest.panel(&name).is_none() {
            return Err(ApiError::not_found(format!(
                "`{panel}` is not declared by the plug-in"
            )));
        }
        let context = self
            .filter_context(
                &gate,
                PluginCommandContext::from_ui(PluginCommandSurface::Global, context),
            )
            .await;
        let (reply, rx) = oneshot::channel();
        send(
            &tx,
            ThreadMsg::Panel {
                name,
                context,
                client_id,
                reply,
            },
        )?;
        self.wait(rx).await
    }

    async fn wait<T>(&self, rx: oneshot::Receiver<Result<T, ApiError>>) -> Result<T, ApiError> {
        match tokio::time::timeout(self.inner.limits.reply_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ApiError::new(ErrorCode::Unavailable, "the plug-in stopped")),
            Err(_) => Err(ApiError::new(
                ErrorCode::Unavailable,
                "the plug-in did not answer in time",
            )),
        }
    }

    /// Drops from a command context what the plug-in may not read: without
    /// `mail.read` every message, thread, account, folder and draft; with it
    /// the messages outside its accounts.
    async fn filter_context(
        &self,
        gate: &Gate,
        mut ctx: PluginCommandContext,
    ) -> PluginCommandContext {
        let services = &self.inner.services;
        let Ok(scope) = gate.mail_scope("context", &self.inner.expand()) else {
            return PluginCommandContext {
                surface: ctx.surface,
                ..PluginCommandContext::default()
            };
        };
        let mut ids = ctx.selection.message_ids.clone();
        ids.extend(ctx.selection.open_message_id.clone());
        let readable = crate::tree::readable_messages(services.as_ref(), Some(&scope), ids).await;
        ctx.selection
            .message_ids
            .retain(|id| readable.contains_key(id));
        if ctx
            .selection
            .open_message_id
            .as_ref()
            .is_some_and(|id| !readable.contains_key(id))
        {
            ctx.selection.open_message_id = None;
            ctx.selection.thread_id = None;
        }
        if let Some(account) = &ctx.account_id {
            let allowed = match &scope {
                AccountScope::All => true,
                AccountScope::Only(_) => {
                    let real = services.expand_accounts(std::slice::from_ref(account));
                    !real.is_empty() && real.iter().all(|a| scope.contains(a))
                }
            };
            if !allowed {
                ctx.account_id = None;
                ctx.folder_id = None;
            }
        }
        ctx
    }

    /// Loads a plug-in folder in dev mode: reads and checks its manifest,
    /// starts it with `grants` and reloads it when its files change
    /// (`notify` watcher; `node_modules` and `.git` are ignored). Returns the
    /// manifest.
    pub async fn load_dev(
        &self,
        folder: &Path,
        grants: Vec<PluginGrant>,
        settings: BTreeMap<String, Value>,
    ) -> Result<PluginManifest, DevLoadError> {
        let (manifest, files) = load_folder(folder)?;
        let manifest = manifest.map_err(DevLoadError::Manifest)?;
        let id = manifest.id.clone();
        let spec = PluginSpec {
            manifest: manifest.clone(),
            files,
            grants,
            settings,
            dev_folder: Some(folder.to_path_buf()),
        };
        let started = self.start(spec).await;
        let weak = Arc::downgrade(&self.inner);
        let handle = self.inner.handle.clone();
        let watch_id = id.clone();
        let watcher = DevWatcher::watch(folder, move || {
            let weak: Weak<Inner> = weak.clone();
            let id = watch_id.clone();
            handle.spawn(async move {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // A dev plug-in the user switched off stays off.
                let enabled = lock(&inner.slots)
                    .get(&id)
                    .is_some_and(|s| s.state != PluginRunState::Disabled);
                if enabled {
                    let _ = reload(&inner, &id).await;
                }
            });
        })
        .map_err(|e| DevLoadError::Watch(e.to_string()))?;
        if let Some(slot) = lock(&self.inner.slots).get_mut(&id) {
            slot.watcher = Some(watcher);
        }
        started.map_err(DevLoadError::Start)?;
        Ok(manifest)
    }
}

fn send(tx: &mpsc::Sender<ThreadMsg>, message: ThreadMsg) -> Result<(), ApiError> {
    tx.send(message)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "the plug-in stopped"))
}

async fn start(inner: &Arc<Inner>, spec: PluginSpec) -> Result<(), ApiError> {
    let id = spec.manifest.id.clone();
    let manifest = Arc::new(spec.manifest.clone());
    let watcher = {
        let mut slots = lock(&inner.slots);
        slots.get_mut(&id).and_then(|s| s.watcher.take())
    };
    if lock(&inner.slots).get(&id).is_some_and(|s| s.tx.is_some()) {
        inner.stop(&id, PluginStopReason::Reloaded);
    }
    let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let gate = Arc::new(RwLock::new(Gate::new(&manifest, &spec.grants)));
    let alive = Arc::new(AtomicBool::new(true));
    if !is_supported_api_version(&manifest.api_version) {
        let error = ApiError::unsupported(format!(
            "plug-in API version {} is not supported",
            manifest.api_version
        ))
        .with_localized(
            LocalizedMessage::new("plugin-manifest-unsupported-api-version")
                .arg("path", "apiVersion")
                .arg("detail", manifest.api_version.as_str()),
        );
        lock(&inner.slots).insert(
            id,
            Slot {
                manifest,
                spec,
                state: PluginRunState::Incompatible,
                error: Some(error.clone()),
                generation,
                tx: None,
                alive,
                gate,
                watcher,
            },
        );
        inner.services.emit(Event::PluginsChanged);
        return Err(error);
    }
    let (tx, rx) = mpsc::channel();
    let (reply, activated) = oneshot::channel();
    let launch = Launch {
        id: id.clone(),
        generation,
        manifest: Arc::clone(&manifest),
        files: spec.files.clone(),
        gate: Arc::clone(&gate),
        settings: effective_settings(&manifest, &spec.settings),
        dev: spec.dev_folder.is_some(),
        alive: Arc::clone(&alive),
        rx,
        tx: tx.clone(),
        activated: reply,
    };
    lock(&inner.slots).insert(
        id.clone(),
        Slot {
            manifest,
            spec,
            state: PluginRunState::Starting,
            error: None,
            generation,
            tx: Some(tx),
            alive,
            gate,
            watcher,
        },
    );
    inner.services.emit(Event::PluginsChanged);
    let thread_inner = Arc::clone(inner);
    let thread_id = id.clone();
    let spawned = std::thread::Builder::new()
        .name(format!("nail-plugin-{id}"))
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let end = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime::run(Arc::clone(&thread_inner), launch)
            }))
            .unwrap_or_else(|_| {
                ThreadEnd::Crash(runtime::Crash {
                    reason: PluginStopReason::Error,
                    message: "the plug-in runtime panicked".to_owned(),
                })
            });
            thread_inner.thread_ended(&thread_id, generation, end);
        });
    if let Err(e) = spawned {
        inner.thread_ended(
            &id,
            generation,
            ThreadEnd::Crash(runtime::Crash {
                reason: PluginStopReason::Error,
                message: format!("cannot start a thread: {e}"),
            }),
        );
        return Err(ApiError::internal(e.to_string()));
    }
    let outcome = tokio::time::timeout(inner.limits.activation_timeout, activated).await;
    let result = match outcome {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(lock(&inner.slots)
            .get(&id)
            .and_then(|s| s.error.clone())
            .unwrap_or_else(|| ApiError::new(ErrorCode::Unavailable, "the plug-in stopped"))),
        Err(_) => {
            let crash = runtime::Crash {
                reason: PluginStopReason::Error,
                message: "activate did not finish in time".to_owned(),
            };
            let error = crash.to_api_error();
            if let Some(tx) = lock(&inner.slots)
                .get(&id)
                .filter(|s| s.generation == generation)
                .and_then(|s| s.tx.clone())
            {
                let _ = tx.send(ThreadMsg::Stop);
            }
            inner.thread_ended(&id, generation, ThreadEnd::Crash(crash));
            Err(error)
        }
    };
    if let Err(error) = &result {
        inner.host_log(
            &id,
            PluginLogLevel::Error,
            format!("could not start: {}", error.message),
            LocalizedMessage::new(l10n::START_FAILED)
                .arg("plugin", id.as_str())
                .arg("detail", error.message.as_str()),
        );
    }
    result
}

async fn reload(inner: &Arc<Inner>, id: &PluginId) -> Result<(), ApiError> {
    let spec = lock(&inner.slots)
        .get(id)
        .map(|s| s.spec.clone())
        .ok_or_else(|| ApiError::not_found(format!("plug-in `{id}` is not loaded")))?;
    let Some(folder) = spec.dev_folder.clone() else {
        return start(inner, spec).await;
    };
    inner.host_log(
        id,
        PluginLogLevel::Info,
        "files changed; reloading",
        LocalizedMessage::new(l10n::RELOADING).arg("plugin", id.as_str()),
    );
    let loaded = load_folder(&folder);
    let (manifest, files) = match loaded {
        Ok((Ok(manifest), files)) if manifest.id == *id => (manifest, files),
        other => {
            let issues: Vec<(String, Option<LocalizedMessage>)> = match other {
                Ok((Err(issues), _)) => issues
                    .iter()
                    .map(|i| (format!("{:?} at {}", i.kind, i.path), Some(i.localized())))
                    .collect(),
                Ok((Ok(m), _)) => vec![(format!("the manifest id changed to {}", m.id), None)],
                Err(e) => vec![(e.to_string(), Some(e.localized()))],
            };
            inner.host_log(
                id,
                PluginLogLevel::Error,
                "the manifest is not valid; keeping the plug-in stopped",
                LocalizedMessage::new(l10n::RELOAD_INVALID).arg("plugin", id.as_str()),
            );
            for (message, localized) in issues {
                inner.log(host_line(
                    id,
                    PluginLogLevel::Error,
                    PluginLogSource::Host,
                    message,
                    localized,
                ));
            }
            inner.stop(id, PluginStopReason::Reloaded);
            return Err(ApiError::invalid_request("the manifest is not valid"));
        }
    };
    start(
        inner,
        PluginSpec {
            manifest,
            files,
            ..spec
        },
    )
    .await
}
