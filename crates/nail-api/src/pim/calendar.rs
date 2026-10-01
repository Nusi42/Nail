//! Calendars and events (iCalendar RFC 5545 `VEVENT`, RFC 7986
//! `CONFERENCE` / `COLOR`, RFC 9074 alarm acknowledgement).
//!
//! An event is stored as a **series**: [`CalendarEvent`] holds the master's
//! fields, its [`Recurrence`] (RRULE, RDATE, EXDATE) and the
//! [`EventOverride`]s (instances with a `RECURRENCE-ID` whose fields
//! differ). Views ask for **instances** in a range
//! ([`crate::Request::ListEventInstances`]), which the core expands and
//! caches. Edits of a recurring event name an [`EditScope`]:
//!
//! * `this` — one occurrence: writes an override (or, on delete, an
//!   `EXDATE`);
//! * `this-and-following` — the series is split: the original ends before
//!   the occurrence (`UNTIL`), a new series (new `UID`, `RELATED-TO` the
//!   old one) starts with it and carries the change;
//! * `all` — the whole series (moving the start shifts every occurrence
//!   and the overrides' recurrence ids follow).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::source::PimItemSync;
use super::{
    MAX_LIST_ENTRIES, PimIssue, PimLink, PimTime, has_url_scheme, is_color, is_plausible_email,
    is_utc_instant, push_issue,
};
use crate::ids::{AccountId, CalendarId, EventId, PimSourceId};
use crate::triage::Weekday;

// ---------------------------------------------------------------------------
// Calendars

/// What a calendar holds (CalDAV `supported-calendar-component-set`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarComponent {
    /// Events (`VEVENT`).
    Event,
    /// Tasks (`VTODO`): the calendar is (also) a task list.
    Task,
}

/// A calendar (or task list) of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Calendar {
    /// The calendar.
    pub id: CalendarId,
    /// Its source.
    pub source_id: PimSourceId,
    /// The mail account the source belongs to, if any (virtual accounts
    /// overlay their members' calendars).
    pub account_id: Option<AccountId>,
    /// The user's settings.
    pub settings: CalendarSettings,
    /// What it holds.
    pub components: Vec<CalendarComponent>,
    /// Read-only (ICS subscriptions, shared calendars without write
    /// access). Writes answer `forbidden`.
    pub read_only: bool,
    /// The calendar's own zone (CalDAV `calendar-timezone`), `null` = none.
    pub timezone: Option<String>,
}

impl Calendar {
    /// Whether it holds events.
    pub fn has_events(&self) -> bool {
        self.components.contains(&CalendarComponent::Event)
    }

    /// Whether it is a task list.
    pub fn has_tasks(&self) -> bool {
        self.components.contains(&CalendarComponent::Task)
    }
}

/// The user-editable settings of a calendar. Name and colour are written
/// to the server where it supports them (CalDAV `displayname`,
/// `calendar-color`); visibility, order and default alarms stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CalendarSettings {
    /// Display name.
    pub name: String,
    /// Description, if any.
    #[serde(default)]
    pub description: Option<String>,
    /// Colour (see [`super::is_color`]), `null` = the theme's default.
    #[serde(default)]
    pub color: Option<String>,
    /// Shown in calendar views (hidden calendars still sync and remind).
    #[serde(default = "yes")]
    pub visible: bool,
    /// Reminders fire for this calendar's events and tasks.
    #[serde(default = "yes")]
    pub reminders: bool,
    /// Alarms new events get by default.
    #[serde(default)]
    pub default_alarms: Vec<Alarm>,
    /// Position in calendar lists (ascending).
    #[serde(default)]
    pub order: i32,
}

fn yes() -> bool {
    true
}

