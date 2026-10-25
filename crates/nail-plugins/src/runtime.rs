//! The runtime of one plug-in: its own OS thread owning a QuickJS
//! [`Runtime`] and [`Context`] (ADR 0051, "Threading").
//!
//! The thread runs an event loop over [`ThreadMsg`]s. Every entry into
//! plug-in code is a **turn**: the CPU budget is armed, the prelude's
//! `dispatch` (or the module import at the start) is called, the promise
//! jobs it queued are drained, and the budget is disarmed. The interrupt
//! handler stops a turn that runs past its budget; the plug-in is then
//! stopped and flagged (`cpu-budget`). Uncaught errors of handlers
//! (`error`) and exhausted memory (`memory-limit`) stop it too. Other
//! plug-ins are unaffected: they run on their own threads with their own
//! runtimes.
//!
//! API calls that need the core ([`crate::PluginServices`]) are spawned on
//! the host's Tokio runtime; their results come back as
//! [`ThreadMsg::Resolve`] and resolve the plug-in's promise in a later
//! turn.
//!
//! The sandbox: a full QuickJS context (the language and its standard
//! objects) plus the `nail` API, `console` and timers from the prelude.
//! QuickJS's `std` / `os` modules are never loaded; the module loader only
//! serves files of the plug-in's own package.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

use crate::cpu_clock::thread_cpu_time;

use base64::Engine as _;
use nail_api::agents::{ActionActor, ActionAttribution, McpClientRef};
use nail_api::plugins::admin::{MAX_LOG_MESSAGE, MAX_STORAGE_KEY, MAX_STORAGE_VALUE};
use nail_api::plugins::extensions::{ComposerEdit, ComposerEditOp, ComposerTextFormat};
use nail_api::plugins::ui::{PluginValueKind, is_value_id};
use nail_api::plugins::{
    PluginCommandContext, PluginDialog, PluginDialogResult, PluginDialogSpec, PluginHookKind,
    PluginLogLevel, PluginLogSource, PluginManifest, PluginNotification, PluginNotificationSpec,
    PluginPermission, PluginStopReason, PluginUiIssue, PluginUiNode, plugin_panel_id,
};
use nail_api::{
    AccountId, ApiError, DraftId, ErrorCode, Event, LocalizedMessage, MessageArg, MessageId,
    PluginDialogId, PluginId, ThreadId,
};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{Context, Ctx, Function, Module, Object, Persistent, Runtime, TypedArray, Value};
use serde_json::json;
use tokio::sync::oneshot;

use crate::gate::{ClientLimit, Denied, FetchInit, FetchRefusal, Gate};
use crate::host::{DialogEntry, Inner};
use crate::jobs::{CpuMeter, minute_of};
use crate::l10n::{self, PluginCatalogs};
use crate::package::{PackageFiles, is_safe_path};
use crate::services::{AccountScope, HostLogLine, HttpRequest, PluginWriteRequest};
use crate::tree;
use crate::writes::{self, Blobs};

/// The gate's scope of `$call` by `$check` (`mail_scope`, `body_scope`),
/// or (returned) the call's denial.
macro_rules! scope_or_deny {
    ($s:expr, $check:ident, $call:expr) => {
        match $s.gate().$check($call, &$s.expand()) {
            Ok(scope) => scope,
            Err(d) => return Ok($s.deny(&d)),
        }
    };
}

/// Longest text of a composer edit (bytes).
pub(crate) const MAX_COMPOSER_TEXT: usize = 256 * 1024;

/// Who a turn runs for: the UI whose request started it (dialogs and
/// notifications go there) and, while a plug-in serves an exposed MCP
/// tool, the client it acts for (its writes are attributed to both) and
/// what that client may reach (the gate is narrowed to it, review M4a).
/// Pending API calls and timers carry the origin of the turn that started
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Origin {
    pub client: Option<String>,
    pub on_behalf: Option<McpClientRef>,
    pub limit: Option<Arc<ClientLimit>>,
}

impl Origin {
    pub(crate) fn client(client: Option<String>) -> Self {
        Self {
            client,
            on_behalf: None,
            limit: None,
        }
    }
}

/// The kinds of handlers a plug-in registers in code (M4a). Each is
/// declared in the manifest; the host calls them with [`Invoke`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Registry {
    /// A mail hook (`name` = the hook kind).
    Hook,
    /// Banner buttons of the open hook (`name` = `default`).
    BannerAction,
    ComposerAction,
    MarkdownBlock,
    AttachmentPreview,
    AttachmentAction,
    LinkAction,
    Job,
    McpTool,
}

impl Registry {
    const ALL: [Registry; 9] = [
        Self::Hook,
        Self::BannerAction,
        Self::ComposerAction,
        Self::MarkdownBlock,
        Self::AttachmentPreview,
        Self::AttachmentAction,
        Self::LinkAction,
        Self::Job,
        Self::McpTool,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::BannerAction => "bannerAction",
            Self::ComposerAction => "composerAction",
            Self::MarkdownBlock => "markdownBlock",
            Self::AttachmentPreview => "attachmentPreview",
            Self::AttachmentAction => "attachmentAction",
            Self::LinkAction => "linkAction",
            Self::Job => "job",
            Self::McpTool => "mcpTool",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == text)
    }

    /// Whether the manifest declares a handler of this kind and name.
    pub(crate) fn declared(self, manifest: &PluginManifest, name: &str) -> bool {
        let c = &manifest.contributes;
        match self {
            Self::Hook => PluginHookKind::parse(name).is_some_and(|k| manifest.hook(k).is_some()),
            Self::BannerAction => {
                name == "default" && manifest.hook(PluginHookKind::Open).is_some()
            }
            Self::ComposerAction => c.composer_actions.iter().any(|a| a.id == name),
            Self::MarkdownBlock => c.markdown_blocks.iter().any(|a| a.id == name),
            Self::AttachmentPreview => c.attachment_previews.iter().any(|a| a.id == name),
            Self::AttachmentAction => c.attachment_actions.iter().any(|a| a.id == name),
            Self::LinkAction => c.link_actions.iter().any(|a| a.id == name),
            Self::Job => manifest.job(name).is_some(),
            Self::McpTool => c.mcp_tools.iter().any(|a| a.id == name),
        }
    }
}

/// A call of a registered handler: it settles with the handler's
/// (awaited) result or its error — a throw never stops the plug-in.
#[derive(Debug)]
pub(crate) struct Invoke {
    pub registry: Registry,
    pub name: String,
    pub input: serde_json::Value,
    /// Handed to the handler as a second argument (`Uint8Array`).
    pub bytes: Option<Vec<u8>>,
    pub origin: Origin,
    /// A composer action: the plug-in may edit this draft for a while.
    pub composer_draft: Option<DraftId>,
}

impl Invoke {
    pub(crate) fn new(registry: Registry, name: &str, input: serde_json::Value) -> Self {
        Self {
            registry,
            name: name.to_owned(),
            input,
            bytes: None,
            origin: Origin::default(),
            composer_draft: None,
        }
    }
}

const PRELUDE: &str = include_str!("prelude.js");

/// The reply channel of a call that waits for the plug-in.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, ApiError>>;

/// What a plug-in thread is asked to do.
pub(crate) enum ThreadMsg {
    /// An API call finished (`bytes`: a binary body handed to the plug-in
    /// as a `Uint8Array` next to the value).
    Resolve {
        id: u64,
        result: Result<serde_json::Value, ApiError>,
        bytes: Option<Vec<u8>>,
    },
    /// Call a registered handler (M4a).
    Invoke(Box<Invoke>, Reply<serde_json::Value>),
    /// Run a registered command.
    Command {
        name: String,
        context: PluginCommandContext,
        client_id: Option<String>,
        reply: Reply<()>,
    },
    /// A dialog was answered or dismissed.
    DialogResponse {
        call_id: u64,
        button: Option<String>,
        values: BTreeMap<String, serde_json::Value>,
    },
    /// A button inside a dialog body was pressed.
    DialogAction {
        call_id: u64,
        component_id: String,
        values: BTreeMap<String, serde_json::Value>,
        client_id: Option<String>,
        reply: Reply<()>,
    },
    /// Render a panel.
    Panel {
        name: String,
        context: PluginCommandContext,
        client_id: Option<String>,
        reply: Reply<Vec<PluginUiNode>>,
    },
    /// A button inside a panel was pressed.
    PanelAction {
        name: String,
        component_id: String,
        values: BTreeMap<String, serde_json::Value>,
        client_id: Option<String>,
        reply: Reply<()>,
    },
    /// The settings changed (effective values).
    Settings(BTreeMap<String, serde_json::Value>),
    /// Stop (call `deactivate`, then end the thread).
    Stop,
}

/// Why a plug-in crashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Crash {
    pub reason: PluginStopReason,
    pub message: String,
}

impl Crash {
    fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        let reason = if message.contains("out of memory") {
            PluginStopReason::MemoryLimit
        } else {
            PluginStopReason::Error
        };
        Self { reason, message }
    }

    fn cpu(budget: Duration) -> Self {
        Self {
            reason: PluginStopReason::CpuBudget,
            message: format!("used more than its CPU time budget of {budget:?}"),
        }
    }

    pub(crate) fn to_api_error(&self) -> ApiError {
        ApiError::new(ErrorCode::Unavailable, self.message.clone())
            .with_localized(LocalizedMessage::new(self.reason.message_id()))
    }
}

/// How a plug-in thread ended.
pub(crate) enum ThreadEnd {
    /// It was asked to stop.
    Requested,
    /// It crashed.
    Crash(Crash),
}

/// Everything a new plug-in thread needs.
pub(crate) struct Launch {
    pub id: PluginId,
    pub generation: u64,
    pub manifest: Arc<PluginManifest>,
    pub files: PackageFiles,
    pub gate: Arc<RwLock<Gate>>,
    pub settings: BTreeMap<String, serde_json::Value>,
    pub dev: bool,
    pub alive: Arc<AtomicBool>,
    pub rx: mpsc::Receiver<ThreadMsg>,
    pub tx: mpsc::Sender<ThreadMsg>,
    pub activated: Reply<()>,
    /// The plug-in's own catalogs (`nail.l10n`).
    pub catalogs: Arc<PluginCatalogs>,
    /// The core's language.
    pub locale: Option<String>,
    /// The plug-in's name in that language (attribution of its writes).
    pub display_name: String,
    /// CPU time of its turns (the hourly quota).
    pub cpu: Arc<Mutex<CpuMeter>>,
}

