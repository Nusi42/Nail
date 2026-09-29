//! POP3 client (RFC 1939) for Nail.
//!
//! An in-house async client ([`Pop3Client`]): implicit TLS (995), `STLS`
//! (110, RFC 2595; never downgraded) or plaintext for local test servers;
//! `CAPA` (RFC 2449); login with `USER`/`PASS`, `APOP`, SASL `PLAIN`,
//! `XOAUTH2` or `OAUTHBEARER` (RFC 5034, including the OAuth error
//! challenge); `STAT`, `LIST`, `UIDL`, `TOP`, `RETR`, `DELE`, `RSET`,
//! `NOOP`, `QUIT` (which commits deletions).
//!
//! [`policy`] plans downloads and server deletions for the leave-on-server
//! settings of [`nail_api::accounts::Pop3Options`] as pure functions.
//!
//! Errors ([`Pop3Error`]) convert into [`nail_api::engine::EngineError`]; see
//! `docs/adr/0017-wp12-outgoing-pop3-auth-discovery.md`.

#![warn(missing_docs)]

mod client;
mod config;
mod error;
pub mod policy;
mod transport;

pub use client::{Capabilities, MAX_MESSAGE_BYTES, Pop3Client, ServerMessage, test_login};
pub use config::{Pop3Config, Pop3Credentials, TlsTrust};
pub use error::{Pop3Error, Result};
pub use policy::{LeavePolicy, plan_deletions, plan_downloads, vanished};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
