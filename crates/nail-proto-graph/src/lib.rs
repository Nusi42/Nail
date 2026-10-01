//! The Microsoft Graph engine for Exchange Online accounts (plan M3c WP91;
//! contract ADR 0072).
//!
//! **Stub (WP90).** WP91 fills this crate; the contract it builds against
//! is `nail_api::exchange` (account options, Focused Inbox, categories,
//! mailboxes, rooms, automatic replies, inbox rules), the engine seams in
//! [`nail_api::exchange::engine`] — `GraphEngine` implements
//! [`nail_api::exchange::engine::ExchangeEngine`] (mail per folder with
//! delta links, settings, directory, PIM, plus
//! [`nail_api::engine::FreeBusyProvider`] and
//! [`nail_api::engine::OutgoingTransport`]) — and the provider-neutral
//! item shapes of [`nail_api::exchange::pim`] (Graph JSON ↔ those; the
//! mapping to Nail's PIM model is `nail-exchange`'s). Expected shape:
//!
//! * `client` — `GraphClient` over `https://graph.microsoft.com/v1.0`
//!   (the account's `exchange` endpoint; plain `http` only for loopback
//!   test servers), bearer tokens from an `AccessTokenProvider` with one
//!   forced refresh on `401`, `/me` or `/users/{mailbox}` paths, errors →
//!   `nail_api::ErrorCode` + `graph-*` texts (`locales/en/core-graph.ftl`),
//!   `429` / `503` / `504` → `EngineError::rate_limited` with the
//!   `Retry-After` and backoff; non-idempotent calls are never repeated
//!   after a 5xx; `$batch` (at most 20 requests) for metadata.
//! * `mail` — `mailFolders/delta` (hierarchy), `mailFolders/{id}/messages/delta`
//!   (per folder, `@odata.deltaLink` = the cursor, `410` / `syncStateNotFound`
//!   → `CursorExpired`), `messages/{id}/$value` (MIME), `PATCH` (isRead,
//!   flag, categories, `inferenceClassification`), `move` (new id), `DELETE`,
//!   MIME create for appends, `sendMail` with MIME (base64 `text/plain`),
//!   `inferenceClassification/overrides`.
//! * `settings` — `outlook/masterCategories`, `mailboxSettings`
//!   (`automaticRepliesSetting`, HTML ↔ text), `mailFolders/inbox/messageRules`.
//! * `directory` — `places/microsoft.graph.roomlist`, `…/rooms`, a probe of
//!   another mailbox; `calendar/getSchedule` (free/busy of people and rooms).
//! * `pim` — `calendars`, `calendarView/delta` / `events` with exceptions,
//!   `accept` / `tentativelyAccept` / `decline`, `contactFolders` + contacts
//!   delta, `todo/lists` + tasks delta (checklist items, linked resources).
//! * `testing` (feature) — `MockGraphServer` on 127.0.0.1 with an
//!   in-memory tenant (mailboxes incl. a shared one, folders, messages,
//!   delta tokens, categories, rules, automatic replies, rooms, calendars,
//!   contacts, To Do; throttling and injected errors; a request log) and
//!   recorded-response fixtures in `tests/fixtures/`.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

#[cfg(feature = "testing")]
pub mod testing;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The Graph base URL on `host` (`https`, or `http` when `tls` is false:
/// loopback test servers only), e.g. `https://graph.microsoft.com/v1.0`.
pub fn api_base(host: &str, port: u16, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    let default_port = if tls { 443 } else { 80 };
    let base = nail_api::exchange::GRAPH_BASE_PATH;
    if port == default_port {
        format!("{scheme}://{host}{base}")
    } else {
        format!("{scheme}://{host}:{port}{base}")
    }
}

/// The path of a mailbox's resources: `/me` for the account's own,
/// `/users/{address}` for a shared / delegate mailbox.
pub fn mailbox_path(mailbox: Option<&str>) -> String {
    match mailbox {
        None => "/me".to_owned(),
        Some(address) => format!("/users/{}", path_segment(address)),
    }
}

/// Percent-encodes a path segment (Graph ids contain `/`, `+`, `=`).
pub fn path_segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'@') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_and_urls() {
        assert_eq!(CRATE, "nail-proto-graph");
        assert_eq!(
            api_base(nail_api::exchange::GRAPH_HOST, 443, true),
            "https://graph.microsoft.com/v1.0"
        );
        assert_eq!(
            api_base("127.0.0.1", 8098, false),
            "http://127.0.0.1:8098/v1.0"
        );
        assert_eq!(mailbox_path(None), "/me");
        assert_eq!(
            mailbox_path(Some("support@contoso.example")),
            "/users/support@contoso.example"
        );
        assert_eq!(path_segment("AAMk+a/b="), "AAMk%2Ba%2Fb%3D");
    }
}