/// The CPU budget of the current turn, checked by the interrupt handler on
/// the plug-in's thread against that thread's CPU time (ADR 0204: waiting
/// for a processor does not count). A stopped plug-in (disabled,
/// uninstalled, reloaded) is interrupted at once, whatever budget its turn
/// has left (M2d review).
struct Budget {
    /// The thread's CPU time in nanoseconds when the turn must end; 0 = no
    /// turn running.
    deadline: AtomicU64,
    fired: AtomicBool,
    /// Cleared by the host when it stops the plug-in.
    alive: Arc<AtomicBool>,
    /// Set while `deactivate` runs (its own short budget, no API access).
    deactivating: AtomicBool,
}

impl Budget {
    fn new(alive: Arc<AtomicBool>) -> Self {
        Self {
            deadline: AtomicU64::new(0),
            fired: AtomicBool::new(false),
            alive,
            deactivating: AtomicBool::new(false),
        }
    }

    /// The calling thread's CPU time in nanoseconds (always the plug-in's:
    /// turns are armed and checked on its thread).
    fn now() -> u64 {
        u64::try_from(thread_cpu_time().as_nanos()).unwrap_or(u64::MAX)
    }

    fn arm(&self, budget: Duration) {
        self.fired.store(false, Ordering::SeqCst);
        let nanos = u64::try_from(budget.as_nanos()).unwrap_or(u64::MAX / 2);
        self.deadline
            .store(Self::now().saturating_add(nanos).max(1), Ordering::SeqCst);
    }

    /// Disarms; whether the budget was exceeded.
    fn disarm(&self) -> bool {
        self.deadline.store(0, Ordering::SeqCst);
        self.fired.load(Ordering::SeqCst)
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    /// The interrupt handler: `true` stops the running code.
    fn check(&self) -> bool {
        if !self.alive.load(Ordering::Relaxed) && !self.deactivating.load(Ordering::Relaxed) {
            return true;
        }
        let deadline = self.deadline.load(Ordering::Relaxed);
        if deadline != 0 && Self::now() > deadline {
            self.fired.store(true, Ordering::SeqCst);
            return true;
        }
        false
    }
}

struct PendingCall {
    origin: Origin,
    op: &'static str,
}

/// The inputs and buttons of the tree a panel last rendered (UI actions
/// are checked against it).
struct PanelTree {
    kinds: HashMap<String, PluginValueKind>,
    buttons: HashSet<String>,
}

/// A panel render waiting for the plug-in: the panel, the UI, the reply.
type PanelRequest = (String, Option<String>, Reply<Vec<PluginUiNode>>);

/// A draft a composer action of this plug-in ran on: it may edit it until
/// `until`.
struct ComposerGrant {
    client: Option<String>,
    until: Instant,
    seq: u64,
}

struct OpenDialog {
    origin: Origin,
    kinds: HashMap<String, PluginValueKind>,
    buttons: HashSet<String>,
    actions: HashSet<String>,
}

struct Timer {
    due: Instant,
    origin: Origin,
}

/// A token bucket over every log line a plug-in causes (its own lines,
/// refused calls, host lines about it): a burst of `per_minute` lines,
/// refilled at `per_minute` per minute.
struct LogRate {
    tokens: f64,
    last: Instant,
}

impl LogRate {
    fn new(per_minute: u32) -> Self {
        Self {
            tokens: f64::from(per_minute),
            last: Instant::now(),
        }
    }

    /// The time one line takes to refill.
    fn interval(per_minute: u32) -> Duration {
        Duration::from_secs_f64(60.0 / f64::from(per_minute.max(1)))
    }

    /// Takes one line; `false` when the plug-in is over its rate.
    fn take(&mut self, per_minute: u32) -> bool {
        let now = Instant::now();
        let capacity = f64::from(per_minute);
        let refill = now.duration_since(self.last).as_secs_f64() * capacity / 60.0;
        self.tokens = (self.tokens + refill).min(capacity);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The thread's state, shared with the native `hostCall` function.
struct State {
    id: PluginId,
    generation: u64,
    manifest: Arc<PluginManifest>,
    files: PackageFiles,
    gate: Arc<RwLock<Gate>>,
    inner: Arc<Inner>,
    tx: mpsc::Sender<ThreadMsg>,
    alive: Arc<AtomicBool>,
    dev: bool,
    /// Who the turn runs for (`client_id` of what it raises, MCP client of
    /// its writes).
    origin: Origin,
    next_id: u64,
    calls: HashMap<u64, PendingCall>,
    commands: HashSet<String>,
    panels: HashSet<String>,
    dialogs: HashMap<u64, OpenDialog>,
    timers: HashMap<u64, Timer>,
    panel_requests: HashMap<u64, PanelRequest>,
    fatal: Option<String>,
    activated: bool,
    logs_this_turn: u32,
    dropped_logs: u32,
    log_rate: LogRate,
    notifications: VecDeque<Instant>,
    // --- M4a ---
    /// Handlers registered in code (kind, name).
    registered: HashSet<(Registry, String)>,
    /// Handler calls waiting for their result.
    invocations: HashMap<u64, Reply<serde_json::Value>>,
    /// The last tree of each panel, per UI (`None`: an update for all).
    panel_trees: HashMap<(String, Option<String>), (u64, PanelTree)>,
    panel_seq: u64,
    /// Drafts composer actions ran on.
    composer_drafts: HashMap<DraftId, ComposerGrant>,
    /// Bytes for the plug-in (`hostTakeBytes`), cleared after each turn.
    blobs_in: HashMap<u64, Vec<u8>>,
    /// Bytes from the plug-in (`hostPutBytes`), cleared after each turn.
    blobs_out: Blobs,
    blob_out_bytes: usize,
    catalogs: Arc<PluginCatalogs>,
    locale: Option<String>,
    display_name: String,
    cpu: Arc<Mutex<CpuMeter>>,
}

/// The answer of `hostCall`.
enum Answer {
    Ok(serde_json::Value),
    Pending(u64),
    Error(ApiError),
}

impl Answer {
    fn encode(self) -> String {
        let value = match self {
            Self::Ok(v) => json!({ "ok": v }),
            Self::Pending(id) => json!({ "pending": id }),
            Self::Error(e) => json!({ "error": error_json(&e) }),
        };
        value.to_string()
    }
}

fn error_json(e: &ApiError) -> serde_json::Value {
    json!({ "code": e.code.as_str(), "message": e.message })
}

fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let end = text.floor_char_boundary(max);
    text[..end].to_owned()
}

fn now_ms() -> i64 {
    i64::try_from(nail_util::time::unix_now_millis()).unwrap_or(i64::MAX)
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::invalid_request(message)
}

fn to_json(value: impl serde::Serialize) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal(e.to_string()))
}

fn str_field<'a>(payload: &'a serde_json::Value, name: &str) -> Result<&'a str, ApiError> {
    payload
        .get(name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid(format!("`{name}` must be a string")))
}

/// Writes a host / API line to a plug-in's log.
pub(crate) fn host_line(
    id: &PluginId,
    level: PluginLogLevel,
    source: PluginLogSource,
    message: impl Into<String>,
    localized: Option<LocalizedMessage>,
) -> HostLogLine {
    HostLogLine {
        plugin_id: id.clone(),
        at_ms: now_ms(),
        level,
        source,
        message: cut(&message.into(), MAX_LOG_MESSAGE),
        localized,
    }
}

impl State {
    fn log(
        &mut self,
        level: PluginLogLevel,
        source: PluginLogSource,
        message: impl Into<String>,
        localized: Option<LocalizedMessage>,
    ) {
        // Every line the plug-in causes counts: its own, refused calls
        // (`api`) and host lines about what it did (M2d review: a loop of
        // refused calls wrote tens of thousands of lines per turn).
        self.logs_this_turn += 1;
        let limits = &self.inner.limits;
        if self.logs_this_turn > limits.max_log_lines_per_call
            || !self.log_rate.take(limits.max_log_lines_per_minute)
        {
            self.dropped_logs = self.dropped_logs.saturating_add(1);
            return;
        }
        self.inner
            .log(host_line(&self.id, level, source, message, localized));
    }

    /// At the end of a turn: one line saying how many were dropped, once
    /// the rate allows it (until then the count grows).
    fn log_dropped(&mut self) {
        if self.dropped_logs == 0
            || !self
                .log_rate
                .take(self.inner.limits.max_log_lines_per_minute)
        {
            return;
        }
        let dropped = std::mem::take(&mut self.dropped_logs);
        self.inner.log(host_line(
            &self.id,
            PluginLogLevel::Warn,
            PluginLogSource::Host,
            format!("{dropped} log lines dropped"),
            Some(LocalizedMessage::new(l10n::LOG_DROPPED).arg("count", u64::from(dropped))),
        ));
    }

    fn deny(&mut self, denied: &Denied) -> Answer {
        let error = denied.to_api_error();
        self.log(
            PluginLogLevel::Warn,
            PluginLogSource::Api,
            error.message.clone(),
            Some(denied.localized()),
        );
        Answer::Error(error)
    }

    /// The plug-in's gate, narrowed to the MCP client the turn serves.
    fn gate(&self) -> Gate {
        let gate = self.gate.read().map(|g| g.clone()).unwrap_or_default();
        match &self.origin.limit {
            Some(limit) => gate.limited(Arc::clone(limit)),
            None => gate,
        }
    }

    /// Folders the served MCP client excludes: messages there are not
    /// found.
    fn hidden_folders(&self) -> Vec<nail_api::FolderId> {
        self.origin
            .limit
            .as_ref()
            .map(|l| l.excluded_folders.clone())
            .unwrap_or_default()
    }

