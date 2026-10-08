//! IMAP client engine.
//!
//! [`ImapClient::connect`] opens a connection with implicit TLS (default),
//! STARTTLS or — for tests only — plaintext, logs in (`PLAIN`, `LOGIN`,
//! `XOAUTH2`, `OAUTHBEARER`), and then switches on `COMPRESS=DEFLATE`, sends
//! `ID` and enables QRESYNC / CONDSTORE when the server offers them.
//!
//! Reading: [list folders](ImapClient::list_folders) with roles (RFC 6154
//! SPECIAL-USE, falling back to well-known names),
//! [plan what is new](ImapClient::plan_fetch) since a stored [`SyncState`]
//! and [fetch it batch by batch](ImapClient::fetch_batch), and
//! [resynchronize flags and expunges](ImapClient::resync_flags) of known
//! messages (QRESYNC, CONDSTORE or a full flag diff).
//!
//! Writing: [flags](ImapClient::store_flags) (also from an API
//! [`nail_api::FlagChange`]), [move](ImapClient::move_messages) (`MOVE` or
//! `COPY` + `\Deleted` + `UID EXPUNGE`), [copy](ImapClient::copy_messages),
//! [expunge](ImapClient::expunge_messages), [append](ImapClient::append)
//! with UIDPLUS results, and folder
//! [create](ImapClient::create_folder) / [rename](ImapClient::rename_folder)
//! / [delete](ImapClient::delete_folder).
//!
//! Sync policies (M2c WP53): [`ImapClient::fetch_batch_with`] fetches a
//! planned batch as whole messages, header blocks only, or whole messages
//! since a day ([`BodyFetch`]); [`ImapClient::fetch_body`] fetches one
//! body on demand. Server-side search: [`ImapClient::search_messages`]
//! runs a [`SearchKey`] (`UID SEARCH`, `CHARSET UTF-8` when needed).
//!
//! Push: [`ImapClient::idle`] waits for changes on one connection;
//! [`MailboxWatcher`] keeps a dedicated connection in `IDLE` (re-issued
//! every 29 minutes) or polls with `NOOP`, reconnects with backoff and
//! reports [`WatchEvent`]s as a stream.
//!
//! The protocol layer is in-house over the `imap-proto` parser, with
//! `tokio-rustls` (ring provider) and the platform certificate verifier;
//! see `docs/adr/0005-imap-library.md` and
//! `docs/adr/0018-wp11-imap-engine.md`.

mod bodies;
mod capabilities;
mod changes;
mod client;
mod config;
mod conn;
mod error;
mod flags;
mod folder;
mod idle;
mod resilient;
mod resync;
mod salvage;
mod sasl;
mod search;
mod transport;
mod watch;
mod write;

pub use bodies::{BodyFetch, ImapDate};
pub use capabilities::Capabilities;
pub use changes::{FlagUpdate, MailboxChange};
pub use client::{
    FetchPlan, FetchedMessage, FolderStatus, ImapClient, IncrementalFetch, SyncState, UidRange,
};
pub use config::{
    AuthMechanism, ClientId, Credentials, FetchBatchLimits, ImapConfig, Secret, Security, TlsTrust,
    default_client_id,
};
pub use error::{ImapError, Result};
pub use flags::{Flag, MessageFlags};
pub use folder::{
    RemoteFolder, decode_modified_utf7, encode_modified_utf7, folder_path, role_from_name,
};
pub use idle::{IDLE_REISSUE, IdleEnd, IdleOutcome};
pub use resilient::{BatchFetch, FetchFailure, FetchFailureKind};
pub use resync::{FlagResync, ResyncMethod, ResyncState};
pub use search::SearchKey;
pub use transport::ImapStream;
pub use watch::{CredentialProvider, MailboxWatcher, WatchConfig, WatchEvent, WatchMode};
pub use write::{
    AppendResult, CopyResult, ExpungeMethod, MoveMethod, MoveResult, StoreMode, StoreResult,
};
