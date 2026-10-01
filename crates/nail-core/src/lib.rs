//! The Nail core service.
//!
//! [`Service`] owns the local store ([`nail_store`]), the search index
//! ([`nail_search`]), the configuration ([`nail_config::ConfigManager`] for
//! theme and layout, the account manager for `accounts.kdl`, all watched
//! for changes), and one engine per account (IMAP with IDLE, POP3 on a
//! schedule), the outbox sender and the OAuth2 token source. Every
//! transport — the desktop shell's `nail_call` command, the WebSocket of
//! `nail serve`, tests — hands it [`nail_api::Request`]s
//! ([`Service::handle`]) and forwards the [`nail_api::Event`]s it
//! broadcasts ([`Service::subscribe`]).
//!
//! With the default `demo` feature, [`demo`] loads the `nail-testkit`
//! corpus as a local account and starts in-process mock servers for
//! `nail serve --seed demo-imap` / `demo-multi`.
//!
//! See `docs/adr/0009-core-service.md` and `docs/adr/0020-m1a-core-integration.md`.

#![warn(missing_docs)]

mod accounts;
mod bodies;
mod clock;
mod composer;
mod context;
mod creds;
mod crypto;
mod customize;
#[cfg(feature = "demo")]
pub mod demo;
mod engine;
mod error;
mod ids;
mod imap_engine;
mod ingest;
mod junk;
mod launch;
mod mail;
mod mcp;
mod notify;
mod oauth;
mod outbox;
pub mod paths;
mod pim;
mod plugin_host;
mod plugin_http;
mod plugins;
mod pop3_engine;
mod remote;
mod remote_access;
mod replay;
mod rule_mail;
mod rules;
mod search;
mod security;
mod service;
mod setup;
mod sieve;
mod snooze;
mod strings;
mod sync;
mod sync_policy;
mod triage;
mod vault;
mod virtuals;
mod watch;

pub use clock::AdjustableClock;
pub use error::{CoreError, Result};
pub use mcp::{McpCore, McpRunning, mcp_endpoint_path};
pub use nail_store::DataDirLock;
pub use remote::RemoteUiCommand;
pub use service::{
    EVENT_CAPACITY, EngineTiming, HISTORY_PURGE_INTERVAL, HISTORY_RETENTION_DAYS, MAX_PAGE_SIZE,
    Service, ServiceOptions,
};
pub use sync::{FolderSyncFailure, ImapAccount, SyncReport};
