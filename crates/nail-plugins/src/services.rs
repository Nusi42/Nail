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

use nail_api::plugins::{
    PluginLogLevel, PluginLogSource, PluginMessage, PluginMessageBody, PluginStorageUsage,
};
use nail_api::{AccountId, ApiError, Event, LocalizedMessage, MessageId, PluginId, ThreadId};
use serde_json::Value;

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
    fn mail_search(
        &self,
        query: String,
        scope: AccountScope,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>>;

    /// The messages of a thread, oldest first.
    fn mail_thread(
        &self,
        thread: ThreadId,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>>;

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
}
