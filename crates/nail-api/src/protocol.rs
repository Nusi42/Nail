//! The transport-agnostic RPC protocol.
//!
//! A client sends [`Request`]s and receives exactly one [`Response`] per
//! request; the core additionally pushes [`Event`]s. The same JSON shapes are
//! used over every transport:
//!
//! * **Tauri** (desktop): the `nail_call` command takes a [`Request`] and
//!   returns a [`Response`]; events arrive on the `nail://event` channel as
//!   [`Event`]s.
//! * **WebSocket** (`nail serve`, `/api/ws`): the client sends [`RpcRequest`]
//!   frames; the server sends [`ServerMessage`] frames, which are either a
//!   response correlated by `id` or an event.
//!
//! See `docs/adr/0002-rpc-protocol.md`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::{
    AccountConfig, AccountStatus, AccountTestResult, AccountsConfig, DiscoveredAccount, Identity,
    SecretInput, VirtualAccountConfig,
};
use crate::compose::{Draft, OutboxEntry, OutboxState};
use crate::config::layout::ResolvedLayout;
use crate::config::theme::ResolvedTheme;
use crate::domain::{Account, Folder, MessageBody, MessageSummary, ThreadSummary};
use crate::error::{ApiError, ErrorCode};
use crate::ids::{
    AccountId, DraftId, FolderId, IdentityId, MessageId, OAuthFlowId, OpId, OutboxId, ThreadId,
};
use crate::ops::{FlagChange, MoveTarget, OpStatus};
use crate::search::{SearchError, SearchHit, SearchQuery};

