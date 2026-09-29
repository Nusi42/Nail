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

use crate::config::layout::ResolvedLayout;
use crate::config::theme::ResolvedTheme;
use crate::domain::{Account, Folder, MessageBody, MessageSummary};
use crate::error::{ApiError, ErrorCode};
use crate::ids::{AccountId, FolderId, MessageId};

/// Version of this protocol. Bumped on incompatible changes; returned by
/// [`Response::Pong`] so clients can detect a mismatched core.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from a client to the core.
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
    /// configuration. Also clears any error shown for [`Event::ConfigError`].
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