    fn expand(&self) -> impl Fn(&[AccountId]) -> Vec<AccountId> + use<> {
        let services = Arc::clone(&self.inner.services);
        move |a: &[AccountId]| services.expand_accounts(a)
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn check_capacity(&self) -> Result<(), ApiError> {
        if self.calls.len() >= self.inner.limits.max_pending_calls {
            return Err(ApiError::new(
                ErrorCode::RateLimited,
                "too many API calls are waiting",
            ));
        }
        Ok(())
    }

    /// Registers a pending call whose result a Tokio task computes.
    fn spawn<F>(&mut self, op: &'static str, payload: &serde_json::Value, fut: F) -> Answer
    where
        F: Future<Output = Result<serde_json::Value, ApiError>> + Send + 'static,
    {
        self.spawn_bytes(op, payload, async move { fut.await.map(|v| (v, None)) })
    }

    /// Like [`Self::spawn`], with bytes handed to the plug-in next to the
    /// value (binary fetch bodies).
    fn spawn_bytes<F>(&mut self, op: &'static str, payload: &serde_json::Value, fut: F) -> Answer
    where
        F: Future<Output = Result<(serde_json::Value, Option<Vec<u8>>), ApiError>> + Send + 'static,
    {
        if let Err(e) = self.check_capacity() {
            return Answer::Error(e);
        }
        let id = self.next_id();
        self.calls.insert(
            id,
            PendingCall {
                origin: self.origin.clone(),
                op,
            },
        );
        if self.dev {
            self.log(
                PluginLogLevel::Debug,
                PluginLogSource::Api,
                cut(&format!("{op} {payload}"), 500),
                None,
            );
        }
        let tx = self.tx.clone();
        self.inner.handle.spawn(async move {
            let (result, bytes) = match fut.await {
                Ok((value, bytes)) => (Ok(value), bytes),
                Err(e) => (Err(e), None),
            };
            drop(tx.send(ThreadMsg::Resolve { id, result, bytes }));
        });
        Answer::Pending(id)
    }

    /// Who writes: this plug-in (and the MCP client it serves).
    fn attribution(&self) -> ActionAttribution {
        ActionAttribution {
            actor: ActionActor::plugin(self.id.clone()),
            name: self.display_name.clone(),
            on_behalf_of: self.origin.on_behalf.clone(),
        }
    }

    /// Keeps bytes for the plug-in; the id goes into the dispatched JSON.
    fn stash(&mut self, bytes: Vec<u8>) -> u64 {
        let id = self.next_id();
        self.blobs_in.insert(id, bytes);
        id
    }

    /// Records the tree a panel rendered (for one UI, or for all).
    fn remember_panel(&mut self, name: &str, client: Option<String>, nodes: &[PluginUiNode]) {
        self.panel_seq += 1;
        let tree = PanelTree {
            kinds: tree::input_kinds(nodes),
            buttons: tree::button_ids(nodes),
        };
        if client.is_none() {
            // An update replaces what every UI was shown.
            self.panel_trees.retain(|(n, _), _| n != name);
        }
        // UIs come and go: keep the newest trees only.
        while self.panel_trees.len() >= 256 {
            let Some(oldest) = self
                .panel_trees
                .iter()
                .min_by_key(|(_, (seq, _))| *seq)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.panel_trees.remove(&oldest);
        }
        self.panel_trees
            .insert((name.to_owned(), client), (self.panel_seq, tree));
    }

    /// The tree a UI last saw of a panel: its own render or a later update
    /// for all UIs; for a UI without either (or without an id), the latest
    /// tree of the panel.
    fn panel_tree(&self, name: &str, client: Option<&String>) -> Option<&PanelTree> {
        let own = client.and_then(|c| self.panel_trees.get(&(name.to_owned(), Some(c.clone()))));
        let all = self.panel_trees.get(&(name.to_owned(), None));
        let newest = match (own, all) {
            (Some(a), Some(b)) => Some(if a.0 >= b.0 { a } else { b }),
            (x, None) | (None, x) => x,
        };
        newest
            .or_else(|| {
                self.panel_trees
                    .iter()
                    .filter(|((n, _), _)| n == name)
                    .map(|(_, t)| t)
                    .max_by_key(|t| t.0)
            })
            .map(|t| &t.1)
    }

    fn tree_issues(&mut self, what: &str, issues: &[PluginUiIssue]) -> ApiError {
        for issue in issues {
            self.log(
                PluginLogLevel::Error,
                PluginLogSource::Host,
                format!("{what} refused: {:?} at {}", issue.kind, issue.path),
                Some(
                    LocalizedMessage::new(issue.kind.message_id()).arg("path", issue.path.as_str()),
                ),
            );
        }
        invalid(format!(
            "the {what} is not valid: {}",
            issues
                .iter()
                .map(|i| format!("{:?} at {}", i.kind, i.path))
                .collect::<Vec<_>>()
                .join("; ")
        ))
    }

    fn require_alive(&self) -> Result<(), ApiError> {
        if self.alive.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::Unavailable,
                "the plug-in is stopping",
            ))
        }
    }
}

/// Copies the bytes of a `Uint8Array` (the prelude passes every
/// `ArrayBuffer` / view as one) out of the engine.
#[expect(
    unsafe_code,
    reason = "rquickjs offers byte access only as an unsafe slice"
)]
fn copy_bytes(array: &TypedArray<'_, u8>) -> Option<Vec<u8>> {
    // SAFETY: the slice is copied at once; no JavaScript runs while it is
    // borrowed (rquickjs' condition for `as_bytes`).
    unsafe { array.as_bytes() }.map(<[u8]>::to_vec)
}

/// `hostPutBytes(u8)`: keeps bytes the plug-in hands over for this turn;
/// the id goes into the JSON of the call that uses them.
fn put_bytes(state: &Rc<RefCell<State>>, array: &TypedArray<'_, u8>) -> rquickjs::Result<u64> {
    let Ok(mut s) = state.try_borrow_mut() else {
        return Err(rquickjs::Error::new_from_js("bytes", "re-entrant call"));
    };
    let too_large = || rquickjs::Error::new_from_js_message("Uint8Array", "bytes", "too large");
    if s.blob_out_bytes.saturating_add(array.len()) > s.inner.limits.max_blob_bytes {
        return Err(too_large());
    }
    let bytes = copy_bytes(array).ok_or_else(too_large)?;
    s.blob_out_bytes += bytes.len();
    let id = s.next_id();
    s.blobs_out.insert(id, bytes);
    Ok(id)
}

fn host_call(state: &Rc<RefCell<State>>, op: &str, payload: &str) -> String {
    let Ok(mut s) = state.try_borrow_mut() else {
        return Answer::Error(ApiError::internal("re-entrant host call")).encode();
    };
    let payload: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    let answer = match op_call(&mut s, op, &payload) {
        Ok(answer) => answer,
        Err(e) => Answer::Error(e),
    };
    answer.encode()
}