impl CalendarSettings {
    /// Settings with a name and defaults for the rest.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            color: None,
            visible: true,
            reminders: true,
            default_alarms: Vec::new(),
            order: 0,
        }
    }

    /// Problems with the settings (empty = valid).
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            push_issue(&mut out, PimIssue::MissingName);
        }
        if self.color.as_deref().is_some_and(|c| !is_color(c)) {
            push_issue(&mut out, PimIssue::InvalidColor);
        }
        for alarm in &self.default_alarms {
            for issue in alarm.issues() {
                push_issue(&mut out, issue);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Recurrence

/// `FREQ` of a recurrence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    /// Every N seconds.
    Secondly,
    /// Every N minutes.
    Minutely,
    /// Every N hours.
    Hourly,
    /// Every N days.
    Daily,
    /// Every N weeks.
    Weekly,
    /// Every N months.
    Monthly,
    /// Every N years.
    Yearly,
}

/// A `BYDAY` entry: a weekday, optionally the n-th of the month / year
/// (`2MO` = second Monday, `-1FR` = last Friday).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct WeekdayNum {
    /// The weekday.
    pub day: Weekday,
    /// `1`–`53` or `-53`–`-1`; `null` = every such weekday.
    #[serde(default)]
    pub ordinal: Option<i8>,
}

/// How a recurrence rule ends.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RecurrenceEnd {
    /// Never.
    #[default]
    Never,
    /// After this many occurrences (`COUNT`).
    Count {
        /// At least 1.
        count: u32,
    },
    /// On or before this time (`UNTIL`; a date for all-day events, else UTC
    /// or the start's kind, as RFC 5545 requires).
    Until {
        /// The last possible start.
        until: PimTime,
    },
}

/// One `RRULE`. Empty `by_*` lists mean "not given".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct RecurrenceRule {
    /// `FREQ`.
    pub freq: Frequency,
    /// `INTERVAL` (≥ 1).
    #[serde(default = "one")]
    pub interval: u32,
    /// `COUNT` / `UNTIL`.
    #[serde(default)]
    pub end: RecurrenceEnd,
    /// `BYSECOND` (0–60).
    #[serde(default)]
    pub by_second: Vec<u8>,
    /// `BYMINUTE` (0–59).
    #[serde(default)]
    pub by_minute: Vec<u8>,
    /// `BYHOUR` (0–23).
    #[serde(default)]
    pub by_hour: Vec<u8>,
    /// `BYDAY`.
    #[serde(default)]
    pub by_day: Vec<WeekdayNum>,
    /// `BYMONTHDAY` (1–31 or -31 – -1).
    #[serde(default)]
    pub by_month_day: Vec<i8>,
    /// `BYYEARDAY` (1–366 or -366 – -1).
    #[serde(default)]
    pub by_year_day: Vec<i16>,
    /// `BYWEEKNO` (1–53 or -53 – -1).
    #[serde(default)]
    pub by_week_no: Vec<i8>,
    /// `BYMONTH` (1–12).
    #[serde(default)]
    pub by_month: Vec<u8>,
    /// `BYSETPOS` (1–366 or -366 – -1).
    #[serde(default)]
    pub by_set_pos: Vec<i16>,
    /// `WKST`, `null` = Monday.
    #[serde(default)]
    pub week_start: Option<Weekday>,
}

fn one() -> u32 {
    1
}

impl RecurrenceRule {
    /// A rule of this frequency, every time, forever.
    pub fn new(freq: Frequency) -> Self {
        Self {
            freq,
            interval: 1,
            end: RecurrenceEnd::Never,
            by_second: Vec::new(),
            by_minute: Vec::new(),
            by_hour: Vec::new(),
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_year_day: Vec::new(),
            by_week_no: Vec::new(),
            by_month: Vec::new(),
            by_set_pos: Vec::new(),
            week_start: None,
        }
    }

    /// Whether the rule is valid by RFC 5545's value ranges (and the
    /// `BYWEEKNO` / ordinal-`BYDAY` restrictions per frequency).
    pub fn is_valid(&self) -> bool {
        let signed = |v: i16, max: i16| v != 0 && (-max..=max).contains(&v);
        let ordinal_allowed = matches!(self.freq, Frequency::Monthly | Frequency::Yearly)
            || self.by_day.iter().all(|d| d.ordinal.is_none());
        let end_ok = match &self.end {
            RecurrenceEnd::Never => true,
            RecurrenceEnd::Count { count } => *count >= 1,
            RecurrenceEnd::Until { until } => until.issue().is_none(),
        };
        self.interval >= 1
            && end_ok
            && self.by_second.iter().all(|&v| v <= 60)
            && self.by_minute.iter().all(|&v| v <= 59)
            && self.by_hour.iter().all(|&v| v <= 23)
            && self
                .by_day
                .iter()
                .all(|d| d.ordinal.is_none_or(|o| signed(o.into(), 53)))
            && ordinal_allowed
            && self.by_month_day.iter().all(|&v| signed(v.into(), 31))
            && self.by_year_day.iter().all(|&v| signed(v, 366))
            && self.by_week_no.iter().all(|&v| signed(v.into(), 53))
            && (self.by_week_no.is_empty() || self.freq == Frequency::Yearly)
            && self.by_month.iter().all(|&v| (1..=12).contains(&v))
            && self.by_set_pos.iter().all(|&v| signed(v, 366))
    }
}

