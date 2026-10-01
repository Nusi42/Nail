//! The Exchange common layer (plan M3c WP93; contract ADR 0072).
//!
//! **Stub (WP90).** WP93 fills this crate. It sits between the two
//! protocol engines (`nail-proto-graph`, `nail-proto-ews`) and the core:
//!
//! * `autodiscover` — account setup with engine choice: Autodiscover v2
//!   (`/autodiscover/autodiscover.json/v1.0/<address>?Protocol=EWS|ActiveSync`)
//!   and POX (`autodiscover.<domain>`, `<domain>/autodiscover`, SRV
//!   `_autodiscover._tcp` through `nail_discovery::SystemDns`, redirects
//!   checked like `nail-discovery` does), with Windows sign-in or a
//!   password where the server wants one; answers ranked
//!   `nail_api::accounts::DiscoveredAccount`s
//!   (`DiscoverySource::Autodiscover`): Graph for Exchange Online, EWS
//!   (with the found URL) for on-premises, IMAP + SMTP as a fallback when
//!   the server lists them; automapped shared mailboxes
//!   (`AlternativeMailbox`) as `nail_api::exchange::ExchangeMailboxSettings`.
//! * [`sspi`] — `SspiNegotiate`: [`nail_api::exchange::engine::NegotiateAuth`]
//!   over SSPI (`AcquireCredentialsHandleW` with the signed-in user or
//!   explicit credentials, `InitializeSecurityContextW` per round,
//!   Negotiate / Kerberos / NTLM per `nail_api::accounts::SspiPackage`);
//!   `unsupported` off Windows.
//! * `mapping` — Exchange ↔ Nail: categories ↔ tags (colours from the
//!   master list), Focused Inbox for search / rules, Nail rules ↔
//!   [`nail_api::exchange::rules::ExchangeInboxRule`] where expressible
//!   (with `exchange-rule-reason-*` texts for the rest, like
//!   `nail-sieve`'s placement), automatic replies ↔
//!   `nail_api::sieve::VacationSettings`, the item shapes of
//!   [`nail_api::exchange::pim`] ↔ Nail's events / tasks / contacts
//!   (Windows zones → IANA through `nail_ical::tz`, recurrence patterns ↔
//!   `RRULE`, exceptions ↔ overrides, To Do checklists ↔ subtasks), rooms
//!   for the free/busy picker; texts in `locales/en/core-exchange.ftl`.
//! * `testing` (feature) — `MockAutodiscoverServer` on 127.0.0.1 (v2 JSON
//!   and POX answers, redirects, logins).
//!
//! Migration notes (EWS retirement in Exchange Online) are in ADR 0072 and
//! `userDocs/en/accounts-reference.md`.

#![warn(missing_docs)]

pub mod sspi;
#[cfg(feature = "testing")]
pub mod testing;

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
