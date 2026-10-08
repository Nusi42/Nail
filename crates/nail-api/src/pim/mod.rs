//! Calendars, contacts and tasks (design.md, "PIM: calendar, contacts,
//! tasks"; M3a, ADR 0058).
//!
//! * [`source`]: where PIM data comes from — the built-in local source,
//!   read-only ICS / webcal subscriptions, CalDAV and CardDAV servers
//!   ([`PimSourceConfig`], configured in `accounts.kdl`), their runtime
//!   status and the PIM operation queue's status.
//! * [`calendar`]: calendars (task lists are calendars that hold tasks),
//!   events with recurrence rules, exceptions and overrides, attendees,
//!   alarms, time zones, conferencing links and attachments; expanded
//!   instances; free time.
//! * [`contact`]: address books, contacts (vCard 3.0 / 4.0 fields: several
//!   addresses, phones, photos, custom fields), groups, duplicates and
//!   merging.
//! * [`task`]: tasks (VTODO: due, priority, status, subtasks, recurrence).
//! * [`reminder`]: reminders (alarms that fired), snooze and dismiss.
//! * [`import`]: importing `.ics` / `.vcf` files.
//! * [`mcp`]: the read-only MCP resources and tools over PIM data.
//!
//! **Times** are [`PimTime`]s: an all-day date, a floating wall-clock
//! time, or a wall-clock time in an IANA zone (or `UTC`). Instants (when a
//! reminder fires, when something changed) are RFC 3339 strings in UTC.
//!
//! **Writes** go through the core's PIM operation queue like mail writes:
//! the local store changes at once, the change replays to the server, and
//! [`crate::Event::PimOpStatus`] reports its progress. Write requests
//! answer with the saved item and the queued operation ids.
//!
//! Parsing and writing iCalendar / vCard (lossless: unknown properties are
//! kept), recurrence expansion and time-zone resolution live in
//! `nail-ical`; this module is the wire model only.

pub mod calendar;
pub mod contact;
pub mod import;
pub mod mcp;
pub mod reminder;
pub mod source;
pub mod task;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{
    AddressBookId, CalendarId, ContactGroupId, ContactId, EventId, MessageId, TaskId,
};

pub use calendar::{
    Alarm, AlarmAction, AlarmTrigger, Attendee, Calendar, CalendarComponent, CalendarEvent,
    CalendarSettings, ConferenceLink, EditScope, EventFields, EventInstance, EventOverride,
    FreeSlot, Organizer, PimAttachment, Recurrence, RecurrenceEnd, RecurrenceRule,
};
pub use contact::{
    AddressBook, AddressBookSettings, Contact, ContactFields, ContactGroup, ContactMatch,
    ContactSummary, DuplicateGroup, LabeledValue, PostalAddress,
};
pub use import::{PimImportPreview, PimImportResult, PimImportSource};
pub use reminder::{Reminder, ReminderState, ReminderTarget};
pub use source::{
    PimItemSync, PimOpStatus, PimSource, PimSourceConfig, PimSourceKind, PimSourceSettings,
    PimSourceState,
};
pub use task::{Task, TaskFields, TaskStatus};

// ---------------------------------------------------------------------------
// Times

/// The zone name of UTC in [`PimTime::Zoned`].
pub const UTC_ZONE: &str = "UTC";

/// A date or time as iCalendar knows it.
///
/// JSON: `{"type": "date", "date": "2026-10-01"}`,
/// `{"type": "floating", "local": "2026-10-01T09:00:00"}`,
/// `{"type": "zoned", "local": "2026-10-01T09:00:00", "tz": "Europe/Berlin"}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PimTime {
    /// An all-day date, `YYYY-MM-DD` (iCalendar `VALUE=DATE`). As an end
    /// it is exclusive: a one-day event on the 1st ends on the 2nd.
    Date {
        /// `YYYY-MM-DD`.
        date: String,
    },
    /// A wall-clock time that is the same in every zone (iCalendar
    /// floating time), `YYYY-MM-DDTHH:MM:SS`.
    Floating {
        /// `YYYY-MM-DDTHH:MM:SS`.
        local: String,
    },
    /// A wall-clock time in a zone: an IANA name (`Europe/Berlin`) or
    /// [`UTC_ZONE`]. Windows / Outlook zone names are mapped to IANA when
    /// parsed (`nail-ical`); the original `VTIMEZONE` is kept with the item.
    Zoned {
        /// `YYYY-MM-DDTHH:MM:SS` in `tz`.
        local: String,
        /// IANA zone name or `UTC`.
        tz: String,
    },
}

