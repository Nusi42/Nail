//! The Nail core service.
//!
//! [`Service`] owns the local store ([`nail_store`]), the configuration
//! ([`nail_config::ConfigManager`], watched for changes) and the read-only
//! IMAP sync engine. Every transport — the desktop shell's `nail_call`
//! command, the WebSocket of `nail serve`, tests — hands it
//! [`nail_api::Request`]s ([`Service::handle`]) and forwards the
//! [`nail_api::Event`]s it broadcasts ([`Service::subscribe`]).
//!
//! With the default `demo` feature, [`demo::seed_demo`] loads the
//! `nail-testkit` corpus as a local account and [`demo::start_demo_imap`]
//! serves it from an in-process mock IMAP server.
//!
//! See `docs/adr/0009-core-service.md`.

#![warn(missing_docs)]

#[cfg(feature = "demo")]
pub mod demo;
mod error;
mod ids;
mod ingest;
pub mod paths;
mod service;
mod sync;

pub use error::{CoreError, Result};
pub use service::{MAX_PAGE_SIZE, Service, ServiceOptions};
pub use sync::{ImapAccount, SyncReport};
