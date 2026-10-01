//! Free/busy of the user's own calendars (M3b WP83; contract ADR 0065).
//!
//! **Slot (WP80).** WP83 adds the store side of free/busy here: busy
//! periods of a range from the instance cache (`pim_event_instances`:
//! `busy`, `cancelled`, the event's `STATUS` / the user's `PARTSTAT` for
//! tentative), across the calendars that count for an identity, merged
//! with `nail_api::scheduling::BusyPeriod::merge`, leaving out one event
//! (the one being edited). The core (WP85) answers
//! `Request::QueryFreeBusy` for the user's own addresses with it
//! (`FreeBusySource::Own`) and asks CalDAV schedule outboxes
//! (`nail-dav`, WP83) for everyone else.
