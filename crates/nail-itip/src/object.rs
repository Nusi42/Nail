//! Reading iTIP calendar data (RFC 5546): the method, the scheduling
//! properties `nail-ical`'s event model leaves out (`DTSTAMP`, `COMMENT`,
//! `RECURRENCE-ID` of lone occurrences, `SENT-BY`, `SCHEDULE-AGENT`) and
//! the problems of the message itself.
//!
//! An iTIP object is one `VCALENDAR` with a `METHOD` and the components
//! of **one** `UID` (RFC 5546 §1.4): the master and / or overridden
//! occurrences. Event fields are read by `nail-ical` (zones, recurrence,
//! attendees exactly as the calendar stores them), one component at a
//! time, so a `REPLY` for a single occurrence keeps its `RECURRENCE-ID`.

use std::collections::HashSet;

use nail_api::pim::PimTime;
use nail_api::pim::calendar::{Attendee, EventFields, Organizer};
use nail_api::scheduling::{ItipIssue, ItipMethod};
use nail_ical::attendee;
pub use nail_ical::attendee::partstat;
use nail_ical::content::{self, Component, Property};
use nail_ical::time::{self};
use nail_ical::{ParseOptions, ParsedEvent, parse_calendar};

use crate::address::{normalize, same};

/// Largest iTIP calendar text read (larger data is [`ItipIssue::Unparsable`]).
pub const MAX_ITIP_BYTES: usize = 4 * 1024 * 1024;

/// Most components of the event read (a master plus overrides); further
/// occurrences are ignored.
pub const MAX_COMPONENTS: usize = 256;

/// `SCHEDULE-AGENT` of an `ORGANIZER` / `ATTENDEE` (RFC 6638 §7.1).
pub use nail_api::scheduling::ScheduleAgent;

/// The agent named by a property's `SCHEDULE-AGENT` parameter.
pub fn schedule_agent_of(property: &Property) -> ScheduleAgent {
    ScheduleAgent::parse(property.param("SCHEDULE-AGENT"))
}

/// An `ATTENDEE` with the scheduling parameters the event model drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItipAttendee {
    /// The attendee as `nail-ical` reads it.
    pub attendee: Attendee,
    /// `SENT-BY` (someone who acts for the attendee).
    pub sent_by: Option<String>,
    /// `SCHEDULE-AGENT`.
    pub schedule_agent: ScheduleAgent,
}

/// One `VEVENT` of an iTIP object (the master or one occurrence).
#[derive(Debug, Clone)]
pub struct ItipComponent {
    /// `UID`.
    pub uid: String,
    /// `RECURRENCE-ID`: the occurrence it is about; `None` = the master.
    pub recurrence_id: Option<PimTime>,
    /// `RANGE=THISANDFUTURE` on the recurrence id.
    pub this_and_future: bool,
    /// `SEQUENCE` (0 when absent).
    pub sequence: u32,
    /// `DTSTAMP` (RFC 3339 UTC).
    pub dtstamp: Option<String>,
    /// `ORGANIZER`.
    pub organizer: Option<Organizer>,
    /// `SENT-BY` of the organizer.
    pub organizer_sent_by: Option<String>,
    /// `SCHEDULE-AGENT` of the organizer.
    pub organizer_agent: ScheduleAgent,
    /// `ATTENDEE`s.
    pub attendees: Vec<ItipAttendee>,
    /// `COMMENT`s (joined by line breaks).
    pub comment: Option<String>,
    /// The event fields, when the component has a readable `DTSTART`.
    pub fields: Option<EventFields>,
    /// `X-MICROSOFT-DISALLOW-COUNTER:TRUE` (the organizer does not take
    /// proposals).
    pub disallow_counter: bool,
    /// The component as received.
    pub component: Component,
}

impl ItipComponent {
    /// The attendees as the contract's type.
    pub fn plain_attendees(&self) -> Vec<Attendee> {
        self.attendees.iter().map(|a| a.attendee.clone()).collect()
    }