impl PimTime {
    /// An all-day date.
    pub fn date(date: impl Into<String>) -> Self {
        Self::Date { date: date.into() }
    }

    /// A floating time.
    pub fn floating(local: impl Into<String>) -> Self {
        Self::Floating {
            local: local.into(),
        }
    }

    /// A time in a zone.
    pub fn zoned(local: impl Into<String>, tz: impl Into<String>) -> Self {
        Self::Zoned {
            local: local.into(),
            tz: tz.into(),
        }
    }

    /// A UTC time.
    pub fn utc(local: impl Into<String>) -> Self {
        Self::zoned(local, UTC_ZONE)
    }

    /// Whether this is an all-day date.
    pub fn is_date(&self) -> bool {
        matches!(self, Self::Date { .. })
    }

    /// The calendar date part, `YYYY-MM-DD` (as written; empty when
    /// malformed).
    pub fn date_part(&self) -> &str {
        let text = match self {
            Self::Date { date } => date.as_str(),
            Self::Floating { local } | Self::Zoned { local, .. } => local.as_str(),
        };
        text.get(..10).unwrap_or("")
    }

    /// The zone, `None` for dates and floating times.
    pub fn tz(&self) -> Option<&str> {
        match self {
            Self::Zoned { tz, .. } => Some(tz),
            _ => None,
        }
    }

    /// A key that orders times of the **same kind and zone** correctly
    /// (dates sort before times of the same day). Comparing times in
    /// different zones needs zone data (`nail-ical`).
    pub fn sort_key(&self) -> &str {
        match self {
            Self::Date { date } => date,
            Self::Floating { local } | Self::Zoned { local, .. } => local,
        }
    }

    /// What is wrong with the value, if anything: a malformed date, time
    /// or zone name. Whether the zone exists is checked by the core.
    pub fn issue(&self) -> Option<PimIssue> {
        match self {
            Self::Date { date } => (!is_date(date)).then_some(PimIssue::InvalidDate),
            Self::Floating { local } => {
                (!is_local_date_time(local)).then_some(PimIssue::InvalidTime)
            }
            Self::Zoned { local, tz } => {
                if !is_local_date_time(local) {
                    Some(PimIssue::InvalidTime)
                } else if !is_time_zone_name(tz) {
                    Some(PimIssue::InvalidTimeZone)
                } else {
                    None
                }
            }
        }
    }
}

fn digits(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Whether `text` is a valid calendar date `YYYY-MM-DD` (years 1–9999).
pub fn is_date(text: &str) -> bool {
    nail_util::time::parse_iso_date(text).is_some_and(|(y, _, _)| y >= 1)
}

/// Whether `text` is a valid wall-clock date and time
/// `YYYY-MM-DDTHH:MM:SS` (second 60 allowed for leap seconds).
pub fn is_local_date_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 19 || bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
        return false;
    }
    let (Some(h), Some(m), Some(s)) = (
        digits(&text[11..13]),
        digits(&text[14..16]),
        digits(&text[17..]),
    ) else {
        return false;
    };
    is_date(&text[..10]) && h < 24 && m < 60 && s <= 60
}

/// Whether `text` looks like an IANA zone name (`Europe/Berlin`,
/// `America/Argentina/Buenos_Aires`, `Etc/GMT+1`) or `UTC`: 1–64
/// characters, segments of letters, digits, `_`, `-`, `+` separated by
/// single `/`. Whether the zone exists is checked with zone data.
pub fn is_time_zone_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
        })
}

/// Whether `text` is an RFC 3339 instant in UTC as the contract writes
/// them: `YYYY-MM-DDTHH:MM:SS` followed by `Z` (fractions allowed).
pub fn is_utc_instant(text: &str) -> bool {
    let Some(rest) = text.strip_suffix('Z') else {
        return false;
    };
    let (main, fraction) = match rest.split_once('.') {
        Some((main, fraction)) => (main, Some(fraction)),
        None => (rest, None),
    };
    is_local_date_time(main)
        && fraction.is_none_or(|f| !f.is_empty() && f.len() <= 9 && digits(f).is_some())
}

/// Whether `text` is a colour Nail accepts for calendars and events:
/// `#RGB`, `#RRGGBB` or `#RRGGBBAA` (hex), or a CSS colour name (letters
/// only, RFC 7986 `COLOR`).
pub fn is_color(text: &str) -> bool {
    match text.strip_prefix('#') {
        Some(hex) => matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => {
            !text.is_empty() && text.len() <= 32 && text.chars().all(|c| c.is_ascii_alphabetic())
        }
    }
}