fn op_call(s: &mut State, op: &str, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    // A stopped plug-in (its `deactivate`, or code still running when it
    // was stopped) may only log: no mail, storage, network or UI.
    if !matches!(op, "log" | "fatal" | "clearTimer") {
        s.require_alive()?;
    }
    let services = Arc::clone(&s.inner.services);
    let plugin = s.id.clone();
    Ok(match op {
        "log" => {
            let level = payload
                .get("level")
                .and_then(serde_json::Value::as_str)
                .and_then(PluginLogLevel::parse)
                .unwrap_or(PluginLogLevel::Info);
            let message = str_field(payload, "message")?.to_owned();
            s.log(level, PluginLogSource::Plugin, message, None);
            Answer::Ok(serde_json::Value::Null)
        }
        "fatal" => {
            let text = format!(
                "{}: {}",
                payload
                    .get("where")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?"),
                payload
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?")
            );
            s.fatal.get_or_insert(text);
            Answer::Ok(serde_json::Value::Null)
        }
        "activated" => {
            s.activated = true;
            Answer::Ok(serde_json::Value::Null)
        }
        "registerCommand" => {
            let name = str_field(payload, "name")?;
            if s.manifest.command(name).is_none() {
                return Err(invalid(format!(
                    "command `{name}` is not declared in manifest.json"
                )));
            }
            s.commands.insert(name.to_owned());
            Answer::Ok(serde_json::Value::Null)
        }
        "registerPanel" => {
            let name = str_field(payload, "name")?;
            if s.manifest.panel(name).is_none() {
                return Err(invalid(format!(
                    "panel `{name}` is not declared in manifest.json"
                )));
            }
            s.panels.insert(name.to_owned());
            Answer::Ok(serde_json::Value::Null)
        }
        "setTimer" => {
            let id = payload
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| invalid("timer id"))?;
            if s.timers.len() >= s.inner.limits.max_timers {
                return Err(ApiError::new(ErrorCode::RateLimited, "too many timers"));
            }
            let ms = payload
                .get("ms")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0);
            let max = s.inner.limits.max_timer_delay;
            let delay = Duration::try_from_secs_f64(ms / 1000.0).map_or(max, |d| d.min(max));
            let origin = s.origin.clone();
            s.timers.insert(
                id,
                Timer {
                    due: Instant::now() + delay,
                    origin,
                },
            );
            Answer::Ok(serde_json::Value::Null)
        }
        "clearTimer" => {
            if let Some(id) = payload.get("id").and_then(serde_json::Value::as_u64) {
                s.timers.remove(&id);
            }
            Answer::Ok(serde_json::Value::Null)
        }
        "panelResult" => {
            let req = payload
                .get("req")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| invalid("panel request"))?;
            let Some((name, client, reply)) = s.panel_requests.remove(&req) else {
                return Ok(Answer::Ok(serde_json::Value::Null));
            };
            let nodes: Result<Vec<PluginUiNode>, _> =
                serde_json::from_value(payload.get("tree").cloned().unwrap_or_default());
            let prepared = match nodes {
                Ok(nodes) => tree::prepare_tree(&s.files, nodes)
                    .map_err(|issues| s.tree_issues("panel", &issues)),
                Err(e) => Err(invalid(format!("the panel is not a component tree: {e}"))),
            };
            match prepared {
                Ok(nodes) => {
                    s.remember_panel(&name, client, &nodes);
                    let scope = s.gate().mail_scope("panel", &s.expand()).ok();
                    s.inner.handle.spawn(async move {
                        let filled =
                            tree::fill_tree(services.as_ref(), scope.as_ref(), nodes).await;
                        drop(reply.send(Ok(filled)));
                    });
                }
                Err(e) => {
                    drop(reply.send(Err(e)));
                }
            }
            Answer::Ok(serde_json::Value::Null)
        }
        "mail.get" => {
            let call = "nail.mail.get";
            let scope = scope_or_deny!(s, mail_scope, call);
            let id = MessageId::from(str_field(payload, "id")?);
            let hidden = s.hidden_folders();
            s.spawn(call, payload, async move {
                match services.mail_get(id).await? {
                    Some(m) if scope.contains(&m.account_id) && !hidden.contains(&m.folder_id) => {
                        Ok(to_json(m)?)
                    }
                    _ => Err(ApiError::not_found("message not found")),
                }
            })
        }
        "mail.getBody" => {
            let call = "nail.mail.getBody";
            let scope = scope_or_deny!(s, body_scope, call);
            let id = MessageId::from(str_field(payload, "id")?);
            let inner = Arc::clone(&s.inner);
            let hidden = s.hidden_folders();
            s.spawn(call, payload, async move {
                let header = services.mail_get(id.clone()).await?;
                let Some(header) = header
                    .filter(|m| scope.contains(&m.account_id) && !hidden.contains(&m.folder_id))
                else {
                    return Err(ApiError::not_found("message not found"));
                };
                if header.encrypted {
                    // M4's `mail.read.encrypted`; never in the preview.
                    let denied = Denied::new(call, "mail.read.encrypted");
                    inner.log(host_line(
                        &plugin,
                        PluginLogLevel::Warn,
                        PluginLogSource::Api,
                        "the body of an end-to-end encrypted message is not available to plug-ins",
                        Some(denied.localized()),
                    ));
                    return Err(denied.to_api_error());
                }
                match services.mail_body(id).await? {
                    Some(body) => Ok(to_json(body)?),
                    None => Err(ApiError::not_found("message not found")),
                }
            })
        }
        "mail.search" => {
            let call = "nail.mail.search";
            let scope = scope_or_deny!(s, mail_scope, call);
            // Where message text may be searched (`mail.read.body`).
            let body_scope = s
                .gate()
                .body_scope(call, &s.expand())
                .unwrap_or_else(|_| AccountScope::Only(Vec::new()));
            let query = str_field(payload, "query")?.to_owned();
            if query.len() > 2000 {
                return Err(invalid("the query is too long"));
            }
            let max = s.inner.limits.max_search_results.max(1);
            let limit = payload
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map_or(50, |l| u32::try_from(l).unwrap_or(max))
                .clamp(1, max);
            let hidden = s.hidden_folders();
            s.spawn(call, payload, async move {
                let found = services
                    .mail_search(query, scope.clone(), body_scope, limit)
                    .await?;
                let found: Vec<_> = found
                    .into_iter()
                    .filter(|m| scope.contains(&m.account_id) && !hidden.contains(&m.folder_id))
                    .take(limit as usize)
                    .collect();
                to_json(found)
            })
        }
        "mail.thread" => {
            let call = "nail.mail.thread";
            let scope = scope_or_deny!(s, mail_scope, call);
            let thread = ThreadId::from(str_field(payload, "threadId")?);
            let hidden = s.hidden_folders();
            s.spawn(call, payload, async move {
                let found = services.mail_thread(thread).await?;
                let found: Vec<_> = found
                    .into_iter()
                    .filter(|m| scope.contains(&m.account_id) && !hidden.contains(&m.folder_id))
                    .collect();
                to_json(found)
            })
        }
        "calendar.events" => {
            let call = "nail.calendar.events";
            if let Err(d) = s.gate().require(&PluginPermission::CalendarRead, call) {
                return Ok(s.deny(&d));
            }
            let from = str_field(payload, "from")?.to_owned();
            let to = str_field(payload, "to")?.to_owned();
            let parse = |t: &str| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .map_err(|_| invalid(format!("not an RFC 3339 time: {t}")))
            };
            let (start, end) = (parse(&from)?, parse(&to)?);
            if end < start
                || end - start > chrono::Duration::days(nail_api::plugins::MAX_PLUGIN_CALENDAR_DAYS)
            {
                return Err(invalid(format!(
                    "the range must be at most {} days",
                    nail_api::plugins::MAX_PLUGIN_CALENDAR_DAYS
                )));
            }
            let max = nail_api::plugins::MAX_PLUGIN_CALENDAR_EVENTS;
            let limit = payload
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map_or(max, |l| u32::try_from(l).unwrap_or(max))
                .clamp(1, max);
            s.spawn(call, payload, async move {
                to_json(services.calendar_events(from, to, limit).await?)
            })
        }
        "storage.get" | "storage.set" | "storage.delete" | "storage.keys" | "storage.clear"
        | "storage.usage" => storage_call(s, op, payload)?,
        "http.fetch" => fetch_call(s, payload)?,
        "notify" => {
            if let Err(d) = s
                .gate()
                .require(&PluginPermission::Notifications, "nail.ui.notify")
            {
                return Ok(s.deny(&d));
            }
            s.require_alive()?;
            let spec: PluginNotificationSpec = serde_json::from_value(payload.clone())
                .map_err(|e| invalid(format!("not a notification: {e}")))?;
            if spec.title.trim().is_empty()
                || spec.title.chars().count() > 200
                || spec.body.as_ref().is_some_and(|b| b.chars().count() > 2000)
            {
                return Err(invalid(
                    "the notification title or body is empty or too long",
                ));
            }
            let now = Instant::now();
            while s
                .notifications
                .front()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_mins(1))
            {
                s.notifications.pop_front();
            }
            if s.notifications.len() >= s.inner.limits.notifications_per_minute {
                return Err(ApiError::new(
                    ErrorCode::RateLimited,
                    "too many notifications",
                ));
            }
            s.notifications.push_back(now);
            s.inner.services.emit(Event::PluginNotification {
                notification: Box::new(PluginNotification {
                    id: format!("pn-{:032x}", rand::random::<u128>()), // spellchecker:disable-line
                    plugin_id: s.id.clone(),
                    client_id: s.origin.client.clone(),
                    spec,
                }),
            });
            Answer::Ok(serde_json::Value::Null)
        }
        "showDialog" => dialog_call(s, payload)?,
        "updatePanel" => {
            let name = str_field(payload, "name")?.to_owned();
            if s.manifest.panel(&name).is_none() {
                return Err(invalid(format!(
                    "panel `{name}` is not declared in manifest.json"
                )));
            }
            s.require_alive()?;
            let nodes: Vec<PluginUiNode> =
                serde_json::from_value(payload.get("tree").cloned().unwrap_or_default())
                    .map_err(|e| invalid(format!("the panel is not a component tree: {e}")))?;
            let nodes = tree::prepare_tree(&s.files, nodes)
                .map_err(|issues| s.tree_issues("panel", &issues))?;
            s.remember_panel(&name, None, &nodes);
            let scope = s.gate().mail_scope("panel", &s.expand()).ok();
            let panel = plugin_panel_id(&s.id, &name);
            let inner = Arc::clone(&s.inner);
            let alive = Arc::clone(&s.alive);
            s.spawn(
                "nail.ui.updatePanel",
                &json!({ "name": name }),
                async move {
                    let tree = tree::fill_tree(services.as_ref(), scope.as_ref(), nodes).await;
                    if alive.load(Ordering::SeqCst) {
                        inner
                            .services
                            .emit(Event::PluginPanelChanged { panel, tree });
                    }
                    Ok(serde_json::Value::Null)
                },
            )
        }
        // --- M4a ---
        "register" => {
            let kind = str_field(payload, "registry")?;
            let registry = Registry::parse(kind)
                .ok_or_else(|| invalid(format!("unknown handler `{kind}`")))?;
            let name = str_field(payload, "name")?;
            if !registry.declared(&s.manifest, name) {
                return Err(invalid(format!(
                    "{} `{name}` is not declared in manifest.json",
                    registry.as_str()
                )));
            }
            s.registered.insert((registry, name.to_owned()));
            Answer::Ok(serde_json::Value::Null)
        }
        "invokeResult" => {
            let req = payload
                .get("req")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| invalid("invocation"))?;
            let Some(reply) = s.invocations.remove(&req) else {
                return Ok(Answer::Ok(serde_json::Value::Null));
            };
            let result = match payload.get("error") {
                Some(error) if !error.is_null() => {
                    let message = error
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("the handler failed");
                    Err(ApiError::internal(cut(message, 2000)))
                }
                _ => {
                    let mut value = payload.get("value").cloned().unwrap_or_default();
                    inline_blobs(&mut value, &s.blobs_out, 0);
                    Ok(value)
                }
            };
            drop(reply.send(result));
            Answer::Ok(serde_json::Value::Null)
        }
        "l10n.t" => {
            let id = str_field(payload, "id")?;
            let mut message = LocalizedMessage::new(id);
            if let Some(args) = payload.get("args").and_then(serde_json::Value::as_object) {
                for (name, value) in args {
                    let arg = match value {
                        serde_json::Value::Number(n) if n.is_i64() => {
                            MessageArg::Number(n.as_i64().unwrap_or_default())
                        }
                        serde_json::Value::String(text) => MessageArg::Text(text.clone()),
                        other => MessageArg::Text(other.to_string()),
                    };
                    message.args.insert(name.clone(), arg);
                }
            }
            Answer::Ok(json!(s.catalogs.message(s.locale.as_deref(), &message)))
        }
        "l10n.has" => {
            let id = str_field(payload, "id")?;
            Answer::Ok(json!(s.catalogs.has(s.locale.as_deref(), id)))
        }
        "calendar.calendars" | "contacts.addressBooks" | "contacts.find" | "tasks.lists" => {
            collections_call(s, op, payload)?
        }
        "composer.edit" => composer_call(s, payload)?,
        op if writes::api_name(op).is_some() => write_call(s, op, payload)?,
        other => return Err(invalid(format!("unknown host call `{other}`"))),
    })
}

/// Replaces `{"$nailBytes": id}` markers a plug-in's result carries with
/// the bytes as base64 (preview images).
fn inline_blobs(value: &mut serde_json::Value, blobs: &Blobs, depth: usize) {
    if depth > 64 {
        return;
    }
    if let Some(bytes) = writes::blob_id(value).and_then(|id| blobs.get(&id)) {
        *value = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        return;
    }
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                inline_blobs(item, blobs, depth + 1);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                inline_blobs(item, blobs, depth + 1);
            }
        }
        _ => {}
    }
}

