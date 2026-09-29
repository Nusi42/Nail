//! What the MCP server reads: the [`McpData`] trait.
//!
//! The server never talks to the store or the engines itself (except for
//! clients, grants and the audit log, which live in `nail-store`); it asks
//! an implementation of this trait. The core implements it (usually through
//! [`crate::CoreData`]), tests use fakes. Every method answers what the
//! *user* can see; the server applies the client's grant on top.

use std::future::Future;

use nail_api::compose::Draft;
use nail_api::{
    Account, AccountId, ApiError, AttachmentInfo, DraftId, Event, Folder, FolderId, Identity,
    MessageBody, MessageId, MessageSummary, SearchError, SearchHit, SearchQuery, ThreadId,
    ThreadSummary, UiContext, VirtualAccountConfig,
};

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

    /// Whether the message is encrypted (S/MIME or PGP/MIME). Reading an
    /// encrypted message's content needs the `encrypted` scope.
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

    /// Pushes an event to the connected UIs ([`Event::McpContextAccess`],
    /// [`Event::McpClientsChanged`]).
    fn emit(&self, event: Event);
}

/// The attachment list entry for `index`, if the body has one.
pub fn attachment_info(body: &MessageBody, index: u32) -> Option<&AttachmentInfo> {
    body.attachments.iter().find(|a| a.index == index)
}