/// When a series recurs: rules plus extra dates, minus exceptions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Recurrence {
    /// `RRULE`s (normally one).
    pub rules: Vec<RecurrenceRule>,
    /// `RDATE`s: extra occurrences (same kind as the start).
    pub dates: Vec<PimTime>,
    /// `EXDATE`s: removed occurrences (by their original start).
    pub exceptions: Vec<PimTime>,
}

impl Recurrence {
    /// A recurrence with one rule.
    pub fn rule(rule: RecurrenceRule) -> Self {
        Self {
            rules: vec![rule],
            ..Self::default()
        }
    }

    /// Whether it produces anything beyond the start.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.dates.is_empty()
    }

    /// Problems (empty = valid). `start` is the series start: dates and
    /// exceptions must be of the same kind (date vs. time).
    pub fn issues(&self, start: &PimTime) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if self.rules.iter().any(|r| !r.is_valid()) {
            push_issue(&mut out, PimIssue::InvalidRecurrence);
        }
        for rule in &self.rules {
            if let RecurrenceEnd::Until { until } = &rule.end
                && until.is_date() != start.is_date()
            {
                push_issue(&mut out, PimIssue::InvalidRecurrence);
            }
        }
        for time in self.dates.iter().chain(&self.exceptions) {
            if let Some(issue) = time.issue() {
                push_issue(&mut out, issue);
            } else if time.is_date() != start.is_date() {
                push_issue(&mut out, PimIssue::MixedAllDay);
            }
        }
        if self.rules.len() + self.dates.len() + self.exceptions.len() > MAX_LIST_ENTRIES {
            push_issue(&mut out, PimIssue::TooMany);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// People

/// The organizer of an event (`ORGANIZER`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Organizer {
    /// Email address (from the `mailto:` URI).
    pub email: String,
    /// Display name (`CN`).
    #[serde(default)]
    pub name: Option<String>,
}

/// `ROLE` of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AttendeeRole {
    /// `CHAIR`.
    Chair,
    /// `REQ-PARTICIPANT` (the default).
    #[default]
    Required,
    /// `OPT-PARTICIPANT`.
    Optional,
    /// `NON-PARTICIPANT` (for information only).
    NonParticipant,
}

/// `PARTSTAT` of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ParticipationStatus {
    /// No answer yet (the default).
    #[default]
    NeedsAction,
    /// Accepted.
    Accepted,
    /// Declined.
    Declined,
    /// Tentatively accepted.
    Tentative,
    /// Delegated to someone else.
    Delegated,
}

/// `CUTYPE` of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AttendeeKind {
    /// A person (the default).
    #[default]
    Individual,
    /// A group or list.
    Group,
    /// A resource (projector, car).
    Resource,
    /// A room.
    Room,
    /// Unknown.
    Unknown,
}

/// An attendee (`ATTENDEE`). Invitations and replies (iTIP) arrive in M3b.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Attendee {
    /// Email address.
    pub email: String,
    /// Display name (`CN`).
    #[serde(default)]
    pub name: Option<String>,
    /// Role.
    #[serde(default)]
    pub role: AttendeeRole,
    /// Answer.
    #[serde(default)]
    pub status: ParticipationStatus,
    /// Kind.
    #[serde(default)]
    pub kind: AttendeeKind,
    /// An answer is requested (`RSVP=TRUE`).
    #[serde(default)]
    pub rsvp: bool,
    /// Delegated to (`DELEGATED-TO`, email addresses).
    #[serde(default)]
    pub delegated_to: Vec<String>,
    /// Delegated from (`DELEGATED-FROM`).
    #[serde(default)]
    pub delegated_from: Vec<String>,
}