/// `nail.calendar.calendars`, `nail.contacts.addressBooks` /
/// `find`, `nail.tasks.lists`: the collections the write permissions reach.
fn collections_call(
    s: &mut State,
    op: &str,
    payload: &serde_json::Value,
) -> Result<Answer, ApiError> {
    let gate = s.gate();
    let services = Arc::clone(&s.inner.services);
    let (call, allowed, needed) = match op {
        "calendar.calendars" => (
            "nail.calendar.calendars",
            gate.has(&PluginPermission::CalendarWrite) || gate.has(&PluginPermission::CalendarRead),
            PluginPermission::CalendarWrite,
        ),
        "contacts.addressBooks" => (
            "nail.contacts.addressBooks",
            gate.has(&PluginPermission::ContactsWrite),
            PluginPermission::ContactsWrite,
        ),
        "contacts.find" => (
            "nail.contacts.find",
            gate.has(&PluginPermission::ContactsWrite),
            PluginPermission::ContactsWrite,
        ),
        _ => (
            "nail.tasks.lists",
            gate.has(&PluginPermission::TasksWrite),
            PluginPermission::TasksWrite,
        ),
    };
    if !allowed {
        return Ok(s.deny(&Denied::new(call, &needed)));
    }
    Ok(match op {
        "calendar.calendars" => s.spawn(call, payload, async move {
            to_json(services.calendars().await?)
        }),
        "contacts.addressBooks" => s.spawn(call, payload, async move {
            to_json(services.address_books().await?)
        }),
        "contacts.find" => {
            let email = str_field(payload, "email")?.trim().to_owned();
            if email.is_empty() || email.len() > 320 || !email.contains('@') {
                return Err(invalid("`email` must be an address"));
            }
            let limit = payload
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map_or(20, |l| u32::try_from(l).unwrap_or(100))
                .clamp(1, 100);
            s.spawn(call, payload, async move {
                to_json(services.find_contacts(email, limit).await?)
            })
        }
        _ => s.spawn(call, payload, async move {
            to_json(services.task_lists().await?)
        }),
    })
}

/// `nail.composer.insert` / `replaceSelection` / `replaceAll`: edits a
/// draft a composer action of this plug-in ran on (within
/// [`crate::HostLimits::composer_edit_window`]); the composer applies it
/// live ([`Event::ComposerEditRequested`]).
fn composer_call(s: &mut State, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    let op = match str_field(payload, "op")? {
        "insert" => ComposerEditOp::Insert,
        "replaceSelection" => ComposerEditOp::ReplaceSelection,
        "replaceAll" => ComposerEditOp::ReplaceAll,
        other => return Err(invalid(format!("unknown composer edit `{other}`"))),
    };
    let call = match op {
        ComposerEditOp::Insert => "nail.composer.insert",
        ComposerEditOp::ReplaceSelection => "nail.composer.replaceSelection",
        ComposerEditOp::ReplaceAll => "nail.composer.replaceAll",
    };
    if let Err(d) = s.gate().require(&PluginPermission::Composer, call) {
        return Ok(s.deny(&d));
    }
    let text = str_field(payload, "text")?.to_owned();
    if text.len() > MAX_COMPOSER_TEXT {
        return Err(ApiError::new(ErrorCode::TooLarge, "the text is too long"));
    }
    let format = match payload.get("format").and_then(serde_json::Value::as_str) {
        None | Some("text") => ComposerTextFormat::Text,
        Some("markdown") => ComposerTextFormat::Markdown,
        Some("html") => ComposerTextFormat::Html,
        Some(other) => return Err(invalid(format!("unknown format `{other}`"))),
    };
    let now = Instant::now();
    s.composer_drafts.retain(|_, g| g.until > now);
    let draft = match payload.get("draftId").and_then(serde_json::Value::as_str) {
        Some(id) => Some(DraftId::from(id)).filter(|d| s.composer_drafts.contains_key(d)),
        // The draft of the latest composer action of this UI (any UI when
        // the turn has none).
        None => s
            .composer_drafts
            .iter()
            .filter(|(_, g)| s.origin.client.is_none() || g.client == s.origin.client)
            .max_by_key(|(_, g)| g.seq)
            .map(|(d, _)| d.clone()),
    };
    let Some(draft) = draft else {
        return Err(ApiError::not_found(
            "no draft to edit: composer edits work only on the draft a composer action ran on",
        ));
    };
    let client = s.composer_drafts.get(&draft).and_then(|g| g.client.clone());
    s.inner.services.emit(Event::ComposerEditRequested {
        draft_id: draft,
        client_id: client,
        by: Box::new(s.attribution()),
        edit: ComposerEdit { op, text, format },
    });
    Ok(Answer::Ok(serde_json::Value::Null))
}

/// A write call (`nail.mail.update`, `nail.tasks.create`, …): parsed, gated
/// (every permission it needs; messages it names must be readable, those
/// it changes within `mail.organize`, attachments it copies within
/// `mail.read.body` and not end-to-end encrypted), then run by the core.
fn write_call(s: &mut State, op: &str, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    let call = writes::api_name(op).unwrap_or("nail.write");
    let write = writes::parse(op, payload, &s.blobs_out)?;
    let gate = s.gate();
    let expand = s.expand();
    for permission in write.permissions() {
        if let Err(d) = gate.require(&permission, call) {
            return Ok(s.deny(&d));
        }
    }
    let read_ids = write.read_messages();
    let content_ids = write.content_messages();
    let read_scope = if read_ids.is_empty() {
        None
    } else {
        match gate.mail_scope(call, &expand) {
            Ok(scope) => Some(scope),
            Err(d) => return Ok(s.deny(&d)),
        }
    };
    let organize_scope = if write.organizes_messages() {
        match gate.scope_with_read(&PluginPermission::MailOrganize, call, &expand) {
            Ok(scope) => Some(scope),
            Err(d) => return Ok(s.deny(&d)),
        }
    } else {
        None
    };
    let content_scope = if content_ids.is_empty() {
        None
    } else {
        match gate.body_scope(call, &expand) {
            Ok(scope) => Some(scope),
            Err(d) => return Ok(s.deny(&d)),
        }
    };
    let scope = if let Some(scope) = &organize_scope {
        scope.clone()
    } else if write.uses_send_scope() {
        match gate.scope(&PluginPermission::MailSend, call, &expand) {
            Ok(scope) => scope,
            Err(d) => return Ok(s.deny(&d)),
        }
    } else {
        AccountScope::All
    };
    let request = PluginWriteRequest {
        attribution: s.attribution(),
        kind: write.kind(),
        operation: call.to_owned(),
        write,
        scope,
    };
    let services = Arc::clone(&s.inner.services);
    let hidden = s.hidden_folders();
    // The payload is logged without its texts and bytes.
    let summary = json!({ "kind": request.kind.as_str() });
    Ok(s.spawn(call, &summary, async move {
        let check_scope = organize_scope.or(read_scope);
        for id in read_ids {
            let found = services.mail_get(id).await?;
            match (&found, &check_scope) {
                (Some(m), Some(scope))
                    if scope.contains(&m.account_id) && !hidden.contains(&m.folder_id) => {}
                _ => return Err(ApiError::not_found("message not found")),
            }
        }
        for id in content_ids {
            let found = services.mail_get(id).await?;
            let found = found.filter(|m| !hidden.contains(&m.folder_id));
            match (&found, &content_scope) {
                (Some(m), Some(scope)) if scope.contains(&m.account_id) && !m.encrypted => {}
                (Some(m), Some(scope)) if scope.contains(&m.account_id) => {
                    return Err(Denied::new(call, "mail.read.encrypted").to_api_error());
                }
                _ => return Err(ApiError::not_found("message not found")),
            }
        }
        let result = services.write(request).await?;
        Ok(writes::result_json(&result))
    }))
}

fn storage_key(payload: &serde_json::Value) -> Result<String, ApiError> {
    let key = str_field(payload, "key")?;
    if key.is_empty() || key.len() > MAX_STORAGE_KEY {
        return Err(invalid(format!(
            "storage keys have 1 to {MAX_STORAGE_KEY} bytes"
        )));
    }
    Ok(key.to_owned())
}

fn storage_call(s: &mut State, op: &str, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    let call: &'static str = match op {
        "storage.get" => "nail.storage.get",
        "storage.set" => "nail.storage.set",
        "storage.delete" => "nail.storage.delete",
        "storage.keys" => "nail.storage.keys",
        "storage.clear" => "nail.storage.clear",
        _ => "nail.storage.usage",
    };
    if let Err(d) = s.gate().require(&PluginPermission::Storage, call) {
        return Ok(s.deny(&d));
    }
    let services = Arc::clone(&s.inner.services);
    let plugin = s.id.clone();
    Ok(match op {
        "storage.get" => {
            let key = storage_key(payload)?;
            s.spawn(call, payload, async move {
                Ok(services
                    .storage_get(plugin, key)
                    .await?
                    .unwrap_or(serde_json::Value::Null))
            })
        }
        "storage.set" => {
            let key = storage_key(payload)?;
            let value = payload.get("value").cloned().unwrap_or_default();
            if value.to_string().len() > MAX_STORAGE_VALUE {
                return Err(ApiError::new(
                    ErrorCode::TooLarge,
                    format!("storage values have at most {MAX_STORAGE_VALUE} bytes of JSON"),
                ));
            }
            s.spawn(call, &json!({ "key": key }), async move {
                services.storage_set(plugin, key, value).await?;
                Ok(serde_json::Value::Null)
            })
        }
        "storage.delete" => {
            let key = storage_key(payload)?;
            s.spawn(call, payload, async move {
                Ok(serde_json::Value::Bool(
                    services.storage_delete(plugin, key).await?,
                ))
            })
        }
        "storage.keys" => {
            let prefix = payload
                .get("prefix")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            s.spawn(call, payload, async move {
                to_json(services.storage_keys(plugin, prefix).await?)
            })
        }
        "storage.clear" => s.spawn(call, payload, async move {
            services.storage_clear(plugin).await?;
            Ok(serde_json::Value::Null)
        }),
        _ => s.spawn(call, payload, async move {
            let usage = services.storage_usage(plugin).await?;
            Ok(json!({
                "keys": usage.keys,
                "usedBytes": usage.used_bytes,
                "quotaBytes": usage.quota_bytes,
            }))
        }),
    })
}

