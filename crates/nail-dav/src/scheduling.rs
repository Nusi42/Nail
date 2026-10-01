//! CalDAV scheduling (RFC 6638) and free/busy (M3b WP83; contract
//! ADR 0065).
//!
//! **Slot (WP80).** WP83 fills this module (and may split it):
//! discovery of `schedule-outbox-URL` / `schedule-inbox-URL` and the
//! `calendar-auto-schedule` capability (stored with the source's
//! discovery so the core can tell `SchedulingTransport::Server` from
//! iMIP), the outbox `POST` of a `VFREEBUSY` request with the
//! `schedule-response` parsed into `nail_api::scheduling::AttendeeFreeBusy`
//! (VFREEBUSY building / parsing lives here, not in `nail-ical`), an
//! implementation of `nail_api::engine::FreeBusyProvider`, processing of
//! the schedule inbox (iTIP messages the server delivered), and room /
//! resource placeholders (`CUTYPE=ROOM` principals; filled for Exchange
//! in M3c). The mock server side is `nail-testkit-net`'s `MockDavServer`
//! (also WP83): outbox, inbox, free/busy of a second demo user.