impl Attendee {
    /// A required individual who has not answered.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            name: None,
            role: AttendeeRole::Required,
            status: ParticipationStatus::NeedsAction,
            kind: AttendeeKind::Individual,
            rsvp: false,
            delegated_to: Vec::new(),
            delegated_from: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Alarms

/// What an alarm does (`ACTION`). Nail shows every kind as a reminder;
/// `email` alarms are kept for the server but never send mail from Nail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AlarmAction {
    /// `DISPLAY` (the default).
    #[default]
    Display,
    /// `AUDIO`.
    Audio,
    /// `EMAIL`.
    Email,
}

/// What a relative trigger is relative to (`RELATED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AlarmRelated {
    /// The start — the default. For tasks (RFC 5545): `DTSTART`, or the
    /// due time when the task has no start.
    #[default]
    Start,
    /// The end. For tasks: the due time (`DUE`), or the start when the
    /// task has no due time — so "n minutes before due" on a task with a
    /// start is `End`.
    End,
}

/// When an alarm fires (`TRIGGER`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AlarmTrigger {
    /// Relative to the start or end: negative = before (`-PT15M` =
    /// `-900`).
    Relative {
        /// Offset in seconds.
        offset_secs: i64,
        /// Start or end.
        #[serde(default)]
        related: AlarmRelated,
    },
    /// At a fixed instant (RFC 3339 UTC).
    Absolute {
        /// When.
        at: String,
    },
}

/// Repetition of an alarm (`REPEAT` + `DURATION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct AlarmRepeat {
    /// Additional times (≥ 1).
    pub count: u32,
    /// Seconds between them (≥ 1).
    pub interval_secs: u32,
}

/// An alarm (`VALARM`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct Alarm {
    /// The alarm's `UID` (RFC 9074), if it has one; keeps snooze / dismiss
    /// bound to it across syncs.
    #[serde(default)]
    pub uid: Option<String>,
    /// What it does.
    #[serde(default)]
    pub action: AlarmAction,
    /// When it fires.
    pub trigger: AlarmTrigger,
    /// Text to show (`DESCRIPTION`); `null` = the item's title.
    #[serde(default)]
    pub description: Option<String>,
    /// Repetition, if any.
    #[serde(default)]
    pub repeat: Option<AlarmRepeat>,
    /// When it was dismissed (`ACKNOWLEDGED`, RFC 3339 UTC; synced so other
    /// devices stop showing it).
    #[serde(default)]
    pub acknowledged: Option<String>,
}

impl Alarm {
    /// A display alarm `minutes` before the start.
    pub fn minutes_before(minutes: u32) -> Self {
        Self {
            uid: None,
            action: AlarmAction::Display,
            trigger: AlarmTrigger::Relative {
                offset_secs: -i64::from(minutes) * 60,
                related: AlarmRelated::Start,
            },
            description: None,
            repeat: None,
            acknowledged: None,
        }
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if let AlarmTrigger::Absolute { at } = &self.trigger
            && !is_utc_instant(at)
        {
            push_issue(&mut out, PimIssue::InvalidAlarm);
        }
        if self
            .repeat
            .is_some_and(|r| r.count == 0 || r.interval_secs == 0)
        {
            push_issue(&mut out, PimIssue::InvalidAlarm);
        }
        if self
            .acknowledged
            .as_deref()
            .is_some_and(|a| !is_utc_instant(a))
        {
            push_issue(&mut out, PimIssue::InvalidTime);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Event details

/// `STATUS` of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum EventStatus {
    /// Tentative.
    Tentative,
    /// Confirmed (the default).
    #[default]
    Confirmed,
    /// Cancelled (an override with this status hides the occurrence).
    Cancelled,
}

/// `TRANSP`: whether the event blocks time (free/busy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Transparency {
    /// Busy (`OPAQUE`, the default).
    #[default]
    Busy,
    /// Free (`TRANSPARENT`).
    Free,
}

/// `CLASS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    /// Public (the default).
    #[default]
    Public,
    /// Private.
    Private,
    /// Confidential.
    Confidential,
}

/// A conferencing link (RFC 7986 `CONFERENCE`, or a recognized link in
/// `URL` / `LOCATION` / `X-` properties of common providers).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ConferenceLink {
    /// The URI (`https:`, `tel:`, `sip:`, …).
    pub uri: String,
    /// Label (`LABEL`), if any.
    #[serde(default)]
    pub label: Option<String>,
    /// `FEATURE` values, lowercase (`audio`, `video`, `chat`, `phone`,
    /// `screen`, `moderator`, `feed`).
    #[serde(default)]
    pub features: Vec<String>,
}

