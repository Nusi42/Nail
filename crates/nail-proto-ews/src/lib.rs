//! The Exchange Web Services engine for Exchange Server on-premises
//! accounts (plan M3c WP92; contract ADR 0072).
//!
//! **Stub (WP90).** WP92 fills this crate; the contract it builds against
//! is `nail_api::exchange` (account options incl. the EWS URL and
//! streaming, Focused Inbox, categories, mailboxes, rooms, automatic
//! replies, inbox rules), the engine seams in
//! [`nail_api::exchange::engine`] — `EwsEngine` implements
//! [`nail_api::exchange::engine::ExchangeEngine`] (mail per folder with
//! sync states, streaming notifications through `wait_for_changes`,
//! settings, directory, PIM, plus [`nail_api::engine::FreeBusyProvider`]
//! and [`nail_api::engine::OutgoingTransport`]) — the provider-neutral
//! item shapes of [`nail_api::exchange::pim`] (EWS XML ↔ those; mapping to
//! Nail's PIM model is `nail-exchange`'s), and the Windows sign-in seam
//! [`nail_api::exchange::engine::NegotiateAuth`] (SSPI is implemented by
//! `nail-exchange`; tests use a scripted fake). Expected shape:
//!
//! * `soap` — envelopes with `RequestServerVersion` (default
//!   `Exchange2013_SP1`), `ExchangeImpersonation` never, `X-AnchorMailbox`
//!   for other mailboxes, faults → `nail_api::ErrorCode` + `ews-*` texts
//!   (`locales/en/core-ews.ftl`), `ErrorServerBusy` → `rate_limited` with
//!   the back-off the server names.
//! * `client` — `EwsClient` over the account's EWS URL (plain `http` only
//!   for loopback test servers): Basic over TLS, OAuth2 bearer with one
//!   refresh, Negotiate (`401` + `WWW-Authenticate: Negotiate` handshake on
//!   one kept-alive connection — NTLM binds to the connection).
//! * `mail` — `SyncFolderHierarchy`, `SyncFolderItems` (the sync state is
//!   the cursor; `ErrorInvalidSyncStateData` → `CursorExpired`),
//!   `GetItem` with `MimeContent`, `CreateItem` (`MimeContent` appends,
//!   `SendAndSaveCopy` for sending), `UpdateItem` (read, flag,
//!   categories), `MoveItem` (new id), `DeleteItem`; `Subscribe` +
//!   `GetStreamingEvents` for push.
//! * `settings` — `GetUserOofSettings` / `SetUserOofSettings`,
//!   `GetInboxRules` / `UpdateInboxRules`, the category list
//!   (`GetUserConfiguration` `CategoryList` on the calendar folder).
//! * `directory` — `GetRoomLists`, `GetRooms`, `GetUserAvailability`
//!   (free/busy of people and rooms), `ResolveNames`, `GetDelegate`.
//! * `pim` — `CalendarItem` (with `ModifiedOccurrences` /
//!   `DeletedOccurrences`), `AcceptItem` / `TentativelyAcceptItem` /
//!   `DeclineItem`, `Contact` / `DistributionList`, `Task`.
//! * `testing` (feature) — `MockEwsServer` on 127.0.0.1: an in-memory
//!   mailbox (folders, items with change keys and sync states, a shared
//!   mailbox, OOF, rules, categories, rooms, calendar, contacts, tasks,
//!   streaming notifications), Basic and a scripted Negotiate handshake,
//!   injected faults, recorded-response fixtures in `tests/fixtures/`.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

#[cfg(feature = "testing")]
pub mod testing;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The SOAP namespace of EWS messages (`m:`).
pub const MESSAGES_NS: &str = "http://schemas.microsoft.com/exchange/services/2006/messages";

/// The SOAP namespace of EWS types (`t:`).
pub const TYPES_NS: &str = "http://schemas.microsoft.com/exchange/services/2006/types";

/// The SOAP 1.1 envelope namespace.
pub const SOAP_NS: &str = "http://schemas.xmlsoap.org/soap/envelope/";

/// The server version Nail asks for unless the server is older
/// (`RequestServerVersion`).
pub const DEFAULT_SERVER_VERSION: &str = "Exchange2013_SP1";

/// Wraps an EWS operation body in a SOAP envelope with the
/// `RequestServerVersion` header.
pub fn envelope(server_version: &str, body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<soap:Envelope xmlns:soap="{soap}" xmlns:t="{types}" xmlns:m="{messages}">"#,
            r#"<soap:Header><t:RequestServerVersion Version="{version}"/></soap:Header>"#,
            r#"<soap:Body>{body}</soap:Body></soap:Envelope>"#
        ),
        soap = SOAP_NS,
        types = TYPES_NS,
        messages = MESSAGES_NS,
        version = server_version,
        body = body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_carry_the_version() {
        assert_eq!(CRATE, "nail-proto-ews");
        let soap = envelope(DEFAULT_SERVER_VERSION, "<m:GetFolder/>");
        assert!(soap.starts_with("<?xml"));
        assert!(soap.contains(r#"Version="Exchange2013_SP1""#));
        assert!(soap.contains("<soap:Body><m:GetFolder/></soap:Body>"));
    }
}
