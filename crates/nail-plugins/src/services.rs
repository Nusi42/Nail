//! What the host needs from the core ([`PluginServices`]).
//!
//! The host never touches the store, the search index or the network
//! itself. Every data access of a plug-in passes the permission gate
//! ([`crate::gate`]) first and is then executed by the core through this
//! trait (`nail-core` implements it over the store, WP64). The gate has
//! already checked the permission and narrowed the accounts when a method
//! is called; the core only has to stay inside the arguments it is given.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use nail_api::agents::{ActionAttribution, ActionKind};
use nail_api::mcp::tools::McpWriteResult;
use nail_api::plugins::{
    PluginCalendarEvent, PluginHookKind, PluginHookOutcome, PluginJobOutcome, PluginLogLevel,
    PluginLogSource, PluginMessage, PluginMessageBody, PluginStorageUsage,
};
use nail_api::{
    AccountId, ApiError, DraftId, Event, LocalizedMessage, MessageId, PluginId, ThreadId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::writes::PluginWrite;

/// A boxed, sendable future (the trait is object safe).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Result of a service call.
pub type ServiceResult<T> = Result<T, ApiError>;

/// The accounts a mail call may reach: real account ids (virtual accounts
/// already expanded by [`PluginServices::expand_accounts`]), or every
/// account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountScope {
    /// Every account.
    All,
    /// Only these real accounts.
    Only(Vec<AccountId>),
}

impl AccountScope {
    /// Whether `account` is inside the scope.
    pub fn contains(&self, account: &AccountId) -> bool {
        match self {
            Self::All => true,
            Self::Only(accounts) => accounts.contains(account),
        }
    }

    /// The intersection of two scopes.
    #[must_use]
    pub fn intersect(&self, other: &AccountScope) -> AccountScope {
        match (self, other) {
            (Self::All, x) | (x, Self::All) => x.clone(),
            (Self::Only(a), Self::Only(b)) => {
                Self::Only(a.iter().filter(|x| b.contains(x)).cloned().collect())
            }
        }
    }
}

/// An HTTP request of `nail.http.fetch`, already checked against the
/// plug-in's `network:` permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// `GET`, `HEAD`, `POST`, `PUT`, `PATCH` or `DELETE`.
    pub method: String,
    /// The URL (`https:`; `http:` only for loopback hosts).
    pub url: String,
    /// Request headers (hop-by-hop, `Host`, `Cookie` and `Content-Length`
    /// are refused by the gate).
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Option<Vec<u8>>,
    /// The plug-in's `network:` host patterns. The core **must not follow
    /// a redirect** to a host outside them
    /// ([`crate::gate::host_allowed`]), and must refuse hosts that resolve
    /// to private or loopback addresses unless the pattern names the IP
    /// literal (SSRF; `nail-proxy`'s checks).
    pub allowed_hosts: Vec<String>,
    /// Largest response body; larger responses fail with `too_large`.
    pub max_response_bytes: u64,
    /// Overall time-out.
    pub timeout: Duration,
}

/// The core's answer to an [`HttpRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Response headers (lower-case names).
    pub headers: Vec<(String, String)>,
    /// The body (at most [`HttpRequest::max_response_bytes`]).
    pub body: Vec<u8>,
}

/// A log line from the host or a plug-in, before the store numbers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLogLine {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Milliseconds since the Unix epoch.
    pub at_ms: i64,
    /// The level.
    pub level: PluginLogLevel,
    /// Who wrote it.
    pub source: PluginLogSource,
    /// The text (at most [`nail_api::plugins::admin::MAX_LOG_MESSAGE`]
    /// bytes).
    pub message: String,
    /// For host lines: the user-facing text.
    pub localized: Option<LocalizedMessage>,
}

/// A write a plug-in asks for (M4a), after the permission gate: the core
/// runs it like the matching MCP write tool — through the operation queue
/// and the audit log, asking the user first when the plug-in's action
/// policy for `kind` says so (a pending confirmation attributed to
/// `attribution`) — and answers how it ended.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginWriteRequest {
    /// Who writes (the plug-in, its resolved name, and the MCP client it
    /// serves while it handles an exposed tool call).
    pub attribution: ActionAttribution,
    /// The action kind (whose policy applies).
    pub kind: ActionKind,
    /// The API that asked (`nail.mail.update`, `hooks.arrival`).
    pub operation: String,
    /// What to do.
    pub write: PluginWrite,
    /// The accounts the plug-in's mail permission reaches for this write
    /// (`mail.organize` ∩ `mail.read` for message changes, `mail.send` for
    /// drafts and sending; [`AccountScope::All`] for calendar, contact and
    /// task writes). The host already checked the message ids it names; the
    /// core must refuse identities, folders and drafts outside it
    /// (`not_found`).
    pub scope: AccountScope,
}

impl PluginWriteRequest {
    /// The plug-in.
    pub fn plugin_id(&self) -> Option<&PluginId> {
        match &self.attribution.actor {
            nail_api::agents::ActionActor::Plugin { plugin_id } => Some(plugin_id),
            nail_api::agents::ActionActor::McpClient { .. } => None,
        }
    }
}