/// Version of this protocol. Bumped on incompatible changes; returned by
/// [`Response::Pong`] so clients can detect a mismatched core.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from a client to the core.
///
/// Every request may also be answered with [`Response::Error`]. Requests
/// the running core does not implement yet are answered with the error code
/// `unsupported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Request {
    /// Liveness and version check. Answered with [`Response::Pong`].
    Ping,
    /// The resolved theme, layout and locale. Answered with [`Response::Config`].
    GetConfig,
    /// All accounts. Answered with [`Response::Accounts`].
    ListAccounts,
    /// All folders of an account, parents before children. Answered with
    /// [`Response::Folders`].
    ListFolders {
        /// The account.
        account_id: AccountId,
    },
    /// A page of a folder's messages, newest first. Answered with
    /// [`Response::Messages`].
    ListMessages {
        /// The folder.
        folder_id: FolderId,
        /// Index of the first message of the page (0 = newest).
        offset: u32,
        /// Maximum number of messages to return.
        limit: u32,
    },
    /// One parsed message. Answered with [`Response::Message`].
    GetMessage {
        /// The message.
        id: MessageId,
    },
    /// Start synchronizing one account, or all accounts when `account_id` is
    /// `null`. Answered with [`Response::SyncStarted`] immediately; progress
    /// arrives as [`Event::SyncProgress`] and [`Event::MailChanged`].
    SyncNow {
        /// The account, or `null` for all accounts.
        account_id: Option<AccountId>,
    },

    // --- Account setup and configuration (M1a) ---
    /// Look up server settings for an address (autoconfig, ISPDB, SRV,
    /// Autodiscover, guessing). Answered with [`Response::Discovered`]; an
    /// empty list means nothing was found and the user enters settings by
    /// hand.
    DiscoverAccount {
        /// The address to set up.
        email: String,
    },
    /// Connect and log in with a configuration without saving anything.
    /// Answered with [`Response::AccountTested`], which reports failures
    /// per server (a failed login is not a [`Response::Error`]).
    TestAccount {
        /// The configuration to test.
        config: Box<AccountConfig>,
        /// New secrets for it (not stored).
        secrets: Vec<SecretInput>,
    },
    /// Start an OAuth2 sign-in (authorization code + PKCE with a loopback
    /// redirect). Answered with [`Response::OAuthStarted`] at once; the
    /// client opens `authorize_url` unless the core already opened a
    /// browser. The outcome arrives as [`Event::OAuthFinished`]; the tokens
    /// stay in the core until a request references the flow in a
    /// [`crate::accounts::SecretValue::OAuth2`], or the flow expires after
    /// 10 minutes.
    StartOAuth {
        /// Provider id (`google`, `microsoft` or one from `accounts.kdl`).
        provider: String,
        /// Address to pre-fill on the provider's sign-in page.
        login_hint: Option<String>,
    },
    /// Abort a running OAuth2 sign-in. Answered with [`Response::Done`]; the
    /// flow finishes with `cancelled`.
    CancelOAuth {
        /// The flow.
        flow_id: OAuthFlowId,
    },
    /// The account configuration (`accounts.kdl`). Answered with
    /// [`Response::AccountsConfig`].
    GetAccountsConfig,
    /// Add an account: store the secrets in the keychain, write
    /// `accounts.kdl`, start syncing. Answered with
    /// [`Response::AccountSaved`]; `conflict` when the id is taken.
    AddAccount {
        /// The new account; an empty `id` lets the core derive one.
        config: Box<AccountConfig>,
        /// Secrets for its servers.
        secrets: Vec<SecretInput>,
    },
    /// Replace an account's configuration (matched by `config.id`).
    /// Answered with [`Response::AccountSaved`].
    UpdateAccount {
        /// The new configuration.
        config: Box<AccountConfig>,
        /// Changed secrets only; the others stay as they are.
        secrets: Vec<SecretInput>,
    },
    /// Remove an account: its entry in `accounts.kdl` (and its membership in
    /// virtual accounts), its keychain secrets and its local mail. Nothing
    /// on the server changes. Answered with [`Response::Done`].
    RemoveAccount {
        /// The account.
        account_id: AccountId,
    },
    /// Sending identities. Answered with [`Response::Identities`].
    ListIdentities {
        /// A real account (its identities), a virtual account (its members'
        /// identities) or `null` for all.
        account_id: Option<AccountId>,
    },
    /// The identity to send from, for a new message, a reply or a forward.
    /// Answered with [`Response::IdentityChosen`]. The first of: the
    /// virtual account's identity rules (the first rule matching any
    /// recipient), the identity the replied-to message was addressed to,
    /// the account's default identity (a virtual account's
    /// `default-identity`, else its first).
    ChooseIdentity {
        /// The active account (real or virtual), `null` for all.
        account_id: Option<AccountId>,
        /// Recipient addresses entered so far (To, Cc, Bcc).
        #[serde(default)]
        recipients: Vec<String>,
        /// The message replied to or forwarded, if any.
        #[serde(default)]
        reply_to: Option<MessageId>,
    },
    /// User-defined virtual accounts. Answered with
    /// [`Response::VirtualAccounts`].
    ListVirtualAccounts,
    /// Add a virtual account. Answered with
    /// [`Response::VirtualAccountSaved`]; `conflict` when the id is taken.
    AddVirtualAccount {
        /// The new virtual account; an empty `id` lets the core derive one.
        config: Box<VirtualAccountConfig>,
    },
    /// Replace a virtual account (matched by `config.id`). Answered with
    /// [`Response::VirtualAccountSaved`].
    UpdateVirtualAccount {
        /// The new configuration.
        config: Box<VirtualAccountConfig>,
    },
    /// Remove a virtual account (its members are not touched). Answered with
    /// [`Response::Done`].
    RemoveVirtualAccount {
        /// The virtual account.
        account_id: AccountId,
    },
    /// Current connection state of every real account. Answered with
    /// [`Response::AccountStatuses`]; changes arrive as
    /// [`Event::AccountStatus`].
    ListAccountStatuses,

    // --- Message writes (through the operation queue) ---
    /// Set or clear flags and tags. Answered with [`Response::Queued`].
    UpdateFlags {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// The change.
        change: FlagChange,
    },
    /// Move messages. Answered with [`Response::Queued`].
    MoveMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// Where to.
        target: MoveTarget,
    },
    /// Delete messages. Answered with [`Response::Queued`].
    DeleteMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// `false`: move to the account's Trash (messages already in Trash
        /// are deleted for good). `true`: delete for good at once.
        permanent: bool,
    },

    // --- Threads and search ---
    /// A page of a folder's threads, newest activity first. Answered with
    /// [`Response::Threads`].
    ListThreads {
        /// The folder (real or virtual).
        folder_id: FolderId,
        /// Index of the first thread of the page.
        offset: u32,
        /// Maximum number of threads to return.
        limit: u32,
    },
    /// All messages of a thread across folders (and, in virtual accounts,
    /// accounts). Answered with [`Response::Thread`].
    GetThread {
        /// The thread.
        thread_id: ThreadId,
    },
    /// Run a query. Answered with [`Response::SearchResults`], or with
    /// [`Response::InvalidQuery`] when the query does not parse.
    Search {
        /// The query.
        query: SearchQuery,
    },
    /// Parse a query without running it (live feedback in the search bar).
    /// Answered with [`Response::QueryChecked`].
    CheckQuery {
        /// The query text.
        text: String,
    },

    // --- Drafts and sending ---
    /// Create or update a draft (stored locally; the core also keeps a copy
    /// in the server's Drafts folder). Answered with
    /// [`Response::DraftSaved`].
    SaveDraft {
        /// The draft; `id: null` creates a new one.
        draft: Draft,
    },
    /// One draft. Answered with [`Response::Draft`].
    GetDraft {
        /// The draft.
        draft_id: DraftId,
    },
    /// Discard a draft. Answered with [`Response::Done`].
    DeleteDraft {
        /// The draft.
        draft_id: DraftId,
    },
    /// Put a saved draft into the outbox. Answered with
    /// [`Response::SendQueued`]; progress arrives as [`Event::SendStatus`].
    SendDraft {
        /// The draft (save it first).
        draft_id: DraftId,
        /// Undo-send delay in seconds; `null` = the configured default
        /// ([`AccountsConfig::undo_send_secs`]), `0` = send at once.
        delay_secs: Option<u32>,
        /// Send later: when to send (RFC 3339); overrides `delay_secs`.
        /// `null` = now plus the delay.
        send_at: Option<String>,
    },
    /// Stop a scheduled message and turn it back into a draft. Answered with
    /// [`Response::SendCancelled`]; `conflict` when it is already being sent.
    CancelSend {
        /// The outbox entry.
        outbox_id: OutboxId,
    },
    /// Messages in the outbox (scheduled, sending, failed). Answered with
    /// [`Response::Outbox`].
    ListOutbox,
}

