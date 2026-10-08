//! iCalendar and vCard for Nail (plan M3a WP71; contract ADR 0058; design
//! ADR 0059).
//!
//! * [`content`] — the content-line layer: unfolding, parameters, lossless
//!   writing (unchanged properties keep their original text), charsets.
//! * [`time`] — [`nail_api::pim::PimTime`] ↔ chrono, iCalendar values,
//!   durations, DST gaps and folds.
//! * [`tz`] — `TZID`s → IANA zones (Windows / Outlook names, `VTIMEZONE`
//!   matching), `VTIMEZONE` generation.
//! * [`recur`] — `RRULE` text and Nail's recurrence engine (every `BY*`
//!   part, `BYSETPOS`, `BYWEEKNO` / `WKST`, `COUNT` / `UNTIL`, `RDATE`,
//!   `EXDATE`).
//! * [`ical`] — iCalendar → events (series + overrides) and tasks.
//! * [`write`](mod@write) — events and tasks → iCalendar, patching the original text.
//! * [`expand`] — series → [`nail_api::pim::EventInstance`]s in a range
//!   (overrides, `THISANDFUTURE`, floating and all-day placement).
//! * [`edit`] — edit scopes: overrides, exceptions, splitting a series.
//! * [`alarm`] — when alarms fire.
//! * [`vcard`] — vCard 2.1 / 3.0 / 4.0 ↔ contacts and groups.
//! * [`feed`] — ICS subscriptions; [`import`] — import previews.
//! * [`issues`] — problems with Fluent ids (`core-ical.ftl`).

#![warn(missing_docs)]

/// The `STANDARD` / `DAYLIGHT` rules of Central European Time as Windows
/// calendars write them (a test fixture shared by several modules; a
/// macro so `concat!` can splice it into a fixture literal).
#[cfg(test)]
macro_rules! cet_rules {
    () => {
        "BEGIN:STANDARD\r\n\
DTSTART:16011028T030000\r\n\
RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:16010325T020000\r\n\
RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\n\
TZOFFSETFROM:+0100\r\n\
TZOFFSETTO:+0200\r\n\
END:DAYLIGHT\r\n"
    };
}
#[cfg(test)]
pub(crate) use cet_rules;

pub mod alarm;
pub mod attendee;
pub mod content;
pub mod edit;
pub mod expand;
pub mod feed;
pub mod ical;
pub mod import;
pub mod issues;
pub mod recur;
pub mod time;
pub mod tz;
pub mod vcard;
pub mod write;

pub use ical::{ParseOptions, ParsedCalendar, ParsedEvent, ParsedTask, parse_calendar};
pub use issues::{Issue, IssueKind};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// The `PRODID` Nail writes into new calendars and cards.
pub const PRODID: &str = "-//Nail//Nail PIM//EN";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-ical");
    }
}