/// Whether `text` is a plausible email address (`local@domain`, no
/// whitespace); used for attendees, organizers and contact addresses.
pub fn is_plausible_email(text: &str) -> bool {
    let Some((local, domain)) = text.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether `text` is an `https:` URL, or an `http:` URL of a loopback
/// host (`localhost`, `127.0.0.1`, `[::1]`; local test servers) — the rule
/// for server URLs in `accounts.kdl`.
pub fn is_https_or_loopback_url(text: &str) -> bool {
    if has_url_scheme(text, &["https"]) {
        return true;
    }
    if !has_url_scheme(text, &["http"]) {
        return false;
    }
    let rest = &text["http://".len()..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    crate::hosts::is_loopback_host(crate::hosts::authority_host(authority))
}

/// Whether `text` is an absolute URL with one of `schemes` (`https`,
/// `webcal`, …): `<scheme>://<something>` without whitespace.
pub fn has_url_scheme(text: &str, schemes: &[&str]) -> bool {
    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };
    schemes.iter().any(|s| scheme.eq_ignore_ascii_case(s))
        && !rest.is_empty()
        && !rest.starts_with('/')
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ---------------------------------------------------------------------------
// Validation issues

/// A problem with PIM data a client sent (create / update requests). The
/// core answers such requests with `invalid_request` and the issue's
/// Fluent id as `localized`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimIssue {
    /// A date is not `YYYY-MM-DD`.
    InvalidDate,
    /// A date-time is not `YYYY-MM-DDTHH:MM:SS`, or an instant not RFC 3339 UTC.
    InvalidTime,
    /// A zone name is malformed or unknown.
    InvalidTimeZone,
    /// The end is before the start.
    EndBeforeStart,
    /// Start and end mix an all-day date with a time.
    MixedAllDay,
    /// A recurrence rule is invalid (interval, `BY…` values, until).
    InvalidRecurrence,
    /// An override's recurrence id or `this` / `this-and-following`
    /// without a recurrence id, or on an event that does not recur.
    InvalidScope,
    /// An email address (attendee, organizer, contact) is malformed.
    InvalidEmail,
    /// A contact or group without any name, or a calendar / address book
    /// without a name.
    MissingName,
    /// Priority outside 0–9 or completion outside 0–100.
    InvalidPriority,
    /// A colour is not `#RGB`, `#RRGGBB`, `#RRGGBBAA` or a CSS name.
    InvalidColor,
    /// An alarm is invalid (absolute trigger not in UTC, repeat without
    /// interval).
    InvalidAlarm,
    /// A URL is malformed.
    InvalidUrl,
    /// A task is its own parent (directly or through its subtasks).
    TaskCycle,
    /// Too many items in one list (attendees, alarms, addresses, …).
    TooMany,
}

impl PimIssue {
    /// All issues.
    pub const ALL: [PimIssue; 15] = [
        Self::InvalidDate,
        Self::InvalidTime,
        Self::InvalidTimeZone,
        Self::EndBeforeStart,
        Self::MixedAllDay,
        Self::InvalidRecurrence,
        Self::InvalidScope,
        Self::InvalidEmail,
        Self::MissingName,
        Self::InvalidPriority,
        Self::InvalidColor,
        Self::InvalidAlarm,
        Self::InvalidUrl,
        Self::TaskCycle,
        Self::TooMany,
    ];

    /// Fluent message id, e.g. `pim-issue-invalid-date`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::InvalidDate => "pim-issue-invalid-date",
            Self::InvalidTime => "pim-issue-invalid-time",
            Self::InvalidTimeZone => "pim-issue-invalid-time-zone",
            Self::EndBeforeStart => "pim-issue-end-before-start",
            Self::MixedAllDay => "pim-issue-mixed-all-day",
            Self::InvalidRecurrence => "pim-issue-invalid-recurrence",
            Self::InvalidScope => "pim-issue-invalid-scope",
            Self::InvalidEmail => "pim-issue-invalid-email",
            Self::MissingName => "pim-issue-missing-name",
            Self::InvalidPriority => "pim-issue-invalid-priority",
            Self::InvalidColor => "pim-issue-invalid-color",
            Self::InvalidAlarm => "pim-issue-invalid-alarm",
            Self::InvalidUrl => "pim-issue-invalid-url",
            Self::TaskCycle => "pim-issue-task-cycle",
            Self::TooMany => "pim-issue-too-many",
        }
    }
}

/// Most entries of one list in an item (attendees, alarms, addresses,
/// categories, …); more are refused with [`PimIssue::TooMany`].
pub const MAX_LIST_ENTRIES: usize = 1000;