/// The core's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Response {
    /// Answer to [`Request::Ping`].
    Pong {
        /// Version of the core (`CARGO_PKG_VERSION` of `nail-core`).
        version: String,
        /// [`PROTOCOL_VERSION`] spoken by the core.
        protocol: u32,
    },
    /// Answer to [`Request::GetConfig`].
    Config {
        /// The resolved theme.
        theme: Box<ResolvedTheme>,
        /// The resolved layout.
        layout: Box<ResolvedLayout>,
        /// UI language override from the settings as a BCP 47 tag (e.g.
        /// `"de"`), or `null` to follow the OS / browser language.
        locale: Option<String>,
    },
    /// Answer to [`Request::ListAccounts`].
    Accounts {
        /// All accounts, in display order.
        accounts: Vec<Account>,
    },
    /// Answer to [`Request::ListFolders`].
    Folders {
        /// The folders, parents before children.
        folders: Vec<Folder>,
    },
    /// Answer to [`Request::ListMessages`].
    Messages {
        /// The folder the page belongs to.
        folder_id: FolderId,
        /// Offset of the first message in `messages`.
        offset: u32,
        /// Total number of messages in the folder.
        total: u32,
        /// The page, newest first.
        messages: Vec<MessageSummary>,
    },
    /// Answer to [`Request::GetMessage`].
    Message {
        /// The parsed message.
        message: Box<MessageBody>,
    },
    /// Answer to [`Request::SyncNow`]: synchronization was started.
    SyncStarted,
    /// Success without data (e.g. [`Request::RemoveAccount`],
    /// [`Request::CancelOAuth`], [`Request::DeleteDraft`]).
    Done,
    /// Answer to [`Request::DiscoverAccount`].
    Discovered {
        /// The address that was looked up.
        email: String,
        /// Proposals, best first.
        candidates: Vec<DiscoveredAccount>,
    },
    /// Answer to [`Request::TestAccount`].
    AccountTested {
        /// Per-server results.
        result: Box<AccountTestResult>,
    },
    /// Answer to [`Request::StartOAuth`].
    OAuthStarted {
        /// The flow, for [`Event::OAuthFinished`] and
        /// [`crate::accounts::SecretValue::OAuth2`].
        flow_id: OAuthFlowId,
        /// The provider's sign-in page.
        authorize_url: String,
        /// Whether the core already opened it in the system browser.
        opened: bool,
    },
    /// Answer to [`Request::GetAccountsConfig`].
    AccountsConfig {
        /// The configuration.
        config: AccountsConfig,
    },
    /// Answer to [`Request::AddAccount`] and [`Request::UpdateAccount`].
    AccountSaved {
        /// The configuration as saved (with the assigned id).
        config: Box<AccountConfig>,
    },
    /// Answer to [`Request::ListIdentities`].
    Identities {
        /// The identities in account order, each account's default first.
        identities: Vec<Identity>,
    },
    /// Answer to [`Request::ChooseIdentity`].
    IdentityChosen {
        /// The identity; `null` when there is none (no accounts).
        identity_id: Option<IdentityId>,
    },
    /// Answer to [`Request::ListVirtualAccounts`].
    VirtualAccounts {
        /// User-defined virtual accounts in file order.
        virtual_accounts: Vec<VirtualAccountConfig>,
    },
    /// Answer to [`Request::AddVirtualAccount`] and
    /// [`Request::UpdateVirtualAccount`].
    VirtualAccountSaved {
        /// The configuration as saved (with the assigned id).
        config: Box<VirtualAccountConfig>,
    },
    /// Answer to [`Request::ListAccountStatuses`].
    AccountStatuses {
        /// One entry per real account.
        statuses: Vec<AccountStatus>,
    },
    /// Answer to message writes: applied locally and queued for the server.
    Queued {
        /// The queued operations (the core may split a request per account
        /// or message), for matching [`Event::OpStatus`].
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::ListThreads`].
    Threads {
        /// The folder the page belongs to.
        folder_id: FolderId,
        /// Offset of the first thread in `threads`.
        offset: u32,
        /// Total number of threads in the folder.
        total: u32,
        /// The page.
        threads: Vec<ThreadSummary>,
    },
    /// Answer to [`Request::GetThread`].
    Thread {
        /// The thread.
        thread: Box<ThreadSummary>,
        /// Its messages, oldest first.
        messages: Vec<MessageSummary>,
    },
    /// Answer to [`Request::Search`].
    SearchResults {
        /// Offset of the first hit in `hits`.
        offset: u32,
        /// Total number of hits.
        total: u32,
        /// The page of hits.
        hits: Vec<SearchHit>,
    },
    /// Answer to [`Request::Search`] when the query does not parse.
    InvalidQuery {
        /// What is wrong and where.
        error: SearchError,
    },
    /// Answer to [`Request::CheckQuery`].
    QueryChecked {
        /// `null` when the query is valid.
        error: Option<SearchError>,
    },
    /// Answer to [`Request::SaveDraft`].
    DraftSaved {
        /// The draft's id (new or unchanged).
        draft_id: DraftId,
    },
    /// Answer to [`Request::GetDraft`].
    Draft {
        /// The draft.
        draft: Box<Draft>,
    },
    /// Answer to [`Request::SendDraft`].
    SendQueued {
        /// The new outbox entry.
        entry: Box<OutboxEntry>,
    },
    /// Answer to [`Request::CancelSend`].
    SendCancelled {
        /// The restored draft.
        draft_id: DraftId,
    },
    /// Answer to [`Request::ListOutbox`].
    Outbox {
        /// Entries, earliest send time first.
        entries: Vec<OutboxEntry>,
    },
    /// The request failed. May answer any request.
    Error {
        /// Machine-readable error class.
        code: ErrorCode,
        /// Diagnostic detail (English, for logs and developer tools). The UI
        /// shows a localized text chosen by `code`.
        message: String,
    },
}

impl From<ApiError> for Response {
    fn from(err: ApiError) -> Self {
        Self::Error {
            code: err.code,
            message: err.message,
        }
    }
}

impl Response {
    /// Converts an [`Response::Error`] into `Err(ApiError)`, anything else into `Ok`.
    pub fn into_result(self) -> Result<Self, ApiError> {
        match self {
            Self::Error { code, message } => Err(ApiError { code, message }),
            other => Ok(other),
        }
    }
}

/// A notification pushed by the core, not tied to a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Event {
    /// The theme or layout file changed and was valid; replaces the previous
    /// configuration. Also clears any error shown for [`Event::ConfigError`];
    /// errors of files that are still invalid follow right after it again.
    ConfigChanged {
        /// The new resolved theme.
        theme: Box<ResolvedTheme>,
        /// The new resolved layout.
        layout: Box<ResolvedLayout>,
    },
    /// A changed configuration file is invalid; the last valid configuration
    /// stays active.
    ConfigError {
        /// File name, e.g. `"theme.kdl"`.
        file: String,
        /// 1-based line of the error.
        line: u32,
        /// 1-based column of the error.
        column: u32,
        /// Diagnostic message (English, like a compiler message).
        message: String,
    },
    /// Messages or counts in these folders changed; clients should refetch
    /// what they display from them.
    MailChanged {
        /// The affected folders.
        folder_ids: Vec<FolderId>,
    },
    /// Progress of a running synchronization.
    SyncProgress {
        /// The account being synchronized.
        account_id: AccountId,
        /// Work units done so far.
        done: u32,
        /// Total work units (0 while unknown). `done == total` means finished.
        total: u32,
    },
    /// Synchronizing an account failed. The account keeps the state of its
    /// last successful synchronization; a later [`Request::SyncNow`] retries.
    SyncFailed {
        /// The account whose synchronization failed.
        account_id: AccountId,
        /// Machine-readable error class (the UI shows a localized text).
        code: ErrorCode,
        /// Diagnostic detail (English, for logs and developer tools).
        message: String,
    },
    /// A queued operation changed state.
    OpStatus {
        /// The new status.
        status: OpStatus,
    },
    /// An outbox entry changed state.
    SendStatus {
        /// The entry.
        outbox_id: OutboxId,
        /// Its new state.
        state: OutboxState,
        /// Error class when `failed`, else `null`.
        error: Option<ErrorCode>,
        /// Diagnostic detail (English), else `null`.
        message: Option<String>,
    },
    /// New messages arrived from a server (for notifications; lists are
    /// refreshed through [`Event::MailChanged`]).
    NewMail {
        /// The account.
        account_id: AccountId,
        /// The folder.
        folder_id: FolderId,
        /// The new messages.
        message_ids: Vec<MessageId>,
    },
    /// An account's connection state changed.
    AccountStatus {
        /// The new status.
        status: AccountStatus,
    },
    /// The account configuration changed (through a request or an edit of
    /// `accounts.kdl`); clients refetch accounts, identities and virtual
    /// accounts. An invalid `accounts.kdl` is reported as
    /// [`Event::ConfigError`] instead.
    AccountsChanged,
    /// An OAuth2 sign-in finished.
    OAuthFinished {
        /// The flow.
        flow_id: OAuthFlowId,
        /// `null` on success (the core holds the tokens for the flow id);
        /// `cancelled`, `auth_failed`, `network`, … otherwise.
        error: Option<ErrorCode>,
        /// Diagnostic detail (English), else `null`.
        message: Option<String>,
    },
}

/// A request frame on a multiplexed transport (WebSocket).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RpcRequest {
    /// Client-chosen correlation id, unique among the client's in-flight
    /// requests. Kept below 2^53 so it is exact in JavaScript.
    pub id: u64,
    /// The request.
    pub request: Request,
}

/// A response frame on a multiplexed transport (WebSocket).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct RpcResponse {
    /// The `id` of the [`RpcRequest`] this answers.
    pub id: u64,
    /// The response.
    pub response: Response,
}

/// Any frame sent by the server on a multiplexed transport (WebSocket).
///
/// JSON: `{"kind":"response","id":1,"response":{…}}` or
/// `{"kind":"event","event":{…}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ServerMessage {
    /// The answer to an [`RpcRequest`].
    Response {
        /// The `id` of the [`RpcRequest`] this answers.
        id: u64,
        /// The response.
        response: Response,
    },
    /// A pushed event.
    Event {
        /// The event.
        event: Event,
    },
}

impl From<RpcResponse> for ServerMessage {
    fn from(r: RpcResponse) -> Self {
        Self::Response {
            id: r.id,
            response: r.response,
        }
    }
}

impl From<Event> for ServerMessage {
    fn from(event: Event) -> Self {
        Self::Event { event }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn requests_use_type_tag() {
        assert_eq!(
            serde_json::to_value(Request::ListAccounts).unwrap(),
            json!({"type": "ListAccounts"})
        );
        assert_eq!(
            serde_json::to_value(Request::ListMessages {
                folder_id: "f".into(),
                offset: 0,
                limit: 50
            })
            .unwrap(),
            json!({"type": "ListMessages", "folder_id": "f", "offset": 0, "limit": 50})
        );
        assert_eq!(
            serde_json::to_value(Request::SyncNow { account_id: None }).unwrap(),
            json!({"type": "SyncNow", "account_id": null})
        );
    }

    #[test]
    fn rpc_request_round_trips() {
        let frame = json!({"id": 7, "request": {"type": "GetMessage", "id": "m1"}});
        let req: RpcRequest = serde_json::from_value(frame.clone()).unwrap();
        assert_eq!(
            req,
            RpcRequest {
                id: 7,
                request: Request::GetMessage { id: "m1".into() }
            }
        );
        assert_eq!(serde_json::to_value(&req).unwrap(), frame);
    }

    #[test]
    fn server_messages_are_kind_tagged() {
        let response: ServerMessage = RpcResponse {
            id: 3,
            response: Response::SyncStarted,
        }
        .into();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"kind": "response", "id": 3, "response": {"type": "SyncStarted"}})
        );
        let event: ServerMessage = Event::MailChanged {
            folder_ids: vec!["f1".into()],
        }
        .into();
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({"kind": "event", "event": {"type": "MailChanged", "folder_ids": ["f1"]}})
        );
        assert_eq!(
            serde_json::from_value::<ServerMessage>(value).unwrap(),
            event
        );
    }

    #[test]
    fn m1a_requests_serialize() {
        assert_eq!(
            serde_json::to_value(Request::DiscoverAccount {
                email: "ada@example.org".into()
            })
            .unwrap(),
            json!({"type": "DiscoverAccount", "email": "ada@example.org"})
        );
        assert_eq!(
            serde_json::to_value(Request::MoveMessages {
                message_ids: vec!["m1".into()],
                target: MoveTarget::Role(crate::FolderRole::Archive),
            })
            .unwrap(),
            json!({"type": "MoveMessages", "message_ids": ["m1"], "target": {"role": "archive"}})
        );
        assert_eq!(
            serde_json::to_value(Request::SendDraft {
                draft_id: "d1".into(),
                delay_secs: None,
                send_at: None,
            })
            .unwrap(),
            json!({"type": "SendDraft", "draft_id": "d1", "delay_secs": null, "send_at": null})
        );
        let req: Request = serde_json::from_value(json!({
            "type": "UpdateFlags", "message_ids": ["m1", "m2"], "change": {"seen": true}
        }))
        .unwrap();
        let Request::UpdateFlags {
            message_ids,
            change,
        } = req
        else {
            panic!("wrong variant");
        };
        assert_eq!(message_ids.len(), 2);
        assert_eq!(change.seen, Some(true));
        assert_eq!(
            serde_json::to_value(Request::GetAccountsConfig).unwrap(),
            json!({"type": "GetAccountsConfig"})
        );
    }

    #[test]
    fn m1a_responses_and_events_serialize() {
        assert_eq!(
            serde_json::to_value(Response::Queued {
                op_ids: vec!["op-1".into()]
            })
            .unwrap(),
            json!({"type": "Queued", "op_ids": ["op-1"]})
        );
        assert_eq!(
            serde_json::to_value(Response::Done).unwrap(),
            json!({"type": "Done"})
        );
        let invalid = Response::InvalidQuery {
            error: SearchError {
                kind: crate::search::SearchErrorKind::UnknownField,
                message: "unknown field `frm`".into(),
                start: 0,
                end: 3,
            },
        };
        assert_eq!(
            serde_json::to_value(&invalid).unwrap(),
            json!({"type": "InvalidQuery", "error": {
                "kind": "unknown-field", "message": "unknown field `frm`", "start": 0, "end": 3
            }})
        );
        let event = Event::OpStatus {
            status: OpStatus {
                op_id: "op-1".into(),
                account_id: "work".into(),
                state: crate::ops::OpState::Done,
                error: None,
                message: None,
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["status"]["state"], json!("done"));
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), event);
        assert_eq!(
            serde_json::to_value(Event::AccountsChanged).unwrap(),
            json!({"type": "AccountsChanged"})
        );
    }

    #[test]
    fn errors_convert_both_ways() {
        let response: Response = ApiError::new(ErrorCode::NotFound, "no such message").into();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"type": "Error", "code": "not_found", "message": "no such message"})
        );
        let err = response.into_result().unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(Response::SyncStarted.into_result().is_ok());
    }
}