/// An attachment of an event or task (`ATTACH`): a link, or inline data
/// kept in the blob store (by hash).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct PimAttachment {
    /// File name (`FILENAME` / `X-FILENAME`), if known.
    #[serde(default)]
    pub name: Option<String>,
    /// MIME type (`FMTTYPE`).
    #[serde(default)]
    pub mime_type: Option<String>,
    /// Size in bytes, if known.
    #[serde(default)]
    pub size: Option<u64>,
    /// The link (`ATTACH;VALUE=URI`), for linked attachments.
    #[serde(default)]
    pub url: Option<String>,
    /// SHA-256 (hex) of inline data in the blob store.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// The editable fields of an event, or of one overridden occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventFields {
    /// `SUMMARY`.
    pub title: String,
    /// `DTSTART`.
    pub start: PimTime,
    /// `DTEND` (exclusive; a `DURATION` is turned into an end when parsed,
    /// and written back as it was).
    pub end: PimTime,
    /// `LOCATION`.
    #[serde(default)]
    pub location: Option<String>,
    /// `DESCRIPTION` (plain text).
    #[serde(default)]
    pub description: Option<String>,
    /// `STATUS`.
    #[serde(default)]
    pub status: EventStatus,
    /// `TRANSP`.
    #[serde(default)]
    pub transparency: Transparency,
    /// `CLASS`.
    #[serde(default)]
    pub visibility: Visibility,
    /// `PRIORITY` (1 = highest … 9 = lowest), `null` = undefined.
    #[serde(default)]
    pub priority: Option<u8>,
    /// `CATEGORIES`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// `COLOR` (RFC 7986), `null` = the calendar's.
    #[serde(default)]
    pub color: Option<String>,
    /// `URL`.
    #[serde(default)]
    pub url: Option<String>,
    /// `ORGANIZER`.
    #[serde(default)]
    pub organizer: Option<Organizer>,
    /// `ATTENDEE`s.
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    /// `VALARM`s.
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    /// Conferencing links.
    #[serde(default)]
    pub conferences: Vec<ConferenceLink>,
    /// Attachments.
    #[serde(default)]
    pub attachments: Vec<PimAttachment>,
    /// Links to messages (create event from email).
    #[serde(default)]
    pub links: Vec<PimLink>,
}

impl EventFields {
    /// Fields with a title, start and end and defaults for the rest.
    pub fn new(title: impl Into<String>, start: PimTime, end: PimTime) -> Self {
        Self {
            title: title.into(),
            start,
            end,
            location: None,
            description: None,
            status: EventStatus::Confirmed,
            transparency: Transparency::Busy,
            visibility: Visibility::Public,
            priority: None,
            categories: Vec::new(),
            color: None,
            url: None,
            organizer: None,
            attendees: Vec::new(),
            alarms: Vec::new(),
            conferences: Vec::new(),
            attachments: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Whether this is an all-day event.
    pub fn is_all_day(&self) -> bool {
        self.start.is_date()
    }

    /// Problems (empty = valid). The end-before-start check compares times
    /// of the same kind and zone only; the core checks the rest with zone
    /// data.
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        for time in [&self.start, &self.end] {
            if let Some(issue) = time.issue() {
                push_issue(&mut out, issue);
            }
        }
        if self.start.is_date() != self.end.is_date() {
            push_issue(&mut out, PimIssue::MixedAllDay);
        } else if self.start.tz() == self.end.tz()
            && std::mem::discriminant(&self.start) == std::mem::discriminant(&self.end)
            && self.end.sort_key() < self.start.sort_key()
        {
            push_issue(&mut out, PimIssue::EndBeforeStart);
        }
        if self.priority.is_some_and(|p| p > 9) {
            push_issue(&mut out, PimIssue::InvalidPriority);
        }
        if self.color.as_deref().is_some_and(|c| !is_color(c)) {
            push_issue(&mut out, PimIssue::InvalidColor);
        }
        if self
            .url
            .as_deref()
            .is_some_and(|u| !has_url_scheme(u, &["http", "https"]))
        {
            push_issue(&mut out, PimIssue::InvalidUrl);
        }
        let emails = self
            .organizer
            .iter()
            .map(|o| o.email.as_str())
            .chain(self.attendees.iter().map(|a| a.email.as_str()));
        for email in emails {
            if !is_plausible_email(email) {
                push_issue(&mut out, PimIssue::InvalidEmail);
            }
        }
        for alarm in &self.alarms {
            for issue in alarm.issues() {
                push_issue(&mut out, issue);
            }
        }
        if self
            .conferences
            .iter()
            .any(|c| c.uri.trim().is_empty() || c.uri.contains(char::is_whitespace))
        {
            push_issue(&mut out, PimIssue::InvalidUrl);
        }
        let longest = [
            self.attendees.len(),
            self.alarms.len(),
            self.categories.len(),
            self.conferences.len(),
            self.attachments.len(),
            self.links.len(),
        ];
        if longest.into_iter().any(|n| n > MAX_LIST_ENTRIES) {
            push_issue(&mut out, PimIssue::TooMany);
        }
        out
    }
}

/// An occurrence of a recurring event whose fields differ from the series
/// (`RECURRENCE-ID`). A cancelled occurrence has `status: cancelled` (or is
/// an exception in [`Recurrence::exceptions`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventOverride {
    /// The occurrence's original start (same kind and zone as the series
    /// start).
    pub recurrence_id: PimTime,
    /// Its fields.
    pub fields: EventFields,
    /// `RECURRENCE-ID;RANGE=THISANDFUTURE` (RFC 5545): the override also
    /// applies to every later occurrence (written by other clients; Nail
    /// splits the series instead).
    #[serde(default)]
    pub this_and_future: bool,
}

