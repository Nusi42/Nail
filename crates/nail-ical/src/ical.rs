//! iCalendar (RFC 5545, RFC 7986, RFC 9074) → the contract's events and
//! tasks.
//!
//! [`parse_calendar`] reads a whole `VCALENDAR` (an import, a feed, a
//! CalDAV resource) and groups its components by `UID`: one
//! [`ParsedEvent`] per series (master + overrides by `RECURRENCE-ID`), one
//! [`ParsedTask`] per task. Each item keeps its own iCalendar text
//! ([`ParsedEvent::raw`]): the calendar's `VERSION` / `PRODID` /
//! `CALSCALE`, the `VTIMEZONE`s it uses and its components, verbatim — the
//! store keeps it and [`crate::write`] patches it on edits, so unknown
//! properties survive.
//!
//! The field readers here are shared with the writer, which compares what
//! the original text means with the edited model to decide what to keep.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use base64::Engine as _;
use chrono::{Datelike, Duration, NaiveTime};
use chrono_tz::Tz;
use nail_api::ids::{CalendarId, EventId, TaskId};
use nail_api::pim::calendar::{
    AlarmRelated, AlarmRepeat, AttendeeKind, AttendeeRole, EventStatus, ParticipationStatus,
    Transparency, Visibility,
};
use nail_api::pim::source::PimItemSync;
use nail_api::pim::task::TaskStatus;
use nail_api::pim::{
    Alarm, AlarmAction, AlarmTrigger, Attendee, CalendarEvent, ConferenceLink, EventFields,
    EventOverride, Organizer, PimAttachment, PimLink, PimTime, Recurrence, RecurrenceRule, Task,
    TaskFields,
};
use sha2::{Digest, Sha256};

use crate::content::{self, Component, Property};
use crate::issues::{Issue, IssueKind};
use crate::recur;
use crate::time::{self, IcalValue};
use crate::tz::{Resolution, ZoneResolver};

/// Options for reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    /// Read floating times in this zone (subscriptions: Google's
    /// `X-WR-TIMEZONE` convention). `None` keeps them floating.
    pub floating_zone: Option<Tz>,
    /// Apply the calendar's own `X-WR-TIMEZONE` to floating times (feeds).
    pub use_calendar_zone: bool,
}

/// Inline data of an attachment (`ATTACH;VALUE=BINARY`), for the blob
/// store; the model refers to it by [`InlineData::sha256`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineData {
    /// SHA-256 (hex).
    pub sha256: String,
    /// MIME type, if given.
    pub mime_type: Option<String>,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// SHA-256 (hex) of bytes, the blob store's key.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// An event series read from iCalendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEvent {
    /// `UID` (derived from the content when missing).
    pub uid: String,
    /// The master's fields.
    pub fields: EventFields,
    /// Recurrence, if any.
    pub recurrence: Option<Recurrence>,
    /// Overridden occurrences (recurrence ids in the start's zone).
    pub overrides: Vec<EventOverride>,
    /// Recurrence ids of overrides with `RANGE=THISANDFUTURE`: they apply
    /// to every later occurrence too ([`crate::expand`] takes them).
    pub this_and_future: Vec<PimTime>,
    /// `SEQUENCE`.
    pub sequence: u32,
    /// `CREATED` (RFC 3339 UTC).
    pub created_at: Option<String>,
    /// `LAST-MODIFIED` (RFC 3339 UTC).
    pub updated_at: Option<String>,
    /// `RELATED-TO` values (a split series points at its origin).
    pub related_to: Vec<String>,
    /// Inline attachment data of the series.
    pub inline_data: Vec<InlineData>,
    /// The item's own iCalendar text.
    pub raw: String,
    /// 1-based line of the master (or first) component.
    pub line: u32,
}

impl ParsedEvent {
    /// The contract's event, with the store's ids and state.
    pub fn into_event(
        self,
        id: EventId,
        calendar_id: CalendarId,
        read_only: bool,
        sync: PimItemSync,
    ) -> CalendarEvent {
        CalendarEvent {
            id,
            calendar_id,
            uid: self.uid,
            fields: self.fields,
            recurrence: self.recurrence,
            overrides: self.overrides,
            sequence: self.sequence,
            created_at: self.created_at,
            updated_at: self.updated_at,
            read_only,
            sync,
        }
    }
}

/// A task read from iCalendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTask {
    /// `UID`.
    pub uid: String,
    /// The fields (`parent_id` is left empty: see `parent_uid`).
    pub fields: TaskFields,
    /// The parent's `UID` (`RELATED-TO;RELTYPE=PARENT`); the store maps
    /// it to a task id.
    pub parent_uid: Option<String>,
    /// Recurrence, if any.
    pub recurrence: Option<Recurrence>,
    /// `SEQUENCE`.
    pub sequence: u32,
    /// `CREATED`.
    pub created_at: Option<String>,
    /// `LAST-MODIFIED`.
    pub updated_at: Option<String>,
    /// The item's own iCalendar text.
    pub raw: String,
    /// 1-based line.
    pub line: u32,
}

impl ParsedTask {
    /// The contract's task, with the store's ids and state.
    #[allow(clippy::too_many_arguments)]
    pub fn into_task(
        self,
        id: TaskId,
        calendar_id: CalendarId,
        parent_id: Option<TaskId>,
        subtask_ids: Vec<TaskId>,
        read_only: bool,
        sync: PimItemSync,
    ) -> Task {
        let mut fields = self.fields;
        fields.parent_id = parent_id;
        Task {
            id,
            calendar_id,
            uid: self.uid,
            fields,
            recurrence: self.recurrence,
            subtask_ids,
            created_at: self.created_at,
            updated_at: self.updated_at,
            read_only,
            sync,
        }
    }
}

/// A whole calendar as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCalendar {
    /// `X-WR-CALNAME` / `NAME` (RFC 7986).
    pub name: Option<String>,
    /// `X-WR-CALDESC` / `DESCRIPTION`.
    pub description: Option<String>,
    /// `COLOR` (RFC 7986) / `X-APPLE-CALENDAR-COLOR`.
    pub color: Option<String>,
    /// `X-WR-TIMEZONE` (IANA), if known.
    pub time_zone: Option<String>,
    /// `REFRESH-INTERVAL` (RFC 7986) / `X-PUBLISHED-TTL`, minutes.
    pub refresh_minutes: Option<u32>,
    /// `METHOD` (iTIP; M3b), if any.
    pub method: Option<String>,
    /// Event series.
    pub events: Vec<ParsedEvent>,
    /// Tasks.
    pub tasks: Vec<ParsedTask>,
    /// Problems.
    pub issues: Vec<Issue>,
}

// ---------------------------------------------------------------------------
// Reading context

/// What field readers need: zone resolution and a place for problems.
pub(crate) struct Ctx<'a> {
    pub resolver: &'a ZoneResolver,
    pub floating_zone: Option<Tz>,
    pub issues: RefCell<Vec<Issue>>,
}

impl<'a> Ctx<'a> {
    pub fn new(resolver: &'a ZoneResolver, floating_zone: Option<Tz>) -> Self {
        Self {
            resolver,
            floating_zone,
            issues: RefCell::new(Vec::new()),
        }
    }

