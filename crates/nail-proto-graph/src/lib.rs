//! The Microsoft Graph engine for Exchange Online accounts (plan M3c WP91;
//! contract ADR 0072; this package's decisions: ADR 0073).
//!
//! [`GraphEngine`] implements
//! [`nail_api::exchange::engine::ExchangeEngine`] over Microsoft Graph
//! `v1.0` and converts between Graph JSON and the provider-neutral shapes
//! of [`nail_api::exchange`] (the mapping to Nail's own models is
//! `nail-exchange`'s):
//!
//! * [`client`] — [`GraphClient`] on the workspace reqwest with nail-auth's
//!   rustls / ring configuration (https, or http to loopback only), OAuth2
//!   bearer tokens from an [`nail_api::engine::AccessTokenProvider`] with
//!   one forced refresh on `401`, throttling (`429` / `503` / `504` with
//!   `Retry-After`) and backoff, paging links checked against the API's
//!   origin; errors → [`GraphError`] → `ErrorCode` + `graph-*` texts.
//! * [`batch`] — JSON batching (`$batch`, 20 requests per call, throttled
//!   parts retried).
//! * `mail` — `mailFolders/delta`, per-folder `messages/delta` with the
//!   `@odata.deltaLink` as cursor and expiry handling, `$value` MIME,
//!   `PATCH` (read, flag, categories), `move` / `copy` / `permanentDelete`,
//!   MIME creates, Focused Inbox and overrides, attachments, `sendMail`
//!   with base64 MIME, shared / delegate mailboxes through `/users/{upn}`.
//! * `settings` — master categories, automatic replies (HTML ↔ text),
//!   inbox rules.
//! * `directory` — room lists and rooms (`places`), other mailboxes,
//!   free/busy (`getSchedule`) and meeting suggestions (`findMeetingTimes`).
//! * `pim` — calendars (`calendarView/delta`, events in their own zone,
//!   series with exceptions, instances, meeting responses incl. new-time
//!   proposals, cancellations), contacts (+ delta), Microsoft To Do (lists,
//!   tasks + delta, checklist items, linked resources).
//! * [`upload`] (M4c) — messages over 4 MB: a draft from the message's
//!   parts, attachments over 3 MB through upload sessions (pre-authenticated
//!   byte ranges, progress reported), then `send`; send as / on behalf of
//!   shared mailboxes ([`nail_api::accounts::SendingMailbox`]); headers-only
//!   fetches for sync policies; moves between mailboxes.
//! * [`poll`] — adaptive polling ([`PollTimer`], [`FolderPoll`]); Graph
//!   has no push for a desktop client.
//! * `testing` (feature `testing`) — `MockGraphServer` on 127.0.0.1 with
//!   an in-memory tenant, recorded-style fixtures and mutation helpers.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

use std::fmt::Write as _;

pub mod batch;
pub mod client;
pub mod directory;
pub mod engine;
pub mod error;
pub mod mail;
pub mod mime;
pub mod pim;
pub mod poll;
pub mod settings;
pub mod text;
pub mod upload;
pub mod wire;

#[cfg(feature = "testing")]
pub mod testing;

pub use client::{GraphAuth, GraphClient, RetryPolicy};
pub use engine::{EngineOptions, GraphEngine};
pub use error::GraphError;
pub use poll::{FolderPoll, PollTimer};

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
            let _ = write!(out, "%{byte:02X}");
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