/// An event: a single event, or the whole series of a recurring one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CalendarEvent {
    /// The event.
    pub id: EventId,
    /// Its calendar.
    pub calendar_id: CalendarId,
    /// iCalendar `UID`.
    pub uid: String,
    /// The fields (of the series master).
    pub fields: EventFields,
    /// Recurrence, `null` = a single event.
    pub recurrence: Option<Recurrence>,
    /// Overridden occurrences, by recurrence id.
    pub overrides: Vec<EventOverride>,
    /// `SEQUENCE`.
    pub sequence: u32,
    /// `CREATED` (RFC 3339 UTC), if known.
    pub created_at: Option<String>,
    /// `LAST-MODIFIED` (RFC 3339 UTC), if known.
    pub updated_at: Option<String>,
    /// Read-only (its calendar is).
    pub read_only: bool,
    /// Sync state.
    pub sync: PimItemSync,
}

impl CalendarEvent {
    /// Whether the event recurs.
    pub fn is_recurring(&self) -> bool {
        self.recurrence.as_ref().is_some_and(|r| !r.is_empty())
    }
}

/// Which occurrences an edit of a recurring event applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum EditScope {
    /// Only the given occurrence.
    This,
    /// The given occurrence and every later one (splits the series).
    ThisAndFollowing,
    /// The whole series (the only scope of single events).
    #[default]
    All,
}

impl EditScope {
    /// Whether the scope needs a recurrence id.
    pub fn needs_occurrence(self) -> bool {
        !matches!(self, Self::All)
    }
}

/// One expanded occurrence, for calendar views
/// ([`crate::Request::ListEventInstances`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventInstance {
    /// The event (series).
    pub event_id: EventId,
    /// Its calendar.
    pub calendar_id: CalendarId,
    /// The occurrence's recurrence id (its original start); `null` for a
    /// single event. Pass it back with an [`EditScope`].
    pub recurrence_id: Option<PimTime>,
    /// Start as stored (its own zone).
    pub start: PimTime,
    /// End as stored.
    pub end: PimTime,
    /// Start as an instant (RFC 3339 UTC); dates and floating times are
    /// resolved in the request's zone.
    pub start_utc: String,
    /// End as an instant (RFC 3339 UTC).
    pub end_utc: String,
    /// All-day.
    pub all_day: bool,
    /// Title.
    pub title: String,
    /// Location.
    pub location: Option<String>,
    /// Status (cancelled occurrences are left out of the list).
    pub status: EventStatus,
    /// Busy or free.
    pub transparency: Transparency,
    /// Colour: the event's, else `null` (the UI uses the calendar's).
    pub color: Option<String>,
    /// Part of a recurring series.
    pub recurring: bool,
    /// This occurrence is an override.
    pub overridden: bool,
    /// The event has attendees (a meeting).
    pub has_attendees: bool,
    /// The event has alarms.
    pub has_alarms: bool,
    /// Read-only.
    pub read_only: bool,
}