fn fetch_call(s: &mut State, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    let call = "nail.http.fetch";
    let mut headers = Vec::new();
    if let Some(map) = payload
        .get("headers")
        .and_then(serde_json::Value::as_object)
    {
        for (name, value) in map {
            let value = value
                .as_str()
                .ok_or_else(|| invalid(format!("header `{name}` must be a string")))?;
            headers.push((name.clone(), value.to_owned()));
        }
    }
    // A string, or a `Uint8Array` / `ArrayBuffer` the prelude handed over
    // as a blob (M4a).
    let body = match payload.get("body") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => Some(text.as_bytes().to_vec()),
        Some(marker) => Some(
            writes::blob_id(marker)
                .and_then(|id| s.blobs_out.get(&id))
                .cloned()
                .ok_or_else(|| {
                    invalid("the body must be a string, a Uint8Array or an ArrayBuffer")
                })?,
        ),
    };
    if body
        .as_ref()
        .is_some_and(|b| b.len() > s.inner.limits.http_max_request_bytes)
    {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "the request body is too large",
        ));
    }
    let init = FetchInit {
        url: str_field(payload, "url")?.to_owned(),
        method: payload
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("GET")
            .to_owned(),
        headers,
        body,
    };
    let checked = match s.gate().check_fetch(&init) {
        Ok(checked) => checked,
        Err(FetchRefusal::Denied(d)) => return Ok(s.deny(&d)),
        Err(FetchRefusal::Invalid(msg)) => return Err(invalid(msg)),
    };
    let request = HttpRequest {
        method: checked.method,
        url: checked.url.clone(),
        headers: checked.headers,
        body: init.body,
        allowed_hosts: checked.allowed_hosts,
        max_response_bytes: s.inner.limits.http_max_response_bytes,
        timeout: s.inner.limits.http_timeout,
    };
    let services = Arc::clone(&s.inner.services);
    let plugin = s.id.clone();
    let max = s.inner.limits.http_max_response_bytes;
    Ok(
        s.spawn_bytes(call, &json!({ "url": checked.url }), async move {
            let response = services.http_fetch(plugin, request).await?;
            if response.body.len() as u64 > max {
                return Err(ApiError::new(
                    ErrorCode::TooLarge,
                    "the response body is too large",
                ));
            }
            let mut headers = serde_json::Map::new();
            for (name, value) in response.headers {
                let name = name.to_ascii_lowercase();
                let merged = match headers.get(&name).and_then(serde_json::Value::as_str) {
                    Some(old) => format!("{old}, {value}"),
                    None => value,
                };
                headers.insert(name, serde_json::Value::String(merged));
            }
            let value = json!({
                "status": response.status,
                "headers": headers,
                "body": String::from_utf8_lossy(&response.body),
            });
            Ok((value, Some(response.body)))
        }),
    )
}

fn dialog_call(s: &mut State, payload: &serde_json::Value) -> Result<Answer, ApiError> {
    let call = "nail.ui.showDialog";
    if let Err(d) = s.gate().require(&PluginPermission::UiDialogs, call) {
        return Ok(s.deny(&d));
    }
    s.require_alive()?;
    s.check_capacity()?;
    let spec: PluginDialogSpec = serde_json::from_value(payload.clone())
        .map_err(|e| invalid(format!("not a dialog: {e}")))?;
    if spec.title.trim().is_empty() || spec.title.chars().count() > 200 {
        return Err(invalid("the dialog title is empty or too long"));
    }
    let mut buttons = HashSet::new();
    if spec.buttons.len() > 10 {
        return Err(invalid("a dialog has at most 10 buttons"));
    }
    for b in &spec.buttons {
        if !is_value_id(&b.id)
            || b.label.trim().is_empty()
            || b.label.chars().count() > 100
            || !buttons.insert(b.id.clone())
        {
            return Err(invalid(format!("dialog button `{}` is not valid", b.id)));
        }
    }
    if s.dialogs.len() >= s.inner.limits.max_open_dialogs {
        return Err(ApiError::new(
            ErrorCode::RateLimited,
            "too many open dialogs",
        ));
    }
    let body = tree::prepare_tree(&s.files, spec.body)
        .map_err(|issues| s.tree_issues("dialog", &issues))?;
    let open = OpenDialog {
        origin: s.origin.clone(),
        kinds: tree::input_kinds(&body),
        buttons,
        actions: tree::button_ids(&body),
    };
    let call_id = s.next_id();
    let dialog_id = PluginDialogId::new(format!("pd-{:032x}", rand::random::<u128>()));
    let entry = DialogEntry {
        plugin: s.id.clone(),
        generation: s.generation,
        call_id,
    };
    if !s.inner.register_dialog(dialog_id.clone(), entry) {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "the plug-in is stopping",
        ));
    }
    s.calls.insert(
        call_id,
        PendingCall {
            origin: s.origin.clone(),
            op: call,
        },
    );
    s.dialogs.insert(call_id, open);
    if s.dev {
        s.log(
            PluginLogLevel::Debug,
            PluginLogSource::Api,
            format!("{call} {}", spec.title),
            None,
        );
    }
    let scope = s.gate().mail_scope("dialog", &s.expand()).ok();
    let services = Arc::clone(&s.inner.services);
    let inner = Arc::clone(&s.inner);
    let dialog = PluginDialog {
        dialog_id,
        plugin_id: s.id.clone(),
        client_id: s.origin.client.clone(),
        spec: PluginDialogSpec { body, ..spec },
    };
    s.inner.handle.spawn(async move {
        let mut dialog = dialog;
        let body = std::mem::take(&mut dialog.spec.body);
        dialog.spec.body = tree::fill_tree(services.as_ref(), scope.as_ref(), body).await;
        inner.emit_dialog_opened(dialog);
    });
    Ok(Answer::Pending(call_id))
}

/// Resolves module names: relative imports inside the package; the main
/// module by its path. Nothing else (no `std`, `os`, bare packages).
struct PackageResolver {
    main: String,
}

fn normalize(base: &str, name: &str) -> Option<String> {
    let mut parts: Vec<&str> = match base.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for part in name.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

impl Resolver for PackageResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let resolved = if name == self.main {
            Some(name.to_owned())
        } else if name.starts_with("./") || name.starts_with("../") {
            normalize(base, name)
        } else {
            None
        };
        match resolved {
            Some(path)
                if is_safe_path(&path)
                    && std::path::Path::new(&path)
                        .extension()
                        .is_some_and(|ext| ext == "js" || ext == "mjs") =>
            {
                Ok(path)
            }
            _ => Err(rquickjs::Error::new_resolving_message(
                base,
                name,
                "plug-ins can import only JavaScript files of their own package; bundle dependencies",
            )),
        }
    }
}

/// Loads modules from the package's files.
struct PackageLoader {
    files: PackageFiles,
    loaded: usize,
    max: usize,
}

impl Loader for PackageLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        self.loaded += 1;
        if self.loaded > self.max {
            return Err(rquickjs::Error::new_loading_message(
                name,
                "too many modules",
            ));
        }
        let bytes = self
            .files
            .read(name)
            .map_err(|e| rquickjs::Error::new_loading_message(name, e.to_string()))?;
        let source = String::from_utf8(bytes)
            .map_err(|_| rquickjs::Error::new_loading_message(name, "not UTF-8"))?;
        Module::declare(ctx.clone(), name, source)
    }
}

fn describe<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> String {
    if let Some(obj) = value.as_object() {
        let get = |key: &str| obj.get::<_, Option<String>>(key).ok().flatten();
        if let Some(message) = get("message") {
            let name = get("name").unwrap_or_else(|| "Error".to_owned());
            return match get("stack").filter(|s| !s.is_empty()) {
                Some(stack) => format!("{name}: {message}\n{stack}"),
                None => format!("{name}: {message}"),
            };
        }
    }
    if let Some(s) = value.as_string().and_then(|s| s.to_string().ok()) {
        return s;
    }
    ctx.json_stringify(value)
        .ok()
        .flatten()
        .and_then(|s| s.to_string().ok())
        .unwrap_or_else(|| "(unknown error)".to_owned())
}

fn error_text(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    match error {
        rquickjs::Error::Exception => describe(ctx, ctx.catch()),
        other => other.to_string(),
    }
}

enum Call {
    Load(String),
    Dispatch(&'static str, String),
}

/// A plug-in's QuickJS runtime. Field order is drop order: the persistent
/// handles must go before the context and the runtime.
struct Sandbox {
    load: Persistent<Function<'static>>,
    dispatch: Persistent<Function<'static>>,
    /// Promises rejected without a handler during the current turn; a
    /// handler attached before the turn ends (an `await` of a call that
    /// failed at once) takes them off again. What is left is logged.
    rejections: Rejections,
    ctx: Context,
    rt: Runtime,
    budget: Arc<Budget>,
    state: Rc<RefCell<State>>,
}

/// Rejected promises of the current turn with their reason's text.
type Rejections = Rc<RefCell<Vec<(Persistent<Value<'static>>, String)>>>;

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Saved values must go before their runtime.
        self.rejections.borrow_mut().clear();
    }
}

/// The native functions the prelude builds on: `hostCall(op, json)`,
/// `hostPutBytes(u8) -> id` and `hostTakeBytes(id) -> Uint8Array`.
fn natives<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<State>>,
) -> rquickjs::Result<(Function<'js>, Function<'js>, Function<'js>)> {
    let call_state = Rc::clone(state);
    let host = Function::new(ctx.clone(), move |op: String, payload: String| {
        host_call(&call_state, &op, &payload)
    })?;
    let put_state = Rc::clone(state);
    let put = Function::new(ctx.clone(), move |array: TypedArray<'js, u8>| {
        put_bytes(&put_state, &array)
    })?;
    let take_state = Rc::clone(state);
    let take = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, id: u64| -> rquickjs::Result<Value<'js>> {
            let bytes = take_state
                .try_borrow_mut()
                .ok()
                .and_then(|mut s| s.blobs_in.remove(&id));
            match bytes {
                Some(bytes) => Ok(TypedArray::<u8>::new(ctx, bytes)?.into_value()),
                None => Ok(Value::new_undefined(ctx)),
            }
        },
    )?;
    Ok((host, put, take))
}

