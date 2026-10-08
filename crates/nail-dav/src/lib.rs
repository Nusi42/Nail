//! CalDAV (RFC 4791) and CardDAV (RFC 6352) client (plan M3a WP72;
//! contract ADR 0058; design ADR 0060), with CalDAV scheduling (RFC 6638)
//! and free/busy (M3b WP83; contract ADR 0065).
//!
//! * [`discovery`] — RFC 6764: the configured URL, `_caldavs._tcp` /
//!   `_carddavs._tcp` SRV records through the OS resolver
//!   ([`nail_discovery::DnsLookup`]; no TXT lookups, no DNS sockets),
//!   `/.well-known/caldav|carddav`, `current-user-principal`,
//!   `calendar-home-set` / `addressbook-home-set`, collections with
//!   display name, colour, components, privileges, `getctag`, sync token.
//! * [`DavClient`] — PROPFIND, REPORT (`calendar-query` with
//!   `time-range`, `calendar-multiget`, `addressbook-query`,
//!   `addressbook-multiget`, `sync-collection`), GET, PUT / DELETE with
//!   `If-Match` / `If-None-Match: *`, MKCALENDAR, extended MKCOL,
//!   PROPPATCH; redirects followed under the URL policy (`https:`, or
//!   `http:` to loopback only); Basic over TLS or OAuth2 bearer
//!   ([`DavAuth`]), with one token refresh on `401`.
//! * [`sync`] — `sync-collection` with paging and invalid-token restarts,
//!   else ctag + ETag comparison; changed members via multiget.
//! * [`write`](mod@write) — conditional writes; `412` → the server's version (server
//!   wins), the core keeps the local copy as a conflict copy.
//! * [`scheduling`] — CalDAV scheduling (RFC 6638, M3b WP83): schedule
//!   inbox / outbox and calendar-user addresses of the principal,
//!   `calendar-auto-schedule`, free/busy `POST`s to the outbox
//!   ([`CalDavFreeBusy`] implements the contract's `FreeBusyProvider`),
//!   reading / processing / deleting schedule-inbox items,
//!   `SCHEDULE-AGENT` / `SCHEDULE-STATUS`.
//! * [`freebusy`] — `VFREEBUSY` building and parsing, and the busy
//!   periods of the user's own calendars from expanded instances
//!   ([`own_busy_periods`]).
//! * [`icalendar`] — the small content-line layer both use (`nail-ical`
//!   stays the full iCalendar model).
//! * XML through `nail_net::xml` ([`xml`]); errors map to
//!   [`nail_api::ErrorCode`]s and `dav-*` Fluent texts ([`DavError`]).
//!
//! The mock CalDAV / CardDAV server lives in `nail-testkit-net`
//! (`MockDavServer`), Radicale interop tests in `scripts/interop`.
//! Every socket the tests open binds 127.0.0.1 (ADR 0057).

#![warn(missing_docs)]

pub mod client;
pub mod discovery;
pub mod error;
pub mod freebusy;
pub mod icalendar;
pub mod multistatus;
pub mod ops;
pub mod scheduling;
pub mod sync;
pub mod write;
pub mod xml;

pub use client::{DavAuth, DavClient, Depth, Reply, is_allowed_url, parse_server_url};
pub use discovery::{
    DavService, Discovery, DiscoveryInput, GOOGLE_CALDAV_URL, GOOGLE_CARDDAV_URL, discover,
    list_collections,
};
pub use error::DavError;
pub use freebusy::{OwnFreeBusy, VFreeBusy, own_busy_periods};
pub use ops::{
    CollectionKind, DavCollection, DavObject, Multiget, NewCollection, Precondition, SyncReport,
    TextMatch, TimeRange, href_key, member_url,
};
pub use scheduling::{
    AttendeeScheduleStatus, CalDavFreeBusy, InboxDecision, InboxItem, InboxReport,
    RecipientOutcome, RecipientResponse, RequestStatus, ScheduleAgent, ScheduleDelivery,
    SchedulingInfo, delete_inbox_item, discover_scheduling, list_inbox, process_inbox,
    query_free_busy, schedule_statuses, server_schedules, set_schedule_agent,
};
pub use sync::{CollectionState, SyncMethod, SyncOptions, SyncOutcome, sync_collection};
pub use write::{DeleteOutcome, ServerVersion, WriteOutcome, delete_object, write_object};

/// The XML namespaces (one definition in `nail_net::xml::ns`, shared with
/// the mock DAV server).
pub use nail_net::xml::ns::{
    APPLE_ICAL as NS_APPLE_ICAL, CALDAV as NS_CALDAV, CALENDARSERVER as NS_CALENDARSERVER,
    CARDDAV as NS_CARDDAV, DAV as NS_DAV,
};

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
