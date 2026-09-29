//! SMTP submission (RFC 6409) for Nail.
//!
//! * [`SmtpSession`]: one connection — implicit TLS (465, RFC 8314),
//!   STARTTLS (587, never downgraded) or plaintext for local test servers;
//!   `EHLO` [`Capabilities`]; `AUTH PLAIN` / `LOGIN` / `XOAUTH2` /
//!   `OAUTHBEARER`; raw MIME submission with `SIZE` check and declaration,
//!   `8BITMIME` / `SMTPUTF8`, optional DSN (RFC 3461).
//! * [`SmtpTransport`]: the account's [`nail_api::engine::OutgoingTransport`],
//!   with credentials from the keychain or the OAuth2 token provider
//!   ([`SmtpAuth`]); a rejected OAuth2 token is refreshed once.
//! * [`test_login`]: connect + login check for account setup.
//!
//! Errors ([`SmtpError`]) separate authentication failures, permanent
//! refusals (5xx → `rejected`, not retryable), temporary ones (4xx,
//! retryable), size limits (`too_large`) and network/TLS trouble, and
//! convert into [`nail_api::engine::EngineError`].
//!
//! Built on `lettre`'s low-level connection with rustls (`ring`) and the
//! platform verifier; see `docs/adr/0014-m1a-libraries.md` and
//! `docs/adr/0017-wp12-outgoing-pop3-auth-discovery.md`.

#![warn(missing_docs)]

mod capabilities;
mod client;
mod config;
mod error;
mod transport;

pub use capabilities::Capabilities;
pub use client::SmtpSession;
pub use config::{SendOptions, SmtpConfig, SmtpCredentials, TlsTrust};
pub use error::{Result, SmtpError};
pub use transport::{SmtpAuth, SmtpTransport, test_login};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