    fn issue(&self, kind: IssueKind, line: u32) {
        let line = (line > 0).then_some(line);
        let mut issues = self.issues.borrow_mut();
        let issue = Issue::new(kind, line);
        if !issues.contains(&issue) {
            issues.push(issue);
        }
    }

    /// One `DATE` / `DATE-TIME` value of a property as a [`PimTime`].
    pub fn time_value(&self, property: &Property, value: &str) -> Option<PimTime> {
        let Some(parsed) = time::parse_ical_value(value) else {
            self.issue(
                IssueKind::InvalidTime {
                    property: property.name.to_ascii_uppercase(),
                },
                property.line,
            );
            return None;
        };
        Some(match parsed {
            IcalValue::Date(date) => PimTime::date(time::format_date(date)),
            IcalValue::DateTime { local, utc: true } => PimTime::utc(time::format_local(local)),
            IcalValue::DateTime { local, utc: false } => match property.param("TZID") {
                Some(tzid) => match self.resolver.resolve(tzid) {
                    Resolution::Iana(tz) => PimTime::zoned(time::format_local(local), tz.name()),
                    Resolution::Custom(definition) => {
                        self.issue(
                            IssueKind::CustomTimeZone {
                                tzid: tzid.to_owned(),
                            },
                            property.line,
                        );
                        match definition.to_utc(local) {
                            Some(utc) => PimTime::utc(time::format_local(utc)),
                            None => PimTime::floating(time::format_local(local)),
                        }
                    }
                    Resolution::Unknown => {
                        self.issue(
                            IssueKind::UnknownTimeZone {
                                tzid: tzid.to_owned(),
                            },
                            property.line,
                        );
                        self.floating(local)
                    }
                },
                None => self.floating(local),
            },
        })
    }

    fn floating(&self, local: chrono::NaiveDateTime) -> PimTime {
        match self.floating_zone {
            Some(tz) => PimTime::zoned(time::format_local(local), tz.name()),
            None => PimTime::floating(time::format_local(local)),
        }
    }

    /// The first value of a time property.
    pub fn time(&self, property: &Property) -> Option<PimTime> {
        let first = property.value.split(',').next()?;
        self.time_value(property, first)
    }

