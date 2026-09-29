//! Read-only IMAP client engine (M0).
//!
//! [`ImapClient::connect`] opens a connection with implicit TLS (default),
//! STARTTLS or — for tests only — plaintext, reads the capabilities and logs
//! in with `AUTHENTICATE PLAIN` or `LOGIN`. A connected client can
//! [list folders](ImapClient::list_folders) with roles (RFC 6154 SPECIAL-USE,
//! falling back to well-known names), [examine](ImapClient::examine) a
//! mailbox, [fetch a UID range](ImapClient::fetch_range) as raw messages with
//! flags, and [fetch what is new](ImapClient::fetch_new) since a stored
//! [`SyncState`], detecting UIDVALIDITY changes.
//!
//! The client never modifies the server: mailboxes are opened with `EXAMINE`
//! and bodies are fetched with `BODY.PEEK[]`.
//!
//! Built on `async-imap` (see `docs/adr/0005-imap-library.md`) with
//! `tokio-rustls` (ring provider) and the platform certificate verifier.

mod capabilities;
mod client;
mod config;
mod error;
mod folder;
mod transport;

pub use capabilities::Capabilities;
pub use client::{
    Envelope, FetchedMessage, FolderStatus, ImapClient, IncrementalFetch, SyncState, UidRange,
};
pub use config::{AuthMechanism, Credentials, ImapConfig, Security, TlsTrust};
pub use error::{ImapError, Result};
pub use folder::{RemoteFolder, decode_modified_utf7, role_from_name};
pub use transport::ImapStream;