impl Sandbox {
    fn new(state: Rc<RefCell<State>>, info: String) -> Result<Self, String> {
        let (limits, files, main, alive) = {
            let s = state.borrow();
            (
                s.inner.limits.clone(),
                s.files.clone(),
                s.manifest.main.clone(),
                Arc::clone(&s.alive),
            )
        };
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(limits.memory_bytes);
        rt.set_max_stack_size(limits.stack_bytes);
        let budget = Arc::new(Budget::new(alive));
        let b = Arc::clone(&budget);
        rt.set_interrupt_handler(Some(Box::new(move || b.check())));
        rt.set_loader(
            PackageResolver { main },
            PackageLoader {
                files,
                loaded: 0,
                max: limits.max_modules,
            },
        );
        let tracked = Rc::clone(&state);
        let rejections: Rejections = Rc::new(RefCell::new(Vec::new()));
        let pending = Rc::clone(&rejections);
        rt.set_host_promise_rejection_tracker(Some(Box::new(
            move |ctx, promise, reason, handled| {
                let Ok(mut pending) = pending.try_borrow_mut() else {
                    return;
                };
                if handled {
                    pending.retain(|(saved, _)| {
                        saved.clone().restore(&ctx).map_or(true, |p| p != promise)
                    });
                    return;
                }
                let text = describe(&ctx, reason);
                if text.contains("out of memory")
                    && let Ok(mut s) = tracked.try_borrow_mut()
                {
                    s.fatal.get_or_insert(text.clone());
                }
                pending.push((Persistent::save(&ctx, promise), text));
            },
        )));
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
        let host_state = Rc::clone(&state);
        let (load, dispatch) = ctx.with(|ctx| {
            let made = (|| -> rquickjs::Result<_> {
                let factory: Function<'_> = ctx.eval(PRELUDE)?;
                let (host, put, take) = natives(&ctx, &host_state)?;
                let info = ctx.json_parse(info)?;
                let api: Object<'_> = factory.call((host, info, put, take))?;
                let load: Function<'_> = api.get("load")?;
                let dispatch: Function<'_> = api.get("dispatch")?;
                Ok((
                    Persistent::save(&ctx, load),
                    Persistent::save(&ctx, dispatch),
                ))
            })();
            made.map_err(|e| error_text(&ctx, e))
        })?;
        Ok(Self {
            load,
            dispatch,
            rejections,
            ctx,
            rt,
            budget,
            state,
        })
    }

    /// One entry into plug-in code (see the module docs).
    fn turn(&self, origin: Origin, budget: Duration, call: &Call) -> Result<(), Crash> {
        {
            let mut s = self.state.borrow_mut();
            s.origin = origin;
            s.logs_this_turn = 0;
        }
        let started = thread_cpu_time();
        self.budget.arm(budget);
        let result = self.ctx.with(|ctx| {
            let run = || -> rquickjs::Result<()> {
                match call {
                    Call::Load(main) => {
                        let load = self.load.clone().restore(&ctx)?;
                        let promise = Module::import(&ctx, main.as_str())?;
                        load.call::<_, ()>((promise,))
                    }
                    Call::Dispatch(kind, json) => {
                        let dispatch = self.dispatch.clone().restore(&ctx)?;
                        dispatch.call::<_, ()>((*kind, json.as_str()))
                    }
                }
            };
            run().map_err(|e| error_text(&ctx, e))
        });
        let mut error = result.err();
        if !self.budget.fired() {
            loop {
                match self.rt.execute_pending_job() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(exception) => {
                        let text = exception
                            .0
                            .with(|ctx| error_text(&ctx, rquickjs::Error::Exception));
                        error.get_or_insert(text);
                        break;
                    }
                }
            }
        }
        let fired = self.budget.disarm();
        // Rejections still unhandled after the turn: logged, not a crash.
        let unhandled: Vec<String> = self
            .rejections
            .borrow_mut()
            .drain(..)
            .map(|(_, text)| text)
            .collect();
        for text in unhandled {
            self.state.borrow_mut().log(
                PluginLogLevel::Error,
                PluginLogSource::Host,
                format!("unhandled promise rejection: {text}"),
                Some(
                    LocalizedMessage::new(l10n::UNHANDLED_REJECTION).arg("detail", cut(&text, 500)),
                ),
            );
        }
        let fatal = {
            let mut s = self.state.borrow_mut();
            s.log_dropped();
            // Bytes live for one turn only.
            s.blobs_in.clear();
            s.blobs_out.clear();
            s.blob_out_bytes = 0;
            s.meter(thread_cpu_time().saturating_sub(started));
            s.fatal.take()
        };
        if fired {
            return Err(Crash::cpu(budget));
        }
        match error.or(fatal) {
            Some(message) => Err(Crash::error(message)),
            None => Ok(()),
        }
    }

    fn dispatch(
        &self,
        origin: Origin,
        budget: Duration,
        kind: &'static str,
        payload: &serde_json::Value,
    ) -> Result<(), Crash> {
        self.turn(origin, budget, &Call::Dispatch(kind, payload.to_string()))
    }

    /// The plug-in was stopped: `deactivate` runs (short budget; the API
    /// refuses everything but logging) and the thread ends. Whatever else
    /// is still queued (resolved calls, timers, commands) never runs.
    fn deactivate(&self, budget: Duration) -> ThreadEnd {
        self.budget.deactivating.store(true, Ordering::SeqCst);
        drop(self.dispatch(
            Origin::default(),
            budget.min(Duration::from_millis(250)),
            "deactivate",
            &json!({}),
        ));
        ThreadEnd::Requested
    }
}

impl State {
    /// Counts a turn's time against the CPU quota and reports it to the
    /// core about once a minute.
    fn meter(&self, used: Duration) {
        let now = self.inner.services.now();
        let minute = minute_of(now);
        let report = {
            let mut cpu = self
                .cpu
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cpu.add(minute, used);
            cpu.take_report(minute)
        };
        if let Some(ms) = report {
            let services = Arc::clone(&self.inner.services);
            let plugin = self.id.clone();
            let at = now.timestamp();
            self.inner.handle.spawn(async move {
                drop(services.record_cpu(plugin, at, ms).await);
            });
        }
    }
}

/// Runs a plug-in until it is stopped or crashes.
pub(crate) fn run(inner: &Arc<Inner>, launch: Launch) -> ThreadEnd {
    let Launch {
        id,
        generation,
        manifest,
        files,
        gate,
        settings,
        dev,
        alive,
        rx,
        tx,
        activated,
        catalogs,
        locale,
        display_name,
        cpu,
    } = launch;
    let limits = inner.limits.clone();
    let info = json!({
        "plugin": { "id": id.as_str(), "version": manifest.version },
        "apiVersion": manifest.api_version,
        "settings": settings,
        "locale": locale.clone().unwrap_or_else(|| "en".to_owned()),
    })
    .to_string();
    let main = manifest.main.clone();
    let state = Rc::new(RefCell::new(State {
        id: id.clone(),
        generation,
        manifest,
        files,
        gate,
        inner: Arc::clone(inner),
        tx,
        alive,
        dev,
        origin: Origin::default(),
        next_id: 0,
        calls: HashMap::new(),
        commands: HashSet::new(),
        panels: HashSet::new(),
        dialogs: HashMap::new(),
        timers: HashMap::new(),
        panel_requests: HashMap::new(),
        fatal: None,
        activated: false,
        logs_this_turn: 0,
        dropped_logs: 0,
        log_rate: LogRate::new(limits.max_log_lines_per_minute),
        notifications: VecDeque::new(),
        registered: HashSet::new(),
        invocations: HashMap::new(),
        panel_trees: HashMap::new(),
        panel_seq: 0,
        composer_drafts: HashMap::new(),
        blobs_in: HashMap::new(),
        blobs_out: Blobs::new(),
        blob_out_bytes: 0,
        catalogs,
        locale,
        display_name,
        cpu,
    }));
    let mut activated = Some(activated);
    // The crash is recorded before anyone waiting hears about it.
    let fail = |activated: &mut Option<Reply<()>>, crash: Crash| {
        inner.thread_ended(&id, generation, ThreadEnd::Crash(crash.clone()));
        if let Some(reply) = activated.take() {
            drop(reply.send(Err(crash.to_api_error())));
        }
        ThreadEnd::Crash(crash)
    };
    let sandbox = match Sandbox::new(Rc::clone(&state), info) {
        Ok(sandbox) => sandbox,
        Err(message) => return fail(&mut activated, Crash::error(message)),
    };
    if let Err(crash) = sandbox.turn(
        Origin::default(),
        limits.activation_budget,
        &Call::Load(main),
    ) {
        return fail(&mut activated, crash);
    }
    let stopped = || !state.borrow().alive.load(Ordering::SeqCst);
    loop {
        if stopped() {
            return sandbox.deactivate(limits.call_budget);
        }
        // Handler calls nobody waits for any more (their time ran out) are
        // forgotten; a late result is ignored.
        state
            .borrow_mut()
            .invocations
            .retain(|_, reply| !reply.is_closed());
        if activated.is_some() && state.borrow().activated {
            inner.activated(&id, generation);
            if let Some(reply) = activated.take() {
                drop(reply.send(Ok(())));
            }
        }
        let budget = if activated.is_some() {
            limits.activation_budget
        } else {
            limits.call_budget
        };
        let next_timer = state.borrow().timers.values().map(|t| t.due).min();
        // Dropped log lines are reported once the rate allows a line, even
        // when no further turn comes.
        let flush = (state.borrow().dropped_logs > 0)
            .then(|| Instant::now() + LogRate::interval(limits.max_log_lines_per_minute));
        let wake = next_timer.into_iter().chain(flush).min();
        let message = match wake {
            Some(due) => {
                let now = Instant::now();
                if due <= now {
                    None
                } else {
                    match rx.recv_timeout(due - now) {
                        Ok(m) => Some(m),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return ThreadEnd::Requested,
                    }
                }
            }
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return ThreadEnd::Requested,
            },
        };
        if stopped() {
            return sandbox.deactivate(limits.call_budget);
        }
        let outcome = match message {
            None => {
                state.borrow_mut().log_dropped();
                fire_timers(&sandbox, &state, budget)
            }
            Some(ThreadMsg::Stop) => return sandbox.deactivate(limits.call_budget),
            Some(message) => handle(&sandbox, &state, message, budget),
        };
        if let Err(crash) = outcome {
            // Interrupted because it was stopped: not a crash.
            if stopped() {
                return sandbox.deactivate(limits.call_budget);
            }
            return fail(&mut activated, crash);
        }
    }
}

fn fire_timers(
    sandbox: &Sandbox,
    state: &Rc<RefCell<State>>,
    budget: Duration,
) -> Result<(), Crash> {
    let now = Instant::now();
    let mut due: Vec<(Instant, u64, Origin)> = {
        let mut s = state.borrow_mut();
        let ids: Vec<u64> = s
            .timers
            .iter()
            .filter(|(_, t)| t.due <= now)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| s.timers.remove(&id).map(|t| (t.due, id, t.origin)))
            .collect()
    };
    due.sort_by_key(|(due, id, _)| (*due, *id));
    for (_, id, origin) in due {
        sandbox.dispatch(origin, budget, "timer", &json!({ "id": id }))?;
    }
    Ok(())
}