    /// Every value of a (multi-valued) time property (`RDATE`, `EXDATE`);
    /// periods give their start.
    pub fn times(&self, property: &Property) -> Vec<PimTime> {
        property
            .value
            .split(',')
            .filter(|v| !v.trim().is_empty())
            .filter_map(|v| {
                let start = v.split('/').next().unwrap_or(v);
                self.time_value(property, start)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Field readers (shared with the writer)

fn non_empty(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

pub(crate) fn read_text(c: &Component, name: &str) -> Option<String> {
    c.text(name).and_then(non_empty)
}

pub(crate) fn read_categories(c: &Component) -> Vec<String> {
    c.properties_named("CATEGORIES")
        .flat_map(Property::texts)
        .collect()
}

pub(crate) fn read_priority(c: &Component) -> Option<u8> {
    c.property("PRIORITY")
        .and_then(|p| p.value.trim().parse::<u8>().ok())
        .filter(|p| (1..=9).contains(p))
}

pub(crate) fn read_visibility(c: &Component) -> Visibility {
    match c
        .property("CLASS")
        .map(|p| p.value.trim().to_ascii_uppercase())
        .as_deref()
    {
        Some("PRIVATE") => Visibility::Private,
        Some("CONFIDENTIAL") => Visibility::Confidential,
        _ => Visibility::Public,
    }
}

pub(crate) fn read_status(c: &Component) -> EventStatus {
    match c
        .property("STATUS")
        .map(|p| p.value.trim().to_ascii_uppercase())
        .as_deref()
    {
        Some("TENTATIVE") => EventStatus::Tentative,
        Some("CANCELLED") => EventStatus::Cancelled,
        _ => EventStatus::Confirmed,
    }
}

pub(crate) fn read_task_status(c: &Component) -> TaskStatus {
    match c
        .property("STATUS")
        .map(|p| p.value.trim().to_ascii_uppercase())
        .as_deref()
    {
        Some("IN-PROCESS") => TaskStatus::InProcess,
        Some("COMPLETED") => TaskStatus::Completed,
        Some("CANCELLED") => TaskStatus::Cancelled,
        _ => TaskStatus::NeedsAction,
    }
}

pub(crate) fn read_transparency(c: &Component) -> Transparency {
    match c.property("TRANSP") {
        Some(p) if p.value.trim().eq_ignore_ascii_case("TRANSPARENT") => Transparency::Free,
        Some(_) => Transparency::Busy,
        None => match c.property("X-MICROSOFT-CDO-BUSYSTATUS") {
            Some(p) if p.value.trim().eq_ignore_ascii_case("FREE") => Transparency::Free,
            _ => Transparency::Busy,
        },
    }
}

/// `mailto:a@b` → `a@b` (other URIs as written).
pub(crate) fn strip_mailto(value: &str) -> String {
    let value = value.trim();
    match value.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("mailto:") => value[7..].to_owned(),
        _ => value.to_owned(),
    }
}

pub(crate) fn read_organizer(c: &Component) -> Option<Organizer> {
    let p = c.property("ORGANIZER")?;
    let email = strip_mailto(&content::unescape_text(&p.value));
    (!email.is_empty()).then(|| Organizer {
        email,
        name: p.param("CN").map(str::to_owned).and_then(non_empty),
    })
}

pub(crate) fn read_attendee(p: &Property) -> Attendee {
    let upper = |name: &str| p.param(name).map(str::to_ascii_uppercase);
    Attendee {
        email: strip_mailto(&content::unescape_text(&p.value)),
        name: p.param("CN").map(str::to_owned).and_then(non_empty),
        role: match upper("ROLE").as_deref() {
            Some("CHAIR") => AttendeeRole::Chair,
            Some("OPT-PARTICIPANT") => AttendeeRole::Optional,
            Some("NON-PARTICIPANT") => AttendeeRole::NonParticipant,
            _ => AttendeeRole::Required,
        },
        status: match upper("PARTSTAT").as_deref() {
            Some("ACCEPTED") => ParticipationStatus::Accepted,
            Some("DECLINED") => ParticipationStatus::Declined,
            Some("TENTATIVE") => ParticipationStatus::Tentative,
            Some("DELEGATED") => ParticipationStatus::Delegated,
            _ => ParticipationStatus::NeedsAction,
        },
        kind: match upper("CUTYPE").as_deref() {
            Some("GROUP") => AttendeeKind::Group,
            Some("RESOURCE") => AttendeeKind::Resource,
            Some("ROOM") => AttendeeKind::Room,
            Some("UNKNOWN") => AttendeeKind::Unknown,
            _ => AttendeeKind::Individual,
        },
        rsvp: upper("RSVP").as_deref() == Some("TRUE"),
        delegated_to: p.param_values("DELEGATED-TO").map(strip_mailto).collect(),
        delegated_from: p.param_values("DELEGATED-FROM").map(strip_mailto).collect(),
    }
}

pub(crate) fn read_attendees(c: &Component) -> Vec<Attendee> {
    c.properties_named("ATTENDEE")
        .map(read_attendee)
        .filter(|a| !a.email.is_empty())
        .collect()
}

fn read_utc_instant(c: &Component, name: &str) -> Option<String> {
    let p = c.property(name)?;
    match time::parse_ical_value(&p.value)? {
        IcalValue::DateTime { local, .. } => Some(time::format_instant(local.and_utc())),
        IcalValue::Date(date) => Some(time::format_instant(
            date.and_time(NaiveTime::MIN).and_utc(),
        )),
    }
}

/// Reads one `VALARM`; `None` for `ACTION:NONE` (RFC 9074) and alarms
/// without a usable trigger.
pub(crate) fn read_alarm(c: &Component) -> Option<Alarm> {
    let action = match c
        .property("ACTION")
        .map(|p| p.value.trim().to_ascii_uppercase())
        .as_deref()
    {
        Some("NONE") => return None,
        Some("AUDIO") => AlarmAction::Audio,
        Some("EMAIL") => AlarmAction::Email,
        _ => AlarmAction::Display,
    };
    let trigger = c.property("TRIGGER")?;
    let absolute = trigger
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME"));
    let trigger = if absolute
        || !trigger
            .value
            .trim_start_matches(['+', '-'])
            .starts_with(['P', 'p'])
    {
        match time::parse_ical_value(&trigger.value)? {
            IcalValue::DateTime { local, .. } => AlarmTrigger::Absolute {
                at: time::format_instant(local.and_utc()),
            },
            IcalValue::Date(_) => return None,
        }
    } else {
        AlarmTrigger::Relative {
            offset_secs: time::parse_duration(&trigger.value)?,
            related: if trigger
                .param("RELATED")
                .is_some_and(|r| r.eq_ignore_ascii_case("END"))
            {
                AlarmRelated::End
            } else {
                AlarmRelated::Start
            },
        }
    };
    let repeat = match (
        c.property("REPEAT")
            .and_then(|p| p.value.trim().parse::<u32>().ok()),
        c.property("DURATION")
            .and_then(|p| time::parse_duration(&p.value)),
    ) {
        (Some(count), Some(interval)) if count > 0 && interval > 0 => Some(AlarmRepeat {
            count,
            interval_secs: u32::try_from(interval).ok()?,
        }),
        _ => None,
    };
    Some(Alarm {
        uid: read_text(c, "UID").or_else(|| read_text(c, "X-WR-ALARMUID")),
        action,
        trigger,
        description: read_text(c, "DESCRIPTION"),
        repeat,
        acknowledged: read_utc_instant(c, "ACKNOWLEDGED"),
    })
}

/// The alarms of a component. A description equal to the item's title
/// reads as none (Nail writes the title for `DISPLAY` alarms, which
/// RFC 5545 requires to have one).
pub(crate) fn read_alarms(c: &Component) -> Vec<Alarm> {
    let title = c.text("SUMMARY").unwrap_or_default();
    c.components_named("VALARM")
        .filter_map(read_alarm)
        .map(|mut alarm| {
            if alarm.description.as_deref() == Some(title.as_str()) {
                alarm.description = None;
            }
            alarm
        })
        .collect()
}

/// Vendor properties that carry a conferencing link.
pub(crate) const CONFERENCE_VENDOR_PROPERTIES: &[&str] = &[
    "X-GOOGLE-CONFERENCE",
    "X-MICROSOFT-SKYPETEAMSMEETINGURL",
    "X-MICROSOFT-ONLINEMEETINGEXTERNALLINK",
    "X-APPLE-CONFERENCE-URL",
];

/// Hosts of conferencing services recognized in text fields.
const CONFERENCE_HOSTS: &[&str] = &[
    "meet.google.com",
    "zoom.us",
    "zoomgov.com",
    "teams.microsoft.com",
    "teams.live.com",
    "webex.com",
    "whereby.com",
    "meet.jit.si",
    "gotomeeting.com",
    "chime.aws",
    "bluejeans.com",
];

/// Conferencing links found in free text (`LOCATION`, `DESCRIPTION`,
/// `URL`): `https://` links to known services.
pub fn conference_links_in_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("https://") {
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | ')' | ']'))
            .unwrap_or(tail.len());
        let link = tail[..end].trim_end_matches(['.', ',', ';']);
        let host = link["https://".len()..]
            .split(['/', '?', '#', ':'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if CONFERENCE_HOSTS
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            && !out.iter().any(|l| l == link)
        {
            out.push(link.to_owned());
        }
        rest = &tail[end.max(1)..];
    }
    out
}

/// The conferencing links of a component: RFC 7986 `CONFERENCE` first,
/// then vendor properties, then links recognized in text, without
/// duplicates.
pub(crate) fn read_conferences(c: &Component) -> Vec<ConferenceLink> {
    let mut out: Vec<ConferenceLink> = Vec::new();
    let mut push = |link: ConferenceLink| {
        if !link.uri.is_empty() && !out.iter().any(|l| l.uri == link.uri) {
            out.push(link);
        }
    };
    for p in c.properties_named("CONFERENCE") {
        push(ConferenceLink {
            uri: p.value.trim().to_owned(),
            label: p.param("LABEL").map(str::to_owned).and_then(non_empty),
            features: p
                .param_values("FEATURE")
                .map(str::to_ascii_lowercase)
                .collect(),
        });
    }
    for name in CONFERENCE_VENDOR_PROPERTIES {
        for p in c.properties_named(name) {
            push(ConferenceLink {
                uri: p.text().trim().to_owned(),
                label: None,
                features: Vec::new(),
            });
        }
    }
    for name in ["LOCATION", "URL", "DESCRIPTION"] {
        for p in c.properties_named(name) {
            for uri in conference_links_in_text(&p.text()) {
                push(ConferenceLink {
                    uri,
                    label: None,
                    features: Vec::new(),
                });
            }
        }
    }
    out
}

/// Percent-decodes a `mid:` URL's message id (RFC 2392).
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a message id for a `mid:` URL.
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'*+=^`{|}@".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// What an `ATTACH` holds.
pub(crate) enum Attached {
    Attachment(PimAttachment, Option<InlineData>),
    Link(PimLink),
}

pub(crate) fn read_attach(p: &Property) -> Option<Attached> {
    let name = [
        "FILENAME",
        "X-FILENAME",
        "X-APPLE-FILENAME",
        "X-ORACLE-FILENAME",
    ]
    .iter()
    .find_map(|n| p.param(n))
    .map(str::to_owned);
    let mime_type = p.param("FMTTYPE").map(str::to_owned);
    let inline = p
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("BINARY"))
        || p.param("ENCODING")
            .is_some_and(|v| v.eq_ignore_ascii_case("BASE64"));
    if inline {
        let cleaned: String = p.value.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cleaned.as_bytes())
            .ok()?;
        let sha256 = sha256_hex(&bytes);
        return Some(Attached::Attachment(
            PimAttachment {
                name,
                mime_type: mime_type.clone(),
                size: u64::try_from(bytes.len()).ok(),
                url: None,
                sha256: Some(sha256.clone()),
            },
            Some(InlineData {
                sha256,
                mime_type,
                bytes,
            }),
        ));
    }
    let value = p.value.trim();
    if value.len() > 4 && value[..4].eq_ignore_ascii_case("mid:") {
        return Some(Attached::Link(PimLink::Message {
            message_id: None,
            rfc822_id: non_empty(percent_decode(&value[4..])),
            subject: p
                .param("X-NAIL-SUBJECT")
                .map(str::to_owned)
                .and_then(non_empty),
        }));
    }
    (!value.is_empty()).then(|| {
        Attached::Attachment(
            PimAttachment {
                name,
                mime_type,
                size: p.param("SIZE").and_then(|s| s.parse().ok()),
                url: Some(value.to_owned()),
                sha256: None,
            },
            None,
        )
    })
}

pub(crate) fn read_attachments(
    c: &Component,
) -> (Vec<PimAttachment>, Vec<PimLink>, Vec<InlineData>) {
    let mut attachments = Vec::new();
    let mut links = Vec::new();
    let mut data = Vec::new();
    for p in c.properties_named("ATTACH") {
        match read_attach(p) {
            Some(Attached::Attachment(a, inline)) => {
                attachments.push(a);
                data.extend(inline);
            }
            Some(Attached::Link(link)) => links.push(link),
            None => {}
        }
    }
    (attachments, links, data)
}

/// The end of an event: `DTEND`, else `DTSTART` + `DURATION`, else the
/// RFC 5545 defaults (one day for dates, zero for times). A malformed or
/// mismatched end is fixed to match the start.
pub(crate) fn read_end(c: &Component, start: &PimTime, end_name: &str, ctx: &Ctx<'_>) -> PimTime {
    let from_property = c.property(end_name).and_then(|p| ctx.time(p));
    let start_wall = time::wall(start);
    let end = match from_property {
        Some(end) => Some(end),
        None => c
            .property("DURATION")
            .and_then(|p| time::parse_duration_parts(&p.value))
            .and_then(|(days, seconds)| {
                let wall = start_wall? + Duration::days(days) + Duration::seconds(seconds);
                Some(time::like(wall, start))
            }),
    };
    let fallback = || match (start, start_wall) {
        (PimTime::Date { .. }, Some(wall)) => time::like(wall + Duration::days(1), start),
        _ => start.clone(),
    };
    match end {
        Some(end) if end.is_date() == start.is_date() => {
            // An end before the start becomes the start (or a day later).
            let same_frame = std::mem::discriminant(&end) == std::mem::discriminant(start)
                && end.tz() == start.tz();
            if same_frame && end.sort_key() < start.sort_key() {
                fallback()
            } else {
                end
            }
        }
        Some(end) => {
            // Mixed date / time: keep the end's day in the start's kind.
            match (time::wall(&end), start) {
                (Some(wall), PimTime::Date { .. }) => {
                    let day = wall.date().and_time(NaiveTime::MIN);
                    let day = if start_wall.is_some_and(|s| day <= s) {
                        day + Duration::days(1)
                    } else {
                        day
                    };
                    time::like(day, start)
                }
                _ => fallback(),
            }
        }
        None => fallback(),
    }
}

pub(crate) fn read_recurrence(c: &Component, start: &PimTime, ctx: &Ctx<'_>) -> Option<Recurrence> {
    let mut rules: Vec<RecurrenceRule> = Vec::new();
    for p in c.properties_named("RRULE") {
        match recur::parse_rrule(&p.value, start).filter(RecurrenceRule::is_valid) {
            Some(rule) => rules.push(rule),
            None => ctx.issue(IssueKind::InvalidRecurrence, p.line),
        }
    }
    let dates: Vec<PimTime> = c
        .properties_named("RDATE")
        .flat_map(|p| ctx.times(p))
        .collect();
    let exceptions: Vec<PimTime> = c
        .properties_named("EXDATE")
        .flat_map(|p| ctx.times(p))
        .collect();
    let recurrence = Recurrence {
        rules,
        dates,
        exceptions,
    };
    (!recurrence.is_empty() || !recurrence.exceptions.is_empty()).then_some(recurrence)
}

/// Reads the fields of a `VEVENT`; `None` without a start.
pub(crate) fn read_event_fields(
    c: &Component,
    ctx: &Ctx<'_>,
) -> Option<(EventFields, Vec<InlineData>)> {
    let start = c.property("DTSTART").and_then(|p| ctx.time(p))?;
    let end = read_end(c, &start, "DTEND", ctx);
    let (attachments, links, data) = read_attachments(c);
    Some((
        EventFields {
            title: c.text("SUMMARY").unwrap_or_default(),
            start,
            end,
            location: read_text(c, "LOCATION"),
            description: read_text(c, "DESCRIPTION"),
            status: read_status(c),
            transparency: read_transparency(c),
            visibility: read_visibility(c),
            priority: read_priority(c),
            categories: read_categories(c),
            color: read_text(c, "COLOR"),
            url: read_text(c, "URL"),
            organizer: read_organizer(c),
            attendees: read_attendees(c),
            alarms: read_alarms(c),
            conferences: read_conferences(c),
            attachments,
            links,
        },
        data,
    ))
}

/// Reads the fields of a `VTODO` (without the parent id).
pub(crate) fn read_task_fields(c: &Component, ctx: &Ctx<'_>) -> TaskFields {
    let start = c.property("DTSTART").and_then(|p| ctx.time(p));
    let due = match c.property("DUE").and_then(|p| ctx.time(p)) {
        Some(due) => Some(due),
        None => match (&start, c.property("DURATION")) {
            (Some(start), Some(_)) => Some(read_end(c, start, "DUE", ctx)),
            _ => None,
        },
    };
    let (_, links, _) = read_attachments(c);
    TaskFields {
        title: c.text("SUMMARY").unwrap_or_default(),
        description: read_text(c, "DESCRIPTION"),
        start,
        due,
        status: read_task_status(c),
        percent_complete: c
            .property("PERCENT-COMPLETE")
            .and_then(|p| p.value.trim().parse::<u8>().ok())
            .map(|p| p.min(100)),
        priority: read_priority(c),
        completed_at: read_utc_instant(c, "COMPLETED"),
        categories: read_categories(c),
        location: read_text(c, "LOCATION"),
        url: read_text(c, "URL"),
        visibility: read_visibility(c),
        alarms: read_alarms(c),
        parent_id: None,
        order: c
            .property("X-APPLE-SORT-ORDER")
            .and_then(|p| p.value.trim().parse().ok()),
        links,
    }
}

/// `RELATED-TO` values with `RELTYPE=PARENT` (the default).
pub(crate) fn read_parent_uid(c: &Component) -> Option<String> {
    c.properties_named("RELATED-TO")
        .find(|p| {
            p.param("RELTYPE")
                .is_none_or(|r| r.eq_ignore_ascii_case("PARENT"))
        })
        .map(Property::text)
        .and_then(non_empty)
}

pub(crate) fn read_sequence(c: &Component) -> u32 {
    c.property("SEQUENCE")
        .and_then(|p| p.value.trim().parse().ok())
        .unwrap_or(0)
}

/// The `RECURRENCE-ID` of a component in the frame of the series start,
/// and whether it has `RANGE=THISANDFUTURE`.
pub(crate) fn read_recurrence_id(
    c: &Component,
    start: Option<&PimTime>,
    ctx: &Ctx<'_>,
) -> Option<(PimTime, bool)> {
    let p = c.property("RECURRENCE-ID")?;
    let rid = ctx.time(p)?;
    let rid = match start {
        Some(start) => time::to_frame(&rid, start).map(|wall| time::like(wall, start))?,
        None => rid,
    };
    Some((
        rid,
        p.param("RANGE")
            .is_some_and(|r| r.eq_ignore_ascii_case("THISANDFUTURE")),
    ))
}

// ---------------------------------------------------------------------------
// Whole calendars

fn calendar_year(calendar: &Component) -> i32 {
    calendar
        .components
        .iter()
        .filter_map(|c| c.property("DTSTART"))
        .filter_map(|p| time::parse_ical_value(&p.value))
        .map(|v| match v {
            IcalValue::Date(d) => d.year(),
            IcalValue::DateTime { local, .. } => local.year(),
        })
        .next()
        .unwrap_or(2026)
}

/// TZIDs used by a component and its children.
pub(crate) fn tzids(component: &Component, out: &mut Vec<String>) {
    for p in &component.properties {
        for tzid in p.param_values("TZID") {
            if !out.iter().any(|t| t == tzid) {
                out.push(tzid.to_owned());
            }
        }
    }
    for child in &component.components {
        tzids(child, out);
    }
}

/// A calendar-level wrapper with the original calendar's
/// `VERSION` / `PRODID` / `CALSCALE` and the given components.
fn wrap(calendar: &Component, components: Vec<Component>) -> String {
    let mut wrapper = calendar.shell(|p| p.is("VERSION") || p.is("PRODID") || p.is("CALSCALE"));
    let mut used = Vec::new();
    for c in &components {
        tzids(c, &mut used);
    }
    let mut zones: Vec<Component> = calendar
        .components_named("VTIMEZONE")
        .filter(|z| z.text("TZID").is_some_and(|id| used.contains(&id)))
        .cloned()
        .collect();
    zones.extend(components);
    wrapper.components = zones;
    wrapper.to_text()
}

fn derived_uid(component: &Component) -> String {
    format!("nail-{}", &sha256_hex(component.to_text().as_bytes())[..32])
}

fn duration_minutes(text: &str) -> Option<u32> {
    let seconds = time::parse_duration(text)?;
    u32::try_from(seconds / 60).ok().filter(|m| *m > 0)
}

/// Reads iCalendar text (one or more `VCALENDAR`s).
pub fn parse_calendar(text: &str, options: ParseOptions) -> ParsedCalendar {
    let doc = content::parse(text);
    let mut out = ParsedCalendar::default();
    let calendars: Vec<&Component> = doc
        .components
        .iter()
        .filter(|c| c.is("VCALENDAR"))
        .collect();
    if calendars.is_empty() {
        out.issues
            .push(Issue::new(IssueKind::NotICalendar, None).skipped());
        return out;
    }
    for issue in &doc.issues {
        out.issues.push(match issue.kind {
            content::ContentIssueKind::Unclosed => Issue::new(
                IssueKind::Unclosed {
                    component: "VCALENDAR".into(),
                },
                Some(issue.line),
            ),
            _ => Issue::new(IssueKind::MalformedLine, Some(issue.line)),
        });
    }
    // A whole document that is exactly one item keeps its text verbatim.
    let single = doc.components.len() == 1;
    for calendar in calendars {
        read_one_calendar(calendar, options, single, text, &mut out);
    }
    // Unclosed components: name them where possible.
    for c in doc.components.iter().flat_map(|c| c.components.iter()) {
        if !c.is_original() && c.line > 0 {
            for issue in &mut out.issues {
                if issue.line == Some(c.line) && matches!(issue.kind, IssueKind::Unclosed { .. }) {
                    issue.kind = IssueKind::Unclosed {
                        component: c.name.to_ascii_uppercase(),
                    };
                }
            }
        }
    }
    if out.events.is_empty() && out.tasks.is_empty() {
        out.issues.push(Issue::new(IssueKind::NoItems, None));
    }
    out
}

fn read_one_calendar(
    calendar: &Component,
    options: ParseOptions,
    single: bool,
    text: &str,
    out: &mut ParsedCalendar,
) {
    if out.name.is_none() {
        out.name = read_text(calendar, "X-WR-CALNAME").or_else(|| read_text(calendar, "NAME"));
    }
    if out.description.is_none() {
        out.description =
            read_text(calendar, "X-WR-CALDESC").or_else(|| read_text(calendar, "DESCRIPTION"));
    }
    if out.color.is_none() {
        out.color =
            read_text(calendar, "COLOR").or_else(|| read_text(calendar, "X-APPLE-CALENDAR-COLOR"));
    }
    let calendar_zone = calendar
        .text("X-WR-TIMEZONE")
        .and_then(|z| crate::tz::iana_for_name(&z));
    if out.time_zone.is_none() {
        out.time_zone = calendar_zone.map(|z| z.name().to_owned());
    }
    if out.refresh_minutes.is_none() {
        out.refresh_minutes = calendar
            .property("REFRESH-INTERVAL")
            .or_else(|| calendar.property("X-PUBLISHED-TTL"))
            .and_then(|p| duration_minutes(&p.value));
    }
    if out.method.is_none() {
        out.method = read_text(calendar, "METHOD").map(|m| m.to_ascii_uppercase());
    }
    let floating_zone = options.floating_zone.or(if options.use_calendar_zone {
        calendar_zone
    } else {
        None
    });
    let resolver = ZoneResolver::new(calendar, calendar_year(calendar));
    let ctx = Ctx::new(&resolver, floating_zone);

    // Group components by kind and UID, in file order.
    let mut groups: BTreeMap<(usize, String), Vec<&Component>> = BTreeMap::new();
    let mut order: Vec<(bool, String)> = Vec::new();
    for (index, c) in calendar.components.iter().enumerate() {
        let is_event = c.is("VEVENT");
        if !(is_event || c.is("VTODO")) {
            continue;
        }
        let uid = match read_text(c, "UID") {
            Some(uid) => uid,
            None => {
                let uid = derived_uid(c);
                out.issues
                    .push(Issue::new(IssueKind::MissingUid, Some(c.line)).item(c.text("SUMMARY")));
                uid
            }
        };
        let key = (usize::from(!is_event), uid.clone());
        if !groups.contains_key(&key) {
            order.push((is_event, uid));
        }
        let _ = index;
        groups.entry(key).or_default().push(c);
    }
    let all_items: usize = groups.len();
    for (is_event, uid) in order {
        let components = groups
            .remove(&(usize::from(!is_event), uid.clone()))
            .unwrap_or_default();
        let raw = if single && all_items == 1 {
            text.to_owned()
        } else {
            wrap(calendar, components.iter().map(|c| (*c).clone()).collect())
        };
        let before = ctx.issues.borrow().len();
        if is_event {
            if let Some(event) = read_event_group(&uid, &components, raw, &ctx, &mut out.issues) {
                out.events.retain(|e| e.uid != event.uid);
                out.events.push(event);
            }
        } else if let Some(task) = read_task_group(&uid, &components, raw, &ctx, &mut out.issues) {
            out.tasks.retain(|t| t.uid != task.uid);
            out.tasks.push(task);
        }
        // Name the item in issues its fields raised.
        let title = components
            .iter()
            .find_map(|c| c.text("SUMMARY"))
            .or(Some(uid));
        for issue in ctx.issues.borrow_mut().iter_mut().skip(before) {
            if issue.item.is_none() {
                issue.item = title.clone().filter(|t| !t.trim().is_empty());
            }
        }
    }
    out.issues.extend(ctx.issues.into_inner());
}

fn read_event_group(
    uid: &str,
    components: &[&Component],
    raw: String,
    ctx: &Ctx<'_>,
    issues: &mut Vec<Issue>,
) -> Option<ParsedEvent> {
    let masters: Vec<&&Component> = components
        .iter()
        .filter(|c| c.property("RECURRENCE-ID").is_none())
        .collect();
    if masters.len() > 1 {
        issues.push(
            Issue::new(IssueKind::DuplicateUid, Some(masters[1].line))
                .item(masters[1].text("SUMMARY")),
        );
    }
    let master = masters.last().copied().copied();
    let Some(master) = master else {
        // Only overrides: the first becomes a single event.
        let first = components.first()?;
        issues.push(
            Issue::new(IssueKind::OrphanOverride, Some(first.line)).item(first.text("SUMMARY")),
        );
        let Some((fields, inline_data)) = read_event_fields(first, ctx) else {
            issues.push(
                Issue::new(IssueKind::MissingStart, Some(first.line))
                    .item(first.text("SUMMARY"))
                    .skipped(),
            );
            return None;
        };
        return Some(ParsedEvent {
            uid: uid.to_owned(),
            fields,
            recurrence: None,
            overrides: Vec::new(),
            this_and_future: Vec::new(),
            sequence: read_sequence(first),
            created_at: read_utc_instant(first, "CREATED"),
            updated_at: read_utc_instant(first, "LAST-MODIFIED"),
            related_to: Vec::new(),
            inline_data,
            raw,
            line: first.line,
        });
    };
    let Some((fields, mut inline_data)) = read_event_fields(master, ctx) else {
        issues.push(
            Issue::new(IssueKind::MissingStart, Some(master.line))
                .item(master.text("SUMMARY"))
                .skipped(),
        );
        return None;
    };
    let recurrence = read_recurrence(master, &fields.start, ctx);
    let mut overrides: Vec<EventOverride> = Vec::new();
    let mut this_and_future = Vec::new();
    for c in components
        .iter()
        .filter(|c| c.property("RECURRENCE-ID").is_some())
    {
        let Some((rid, future)) = read_recurrence_id(c, Some(&fields.start), ctx) else {
            continue;
        };
        let Some((override_fields, data)) = read_event_fields(c, ctx).or_else(|| {
            // An override without DTSTART starts at its recurrence id.
            let mut fixed = (*c).clone();
            fixed
                .properties
                .push(c.property("RECURRENCE-ID").cloned().map(|mut p| {
                    p.name = "DTSTART".into();
                    p.remove_param("RANGE");
                    p
                })?);
            read_event_fields(&fixed, ctx)
        }) else {
            continue;
        };
        inline_data.extend(data);
        if future {
            this_and_future.push(rid.clone());
        }
        overrides.retain(|o| o.recurrence_id != rid);
        overrides.push(EventOverride {
            recurrence_id: rid,
            fields: override_fields,
            this_and_future: future,
        });
    }
    let mut seen = HashSet::new();
    inline_data.retain(|d| seen.insert(d.sha256.clone()));
    Some(ParsedEvent {
        uid: uid.to_owned(),
        fields,
        recurrence,
        overrides,
        this_and_future,
        sequence: read_sequence(master),
        created_at: read_utc_instant(master, "CREATED"),
        updated_at: read_utc_instant(master, "LAST-MODIFIED"),
        related_to: master
            .properties_named("RELATED-TO")
            .map(Property::text)
            .filter(|t| !t.is_empty())
            .collect(),
        inline_data,
        raw,
        line: master.line,
    })
}

fn read_task_group(
    uid: &str,
    components: &[&Component],
    raw: String,
    ctx: &Ctx<'_>,
    issues: &mut Vec<Issue>,
) -> Option<ParsedTask> {
    let masters: Vec<&&Component> = components
        .iter()
        .filter(|c| c.property("RECURRENCE-ID").is_none())
        .collect();
    if masters.len() > 1 {
        issues.push(
            Issue::new(IssueKind::DuplicateUid, Some(masters[1].line))
                .item(masters[1].text("SUMMARY")),
        );
    }
    let master = masters
        .last()
        .copied()
        .copied()
        .or_else(|| components.first().copied())?;
    let fields = read_task_fields(master, ctx);
    let anchor = fields.start.clone().or_else(|| fields.due.clone());
    let recurrence = anchor
        .as_ref()
        .and_then(|anchor| read_recurrence(master, anchor, ctx));
    Some(ParsedTask {
        uid: uid.to_owned(),
        parent_uid: read_parent_uid(master),
        fields,
        recurrence,
        sequence: read_sequence(master),
        created_at: read_utc_instant(master, "CREATED"),
        updated_at: read_utc_instant(master, "LAST-MODIFIED"),
        raw,
        line: master.line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n{body}END:VCALENDAR\r\n")
    }

    fn one(body: &str) -> ParsedEvent {
        let parsed = parse_calendar(&cal(body), ParseOptions::default());
        assert_eq!(parsed.events.len(), 1, "{:?}", parsed.issues);
        parsed.events.into_iter().next().unwrap()
    }

    #[test]
    fn reads_a_full_event() {
        let event = one("BEGIN:VEVENT\r\nUID:full@x\r\nDTSTAMP:20261001T000000Z\r\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\r\nDTEND;TZID=Europe/Berlin:20261005T100000\r\n\
SUMMARY:Planning\\, Q4\r\nLOCATION:Room 1\r\nDESCRIPTION:Line 1\\nJoin: https://meet.google.com/abc-defg-hij\r\n\
STATUS:TENTATIVE\r\nTRANSP:TRANSPARENT\r\nCLASS:PRIVATE\r\nPRIORITY:1\r\nCATEGORIES:Work,Q4\r\nCATEGORIES:Team\r\n\
COLOR:teal\r\nURL:https://example.org/plan\r\nORGANIZER;CN=Ada:mailto:ada@example.org\r\n\
ATTENDEE;CN=\"Bob, B.\";ROLE=OPT-PARTICIPANT;PARTSTAT=ACCEPTED;RSVP=TRUE:MAILTO:bob@example.org\r\n\
ATTENDEE;CUTYPE=ROOM;DELEGATED-FROM=\"mailto:c@example.org\":mailto:room@example.org\r\n\
CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=Join:https://zoom.us/j/1\r\n\
ATTACH;FMTTYPE=application/pdf;FILENAME=a.pdf:https://example.org/a.pdf\r\n\
ATTACH;ENCODING=BASE64;VALUE=BINARY;FMTTYPE=text/plain;X-FILENAME=n.txt:aGVsbG8=\r\n\
ATTACH;FMTTYPE=message/rfc822;X-NAIL-SUBJECT=Hi:mid:abc%40example.org\r\n\
SEQUENCE:3\r\nCREATED:20260901T100000Z\r\nLAST-MODIFIED:20260902T100000Z\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Soon\r\nUID:al-1\r\nEND:VALARM\r\n\
BEGIN:VALARM\r\nACTION:AUDIO\r\nTRIGGER;RELATED=END:PT0S\r\nREPEAT:2\r\nDURATION:PT5M\r\nACKNOWLEDGED:20261005T080000Z\r\nEND:VALARM\r\n\
BEGIN:VALARM\r\nACTION:NONE\r\nTRIGGER;VALUE=DATE-TIME:19760401T005545Z\r\nEND:VALARM\r\n\
BEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER;VALUE=DATE-TIME:20261004T120000Z\r\nEND:VALARM\r\n\
END:VEVENT\r\n");
        let f = &event.fields;
        assert_eq!(f.title, "Planning, Q4");
        assert_eq!(
            f.start,
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            f.end,
            PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin")
        );
        assert_eq!(
            f.description.as_deref(),
            Some("Line 1\nJoin: https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(f.status, EventStatus::Tentative);
        assert_eq!(f.transparency, Transparency::Free);
        assert_eq!(f.visibility, Visibility::Private);
        assert_eq!(f.priority, Some(1));
        assert_eq!(f.categories, ["Work", "Q4", "Team"]);
        assert_eq!(f.color.as_deref(), Some("teal"));
        assert_eq!(f.organizer.as_ref().unwrap().name.as_deref(), Some("Ada"));
        assert_eq!(f.attendees.len(), 2);
        assert_eq!(f.attendees[0].email, "bob@example.org");
        assert_eq!(f.attendees[0].name.as_deref(), Some("Bob, B."));
        assert_eq!(f.attendees[0].role, AttendeeRole::Optional);
        assert_eq!(f.attendees[0].status, ParticipationStatus::Accepted);
        assert!(f.attendees[0].rsvp);
        assert_eq!(f.attendees[1].kind, AttendeeKind::Room);
        assert_eq!(f.attendees[1].delegated_from, ["c@example.org"]);
        assert_eq!(f.conferences.len(), 2);
        assert_eq!(f.conferences[0].features, ["audio", "video"]);
        assert_eq!(f.conferences[0].label.as_deref(), Some("Join"));
        assert_eq!(f.conferences[1].uri, "https://meet.google.com/abc-defg-hij");
        assert_eq!(f.attachments.len(), 2);
        assert_eq!(
            f.attachments[0].url.as_deref(),
            Some("https://example.org/a.pdf")
        );
        assert_eq!(f.attachments[1].size, Some(5));
        assert_eq!(f.attachments[1].name.as_deref(), Some("n.txt"));
        assert_eq!(event.inline_data[0].bytes, b"hello");
        assert_eq!(
            event.inline_data[0].sha256,
            f.attachments[1].sha256.clone().unwrap()
        );
        assert_eq!(
            f.links,
            [PimLink::Message {
                message_id: None,
                rfc822_id: Some("abc@example.org".into()),
                subject: Some("Hi".into())
            }]
        );
        assert_eq!(f.alarms.len(), 3, "ACTION:NONE is not a reminder");
        assert_eq!(f.alarms[0].uid.as_deref(), Some("al-1"));
        assert_eq!(
            f.alarms[1].trigger,
            AlarmTrigger::Relative {
                offset_secs: 0,
                related: AlarmRelated::End
            }
        );
        assert_eq!(
            f.alarms[1].repeat,
            Some(AlarmRepeat {
                count: 2,
                interval_secs: 300
            })
        );
        assert_eq!(
            f.alarms[1].acknowledged.as_deref(),
            Some("2026-10-05T08:00:00Z")
        );
        assert_eq!(
            f.alarms[2].trigger,
            AlarmTrigger::Absolute {
                at: "2026-10-04T12:00:00Z".into()
            }
        );
        assert!(f.issues().is_empty(), "{:?}", f.issues());
        assert_eq!(event.sequence, 3);
        assert_eq!(event.created_at.as_deref(), Some("2026-09-01T10:00:00Z"));
        assert_eq!(event.updated_at.as_deref(), Some("2026-09-02T10:00:00Z"));
        // A single-item document keeps its text exactly.
        assert!(event.raw.contains("ACTION:NONE"));
    }

    #[test]
    fn durations_defaults_and_bad_ends() {
        let e = one("BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261003\r\nEND:VEVENT\r\n");
        assert_eq!(e.fields.end, PimTime::date("2026-10-04"));
        let e = one(
            "BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261003T100000Z\r\nDURATION:PT90M\r\nEND:VEVENT\r\n",
        );
        assert_eq!(e.fields.end, PimTime::utc("2026-10-03T11:30:00"));
        let e = one(
            "BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261003\r\nDURATION:P2D\r\nEND:VEVENT\r\n",
        );
        assert_eq!(e.fields.end, PimTime::date("2026-10-05"));
        let e = one("BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261003T100000\r\nEND:VEVENT\r\n");
        assert_eq!(e.fields.end, PimTime::floating("2026-10-03T10:00:00"));
        // End before start: becomes the start.
        let e = one(
            "BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261003T100000Z\r\nDTEND:20261003T090000Z\r\nEND:VEVENT\r\n",
        );
        assert_eq!(e.fields.end, e.fields.start);
        // Mixed: a date start with a date-time end keeps the end's day.
        let e = one(
            "BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261003\r\nDTEND:20261005T000000\r\nEND:VEVENT\r\n",
        );
        assert_eq!(e.fields.end, PimTime::date("2026-10-05"));
        assert!(e.fields.issues().is_empty());
    }

    #[test]
    fn overrides_group_by_uid_and_frame() {
        let parsed = parse_calendar(
            &cal(
                "BEGIN:VEVENT\r\nUID:s\r\nDTSTART;TZID=Europe/Berlin:20261005T090000\r\nDTEND;TZID=Europe/Berlin:20261005T091500\r\n\
RRULE:FREQ=DAILY;COUNT=5\r\nEXDATE;TZID=Europe/Berlin:20261006T090000,20261007T090000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20261008T070000Z\r\nDTSTART;TZID=Europe/Berlin:20261008T100000\r\nDTEND;TZID=Europe/Berlin:20261008T101500\r\nSUMMARY:Standup (late)\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/Berlin:20261009T090000\r\nDTSTART;TZID=Europe/Berlin:20261009T083000\r\nDTEND;TZID=Europe/Berlin:20261009T084500\r\nSUMMARY:Standup (early)\r\nEND:VEVENT\r\n\
BEGIN:VTODO\r\nUID:s\r\nSUMMARY:Same UID, other kind\r\nEND:VTODO\r\n",
            ),
            ParseOptions::default(),
        );
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let e = &parsed.events[0];
        let r = e.recurrence.as_ref().unwrap();
        assert_eq!(r.rules.len(), 1);
        assert_eq!(r.exceptions.len(), 2);
        assert_eq!(e.overrides.len(), 2);
        assert_eq!(
            e.overrides[0].recurrence_id,
            PimTime::zoned("2026-10-08T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            e.this_and_future,
            [PimTime::zoned("2026-10-09T09:00:00", "Europe/Berlin")]
        );
        assert_eq!(parsed.tasks.len(), 1);
        assert!(!e.raw.contains("VTODO"));
        assert!(e.raw.contains("RANGE=THISANDFUTURE"));
    }

    #[test]
    fn orphan_overrides_missing_uids_and_starts() {
        let parsed = parse_calendar(
            &cal(
                "BEGIN:VEVENT\r\nUID:o\r\nRECURRENCE-ID:20261008T070000Z\r\nDTSTART:20261008T080000Z\r\nSUMMARY:Only me\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:No uid\r\nDTSTART:20261008T080000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:nostart\r\nSUMMARY:No start\r\nEND:VEVENT\r\n",
            ),
            ParseOptions::default(),
        );
        assert_eq!(parsed.events.len(), 2);
        let kinds: Vec<_> = parsed.issues.iter().map(|i| (&i.kind, i.skipped)).collect();
        assert_eq!(
            kinds,
            [
                (&IssueKind::MissingUid, false),
                (&IssueKind::OrphanOverride, false),
                (&IssueKind::MissingStart, true)
            ]
        );
        assert!(parsed.events[1].uid.starts_with("nail-"));
        // Derived UIDs are stable.
        let again = parse_calendar(
            &cal("BEGIN:VEVENT\r\nSUMMARY:No uid\r\nDTSTART:20261008T080000Z\r\nEND:VEVENT\r\n"),
            ParseOptions::default(),
        );
        assert_eq!(again.events[0].uid, parsed.events[1].uid);
    }

    #[test]
    fn zones_floating_and_feeds() {
        let text = cal(
            "X-WR-CALNAME:Holidays\r\nX-WR-TIMEZONE:Europe/Paris\r\nX-PUBLISHED-TTL:PT1H\r\nCOLOR:red\r\n\
BEGIN:VEVENT\r\nUID:f\r\nDTSTART:20261005T090000\r\nSUMMARY:Floating\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:z\r\nDTSTART;TZID=Nowhere/Land:20261005T090000\r\nSUMMARY:Unknown zone\r\nEND:VEVENT\r\n",
        );
        let plain = parse_calendar(&text, ParseOptions::default());
        assert_eq!(plain.name.as_deref(), Some("Holidays"));
        assert_eq!(plain.time_zone.as_deref(), Some("Europe/Paris"));
        assert_eq!(plain.refresh_minutes, Some(60));
        assert_eq!(plain.color.as_deref(), Some("red"));
        assert_eq!(
            plain.events[0].fields.start,
            PimTime::floating("2026-10-05T09:00:00")
        );
        assert_eq!(
            plain.events[1].fields.start,
            PimTime::floating("2026-10-05T09:00:00")
        );
        assert_eq!(
            plain.issues,
            [Issue::new(
                IssueKind::UnknownTimeZone {
                    tzid: "Nowhere/Land".into()
                },
                Some(15)
            )
            .item(Some("Unknown zone".into()))]
        );
        let feed = parse_calendar(
            &text,
            ParseOptions {
                use_calendar_zone: true,
                ..ParseOptions::default()
            },
        );
        assert_eq!(
            feed.events[0].fields.start,
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Paris")
        );
    }

    #[test]
    fn tasks() {
        let parsed = parse_calendar(
            &cal(
                "BEGIN:VTODO\r\nUID:t1\r\nSUMMARY:Child\r\nDUE;VALUE=DATE:20261010\r\nSTATUS:IN-PROCESS\r\nPERCENT-COMPLETE:40\r\n\
PRIORITY:5\r\nRELATED-TO:t0\r\nX-APPLE-SORT-ORDER:12\r\nRRULE:FREQ=WEEKLY\r\nCOMPLETED:20261001T120000Z\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER;RELATED=END:-PT1H\r\nEND:VALARM\r\nEND:VTODO\r\n\
BEGIN:VTODO\r\nUID:t2\r\nSUMMARY:Sibling of\r\nRELATED-TO;RELTYPE=SIBLING:t1\r\nDTSTART:20261001T100000Z\r\nDURATION:P1D\r\nEND:VTODO\r\n",
            ),
            ParseOptions::default(),
        );
        let t = &parsed.tasks[0];
        assert_eq!(t.parent_uid.as_deref(), Some("t0"));
        assert_eq!(t.fields.due, Some(PimTime::date("2026-10-10")));
        assert_eq!(t.fields.status, TaskStatus::InProcess);
        assert_eq!(t.fields.percent_complete, Some(40));
        assert_eq!(t.fields.order, Some(12));
        assert_eq!(
            t.fields.completed_at.as_deref(),
            Some("2026-10-01T12:00:00Z")
        );
        assert!(t.recurrence.is_some());
        assert_eq!(t.fields.alarms.len(), 1);
        let t2 = &parsed.tasks[1];
        assert_eq!(t2.parent_uid, None);
        assert_eq!(t2.fields.due, Some(PimTime::utc("2026-10-02T10:00:00")));
        let task = t.clone().into_task(
            "t".into(),
            "c".into(),
            Some("p".into()),
            vec![],
            false,
            PimItemSync::Synced,
        );
        assert_eq!(
            task.fields.parent_id.as_ref().map(|p| p.as_str()),
            Some("p")
        );
    }

    #[test]
    fn not_icalendar() {
        let parsed = parse_calendar(
            "BEGIN:VCARD\r\nFN:x\r\nEND:VCARD\r\n",
            ParseOptions::default(),
        );
        assert_eq!(parsed.issues[0].kind, IssueKind::NotICalendar);
        let empty = parse_calendar(&cal(""), ParseOptions::default());
        assert_eq!(empty.issues[0].kind, IssueKind::NoItems);
    }

    #[test]
    fn conference_detection() {
        assert_eq!(
            conference_links_in_text(
                "Join https://us02web.zoom.us/j/123?pwd=x. or (https://teams.microsoft.com/l/meetup-join/1) and https://example.org/x"
            ),
            [
                "https://us02web.zoom.us/j/123?pwd=x",
                "https://teams.microsoft.com/l/meetup-join/1"
            ]
        );
        assert!(conference_links_in_text("https://notzoom.us.evil.example/").is_empty());
        assert_eq!(percent_decode(&percent_encode("a b%<c>@x")), "a b%<c>@x");
        assert_eq!(percent_encode("abc@x.org"), "abc@x.org");
    }
}