/// A calendar, address book or task list a plug-in may write to
/// (plug-in-facing, camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCollection {
    /// The collection.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The default one of its kind.
    #[serde(default)]
    pub default: bool,
    /// Read-only (a subscription): writes fail.
    #[serde(default)]
    pub read_only: bool,
}

/// A contact as `nail.contacts.find` returns it (plug-in-facing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContact {
    /// The contact.
    pub id: String,
    /// Its address book.
    pub address_book_id: String,
    /// The full name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// Email addresses.
    #[serde(default)]
    pub emails: Vec<String>,
    /// Organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

/// An attachment with its bytes (attachment previews and actions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentData {
    /// File name.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// Size in bytes (of the decoded content).
    pub size: u64,
    /// The content; empty when it is larger than the `max_bytes` asked for
    /// ([`PluginServices::attachment`]).
    pub bytes: Vec<u8>,
}

/// A job's stored state, as the host needs it to schedule it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredJobState {
    /// The job.
    pub job: String,
    /// The next scheduled run (Unix seconds) the host stored.
    pub next_run_at: Option<i64>,
}

/// One finished (or skipped) job run (`Store::record_plugin_job_run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRunRecord {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// The job.
    pub job: String,
    /// When it ended (Unix seconds, [`PluginServices::now`]).
    pub at: i64,
    /// How it ended.
    pub outcome: PluginJobOutcome,
    /// Why it failed (English).
    pub error: Option<String>,
    /// The next scheduled run (Unix seconds).
    pub next_run_at: Option<i64>,
}

/// One hook call (`Store::record_plugin_hook_call`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCallRecord {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// The hook.
    pub hook: PluginHookKind,
    /// When it ended (Unix seconds).
    pub at: i64,
    /// How long it took (ms).
    pub ms: u64,
    /// How it ended.
    pub outcome: PluginHookOutcome,
    /// Why it failed (English).
    pub error: Option<String>,
}

fn unsupported<T: Send + 'static>(what: &'static str) -> BoxFuture<'static, ServiceResult<T>> {
    Box::pin(async move { Err(ApiError::unsupported(what)) })
}

fn done() -> BoxFuture<'static, ServiceResult<()>> {
    Box::pin(async { Ok(()) })
}