    /// The attendee with this address.
    pub fn attendee(&self, address: &str) -> Option<&ItipAttendee> {
        self.attendees
            .iter()
            .find(|a| same(&a.attendee.email, address))
    }

    /// Whether `sender` may act for the organizer (the organizer or its
    /// `SENT-BY`).
    pub fn sent_by_organizer(&self, sender: &str) -> bool {
        self.organizer
            .as_ref()
            .is_some_and(|o| same(&o.email, sender))
            || self
                .organizer_sent_by
                .as_deref()
                .is_some_and(|s| same(s, sender))
    }

    /// The scheduling version of the component.
    pub fn version(&self) -> crate::order::Version {
        crate::order::Version::new(self.sequence, self.dtstamp.clone())
    }
}

/// A parsed iTIP object.
#[derive(Debug, Clone)]
pub struct ItipObject {
    /// The method (`METHOD` property, else the MIME `method` parameter).
    pub method: Option<ItipMethod>,
    /// The `UID` of the event (the first one when there are several).
    pub uid: Option<String>,
    /// The components of that `UID`, master first, then occurrences in
    /// file order.
    pub components: Vec<ItipComponent>,
    /// The event series as `nail-ical` reads it (master, recurrence,
    /// overrides), when the message carries event data.
    pub event: Option<ParsedEvent>,
    /// Problems of the message itself (sender / user checks come with
    /// [`crate::card`]).
    pub issues: Vec<ItipIssue>,
    /// The calendar text as received.
    pub text: String,
    calendar: Option<Component>,
}

impl ItipObject {
    /// The master component, if the message has one.
    pub fn master(&self) -> Option<&ItipComponent> {
        self.components.iter().find(|c| c.recurrence_id.is_none())
    }

    /// The component the card describes: the master, else the first
    /// occurrence.
    pub fn main(&self) -> Option<&ItipComponent> {
        self.master().or_else(|| self.components.first())
    }

    /// The component for an occurrence (`None` = the master).
    pub fn component(&self, recurrence_id: Option<&PimTime>) -> Option<&ItipComponent> {
        self.components
            .iter()
            .find(|c| c.recurrence_id.as_ref() == recurrence_id)
    }

    /// The occurrence components.
    pub fn occurrences(&self) -> impl Iterator<Item = &ItipComponent> {
        self.components.iter().filter(|c| c.recurrence_id.is_some())
    }

    /// The organizer (from the main component).
    pub fn organizer(&self) -> Option<&Organizer> {
        self.main().and_then(|c| c.organizer.as_ref())
    }

    /// Whether a fatal problem makes the object unusable.
    pub fn is_invalid(&self) -> bool {
        self.method.is_none() || self.issues.iter().any(|i| i.is_fatal())
    }

    /// The highest version among the components.
    pub fn version(&self) -> crate::order::Version {
        self.components
            .iter()
            .map(ItipComponent::version)
            .max()
            .unwrap_or_default()
    }

    /// The `VCALENDAR` as received (`None` when unparsable).
    pub fn calendar(&self) -> Option<&Component> {
        self.calendar.as_ref()
    }

    /// The text to store in a calendar: the `VCALENDAR` without `METHOD`
    /// (RFC 6638 §3.2: calendar object resources carry none), with the
    /// event's components and the `VTIMEZONE`s only. `None` without
    /// components.
    pub fn storable_text(&self) -> Option<String> {
        let calendar = self.calendar.as_ref()?;
        let uid = self.uid.as_deref()?;
        let mut out = calendar.shell(|p| !p.is("METHOD"));
        out.components = calendar
            .components
            .iter()
            .filter(|c| c.is("VTIMEZONE") || (c.is("VEVENT") && component_uid(c) == Some(uid)))
            .cloned()
            .collect();
        Some(out.to_text())
    }
}

fn component_uid(c: &Component) -> Option<&str> {
    c.property("UID")
        .map(|p| p.value.trim())
        .filter(|v| !v.is_empty())
}