/// Pushes `issue` once.
pub(crate) fn push_issue(out: &mut Vec<PimIssue>, issue: PimIssue) {
    if !out.contains(&issue) {
        out.push(issue);
    }
}

// ---------------------------------------------------------------------------
// Links and references

/// A link from a PIM item to something else in Nail. Stored in the item
/// itself (iCalendar `ATTACH` / vCard `X-` property with a `mid:` URL,
/// RFC 2392) so it survives a sync round trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PimLink {
    /// The message an event or task was created from.
    Message {
        /// The message in the local store, when the core found it (a link
        /// synced from another device has only the `Message-ID`).
        message_id: Option<MessageId>,
        /// The message's `Message-ID` header (without angle brackets).
        rfc822_id: Option<String>,
        /// Its subject when the link was made.
        subject: Option<String>,
    },
}

/// What kind of PIM item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimItemKind {
    /// A calendar event.
    Event,
    /// A task.
    Task,
    /// A contact.
    Contact,
    /// A contact group.
    ContactGroup,
}

/// A reference to one PIM item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PimItemRef {
    /// An event (the whole series).
    Event {
        /// The event.
        event_id: EventId,
    },
    /// A task.
    Task {
        /// The task.
        task_id: TaskId,
    },
    /// A contact.
    Contact {
        /// The contact.
        contact_id: ContactId,
    },
    /// A contact group.
    ContactGroup {
        /// The group.
        group_id: ContactGroupId,
    },
}

impl PimItemRef {
    /// The reference of an item of `kind` with `id`.
    pub fn new(kind: PimItemKind, id: impl Into<String>) -> Self {
        let id = id.into();
        match kind {
            PimItemKind::Event => Self::Event {
                event_id: id.into(),
            },
            PimItemKind::Task => Self::Task { task_id: id.into() },
            PimItemKind::Contact => Self::Contact {
                contact_id: id.into(),
            },
            PimItemKind::ContactGroup => Self::ContactGroup {
                group_id: id.into(),
            },
        }
    }

    /// The item's id.
    pub fn id(&self) -> &str {
        match self {
            Self::Event { event_id } => event_id.as_str(),
            Self::Task { task_id } => task_id.as_str(),
            Self::Contact { contact_id } => contact_id.as_str(),
            Self::ContactGroup { group_id } => group_id.as_str(),
        }
    }

    /// The item's kind.
    pub fn kind(&self) -> PimItemKind {
        match self {
            Self::Event { .. } => PimItemKind::Event,
            Self::Task { .. } => PimItemKind::Task,
            Self::Contact { .. } => PimItemKind::Contact,
            Self::ContactGroup { .. } => PimItemKind::ContactGroup,
        }
    }
}

/// Which copy wins when resolving a sync conflict
/// ([`crate::Request::ResolvePimConflict`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictChoice {
    /// Keep the server's version (already shown) and drop the local copy.
    Server,
    /// Write the kept local copy back over the server's version.
    Local,
}

