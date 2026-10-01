//! The Exchange common layer (plan M3c WP93; contract ADR 0072; decisions
//! ADR 0075).
//!
//! It sits between the two protocol engines (`nail-proto-graph`,
//! `nail-proto-ews`) and the core:
//!
//! * [`autodiscover`] — account setup with engine choice: Autodiscover v2
//!   (JSON) and POX, the HTTP redirect method and SRV
//!   `_autodiscover._tcp` (through `nail_discovery::DnsLookup`); redirects
//!   are followed over `https` only and **credentials never go to a host
//!   outside the address's domain** without the user's confirmation
//!   ([`autodiscover::AutodiscoverOutcome::ConfirmRedirect`]). Graph for
//!   Exchange Online, EWS (with the found URL) for on-premises, IMAP +
//!   SMTP as a fallback; automapped shared mailboxes.
//! * [`sspi`] — [`sspi::SspiNegotiate`]: the contract's
//!   [`nail_api::exchange::engine::NegotiateAuth`] over Windows SSPI
//!   (Negotiate / Kerberos / NTLM, signed-in user or explicit
//!   credentials); `unsupported` off Windows. [`negotiate`] runs the HTTP
//!   401 handshake over any `NegotiateAuth` (tests use fakes) and Basic
//!   over TLS.
//! * [`mapping`] — Exchange ↔ Nail: categories ↔ tags with colours,
//!   Focused Inbox filtering, Nail rules ↔ Exchange inbox rules where
//!   expressible (with `exchange-rule-reason-*` texts), automatic replies ↔
//!   vacation settings, the [`nail_api::exchange::pim`] item shapes ↔
//!   Nail events / tasks (To Do checklists ↔ subtasks) / contacts, rooms
//!   for the free/busy picker.
//! * [`http`] — the HTTP seam (no redirects followed; a loopback routing
//!   switch for tests and demo runs).
//! * `testing` (feature) — `MockAutodiscoverServer` on 127.0.0.1 (v2 JSON
//!   and POX answers, redirects, Basic and scripted Negotiate logins,
//!   automapped mailboxes, a request log).
//!
//! Texts are in `locales/en/core-exchange.ftl` ([`texts`]). Notes on the
//! EWS retirement in Exchange Online: ADR 0075 and
//! `userDocs/en/accounts-reference.md`.

#![warn(missing_docs)]

pub mod autodiscover;
pub mod http;
pub mod mapping;
pub mod negotiate;
pub mod sspi;
#[cfg(feature = "testing")]
pub mod testing;
pub mod texts;
mod xml;

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The Autodiscover v2 URL for `address` on `host` (`https`, or `http` for
/// loopback test servers) asking for `protocol` (`EWS`, `ActiveSync`).
pub fn autodiscover_v2_url(host: &str, address: &str, protocol: &str, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    format!("{scheme}://{host}/autodiscover/autodiscover.json/v1.0/{address}?Protocol={protocol}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_urls() {
        assert_eq!(CRATE, "nail-exchange");
        assert_eq!(
            autodiscover_v2_url("outlook.office365.com", "ada@contoso.example", "EWS", true),
            "https://outlook.office365.com/autodiscover/autodiscover.json/v1.0/ada@contoso.example?Protocol=EWS"
        );
    }
}