/// Calls a registered handler; its result comes back with `invokeResult`.
fn invoke(
    sandbox: &Sandbox,
    state: &Rc<RefCell<State>>,
    call: Invoke,
    reply: Reply<serde_json::Value>,
    budget: Duration,
) -> Result<(), Crash> {
    // Nobody waits any more (its time ran out while it was queued).
    if reply.is_closed() {
        return Ok(());
    }
    let payload = {
        let mut s = state.borrow_mut();
        if !s.registered.contains(&(call.registry, call.name.clone())) {
            drop(reply.send(Err(ApiError::not_found(format!(
                "the plug-in has not registered the {} `{}`",
                call.registry.as_str(),
                call.name
            )))));
            return Ok(());
        }
        let req = s.next_id();
        s.invocations.insert(req, reply);
        if let Some(draft) = call.composer_draft {
            let now = Instant::now();
            s.composer_drafts.retain(|_, g| g.until > now);
            let seq = s.next_id();
            let until = now + s.inner.limits.composer_edit_window;
            s.composer_drafts.insert(
                draft,
                ComposerGrant {
                    client: call.origin.client.clone(),
                    until,
                    seq,
                },
            );
        }
        let bytes = call.bytes.map(|b| s.stash(b));
        let mut payload = json!({
            "req": req,
            "registry": call.registry.as_str(),
            "name": call.name,
            "input": call.input,
        });
        if let Some(id) = bytes {
            payload["bytes"] = json!(id);
        }
        payload
    };
    sandbox.dispatch(call.origin, budget, "invoke", &payload)
}

fn answer<T>(state: &Rc<RefCell<State>>, reply: Reply<T>, outcome: &Result<(), Crash>, ok: T) {
    if let Err(crash) = outcome {
        // Record the crash before the caller hears about it.
        let (inner, id, generation) = {
            let s = state.borrow();
            (Arc::clone(&s.inner), s.id.clone(), s.generation)
        };
        inner.thread_ended(&id, generation, ThreadEnd::Crash(crash.clone()));
    }
    drop(reply.send(match outcome {
        Ok(()) => Ok(ok),
        Err(crash) => Err(crash.to_api_error()),
    }));
}

fn handle(
    sandbox: &Sandbox,
    state: &Rc<RefCell<State>>,
    message: ThreadMsg,
    budget: Duration,
) -> Result<(), Crash> {
    match message {
        ThreadMsg::Invoke(call, reply) => invoke(sandbox, state, *call, reply, budget),
        ThreadMsg::Resolve { id, result, bytes } => {
            let Some(call) = state.borrow_mut().calls.remove(&id) else {
                return Ok(());
            };
            let mut payload = match &result {
                Ok(value) => json!({ "id": id, "value": value }),
                Err(e) => json!({ "id": id, "error": error_json(e) }),
            };
            if let Some(bytes) = bytes {
                payload["bytes"] = json!(state.borrow_mut().stash(bytes));
            }
            {
                let mut s = state.borrow_mut();
                if s.dev {
                    let outcome = match &result {
                        Ok(_) => "ok".to_owned(),
                        Err(e) => format!("{}: {}", e.code.as_str(), e.message),
                    };
                    s.log(
                        PluginLogLevel::Debug,
                        PluginLogSource::Api,
                        format!("{} -> {outcome}", call.op),
                        None,
                    );
                }
            }
            sandbox.dispatch(call.origin, budget, "resolve", &payload)
        }
        ThreadMsg::DialogResponse {
            call_id,
            button,
            values,
        } => {
            let (dialog, _) = {
                let mut s = state.borrow_mut();
                (s.dialogs.remove(&call_id), s.calls.remove(&call_id))
            };
            let Some(dialog) = dialog else {
                return Ok(());
            };
            let result = PluginDialogResult {
                button: button.filter(|b| dialog.buttons.contains(b)),
                values: tree::clean_values(&dialog.kinds, values),
            };
            sandbox.dispatch(
                dialog.origin,
                budget,
                "resolve",
                &json!({ "id": call_id, "value": result }),
            )
        }
        ThreadMsg::DialogAction {
            call_id,
            component_id,
            values,
            client_id,
            reply,
        } => {
            let found = state.borrow().dialogs.get(&call_id).map(|d| {
                (
                    d.actions.contains(&component_id),
                    tree::clean_values(&d.kinds, values),
                    d.origin.clone(),
                )
            });
            let Some((true, values, origin)) = found else {
                drop(reply.send(Err(ApiError::not_found("no such button in the dialog"))));
                return Ok(());
            };
            let outcome = sandbox.dispatch(
                Origin {
                    client: client_id.or(origin.client),
                    on_behalf: origin.on_behalf,
                    limit: origin.limit,
                },
                budget,
                "dialogAction",
                &json!({ "id": call_id, "componentId": component_id, "values": values }),
            );
            answer(state, reply, &outcome, ());
            outcome
        }
        ThreadMsg::Command {
            name,
            context,
            client_id,
            reply,
        } => {
            if !state.borrow().commands.contains(&name) {
                drop(reply.send(Err(ApiError::not_found(format!(
                    "the plug-in has not registered the command `{name}`"
                )))));
                return Ok(());
            }
            let outcome = sandbox.dispatch(
                Origin::client(client_id),
                budget,
                "command",
                &json!({ "name": name, "context": context }),
            );
            answer(state, reply, &outcome, ());
            outcome
        }
        ThreadMsg::Panel {
            name,
            context,
            client_id,
            reply,
        } => {
            let req = {
                let mut s = state.borrow_mut();
                if !s.panels.contains(&name) {
                    drop(reply.send(Err(ApiError::not_found(format!(
                        "the plug-in has not registered the panel `{name}`"
                    )))));
                    return Ok(());
                }
                let req = s.next_id();
                s.panel_requests
                    .insert(req, (name.clone(), client_id.clone(), reply));
                req
            };
            sandbox.dispatch(
                Origin::client(client_id),
                budget,
                "panel",
                &json!({ "req": req, "name": name, "context": context }),
            )
        }
        ThreadMsg::PanelAction {
            name,
            component_id,
            values,
            client_id,
            reply,
        } => {
            if !state.borrow().panels.contains(&name) {
                drop(reply.send(Err(ApiError::not_found(format!(
                    "the plug-in has not registered the panel `{name}`"
                )))));
                return Ok(());
            }
            // Only buttons and inputs of the tree this UI was last shown
            // (M4a): anything else is refused or dropped.
            let values = {
                let s = state.borrow();
                match s.panel_tree(&name, client_id.as_ref()) {
                    Some(tree) if tree.buttons.contains(&component_id) => {
                        tree::clean_values(&tree.kinds, values)
                    }
                    _ => {
                        drop(reply.send(Err(ApiError::not_found("no such button in the panel"))));
                        return Ok(());
                    }
                }
            };
            let outcome = sandbox.dispatch(
                Origin::client(client_id),
                budget,
                "panelAction",
                &json!({ "name": name, "componentId": component_id, "values": values }),
            );
            answer(state, reply, &outcome, ());
            outcome
        }
        ThreadMsg::Settings(values) => sandbox.dispatch(
            Origin::default(),
            budget,
            "settings",
            &json!({ "values": values }),
        ),
        ThreadMsg::Stop => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_paths_normalize_inside_the_package() {
        assert_eq!(
            normalize("dist/index.js", "./util.js").as_deref(),
            Some("dist/util.js")
        );
        assert_eq!(
            normalize("dist/a/b.js", "../c.js").as_deref(),
            Some("dist/c.js")
        );
        assert_eq!(normalize("index.js", "./x/y.js").as_deref(), Some("x/y.js"));
        assert_eq!(normalize("index.js", "../x.js"), None);
        assert_eq!(normalize("dist/index.js", "../../x.js"), None);
    }

    #[test]
    fn budgets_fire_only_when_armed_and_late() {
        let budget = Budget::new(Arc::new(AtomicBool::new(true)));
        assert!(!budget.check());
        budget.arm(Duration::from_mins(1));
        assert!(!budget.check());
        assert!(!budget.disarm());
        budget.arm(Duration::ZERO);
        // Late once the thread has computed past the deadline.
        let spin = Instant::now();
        let mut x = 0u64;
        while !budget.check() {
            x = std::hint::black_box(x.wrapping_add(1));
            assert!(
                spin.elapsed() < Duration::from_mins(1),
                "the budget never fired"
            );
        }
        assert!(budget.disarm());
        assert!(!budget.check());
    }

    /// ADR 0204: time the thread waits (for a processor, here asleep) does
    /// not count against its budget.
    #[test]
    fn waiting_does_not_use_the_budget() {
        let budget = Budget::new(Arc::new(AtomicBool::new(true)));
        budget.arm(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(400));
        assert!(!budget.check());
        assert!(!budget.disarm());
    }

    /// M2d review: a stopped plug-in is interrupted whatever budget is
    /// left, except while its `deactivate` runs.
    #[test]
    fn a_stopped_plugin_is_interrupted_except_in_deactivate() {
        let alive = Arc::new(AtomicBool::new(true));
        let budget = Budget::new(Arc::clone(&alive));
        budget.arm(Duration::from_mins(1));
        assert!(!budget.check());
        alive.store(false, Ordering::SeqCst);
        assert!(budget.check());
        assert!(!budget.fired(), "not a CPU budget crash");
        budget.deactivating.store(true, Ordering::SeqCst);
        assert!(!budget.check());
    }

    #[test]
    fn the_log_rate_allows_a_burst_then_refills() {
        let mut rate = LogRate::new(3);
        assert!(rate.take(3));
        assert!(rate.take(3));
        assert!(rate.take(3));
        assert!(!rate.take(3));
        // 3 a minute = one every 20 s.
        rate.last -= Duration::from_secs(21);
        assert!(rate.take(3));
        assert!(!rate.take(3));
    }

    #[test]
    fn crashes_are_classified() {
        assert_eq!(
            Crash::error("InternalError: out of memory").reason,
            PluginStopReason::MemoryLimit
        );
        assert_eq!(Crash::error("TypeError: x").reason, PluginStopReason::Error);
        let e = Crash::cpu(Duration::from_millis(5)).to_api_error();
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert_eq!(e.localized.unwrap().id, "plugin-stopped-cpu-budget");
        assert_eq!(cut("héllo", 2), "h");
    }
}