/// The core's side of the plug-in host (implemented by `nail-core`).
///
/// Async methods run on the Tokio runtime the host was created with; the
/// synchronous ones ([`Self::emit`], [`Self::log`],
/// [`Self::expand_accounts`]) are called from plug-in threads (sometimes
/// while the host holds an internal lock) and must neither block for long
/// nor call back into the [`crate::PluginHost`].
pub trait PluginServices: Send + Sync + 'static {
    /// Expands account ids of a grant (real or virtual) to real account
    /// ids; unknown ids are dropped.
    fn expand_accounts(&self, accounts: &[AccountId]) -> Vec<AccountId>;

    /// A message's headers view, or `None` when it does not exist.
    fn mail_get(&self, id: MessageId) -> BoxFuture<'static, ServiceResult<Option<PluginMessage>>>;

    /// A message's body. Called only for messages that are not end-to-end
    /// encrypted ([`PluginMessage::encrypted`]); the core must never return
    /// decrypted end-to-end encrypted content here.
    fn mail_body(
        &self,
        id: MessageId,
    ) -> BoxFuture<'static, ServiceResult<Option<PluginMessageBody>>>;

    /// Messages matching a query of Nail's search language, newest first,
    /// only in `scope`.
    ///
    /// `body_scope` (within `scope`) is where the plug-in may read message
    /// text (`mail.read.body`). Elsewhere the core must search header
    /// fields only: free text matches sender, recipients and subject, and
    /// `body:` / `filename:` terms match nothing there (no probing of
    /// bodies through search). End-to-end encrypted messages must never be
    /// found by text that exists only in their decrypted content (the core
    /// leaves them out while its index holds decrypted text).
    fn mail_search(
        &self,
        query: String,
        scope: AccountScope,
        body_scope: AccountScope,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>>;

    /// The messages of a thread, oldest first.
    fn mail_thread(
        &self,
        thread: ThreadId,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>>;

    /// Event occurrences overlapping `[from, to)` (RFC 3339) in the
    /// calendars the user shows, by start, at most `limit` (M3a,
    /// `calendar.read`). The default answers `unsupported`.
    fn calendar_events(
        &self,
        _from: String,
        _to: String,
        _limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginCalendarEvent>>> {
        Box::pin(async { Err(ApiError::unsupported("no calendars")) })
    }

    /// A storage value.
    fn storage_get(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<Option<Value>>>;

    /// Stores a value (`too_large` when the quota is exceeded).
    fn storage_set(
        &self,
        plugin: PluginId,
        key: String,
        value: Value,
    ) -> BoxFuture<'static, ServiceResult<()>>;

    /// Deletes a value; whether it existed.
    fn storage_delete(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<bool>>;

    /// The keys, optionally only those starting with `prefix`.
    fn storage_keys(
        &self,
        plugin: PluginId,
        prefix: Option<String>,
    ) -> BoxFuture<'static, ServiceResult<Vec<String>>>;

    /// Deletes every value.
    fn storage_clear(&self, plugin: PluginId) -> BoxFuture<'static, ServiceResult<()>>;

    /// Keys, bytes used and the quota.
    fn storage_usage(
        &self,
        plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<PluginStorageUsage>>;

    /// Executes an HTTP request for a plug-in.
    fn http_fetch(
        &self,
        plugin: PluginId,
        request: HttpRequest,
    ) -> BoxFuture<'static, ServiceResult<HttpResponse>>;

    /// Delivers an event to the UIs (dialogs, notifications, panels,
    /// stops, `PluginsChanged`). The core routes events carrying a
    /// `client_id` to that UI (ADR 0050).
    fn emit(&self, event: Event);

    /// Stores a log line and emits it as [`Event::PluginLogLine`].
    fn log(&self, line: HostLogLine);

    // --- M4a (ADR 0079, WP102). Every method has a default, so a core that
    // does not offer it yet answers `unsupported` (or does nothing). ---

    /// Runs a plug-in write ([`PluginWriteRequest`]): policy, confirmation,
    /// operation queue, audit. Answers [`McpWriteResult::Done`] (queued
    /// operations, created ids), `Denied`, `Expired` or `Vetoed` (a send
    /// stopped by a before-send hook); errors as for the MCP tool
    /// (`not_found`, `invalid_request`, `rate_limited` over the pending
    /// confirmations cap, …).
    fn write(
        &self,
        _request: PluginWriteRequest,
    ) -> BoxFuture<'static, ServiceResult<McpWriteResult>> {
        unsupported("plug-in writes")
    }

    /// The calendars a plug-in may write to (`calendar.write`) or read
    /// (`calendar.read`).
    fn calendars(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        unsupported("calendars")
    }

    /// The address books (`contacts.write`).
    fn address_books(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        unsupported("address books")
    }

    /// The task lists (`tasks.write`).
    fn task_lists(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        unsupported("task lists")
    }

    /// Contacts with this email address (case-insensitive), at most
    /// `limit` (`contacts.write`).
    fn find_contacts(
        &self,
        _email: String,
        _limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginContact>>> {
        unsupported("contacts")
    }

    /// An attachment of a message by index: its name, type and size, and
    /// its bytes when `size <= max_bytes` (else `bytes` stays empty).
    /// `None` when the message or attachment does not exist. Called only
    /// for messages that are not end-to-end encrypted; the core must never
    /// return decrypted end-to-end encrypted content here.
    fn attachment(
        &self,
        _message: MessageId,
        _index: u32,
        _max_bytes: u64,
    ) -> BoxFuture<'static, ServiceResult<Option<AttachmentData>>> {
        unsupported("attachments")
    }

    /// The mode of a saved draft (`plain`, `html`, `markdown`); `None` when
    /// it does not exist (composer actions).
    fn draft_mode(&self, _draft: DraftId) -> BoxFuture<'static, ServiceResult<Option<String>>> {
        unsupported("drafts")
    }

    /// Whether a message matches a query of Nail's search language (the
    /// `filter` of arrival and open hooks). An error skips the hook.
    fn message_matches(
        &self,
        _message: MessageId,
        _query: String,
    ) -> BoxFuture<'static, ServiceResult<bool>> {
        unsupported("hook filters")
    }

    /// The core's language (`nail.l10n.locale`, plug-in names); `None` =
    /// English.
    fn locale(&self) -> Option<String> {
        None
    }

    /// Now (job schedules, the CPU quota window, records). The core passes
    /// its adjustable clock (`ServiceOptions::clock`), so tests and the
    /// Playwright scenarios control when jobs run.
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    /// The local wall-clock time of `at` (cron schedules are local time).
    fn local_time(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.with_timezone(&chrono::Local).naive_local()
    }

    /// The stored state of a plug-in's jobs (`Store::plugin_job_states`).
    fn job_states(
        &self,
        _plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<Vec<StoredJobState>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    /// Stores when a job runs next (`Store::set_plugin_job_next_run`).
    fn set_job_next_run(
        &self,
        _plugin: PluginId,
        _job: String,
        _next_run_at: Option<i64>,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        done()
    }

    /// Records a job run (`Store::record_plugin_job_run`); the host emits
    /// [`Event::PluginActivityChanged`] afterwards.
    fn record_job_run(&self, _record: JobRunRecord) -> BoxFuture<'static, ServiceResult<()>> {
        done()
    }

    /// Records a hook call (`Store::record_plugin_hook_call`). The core
    /// decides when to emit [`Event::PluginActivityChanged`] for hooks
    /// (they run often; the host does not emit for them).
    fn record_hook_call(&self, _record: HookCallRecord) -> BoxFuture<'static, ServiceResult<()>> {
        done()
    }

    /// Adds CPU time a plug-in used (`Store::add_plugin_cpu`; `at` in Unix
    /// seconds; flushed about once a minute per plug-in).
    fn record_cpu(
        &self,
        _plugin: PluginId,
        _at: i64,
        _cpu_ms: u64,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        done()
    }
}
