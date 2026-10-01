//! The Gmail API engine (plan M3b WP82; contract ADR 0065).
//!
//! **Stub (WP80).** WP82 fills this crate; the contract it builds against
//! is `nail_api::gmail` (label ↔ folder / tag mapping, categories,
//! [`nail_api::gmail::GmailFolder::move_change`],
//! [`nail_api::gmail::LabelChange::from_flags`]), the provider-neutral
//! [`nail_api::engine::MailApiEngine`] (folders, full listing pages,
//! history deltas with [`nail_api::engine::DeltaOutcome::CursorExpired`],
//! raw fetches, replayed [`nail_api::engine::RemoteWrite`]s),
//! [`nail_api::engine::OutgoingTransport`] (`users.messages.send`),
//! [`nail_api::engine::AdaptivePoll`] and the store's Gmail tables
//! (`nail_store::gmail`, used by the core, not by this crate). Expected
//! shape:
//!
//! * `client` — `GmailClient` over `https://<host>/gmail/v1/users/me`
//!   (the account's `gmail` endpoint; plain `http` only for loopback test
//!   servers), bearer tokens from an `AccessTokenProvider` with one forced
//!   refresh on `401`, errors → `nail_api::ErrorCode` + `gmail-*` texts
//!   (`locales/en/core-gmail.ftl`), `429` / `403 rateLimitExceeded` /
//!   `5xx` → `EngineError::rate_limited` with the server's `Retry-After`
//!   and exponential backoff.
//! * `messages` — `messages.list` (paging, `q=after:` for "recent N days"),
//!   `messages.get?format=raw|metadata`, `modify` (labels), `trash`,
//!   `untrash`, `delete`, `insert`, `send` (raw MIME, `threadId` for
//!   replies), `attachments.get`; `batchModify`, and HTTP batch requests
//!   (`multipart/mixed`, at most 100 parts) for metadata fetches.
//! * `history` — `history.list` from a `historyId` (all history types,
//!   paging), mapped to `SyncDelta`; `404` → `CursorExpired`.
//! * `labels` — `labels.list` / `get` / `create` / `patch` / `delete`,
//!   system labels → `GmailFolder`s, user labels → tags.
//! * `engine` — `GmailEngine: MailApiEngine + OutgoingTransport`.
//! * `testing` (feature) — `MockGmailServer` on 127.0.0.1 with an
//!   in-memory mailbox (messages, labels, threads, history ids, quotas /
//!   injected errors, server-side changes for tests) and recorded-response
//!   fixtures in `tests/fixtures/`.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

#[cfg(feature = "testing")]
pub mod testing;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The URL of the Gmail API for `users/me` on `host` (`https`, or `http`
/// when `tls` is false: loopback test servers only), e.g.
/// `https://gmail.googleapis.com/gmail/v1/users/me`.
pub fn api_base(host: &str, port: u16, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    let default_port = if tls { 443 } else { 80 };
    let base = nail_api::gmail::GMAIL_API_BASE_PATH;
    if port == default_port {
        format!("{scheme}://{host}{base}")
    } else {
        format!("{scheme}://{host}:{port}{base}")
    }
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
    }
}
