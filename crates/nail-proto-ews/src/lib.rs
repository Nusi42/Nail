//! The Exchange Web Services engine for Exchange Server on-premises
//! accounts (plan M3c WP92; contract ADR 0072, decisions ADR 0074).
//!
//! [`EwsEngine`] implements [`nail_api::exchange::engine::ExchangeEngine`]
//! (mail per folder with sync states, streaming notifications through
//! `wait_for_changes`, settings, directory, PIM, free/busy and sending)
//! on an [`EwsClient`]:
//!
//! * [`soap`] — envelopes with `RequestServerVersion` (`Exchange2016`,
//!   falling back to `Exchange2013_SP1` when a server refuses it) and
//!   `TimeZoneContext`, `ServerVersionInfo`, response messages, faults
//!   (`ErrorServerBusy` → [`EwsError::RateLimited`] with the back-off the
//!   server names).
//! * [`client`] — the endpoint (plain `http` only for loopback test
//!   servers), Basic over TLS, OAuth2 bearer with one refresh, Negotiate
//!   through the injected [`nail_api::exchange::engine::NegotiateAuth`]
//!   (SSPI lives in `nail-exchange`; the handshake shares one kept-alive
//!   connection because NTLM binds to it), `X-AnchorMailbox`, inline
//!   retries.
//! * [`mail`] — `SyncFolderHierarchy`, `SyncFolderItems`, `GetItem` with
//!   `MimeContent`, `CreateItem` (appends, `SendAndSaveCopy`),
//!   `UpdateItem` (read, flag, answered, categories with change keys),
//!   `MoveItem` / `CopyItem` (new ids), `DeleteItem`.
//! * [`notify`] — streaming notifications: `Subscribe` +
//!   `GetStreamingEvents` with reconnects and re-subscription.
//! * [`calendar`], [`recurrence`], [`tasks`], [`contacts`] — calendar items
//!   (`FindItem` `CalendarView`, exceptions, meeting responses), tasks and
//!   contacts ↔ [`nail_api::exchange::pim`] shapes.
//! * [`settings`] — `GetUserOofSettings` / `SetUserOofSettings`,
//!   `GetInboxRules` / `UpdateInboxRules`, the master category list
//!   (`CategoryList` user configuration on the calendar).
//! * [`directory`] — `GetRoomLists`, `GetRooms`, `GetUserAvailability`,
//!   `ResolveNames`, `GetDelegate`.
//! * `testing` (feature) — `testing::MockEwsServer` on 127.0.0.1.
//!
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

pub mod auth;
pub mod calendar;
pub mod client;
pub mod contacts;
pub mod directory;
pub mod engine;
pub mod error;
pub mod html;
pub mod mail;
pub mod notify;
pub mod recurrence;
pub mod settings;
pub mod soap;
pub mod tasks;
#[cfg(feature = "testing")]
pub mod testing;
pub mod types;
pub mod xml;

pub use auth::{EwsAuth, Password};
pub use client::{EwsClient, RetryPolicy};
pub use engine::{EwsEngine, EwsOptions};
pub use error::EwsError;
pub use soap::{
    DEFAULT_SERVER_VERSION, FALLBACK_SERVER_VERSION, MESSAGES_NS, SOAP_NS, TYPES_NS, envelope,
};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_carry_the_version() {
        assert_eq!(CRATE, "nail-proto-ews");
        let soap = envelope(DEFAULT_SERVER_VERSION, "<m:GetFolder/>");
        assert!(soap.starts_with("<?xml"));
        assert!(soap.contains(r#"Version="Exchange2016""#));
        assert!(soap.contains("<soap:Body><m:GetFolder/></soap:Body>"));
    }
}