/// Reads iTIP calendar text. `method_param` is the `method` parameter of
/// the MIME part; the `METHOD` property wins when both are present
/// (RFC 6047 §2.4 requires them to match; the data is what calendars
/// act on).
pub fn parse_itip(text: &str, method_param: Option<&str>) -> ItipObject {
    let mut object = ItipObject {
        method: None,
        uid: None,
        components: Vec::new(),
        event: None,
        issues: Vec::new(),
        text: text.to_owned(),
        calendar: None,
    };
    if text.len() > MAX_ITIP_BYTES {
        object.issues.push(ItipIssue::Unparsable);
        return object;
    }
    let document = content::parse(text);
    let Some(calendar) = document.components.into_iter().find(|c| c.is("VCALENDAR")) else {
        object.issues.push(ItipIssue::Unparsable);
        return object;
    };

    object.method = match calendar.property("METHOD") {
        Some(p) => ItipMethod::from_ical(&p.value),
        None => method_param.and_then(ItipMethod::from_ical),
    };
    if object.method.is_none() {
        object.issues.push(ItipIssue::UnknownMethod);
    }

    let events: Vec<&Component> = calendar.components_named("VEVENT").collect();
    let Some(first) = events.first() else {
        object.issues.push(ItipIssue::NoEvent);
        object.calendar = Some(calendar);
        return object;
    };
    let Some(uid) = component_uid(first).map(str::to_owned) else {
        object.issues.push(ItipIssue::MissingUid);
        object.calendar = Some(calendar);
        return object;
    };
    let uids: HashSet<&str> = events.iter().filter_map(|c| component_uid(c)).collect();
    if uids.len() > 1 {
        object.issues.push(ItipIssue::SeveralEvents);
    }

    let mine: Vec<&Component> = events
        .iter()
        .copied()
        .filter(|c| component_uid(c) == Some(uid.as_str()))
        .take(MAX_COMPONENTS)
        .collect();
    let zones: Vec<Component> = calendar.components_named("VTIMEZONE").cloned().collect();
    let mut components: Vec<ItipComponent> = mine
        .iter()
        .map(|c| read_component(c, &uid, &zones))
        .collect();
    // `ADD` components carry no `RECURRENCE-ID` (RFC 5546 §3.2.4): each
    // is one new occurrence, identified by its start.
    if object.method == Some(ItipMethod::Add) {
        for c in &mut components {
            if c.recurrence_id.is_none() {
                c.recurrence_id = c.fields.as_ref().map(|f| f.start.clone());
            }
        }
    }
    // Master first (a later master copy replaces an earlier one).
    components.sort_by_key(|c| c.recurrence_id.is_some());
    let mut seen = HashSet::new();
    components.retain(|c| seen.insert(c.recurrence_id.clone()));

    object.uid = Some(uid.clone());
    object.components = components;
    object.calendar = Some(calendar);
    if let Some(stored) = object.storable_text() {
        object.event = parse_calendar(&stored, ParseOptions::default())
            .events
            .into_iter()
            .find(|e| e.uid == uid)
            .filter(|_| object.components.iter().any(|c| c.fields.is_some()));
    }

    if let Some(method) = object.method {
        check_people(method, &mut object);
    }
    object
}

/// `ORGANIZER` / `ATTENDEE` rules per method (RFC 5546 §3.2).
fn check_people(method: ItipMethod, object: &mut ItipObject) {
    let needs_organizer = !matches!(method, ItipMethod::Publish);
    if needs_organizer && object.components.iter().any(|c| c.organizer.is_none()) {
        object.issues.push(ItipIssue::MissingOrganizer);
    }
    let attendee_ok = |c: &ItipComponent| match method {
        // Exactly one attendee; a delegation reply also lists the
        // delegates (RFC 5546 §4.2.5).
        ItipMethod::Reply | ItipMethod::Refresh => match c.attendees.split_first() {
            Some((first, rest)) => rest.iter().all(|d| {
                d.attendee
                    .delegated_from
                    .iter()
                    .any(|f| same(f, &first.attendee.email))
            }),
            None => false,
        },
        // A proposal names its proposer; Outlook lists everyone.
        ItipMethod::Counter => !c.attendees.is_empty(),
        _ => true,
    };
    if !object.components.iter().all(attendee_ok) {
        object.issues.push(ItipIssue::AttendeeCount);
    }
}

