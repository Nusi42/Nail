//! iCalendar and vCard for Nail (plan M3a WP71; contract ADR 0058).
//!
//! **Stub (WP70).** WP71 fills this crate; the contract it builds against
//! is `nail_api::pim` (events, recurrence, alarms, contacts, tasks,
//! import previews). Expected shape:
//!
//! * `ical` — parse iCalendar (RFC 5545, RFC 7986) into
//!   `nail_api::pim::CalendarEvent` / `Task` (series master + overrides by
//!   `RECURRENCE-ID`) and write them back **losslessly**: the original
//!   component is kept and only changed properties are rewritten, unknown
//!   properties and parameters survive (`X-…`, `VTIMEZONE`, Apple / Google
//!   / Outlook extensions); `PimLink`s as `ATTACH:mid:` (RFC 2392);
//!   conferencing links (RFC 7986 `CONFERENCE` and the common vendor
//!   properties).
//! * `vcard` — vCard 3.0 / 4.0 ↔ `nail_api::pim::ContactFields` /
//!   `ContactGroup` (vCard 4 `KIND:group` + `MEMBER`, Apple
//!   `X-ADDRESSBOOKSERVER-*` groups), kept in the card's version; photos
//!   (inline data → bytes for the blob store, `URI` kept as URL).
//! * `recur` — expansion of a series into `EventInstance`s in a range
//!   (RRULE, RDATE, EXDATE, overrides; `COUNT` / `UNTIL`; floating and
//!   all-day events placed in a given zone), and "this and following" /
//!   "this" edits (split with `UNTIL`, overrides, `EXDATE`).
//! * `tz` — `TZID`s → IANA (`chrono-tz`), Windows / Outlook zone names,
//!   custom `VTIMEZONE` definitions; DST gaps and folds.
//! * `alarm` — when each `VALARM` fires (relative to start / end / due,
//!   absolute, `REPEAT`), for the reminder scheduler.
//! * `feed` — reading ICS subscription feeds (`X-WR-CALNAME`,
//!   `REFRESH-INTERVAL`, `COLOR`).
//! * import previews with `PimImportIssue`s (`ical-*` / `vcard-*` ids in
//!   `locales/en/core-ical.ftl`).
//!
//! Library: `calcard` (Apache-2.0 OR MIT, used by Stalwart): parser,
//! writer, RRULE expansion and zone detection. A real-world corpus
//! (Google, Outlook, Apple, Thunderbird exports) and property tests come
//! with WP71.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-ical");
    }
}
