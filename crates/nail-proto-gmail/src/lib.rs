//! The Gmail API engine (plan M3b WP82; contract ADR 0065, package ADR
//! `docs/adr/0067-gmail-engine.md`).
//!
//! A Gmail account syncs through the Gmail REST API instead of IMAP. The
//! crate builds on the contract in `nail_api::gmail` (labels ↔ folders /
//! tags, categories) and `nail_api::engine` ([`MailApiEngine`],
//! [`OutgoingTransport`], [`AdaptivePoll`]):
//!
//! * [`client`] — [`GmailClient`] over `https://<host>/gmail/v1/users/me`
//!   (plain `http` only for loopback test servers): bearer tokens from an
//!   [`AccessTokenProvider`] with one forced refresh on `401`; `429`,
//!   `403 rateLimitExceeded` and `5xx` retried with exponential backoff or
//!   the server's `Retry-After` ([`RetryPolicy`]), longer waits handed to
//!   the caller as `EngineError::retry_after_secs`; errors →
//!   `nail_api::ErrorCode` + `gmail-*` texts ([`GmailError`],
//!   `locales/en/core-gmail.ftl`).
//! * [`messages`] — `messages.list` / `get` (`raw`, `minimal`,
//!   `metadata`, `full`) / `modify` / `batchModify` / `trash` / `untrash`
//!   / `delete` / `insert` / `send` (raw MIME; media upload above 4 MB),
//!   `drafts.create`, `attachments.get`. Attachments come with the raw
//!   MIME.
//! * [`batch`] — HTTP batch requests (`multipart/mixed`, at most 100
//!   parts) for metadata fetches, with per-part retries.
//! * [`history`] — `history.list` (paging; `404` → expired cursor) and
//!   [`history::fold_history`].
//! * [`labels`] — `labels.list` / `get` / `create` / `patch` / `delete`,
//!   [`labels::LabelTable`] (user labels ↔ tags).
//! * [`threads`] — `threads.list` / `get` / `modify`.
//! * [`mapping`] — labels → folder, flags, tags, category.
//! * [`engine`] — [`GmailEngine`]: [`MailApiEngine`] + [`OutgoingTransport`].
//! * [`poll`] — [`PollTimer`]: adaptive polling (no Pub/Sub).
//! * [`wire`] — the JSON resources.
//! * `testing` (feature) — `MockGmailServer` on 127.0.0.1: an in-memory
//!   Gmail (messages, labels, threads, history, drafts, batch, quotas and
//!   injected errors, server-side changes) loaded from recorded-style
//!   fixtures in `fixtures/`.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).
//!
//! [`MailApiEngine`]: nail_api::engine::MailApiEngine
//! [`OutgoingTransport`]: nail_api::engine::OutgoingTransport
//! [`AdaptivePoll`]: nail_api::engine::AdaptivePoll
//! [`AccessTokenProvider`]: nail_api::engine::AccessTokenProvider

#![warn(missing_docs)]

pub mod batch;
pub mod client;
pub mod engine;
pub mod error;
pub mod history;
pub mod labels;
pub mod mapping;
pub mod messages;
pub mod poll;
pub mod threads;
pub mod wire;

#[cfg(feature = "testing")]
pub mod testing;

pub use client::{GmailAuth, GmailClient, RetryPolicy};
pub use engine::{EngineOptions, GmailEngine};
pub use error::GmailError;
pub use labels::GmailLabelInfo;
pub use poll::PollTimer;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The URL of the Gmail API for `users/me` on `host` (`https`, or `http`
/// when `tls` is false: loopback test servers only), e.g.
/// `https://gmail.googleapis.com/gmail/v1/users/me`.
pub fn api_base(host: &str, port: u16, tls: bool) -> String {
    nail_net::http::origin_url(host, port, tls, nail_api::gmail::GMAIL_API_BASE_PATH)
}

/// The API base URL of a `gmail` account's server settings
/// (`tls="plaintext"` = `http`, which the client accepts for loopback
/// hosts only).
pub fn api_base_for(server: &nail_api::accounts::ServerEndpoint) -> String {
    api_base(
        &server.host,
        server.port,
        server.tls != nail_api::accounts::TlsMode::Plaintext,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_and_base_url() {
        assert_eq!(CRATE, "nail-proto-gmail");
        assert_eq!(
            api_base(nail_api::gmail::GMAIL_API_HOST, 443, true),
            "https://gmail.googleapis.com/gmail/v1/users/me"
        );
        assert_eq!(
            api_base("127.0.0.1", 8099, false),
            "http://127.0.0.1:8099/gmail/v1/users/me"
        );
        assert_eq!(
            api_base("::1", 9, false),
            "http://[::1]:9/gmail/v1/users/me"
        );
    }
}