fn read_component(c: &Component, uid: &str, zones: &[Component]) -> ItipComponent {
    let organizer_property = c.property("ORGANIZER");
    let organizer = organizer_property.and_then(|p| {
        let email = normalize_uri(&p.value);
        (!email.is_empty()).then(|| Organizer {
            email,
            name: p
                .param("CN")
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty()),
        })
    });
    let comments: Vec<String> = c
        .properties_named("COMMENT")
        .map(Property::text)
        .filter(|t| !t.trim().is_empty())
        .collect();
    let fields = read_fields(c, zones);
    let attendees = read_attendees(c);
    let (recurrence_id, this_and_future) = match c.property("RECURRENCE-ID") {
        Some(p) => (
            read_time(p, zones),
            p.param("RANGE")
                .is_some_and(|r| r.trim().eq_ignore_ascii_case("THISANDFUTURE")),
        ),
        None => (None, false),
    };
    let flag = |name: &str| {
        c.property(name)
            .is_some_and(|p| p.value.trim().eq_ignore_ascii_case("TRUE"))
    };
    ItipComponent {
        uid: uid.to_owned(),
        recurrence_id,
        this_and_future,
        sequence: c
            .property("SEQUENCE")
            .and_then(|p| p.value.trim().parse::<u32>().ok())
            .unwrap_or(0),
        dtstamp: c.property("DTSTAMP").and_then(|p| utc_instant(&p.value)),
        organizer,
        organizer_sent_by: organizer_property
            .and_then(|p| p.param("SENT-BY"))
            .map(normalize_uri)
            .filter(|s| !s.is_empty()),
        organizer_agent: organizer_property
            .map(schedule_agent_of)
            .unwrap_or_default(),
        attendees,
        comment: (!comments.is_empty()).then(|| comments.join("\n")),
        fields,
        disallow_counter: flag("X-MICROSOFT-DISALLOW-COUNTER")
            || flag("X-MICROSOFT-CDO-DISALLOW-COUNTER"),
        component: c.clone(),
    }
}

/// A calendar user address as Nail compares it (`mailto:` stripped,
/// lowercase).
fn normalize_uri(value: &str) -> String {
    normalize(&content::unescape_text(value))
}

/// RFC 3339 UTC of a `DATE-TIME` (a value without `Z` is read as UTC, as
/// `DTSTAMP` must be UTC).
fn utc_instant(value: &str) -> Option<String> {
    time::value_instant(value)
}

/// `TZID`s used by a component's properties (alarms included).
fn used_tzids(c: &Component, out: &mut HashSet<String>) {
    for p in &c.properties {
        if let Some(tzid) = p.param("TZID") {
            out.insert(tzid.trim().to_owned());
        }
    }
    for sub in &c.components {
        used_tzids(sub, out);
    }
}

/// Wraps components into a calendar with the message's zones they use
/// (only those: re-reading every component with every zone of a large
/// message would multiply the work).
fn wrap(components: Vec<Component>, zones: &[Component]) -> String {
    let mut used = HashSet::new();
    for c in &components {
        used_tzids(c, &mut used);
    }
    let mut calendar = Component::new("VCALENDAR");
    calendar.properties.push(Property::new("VERSION", "2.0"));
    calendar
        .properties
        .push(Property::new("PRODID", nail_ical::PRODID));
    calendar.components.extend(
        zones
            .iter()
            .filter(|z| {
                z.property("TZID")
                    .is_some_and(|p| used.contains(p.text().trim()))
            })
            .cloned(),
    );
    calendar.components.extend(components);
    calendar.to_text()
}

/// The event fields of one component (read as a lone event, so an
/// occurrence keeps its own fields).
fn read_fields(c: &Component, zones: &[Component]) -> Option<EventFields> {
    c.property("DTSTART")?;
    let mut lone = c.clone();
    lone.properties.retain(|p| !p.is("RECURRENCE-ID"));
    let text = wrap(vec![lone], zones);
    parse_calendar(&text, ParseOptions::default())
        .events
        .into_iter()
        .next()
        .map(|e| e.fields)
}

