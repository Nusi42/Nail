//! What the MCP server reads: the [`McpData`] trait.
//!
//! The server never talks to the store or the engines itself (except for
//! clients, grants and the audit log, which live in `nail-store`); it asks
//! an implementation of this trait. The core implements it (usually through
//! [`crate::CoreData`]), tests use fakes. Every method answers what the
//! *user* can see; the server applies the client's grant on top.
//!
//! M4a (ADR 0079, WP101): writes go through [`McpData::write`] (the core
//! runs them like the UI's own requests, through the operation queue,
//! attributed to the client); confirmations through the
//! [`crate::broker::ConfirmationBroker`] of [`McpData::confirmations`]
//! (the server's store-backed [`crate::broker::StoreBroker`] when the
//! implementation brings none); user-facing texts are rendered with
//! [`McpData::render`] in the core's locale.

use std::future::Future;
use std::sync::Arc;

use nail_api::LocalizedMessage;
use nail_api::agents::ActionAttribution;
use nail_api::compose::Draft;
use nail_api::{
    Account, AccountId, ApiError, AttachmentInfo, DraftId, Event, Folder, FolderId, Identity,
    MessageBody, MessageId, MessageSummary, SearchError, SearchHit, SearchQuery, ThreadId,
    ThreadSummary, UiContext, VirtualAccountConfig,
};

use crate::broker::ConfirmationBroker;

/// Result of a data call.
pub type DataResult<T> = Result<T, ApiError>;

/// One page of a folder's messages, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    /// Offset of the first message in `messages`.
    pub offset: u32,
    /// Total number of messages in the folder.
    pub total: u32,
    /// The page.
    pub messages: Vec<MessageSummary>,
}

/// A thread with its messages, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadData {
    /// The thread.
    pub thread: ThreadSummary,
    /// Its messages, oldest first.
    pub messages: Vec<MessageSummary>,
}

/// The bytes of one attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentData {
    /// File name, if the message gives one.
    pub filename: Option<String>,
    /// MIME type.
    pub mime: String,
    /// The decoded bytes.
    pub data: Vec<u8>,
}

/// What a search returned.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchOutcome {
    /// Hits of a valid query.
    Hits {
        /// Offset of the first hit.
        offset: u32,
        /// Total number of hits.
        total: u32,
        /// The page.
        hits: Vec<SearchHit>,
    },
    /// The query does not parse.
    Invalid(SearchError),
}