/// A stretch of free time ([`crate::Request::FindFreeTime`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FreeSlot {
    /// Start (RFC 3339 UTC).
    pub start: String,
    /// End (RFC 3339 UTC).
    pub end: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meeting() -> EventFields {
        EventFields::new(
            "Standup",
            PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-01T09:15:00", "Europe/Berlin"),
        )
    }

    #[test]
    fn event_fields_defaults_and_wire_shape() {
        let fields: EventFields = serde_json::from_value(json!({
            "title": "Standup",
            "start": {"type": "zoned", "local": "2026-10-01T09:00:00", "tz": "Europe/Berlin"},
            "end": {"type": "zoned", "local": "2026-10-01T09:15:00", "tz": "Europe/Berlin"}
        }))
        .unwrap();
        assert_eq!(fields, meeting());
        assert!(fields.issues().is_empty());
        let value = serde_json::to_value(&fields).unwrap();
        assert_eq!(value["status"], "confirmed");
        assert_eq!(value["transparency"], "busy");
        assert_eq!(value["visibility"], "public");
    }

    #[test]
    fn event_fields_issues() {
        let mut f = meeting();
        f.end = PimTime::zoned("2026-10-01T08:00:00", "Europe/Berlin");
        assert_eq!(f.issues(), [PimIssue::EndBeforeStart]);
        f.end = PimTime::date("2026-10-02");
        assert_eq!(f.issues(), [PimIssue::MixedAllDay]);
        let mut f = meeting();
        f.priority = Some(10);
        f.color = Some("nope nope".into());
        f.attendees.push(Attendee::new("not-an-address"));
        f.url = Some("javascript:alert(1)".into());
        assert_eq!(
            f.issues(),
            [
                PimIssue::InvalidPriority,
                PimIssue::InvalidColor,
                PimIssue::InvalidUrl,
                PimIssue::InvalidEmail
            ]
        );
        let all_day = EventFields::new(
            "Holiday",
            PimTime::date("2026-10-03"),
            PimTime::date("2026-10-04"),
        );
        assert!(all_day.is_all_day() && all_day.issues().is_empty());
        // Different zones are compared by the core, not here.
        let mut f = meeting();
        f.end = PimTime::utc("2026-10-01T07:10:00");
        assert!(f.issues().is_empty());
    }

    #[test]
    fn recurrence_rules_validate() {
        let mut weekly = RecurrenceRule::new(Frequency::Weekly);
        weekly.by_day = vec![
            WeekdayNum {
                day: Weekday::Mon,
                ordinal: None,
            },
            WeekdayNum {
                day: Weekday::Wed,
                ordinal: None,
            },
        ];
        assert!(weekly.is_valid());
        let mut bad = weekly.clone();
        bad.by_day[0].ordinal = Some(2); // ordinals only for monthly / yearly
        assert!(!bad.is_valid());
        let mut monthly = RecurrenceRule::new(Frequency::Monthly);
        monthly.by_day = vec![WeekdayNum {
            day: Weekday::Fri,
            ordinal: Some(-1),
        }];
        monthly.end = RecurrenceEnd::Count { count: 12 };
        assert!(monthly.is_valid());
        for broken in [
            RecurrenceRule {
                interval: 0,
                ..monthly.clone()
            },
            RecurrenceRule {
                end: RecurrenceEnd::Count { count: 0 },
                ..monthly.clone()
            },
            RecurrenceRule {
                by_month_day: vec![0],
                ..monthly.clone()
            },
            RecurrenceRule {
                by_month: vec![13],
                ..monthly.clone()
            },
            RecurrenceRule {
                by_week_no: vec![10],
                ..monthly.clone()
            },
            RecurrenceRule {
                by_hour: vec![24],
                ..monthly.clone()
            },
        ] {
            assert!(!broken.is_valid(), "{broken:?}");
        }
        let value = serde_json::to_value(&monthly).unwrap();
        assert_eq!(value["freq"], "monthly");
        assert_eq!(value["end"], json!({"type": "count", "count": 12}));
        assert_eq!(value["by_day"], json!([{"day": "fri", "ordinal": -1}]));
        let parsed: RecurrenceRule = serde_json::from_value(json!({"freq": "daily"})).unwrap();
        assert_eq!(parsed, RecurrenceRule::new(Frequency::Daily));
    }

    #[test]
    fn recurrence_issues_check_kinds() {
        let start = PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin");
        let mut rule = RecurrenceRule::new(Frequency::Daily);
        rule.end = RecurrenceEnd::Until {
            until: PimTime::utc("2026-10-31T23:59:59"),
        };
        let mut rec = Recurrence::rule(rule.clone());
        rec.exceptions
            .push(PimTime::zoned("2026-10-03T09:00:00", "Europe/Berlin"));
        assert!(rec.issues(&start).is_empty());
        assert!(!rec.is_empty());
        rec.exceptions.push(PimTime::date("2026-10-04"));
        assert_eq!(rec.issues(&start), [PimIssue::MixedAllDay]);
        let all_day_until = Recurrence::rule(rule);
        assert_eq!(
            all_day_until.issues(&PimTime::date("2026-10-01")),
            [PimIssue::InvalidRecurrence]
        );
        assert!(Recurrence::default().is_empty());
    }

    #[test]
    fn alarms() {
        let alarm = Alarm::minutes_before(15);
        assert_eq!(
            serde_json::to_value(&alarm.trigger).unwrap(),
            json!({"type": "relative", "offset_secs": -900, "related": "start"})
        );
        assert!(alarm.issues().is_empty());
        let absolute = Alarm {
            trigger: AlarmTrigger::Absolute {
                at: "2026-10-01T08:00:00".into(),
            },
            repeat: Some(AlarmRepeat {
                count: 2,
                interval_secs: 0,
            }),
            ..alarm
        };
        assert_eq!(absolute.issues(), [PimIssue::InvalidAlarm]);
        let parsed: Alarm = serde_json::from_value(json!({
            "trigger": {"type": "absolute", "at": "2026-10-01T08:00:00Z"}
        }))
        .unwrap();
        assert_eq!(parsed.action, AlarmAction::Display);
        assert!(parsed.issues().is_empty());
    }

    #[test]
    fn calendars_and_scopes() {
        let settings: CalendarSettings = serde_json::from_value(json!({"name": "Work"})).unwrap();
        assert_eq!(settings, CalendarSettings::named("Work"));
        assert!(settings.visible && settings.reminders);
        assert_eq!(
            CalendarSettings::named(" ").issues(),
            [PimIssue::MissingName]
        );
        let calendar = Calendar {
            id: "c1".into(),
            source_id: "local".into(),
            account_id: None,
            settings,
            components: vec![CalendarComponent::Event, CalendarComponent::Task],
            read_only: false,
            timezone: None,
        };
        assert!(calendar.has_events() && calendar.has_tasks());
        assert_eq!(
            serde_json::to_value(EditScope::ThisAndFollowing).unwrap(),
            json!("this-and-following")
        );
        assert!(EditScope::This.needs_occurrence() && !EditScope::All.needs_occurrence());
        assert_eq!(
            serde_json::to_value(CalendarComponent::Task).unwrap(),
            json!("task")
        );
    }

    #[test]
    fn events_recur() {
        let event = CalendarEvent {
            id: "ev1".into(),
            calendar_id: "c1".into(),
            uid: "uid-1@example.org".into(),
            fields: meeting(),
            recurrence: Some(Recurrence::rule(RecurrenceRule::new(Frequency::Daily))),
            overrides: vec![EventOverride {
                recurrence_id: PimTime::zoned("2026-10-02T09:00:00", "Europe/Berlin"),
                fields: EventFields {
                    title: "Standup (moved)".into(),
                    ..meeting()
                },
                this_and_future: false,
            }],
            sequence: 1,
            created_at: None,
            updated_at: Some("2026-10-01T08:00:00Z".into()),
            read_only: false,
            sync: PimItemSync::Pending,
        };
        assert!(event.is_recurring());
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["sync"], "pending");
        assert_eq!(
            serde_json::from_value::<CalendarEvent>(value).unwrap(),
            event
        );
    }
}
