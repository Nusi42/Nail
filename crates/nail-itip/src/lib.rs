//! iTIP (RFC 5546) and iMIP (RFC 6047) for Nail (plan M3b WP81; contract
//! ADR 0065).
//!
//! **Stub (WP80).** WP81 fills this crate; the contract it builds against
//! is `nail_api::scheduling` (methods, the invitation card, targets,
//! results, issues with `itip-issue-*` ids in `core-scheduling.ftl`) and
//! the store's scheduling state (`nail_store::scheduling`: `SEQUENCE` /
//! `DTSTAMP` / answer per event and address, received and sent iTIP
//! messages). Expected shape (pure logic, no I/O; the core wires it):
//!
//! * `imip` — find the iTIP part of a received message
//!   (`text/calendar; method=…`, also as an `.ics` attachment; the
//!   `METHOD` property wins over the parameter) and build outgoing iMIP
//!   mail: `multipart/mixed` with a readable text part (Fluent texts from
//!   `core-scheduling.ftl`), the `text/calendar; method=…; charset=utf-8`
//!   part and, for Outlook, an `invite.ics` attachment.
//! * `process` — classify an iTIP message against the user's identities
//!   and the stored event: role (organizer / attendee / observer), state
//!   (new / current / update / outdated / cancelled / unknown event),
//!   issues (sender is not the organizer / attendee, …), the card's
//!   actions; apply `REQUEST` / `ADD` / `CANCEL` / `REPLY` /
//!   `DECLINECOUNTER` to the stored event (patching its raw text through
//!   `nail-ical`), per occurrence (`RECURRENCE-ID`).
//! * `respond` — attendee side: `REPLY` with `PARTSTAT` and `COMMENT`
//!   (only the replying attendee, RFC 5546 §3.2.3), `COUNTER`,
//!   `REFRESH`, forwarding a `REQUEST` (§2.1.5); organizer side:
//!   `REQUEST` for new / updated events with `SEQUENCE` rules (significant
//!   changes bump it), `CANCEL` (meeting, occurrence, removed attendees),
//!   `DECLINECOUNTER`, answering a `REFRESH`.
//! * `conflicts` — overlapping busy instances of the user's calendars
//!   for an invited time (input: instances from the store's cache).
//! * `server_scheduling` — RFC 6638: whether a CalDAV source schedules
//!   itself (`calendar-auto-schedule` in the DAV capabilities, no
//!   `SCHEDULE-AGENT=CLIENT`), so the core does not mail duplicates.
//!
//! Also owned by WP81: `nail-ical` (iTIP needs, e.g. `METHOD`,
//! `SCHEDULE-AGENT` / `SCHEDULE-STATUS` parameters) and
//! `locales/{en,de}/core-scheduling.ftl`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The MIME type of iTIP data (RFC 6047 §2.4).
pub const ITIP_MIME_TYPE: &str = "text/calendar";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-itip");
        assert_eq!(super::ITIP_MIME_TYPE, "text/calendar");
    }
}