/// Read access to the user's data for the MCP server.
///
/// Implementations answer with the same data and errors the UI would get
/// ([`ApiError`] codes). Futures must be `Send`: the server runs on a
/// multi-threaded runtime.
pub trait McpData: Send + Sync + 'static {
    /// Real and virtual accounts ([`nail_api::Request::ListAccounts`]).
    fn accounts(&self) -> impl Future<Output = DataResult<Vec<Account>>> + Send;

    /// User-defined virtual accounts and their members
    /// ([`nail_api::Request::ListVirtualAccounts`]); used to resolve a
    /// grant naming a virtual account to its real members.
    fn virtual_accounts(
        &self,
    ) -> impl Future<Output = DataResult<Vec<VirtualAccountConfig>>> + Send;

    /// Folders of an account ([`nail_api::Request::ListFolders`]).
    fn folders(&self, account: &AccountId) -> impl Future<Output = DataResult<Vec<Folder>>> + Send;

    /// A page of a folder's messages ([`nail_api::Request::ListMessages`]).
    fn messages(
        &self,
        folder: &FolderId,
        offset: u32,
        limit: u32,
    ) -> impl Future<Output = DataResult<MessagePage>> + Send;

    /// One message's summary (account, folder, flags …); `not_found` for an
    /// unknown id.
    fn message_summary(
        &self,
        id: &MessageId,
    ) -> impl Future<Output = DataResult<MessageSummary>> + Send;

    /// The parsed message ([`nail_api::Request::GetMessage`]).
    fn message_body(&self, id: &MessageId) -> impl Future<Output = DataResult<MessageBody>> + Send;

    /// The original RFC 5322 source.
    fn raw_message(&self, id: &MessageId) -> impl Future<Output = DataResult<Vec<u8>>> + Send;

    /// One attachment's bytes by its index in [`MessageBody::attachments`]
    /// ([`AttachmentInfo::index`]).
    fn attachment(
        &self,
        id: &MessageId,
        index: u32,
    ) -> impl Future<Output = DataResult<AttachmentData>> + Send;

    /// Whether the message is end-to-end encrypted (S/MIME, PGP/MIME or
    /// inline PGP): every message the reader decrypts counts, however it
    /// is wrapped or labelled. Reading an encrypted message's content needs
    /// the `encrypted` scope.
    fn is_encrypted(&self, id: &MessageId) -> impl Future<Output = DataResult<bool>> + Send;

    /// A thread and its messages ([`nail_api::Request::GetThread`]).
    fn thread(&self, id: &ThreadId) -> impl Future<Output = DataResult<ThreadData>> + Send;

    /// Runs a query ([`nail_api::Request::Search`]).
    fn search(&self, query: SearchQuery) -> impl Future<Output = DataResult<SearchOutcome>> + Send;

    /// Whether the search index may hold decrypted content of encrypted
    /// mail (an account opted in to decrypted indexing, M2a). Searches of a
    /// client without the `encrypted` scope then leave encrypted messages
    /// out, so a match cannot reveal what they say.
    fn index_holds_decrypted(&self) -> impl Future<Output = DataResult<bool>> + Send {
        async { Ok(false) }
    }

    /// All filter rules in their `rules.kdl` text form.
    fn rules_text(&self) -> impl Future<Output = DataResult<String>> + Send;

    /// The last UI context the UI reported.
    fn ui_context(&self) -> impl Future<Output = DataResult<UiContext>> + Send;

    /// The sending identities ([`nail_api::Request::ListIdentities`]); a
    /// draft belongs to its identity's account.
    fn identities(&self) -> impl Future<Output = DataResult<Vec<Identity>>> + Send;

    /// A saved draft ([`nail_api::Request::GetDraft`]).
    fn draft(&self, id: &DraftId) -> impl Future<Output = DataResult<Draft>> + Send;

    /// Answers a calendar, contact or task read (M3a: `ListCalendars`,
    /// `ListEventInstances`, `GetEvent`, `FindFreeTime`, `ListAddressBooks`,
    /// `ListContacts`, `GetContact`, `SearchContacts`, `ListTasks`). The
    /// default answers `unsupported` (fakes without PIM data).
    fn pim(
        &self,
        request: nail_api::Request,
    ) -> impl Future<Output = DataResult<nail_api::Response>> + Send {
        let _ = request;
        async { Err(ApiError::unsupported("no calendar data")) }
    }

    /// The stored iCalendar / vCard text of a PIM item, if any.
    fn pim_raw(
        &self,
        item: &nail_api::pim::PimItemRef,
    ) -> impl Future<Output = DataResult<Option<String>>> + Send {
        let _ = item;
        async { Ok(None) }
    }

    /// Answers any other read-only Client API request the write tools and
    /// prompts need before they act (`NewDraft`, `GetEvent`, `GetContact`,
    /// `GetTask`, `RuleFromText`, `TestRule`, …). The default asks
    /// [`Self::pim`].
    fn query(
        &self,
        request: nail_api::Request,
    ) -> impl Future<Output = DataResult<nail_api::Response>> + Send {
        self.pim(request)
    }

    /// Runs one write for an MCP client (M4a): the request is the Client
    /// API request the UI would send (`UpdateFlags`, `SaveDraft`,
    /// `SendDraft`, `CreateEvent`, …); the core runs it through the
    /// operation queue like the user's own writes, attributed to `by`.
    /// The server has checked the grant, the policy and the confirmation
    /// before. The default answers `unsupported` (fakes without writes).
    fn write(
        &self,
        by: &ActionAttribution,
        request: nail_api::Request,
    ) -> impl Future<Output = DataResult<nail_api::Response>> + Send {
        let _ = (by, request);
        async { Err(ApiError::unsupported("writes are not available")) }
    }

    /// The confirmation broker the core provides (it wakes waiting calls
    /// when the user decides). `None` (the default): the server uses its
    /// own [`crate::broker::StoreBroker`] over the store.
    fn confirmations(&self) -> Option<Arc<dyn ConfirmationBroker>> {
        None
    }

    /// The per-client rate limiter the core shares with plug-in writes made
    /// for MCP clients (review M4a). `None` (the default): the server keeps
    /// its own.
    fn rate_limiter(&self) -> Option<Arc<crate::limits::RateLimiter>> {
        None
    }

    /// Renders a user-facing text (confirmation values, elicitation
    /// questions, prompt templates) in the core's locale. The default is
    /// English.
    fn render(&self, message: &LocalizedMessage) -> String {
        nail_config::l10n::english(message)
    }

    /// Tools plug-ins expose (`mcp.expose`, M4a): listed for clients whose
    /// grant has every scope of the tool. The default offers none.
    fn plugin_tools(&self) -> Vec<PluginTool> {
        Vec::new()
    }

    /// Calls a plug-in tool for `client` (the server checked the scopes).
    /// The answer is plug-in output: the server marks it untrusted.
    fn call_plugin_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        client: nail_api::agents::McpClientRef,
    ) -> impl Future<Output = DataResult<serde_json::Value>> + Send {
        let _ = (arguments, client);
        let name = name.to_owned();
        async move { Err(ApiError::not_found(format!("no plug-in tool {name}"))) }
    }

    /// Pushes an event to the connected UIs ([`Event::McpContextAccess`],
    /// [`Event::McpClientsChanged`]).
    fn emit(&self, event: Event);
}

/// A tool a plug-in exposes (`<plugin>__<tool>`).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginTool {
    /// The MCP name.
    pub name: String,
    /// Its plug-in's name (in the core's language).
    pub plugin_name: String,
    /// What it does (the plug-in's words).
    pub description: String,
    /// The JSON Schema of its arguments (an object schema).
    pub input_schema: serde_json::Value,
    /// The scopes a client needs to see and call it.
    pub scopes: Vec<nail_api::mcp::McpScope>,
}

/// The attachment list entry for `index`, if the body has one.
pub fn attachment_info(body: &MessageBody, index: u32) -> Option<&AttachmentInfo> {
    body.attachments.iter().find(|a| a.index == index)
}