/// A time property read with the message's zones.
fn read_time(p: &Property, zones: &[Component]) -> Option<PimTime> {
    let mut probe = Component::new("VEVENT");
    probe.properties.push(Property::new("UID", "probe"));
    let mut start = p.clone();
    start.name = "DTSTART".into();
    start.remove_param("RANGE");
    start.touch();
    probe.properties.push(start);
    parse_calendar(&wrap(vec![probe], zones), ParseOptions::default())
        .events
        .into_iter()
        .next()
        .map(|e| e.fields.start)
}

fn read_attendees(c: &Component) -> Vec<ItipAttendee> {
    c.properties_named("ATTENDEE")
        .filter_map(|p| {
            let attendee = read_attendee(p)?;
            Some(ItipAttendee {
                attendee,
                sent_by: p
                    .param("SENT-BY")
                    .map(normalize_uri)
                    .filter(|s| !s.is_empty()),
                schedule_agent: schedule_agent_of(p),
            })
        })
        .collect()
}

/// One `ATTENDEE` (the same reading as `nail-ical`'s, with addresses
/// normalized for comparison).
pub(crate) fn read_attendee(p: &Property) -> Option<Attendee> {
    let email = normalize_uri(&p.value);
    if email.is_empty() {
        return None;
    }
    let name = p
        .param("CN")
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty());
    Some(attendee::read(
        p,
        email,
        name,
        |param| p.param(param).map(|v| v.trim().to_ascii_uppercase()),
        |v| Some(normalize_uri(v)).filter(|s| !s.is_empty()),
    ))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use nail_api::pim::calendar::ParticipationStatus;
    use pretty_assertions::assert_eq;

    pub(crate) fn cal(method: &str, body: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n{method}{body}END:VCALENDAR\r\n"
        )
    }

    const EVENT: &str = "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261001T080000Z\r\n\
SEQUENCE:2\r\nDTSTART;TZID=Europe/Berlin:20261005T090000\r\nDTEND;TZID=Europe/Berlin:20261005T100000\r\n\
SUMMARY:Planning\r\nORGANIZER;CN=Ada;SENT-BY=\"mailto:assistant@example.org\":mailto:Ada@Example.org\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;SCHEDULE-AGENT=CLIENT:mailto:bob@example.org\r\n\
ATTENDEE;CN=Carol;PARTSTAT=ACCEPTED:mailto:carol@example.org\r\n\
COMMENT:Bring slides\r\nX-MICROSOFT-DISALLOW-COUNTER:TRUE\r\nEND:VEVENT\r\n";

    #[test]
    fn reads_a_request() {
        let object = parse_itip(&cal("METHOD:REQUEST\r\n", EVENT), None);
        assert_eq!(object.method, Some(ItipMethod::Request));
        assert!(object.issues.is_empty(), "{:?}", object.issues);
        assert_eq!(object.uid.as_deref(), Some("m1@example.org"));
        let main = object.main().unwrap();
        assert_eq!(main.sequence, 2);
        assert_eq!(main.dtstamp.as_deref(), Some("2026-10-01T08:00:00Z"));
        assert_eq!(main.organizer.as_ref().unwrap().email, "ada@example.org");
        assert_eq!(
            main.organizer.as_ref().unwrap().name.as_deref(),
            Some("Ada")
        );
        assert!(main.sent_by_organizer("assistant@example.org"));
        assert!(main.sent_by_organizer("ADA@example.org"));
        assert!(!main.sent_by_organizer("mallory@example.org"));
        assert_eq!(main.attendees.len(), 2);
        assert_eq!(main.attendees[0].schedule_agent, ScheduleAgent::Client);
        assert_eq!(main.attendees[1].schedule_agent, ScheduleAgent::Server);
        assert_eq!(
            main.attendee("carol@example.org").unwrap().attendee.status,
            ParticipationStatus::Accepted
        );
        assert!(main.attendees[0].attendee.rsvp);
        assert_eq!(main.comment.as_deref(), Some("Bring slides"));
        assert!(main.disallow_counter);
        let fields = main.fields.as_ref().unwrap();
        assert_eq!(fields.title, "Planning");
        assert_eq!(
            fields.start,
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert!(object.event.is_some());
        let stored = object.storable_text().unwrap();
        assert!(!stored.contains("METHOD"));
        assert!(stored.contains("UID:m1@example.org"));
        assert!(!object.is_invalid());
    }

    #[test]
    fn method_property_wins_over_the_parameter() {
        let object = parse_itip(&cal("METHOD:CANCEL\r\n", EVENT), Some("REQUEST"));
        assert_eq!(object.method, Some(ItipMethod::Cancel));
        let object = parse_itip(&cal("", EVENT), Some("request"));
        assert_eq!(object.method, Some(ItipMethod::Request));
        let object = parse_itip(&cal("", EVENT), None);
        assert_eq!(object.method, None);
        assert!(object.issues.contains(&ItipIssue::UnknownMethod));
        assert!(object.is_invalid());
        let object = parse_itip(&cal("METHOD:POLL\r\n", EVENT), Some("REQUEST"));
        assert_eq!(object.method, None);
    }

    #[test]
    fn reports_broken_messages() {
        let none = parse_itip("hello", Some("REQUEST"));
        assert_eq!(none.issues, [ItipIssue::Unparsable]);
        let todo = parse_itip(
            &cal(
                "METHOD:REQUEST\r\n",
                "BEGIN:VTODO\r\nUID:t\r\nEND:VTODO\r\n",
            ),
            None,
        );
        assert_eq!(todo.issues, [ItipIssue::NoEvent]);
        let no_uid = parse_itip(
            &cal(
                "METHOD:REQUEST\r\n",
                "BEGIN:VEVENT\r\nDTSTART:20261005T090000Z\r\nEND:VEVENT\r\n",
            ),
            None,
        );
        assert_eq!(no_uid.issues, [ItipIssue::MissingUid]);
        assert!(no_uid.is_invalid());
        let two = parse_itip(
            &cal(
                "METHOD:REQUEST\r\n",
                &format!(
                    "{EVENT}BEGIN:VEVENT\r\nUID:other\r\nDTSTART:20261005T090000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\nEND:VEVENT\r\n"
                ),
            ),
            None,
        );
        assert_eq!(two.issues, [ItipIssue::SeveralEvents]);
        assert_eq!(two.components.len(), 1);
        let no_org = parse_itip(
            &cal(
                "METHOD:REQUEST\r\n",
                "BEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261005T090000Z\r\nEND:VEVENT\r\n",
            ),
            None,
        );
        assert_eq!(no_org.issues, [ItipIssue::MissingOrganizer]);
        let publish = parse_itip(
            &cal(
                "METHOD:PUBLISH\r\n",
                "BEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261005T090000Z\r\nEND:VEVENT\r\n",
            ),
            None,
        );
        assert!(publish.issues.is_empty(), "{:?}", publish.issues);
        let huge = parse_itip(&"x".repeat(MAX_ITIP_BYTES + 1), None);
        assert_eq!(huge.issues, [ItipIssue::Unparsable]);
    }

    #[test]
    fn reply_attendee_rules() {
        let reply = |attendees: &str| {
            parse_itip(
                &cal(
                    "METHOD:REPLY\r\n",
                    &format!(
                        "BEGIN:VEVENT\r\nUID:m1\r\nDTSTAMP:20261001T090000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\n{attendees}END:VEVENT\r\n"
                    ),
                ),
                None,
            )
        };
        assert!(
            reply("ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\n")
                .issues
                .is_empty(),
            "{:?}",
            reply("ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\n").issues
        );
        assert_eq!(reply("").issues, [ItipIssue::AttendeeCount]);
        assert_eq!(
            reply("ATTENDEE:mailto:bob@example.org\r\nATTENDEE:mailto:carol@example.org\r\n")
                .issues,
            [ItipIssue::AttendeeCount]
        );
        // A delegation reply lists the delegate too (RFC 5546 §4.2.5).
        let delegation = reply(
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:dan@example.org\":mailto:bob@example.org\r\n\
ATTENDEE;DELEGATED-FROM=\"mailto:bob@example.org\":mailto:dan@example.org\r\n",
        );
        assert!(delegation.issues.is_empty(), "{:?}", delegation.issues);
        let main = delegation.main().unwrap();
        assert_eq!(main.attendees[0].attendee.delegated_to, ["dan@example.org"]);
        // A bare REPLY has no fields and no event.
        assert!(main.fields.is_none());
        assert!(delegation.event.is_none());
    }

    #[test]
    fn occurrences_keep_their_recurrence_ids() {
        let text = cal(
            "METHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\nUID:s1\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:1\r\n\
RECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\r\nDTSTART;TZID=Europe/Berlin:20261012T110000\r\n\
DTEND;TZID=Europe/Berlin:20261012T120000\r\nSUMMARY:Weekly (moved)\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s1\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:1\r\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\r\nDTEND;TZID=Europe/Berlin:20261005T100000\r\n\
RRULE:FREQ=WEEKLY;COUNT=5\r\nSUMMARY:Weekly\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s1\r\nDTSTAMP:20261001T080000Z\r\nRECURRENCE-ID;RANGE=THISANDFUTURE:20261019T070000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
        );
        let object = parse_itip(&text, None);
        assert!(object.issues.is_empty(), "{:?}", object.issues);
        assert_eq!(object.components.len(), 3);
        assert!(object.components[0].recurrence_id.is_none());
        assert_eq!(
            object.master().unwrap().fields.as_ref().unwrap().title,
            "Weekly"
        );
        let moved = object
            .component(Some(&PimTime::zoned(
                "2026-10-12T09:00:00",
                "Europe/Berlin",
            )))
            .unwrap();
        assert_eq!(moved.fields.as_ref().unwrap().title, "Weekly (moved)");
        assert_eq!(object.occurrences().count(), 2);
        let future = &object.components[2];
        assert!(future.this_and_future);
        assert_eq!(
            future.recurrence_id,
            Some(PimTime::utc("2026-10-19T07:00:00"))
        );
        assert!(future.fields.is_none());
        let event = object.event.as_ref().unwrap();
        assert!(event.recurrence.is_some());
        assert_eq!(event.overrides.len(), 2);
        assert_eq!(event.this_and_future.len(), 1);
    }

    #[test]
    fn large_messages_are_bounded() {
        // 300 occurrences and 50 unused zones: at most MAX_COMPONENTS are
        // read, each with only the zones it uses.
        let mut body = String::new();
        for i in 0..50 {
            write!(
                body,
                "BEGIN:VTIMEZONE\r\nTZID:Zone {i}\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\n\
TZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n"
            )
            .unwrap();
        }
        for day in 0..300 {
            let date =
                chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Duration::days(day);
            let stamp = date.format("%Y%m%d");
            write!(
                body,
                "BEGIN:VEVENT\r\nUID:big\r\nRECURRENCE-ID:{stamp}T090000Z\r\nDTSTART:{stamp}T100000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\nEND:VEVENT\r\n"
            ).unwrap();
        }
        let object = parse_itip(&cal("METHOD:REQUEST\r\n", &body), None);
        assert_eq!(object.components.len(), MAX_COMPONENTS);
        assert!(object.components.iter().all(|c| c.fields.is_some()));
    }

    #[test]
    fn schedule_agent_values() {
        let p = Property::new("ATTENDEE", "mailto:a@b.c").with_param("SCHEDULE-AGENT", "none");
        assert_eq!(schedule_agent_of(&p), ScheduleAgent::None);
        let p = Property::new("ATTENDEE", "mailto:a@b.c").with_param("SCHEDULE-AGENT", "X-FOO");
        assert_eq!(schedule_agent_of(&p), ScheduleAgent::Server);
        assert_eq!(ScheduleAgent::Client.as_str(), "CLIENT");
        assert_eq!(partstat(ParticipationStatus::Delegated), "DELEGATED");
    }
}
