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
mod composer;
mod context;
mod creds;
mod customize;
#[cfg(feature = "demo")]
pub mod demo;
mod engine;
mod error;
mod ids;
mod imap_engine;
mod ingest;
mod mail;
mod mcp;
mod mime;
mod oauth;
mod outbox;
pub mod paths;
mod pop3_engine;
mod replay;
mod rules;
mod search;
mod service;
mod setup;
mod strings;
mod sync;
mod virtuals;
mod watch;

pub use error::{CoreError, Result};
pub use service::{EVENT_CAPACITY, EngineTiming, MAX_PAGE_SIZE, Service, ServiceOptions};
pub use sync::{FolderSyncFailure, ImapAccount, SyncReport};
