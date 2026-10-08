//! Shared network plumbing of the protocol clients and mock servers (wave
//! Q2, SSOT / ODR / DRY, ADR 0171). Before Q2 the IMAP, POP3, SMTP and
//! ManageSieve clients, the HTTP API clients (Gmail, Graph, EWS, DAV) and
//! the mock servers of `nail-testkit-net` each had their own copy of the
//! same plumbing; this crate holds the one definition.
//!
//! Each module is a cargo feature, so a crate pulls only what it uses:
//!
//! - [`http`] (feature `http`): the URL policy, the retry policy with
//!   `Retry-After`, bounded bodies, transport error classes, bearer
//!   sign-in, mock routing; [`api`]: the request loop of the OAuth2 JSON
//!   API clients (Gmail, Graph);
//! - [`io`] (feature `io`): stream delegation macros, bounded line
//!   reading, the idle timeout;
//! - [`deflate`] (feature `deflate`): DEFLATE-compressed streams;
//! - [`tls`] (feature `tls`): the rustls client configuration (ring, ADR
//!   0014) for Nail's trust choices;
//! - [`xml`] (feature `xml`): a namespace-aware XML tree for protocol
//!   bodies;
//! - [`ports`]: the well-known ports of the mail protocols;
//! - [`status`]: the HTTP status codes the clients act on;
//! - [`trust`]: the TLS trust choices and the builders of the clients'
//!   connection settings;
//! - [`wire`]: small protocol text forms (IMAP sequence sets).

use std::time::Duration;

/// The default per-operation timeout of the line-based protocol clients
/// (IMAP, POP3, SMTP) and of Autodiscover.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_mins(1);

#[cfg(feature = "http")]
pub mod api;
#[cfg(feature = "deflate")]
pub mod deflate;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "io")]
pub mod io;
#[cfg(feature = "http")]
pub mod poll;
pub mod ports;
pub mod status;
#[cfg(feature = "tls")]
pub mod tls;
pub mod trust;
pub mod wire;
#[cfg(feature = "xml")]
pub mod xml;