/// One hit of [`crate::Request::SearchPim`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimSearchHit {
    /// The item.
    pub item: PimItemRef,
    /// Its title (event / task summary, contact display name).
    pub title: String,
    /// A second line (location, due date, email address), if any.
    pub detail: Option<String>,
    /// The calendar of an event or task.
    pub calendar_id: Option<CalendarId>,
    /// The address book of a contact or group.
    pub address_book_id: Option<AddressBookId>,
    /// Start of an event (its first occurrence) or due date of a task.
    pub start: Option<PimTime>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn times_serialize_tagged() {
        assert_eq!(
            serde_json::to_value(PimTime::date("2026-10-01")).unwrap(),
            json!({"type": "date", "date": "2026-10-01"})
        );
        assert_eq!(
            serde_json::to_value(PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin")).unwrap(),
            json!({"type": "zoned", "local": "2026-10-01T09:00:00", "tz": "Europe/Berlin"})
        );
        let floating: PimTime =
            serde_json::from_value(json!({"type": "floating", "local": "2026-10-01T09:00:00"}))
                .unwrap();
        assert_eq!(floating, PimTime::floating("2026-10-01T09:00:00"));
        assert_eq!(floating.date_part(), "2026-10-01");
        assert!(PimTime::date("2026-10-01").is_date());
        assert_eq!(PimTime::utc("2026-10-01T09:00:00").tz(), Some("UTC"));
    }

    #[test]
    fn date_and_time_validation() {
        for ok in ["2026-10-01", "2024-02-29", "0001-01-01", "9999-12-31"] {
            assert!(is_date(ok), "{ok}");
        }
        for bad in [
            "2025-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-1-01",
            "20261001",
            "0000-01-01",
            "2100-02-29",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
        assert!(is_local_date_time("2026-10-01T23:59:60"));
        for bad in [
            "2026-10-01T24:00:00",
            "2026-10-01 09:00:00",
            "2026-10-01T09:00",
            "2026-10-01T09:00:00Z",
        ] {
            assert!(!is_local_date_time(bad), "{bad}");
        }
        for ok in [
            "UTC",
            "Europe/Berlin",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+1",
        ] {
            assert!(is_time_zone_name(ok), "{ok}");
        }
        for bad in [
            "",
            "Europe//Berlin",
            "/UTC",
            "W. Europe Standard Time",
            &"A".repeat(65),
        ] {
            assert!(!is_time_zone_name(bad), "{bad}");
        }
        assert!(is_utc_instant("2026-10-01T09:00:00Z"));
        assert!(is_utc_instant("2026-10-01T09:00:00.123Z"));
        for bad in [
            "2026-10-01T09:00:00",
            "2026-10-01T09:00:00+02:00",
            "2026-10-01T09:00:00.Z",
        ] {
            assert!(!is_utc_instant(bad), "{bad}");
        }
        assert_eq!(
            PimTime::date("2026-02-30").issue(),
            Some(PimIssue::InvalidDate)
        );
        assert_eq!(
            PimTime::floating("2026-02-01").issue(),
            Some(PimIssue::InvalidTime)
        );
        assert_eq!(
            PimTime::zoned("2026-02-01T10:00:00", "Not a zone").issue(),
            Some(PimIssue::InvalidTimeZone)
        );
        assert_eq!(PimTime::utc("2026-02-01T10:00:00").issue(), None);
    }

    #[test]
    fn colors_emails_urls() {
        for ok in ["#fff", "#3b82f6", "#3B82F6CC", "teal"] {
            assert!(is_color(ok), "{ok}");
        }
        for bad in ["", "#ff", "#ggg", "rgb(1,2,3)", "light blue"] {
            assert!(!is_color(bad), "{bad}");
        }
        assert!(is_plausible_email("ada@example.org"));
        for bad in [
            "ada",
            "@example.org",
            "ada@",
            "a da@example.org",
            "ada@.org",
        ] {
            assert!(!is_plausible_email(bad), "{bad}");
        }
        assert!(has_url_scheme(
            "webcal://example.org/h.ics",
            &["webcal", "https"]
        ));
        assert!(has_url_scheme("HTTPS://example.org", &["https"]));
        assert!(!has_url_scheme("ftp://example.org", &["https"]));
        assert!(!has_url_scheme("https:///path", &["https"]));
        assert!(!has_url_scheme("https://exa mple.org", &["https"]));
        assert!(is_https_or_loopback_url("https://dav.example.org/"));
        assert!(is_https_or_loopback_url("http://127.0.0.1:5232/"));
        assert!(is_https_or_loopback_url("http://localhost/dav"));
        assert!(!is_https_or_loopback_url("http://dav.example.org/"));
        assert!(!is_https_or_loopback_url("http://127.0.0.1.evil.example/"));
        assert!(is_https_or_loopback_url("http://[::1]:8080/token"));
        assert!(!is_https_or_loopback_url("ftp://example.org"));
        assert!(!is_https_or_loopback_url("https://"));
    }

    #[test]
    fn issues_have_unique_message_ids() {
        let ids: std::collections::HashSet<_> =
            PimIssue::ALL.iter().map(|i| i.message_id()).collect();
        assert_eq!(ids.len(), PimIssue::ALL.len());
        for issue in PimIssue::ALL {
            assert!(crate::localized::is_message_id(issue.message_id()));
            let wire = serde_json::to_value(issue).unwrap();
            assert_eq!(
                format!("pim-issue-{}", wire.as_str().unwrap()),
                issue.message_id()
            );
        }
    }

    #[test]
    fn item_refs_and_links() {
        let item = PimItemRef::Event {
            event_id: "ev1".into(),
        };
        assert_eq!(item.kind(), PimItemKind::Event);
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            json!({"type": "event", "event_id": "ev1"})
        );
        let link = PimLink::Message {
            message_id: Some("m1".into()),
            rfc822_id: Some("abc@example.org".into()),
            subject: None,
        };
        let value = serde_json::to_value(&link).unwrap();
        assert_eq!(value["type"], "message");
        assert_eq!(serde_json::from_value::<PimLink>(value).unwrap(), link);
        assert_eq!(
            serde_json::to_value(ConflictChoice::Local).unwrap(),
            json!("local")
        );
    }
}
