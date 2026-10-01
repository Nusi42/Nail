//! CalDAV and CardDAV client (plan M3a WP72; contract ADR 0058).
//!
//! **Stub (WP70).** WP72 fills this crate; the contract it builds against
//! is `nail_api::pim::source` (source settings, collections found, sync
//! states, PIM operation status). Expected shape:
//!
//! * `discover` — RFC 6764: `_caldavs._tcp` / `_carddavs._tcp` SRV
//!   records through the OS resolver (`nail_discovery::DnsLookup`; no TXT
//!   lookups in M3a), `/.well-known/caldav` / `carddav`, then
//!   `current-user-principal`, `calendar-home-set` /
//!   `addressbook-home-set`, collections with display name, colour,
//!   supported components, privileges (read-only), `getctag`, sync token.
//! * `propfind` / `report` — `calendar-query`, `calendar-multiget`,
//!   `addressbook-query`, `addressbook-multiget`; multistatus parsing
//!   (`quick-xml`, namespaces).
//! * `sync` — `sync-collection` (RFC 6578) with fallback to ctag / ETag
//!   comparison; changed hrefs → multiget in batches.
//! * `write` — `PUT` with `If-Match` / `If-None-Match: *`, `DELETE` with
//!   `If-Match`, `MKCALENDAR` / extended `MKCOL`, `PROPPATCH`
//!   (display name, colour); `412` → conflict (server wins, the core keeps
//!   the local copy as a conflict copy).
//! * `auth` — Basic over TLS (or loopback http for tests), OAuth2 bearer
//!   (Google) through `nail_api::engine::AccessTokenProvider`.
//! * Errors map to `nail_api::ErrorCode` with `LocalizedMessage`s (`dav-*`
//!   ids in `locales/en/core-dav.ftl`).
//!
//! The mock CalDAV / CardDAV server lives in `nail-testkit-net`
//! (`dav.rs`, also WP72), Radicale interop tests in `scripts/interop`.
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

/// The WebDAV namespace.
pub const NS_DAV: &str = "DAV:";
/// The CalDAV namespace (RFC 4791).
pub const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
/// The CardDAV namespace (RFC 6352).
pub const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
/// The CalendarServer namespace (`getctag`).
pub const NS_CALENDARSERVER: &str = "http://calendarserver.org/ns/";

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-dav");
        assert!(super::NS_CALDAV.ends_with("caldav"));
    }
}
