//! Building outgoing iTIP objects (RFC 5546 §3.2).
//!
//! Every builder starts from iCalendar text that holds the event — the
//! organizer's stored copy, or the invitation the attendee received — and
//! writes a new `VCALENDAR` with `METHOD`, Nail's `PRODID`, the
//! `VTIMEZONE`s its times need (the source's, else generated from
//! `chrono-tz`) and the components the method calls for:
//!
//! | Method | Sender | Components |
//! | --- | --- | --- |
//! | `REQUEST` / `PUBLISH` | organizer | the full event (or one occurrence), no alarms |
//! | `CANCEL` | organizer | the meeting, one occurrence, or only removed attendees |
//! | `REPLY` | attendee | one attendee with `PARTSTAT` (+ delegates), `COMMENT` |
//! | `COUNTER` | attendee | the event with the proposed time / location |
//! | `REFRESH` | attendee | `UID`, organizer, the attendee |
//! | `DECLINECOUNTER` | organizer | `UID`, the proposer, `COMMENT` |
//!
//! `SCHEDULE-AGENT`, `SCHEDULE-STATUS` and `SCHEDULE-FORCE-SEND` (RFC
//! 6638, between a client and its server) never leave in iTIP mail.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Utc};
use nail_api::pim::PimTime;
use nail_api::pim::calendar::{Attendee, AttendeeKind, AttendeeRole};
use nail_api::scheduling::{CounterProposal, ItipMethod};
use nail_ical::content::{Component, Property};
use nail_ical::time;

use crate::address::{mailto, normalize, same};
use crate::card::same_occurrence;
use crate::object::{ItipComponent, ItipObject, parse_itip, partstat};

/// RFC 6638 parameters stripped from outgoing `ORGANIZER` / `ATTENDEE`.
const SCHEDULING_PARAMS: &[&str] = &["SCHEDULE-AGENT", "SCHEDULE-STATUS", "SCHEDULE-FORCE-SEND"];

/// Why an iTIP object cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// The source text holds no event with that `UID`.
    #[error("the event is not in the source")]
    NoEvent,
    /// The event has no organizer (it is not a meeting).
    #[error("the event has no organizer")]
    NoOrganizer,
    /// The occurrence is not in the source.
    #[error("the occurrence is not in the source")]
    NoOccurrence,
}

/// The event in a source text.
struct Source {
    object: ItipObject,
}

impl Source {
    fn read(text: &str, uid: &str) -> Result<Self, BuildError> {
        // The source is a stored calendar object (no METHOD) or a received
        // iTIP message; either way only its components matter.
        let object = parse_itip(text, Some("PUBLISH"));
        if object.uid.as_deref() != Some(uid) || object.components.is_empty() {
            return Err(BuildError::NoEvent);
        }
        Ok(Self { object })
    }

    fn master(&self) -> Option<&ItipComponent> {
        self.object.master()
    }

    /// The override component for an occurrence, if the source has one.
    fn occurrence(&self, rid: &PimTime) -> Option<&ItipComponent> {
        let start = self
            .master()
            .and_then(|m| m.fields.as_ref())
            .map(|f| &f.start);
        self.object.occurrences().find(|c| {
            c.recurrence_id
                .as_ref()
                .is_some_and(|r| r == rid || start.is_some_and(|s| same_occurrence(r, rid, s)))
        })
    }

    /// The component an answer is about: the occurrence's override, else
    /// the master, else the first component.
    fn about(&self, rid: Option<&PimTime>) -> &ItipComponent {
        rid.and_then(|r| self.occurrence(r))
            .or_else(|| self.master())
            .unwrap_or(&self.object.components[0])
    }

    fn zones(&self) -> Vec<Component> {
        self.object
            .calendar()
            .map(|c| c.components_named("VTIMEZONE").cloned().collect())
            .unwrap_or_default()
    }

    /// A `RECURRENCE-ID` property for an occurrence: the override's own,
    /// else written like the master's `DTSTART` (same `TZID` spelling, so
    /// the receiver matches it with its generated occurrence).
    fn recurrence_id_property(&self, rid: &PimTime) -> Property {
        if let Some(p) = self
            .occurrence(rid)
            .and_then(|c| c.component.property("RECURRENCE-ID"))
        {
            let mut p = p.clone();
            p.remove_param("RANGE");
            return p;
        }
        let master = self.master();
        let dtstart = master.and_then(|m| m.component.property("DTSTART"));
        let start = master.and_then(|m| m.fields.as_ref()).map(|f| &f.start);
        if let (Some(dtstart), Some(start)) = (dtstart, start)
            && let Some(local) = time::to_frame(rid, start)
        {
            let mut p = Property::new("RECURRENCE-ID", "");
            for param in &dtstart.params {
                p = p.with_param_values(&param.name, param.values.clone());
            }
            let value = match start {
                PimTime::Date { .. } => time::ical_date(local.date()),
                PimTime::Zoned { tz, .. } if dtstart.param("TZID").is_none() => {
                    match time::zone(tz) {
                        Some(zone) => time::ical_utc(time::resolve_local(local, zone)),
                        None => time::ical_local(local),
                    }
                }
                _ => time::ical_local(local),
            };
            p.set_value(value);
            return p;
        }
        time_property("RECURRENCE-ID", rid)
    }
}

/// A `DATE` / `DATE-TIME` property for a time (`TZID` = the IANA name).
pub fn time_property(name: &str, t: &PimTime) -> Property {
    match t {
        PimTime::Date { date } => {
            Property::new(name, date.replace('-', "")).with_param("VALUE", "DATE")
        }
        PimTime::Floating { local } => Property::new(name, local.replace(['-', ':'], "")),
        PimTime::Zoned { local, tz } => {
            let value = local.replace(['-', ':'], "");
            if time::zone(tz) == Some(chrono_tz::Tz::UTC) {
                Property::new(name, format!("{value}Z"))
            } else {
                Property::new(name, value).with_param("TZID", tz.clone())
            }
        }
    }
}

/// An `ATTENDEE` property for an attendee.
pub fn attendee_property(a: &Attendee) -> Property {
    let mut p = Property::new("ATTENDEE", mailto(&a.email));
    if let Some(name) = &a.name {
        p = p.with_param("CN", name.clone());
    }
    let role = match a.role {
        AttendeeRole::Chair => "CHAIR",
        AttendeeRole::Required => "REQ-PARTICIPANT",
        AttendeeRole::Optional => "OPT-PARTICIPANT",
        AttendeeRole::NonParticipant => "NON-PARTICIPANT",
    };
    p = p
        .with_param("ROLE", role)
        .with_param("PARTSTAT", partstat(a.status));
    let kind = match a.kind {
        AttendeeKind::Individual => None,
        AttendeeKind::Group => Some("GROUP"),
        AttendeeKind::Resource => Some("RESOURCE"),
        AttendeeKind::Room => Some("ROOM"),
        AttendeeKind::Unknown => Some("UNKNOWN"),
    };
    if let Some(kind) = kind {
        p = p.with_param("CUTYPE", kind);
    }
    if a.rsvp {
        p = p.with_param("RSVP", "TRUE");
    }
    let uris = |v: &[String]| v.iter().map(|e| mailto(e)).collect::<Vec<_>>();
    p.with_param_values("DELEGATED-TO", uris(&a.delegated_to))
        .with_param_values("DELEGATED-FROM", uris(&a.delegated_from))
}

/// A copy of an `ORGANIZER` / `ATTENDEE` property without RFC 6638
/// parameters.
fn clean(p: &Property) -> Property {
    let mut p = p.clone();
    for name in SCHEDULING_PARAMS {
        p.remove_param(name);
    }
    p
}

fn copy(from: &Component, to: &mut Component, names: &[&str]) {
    for name in names {
        to.properties.extend(from.properties_named(name).cloned());
    }
}

fn stamp(now: DateTime<Utc>) -> Property {
    Property::new("DTSTAMP", time::ical_utc(now))
}

fn comment(text: Option<&str>) -> Option<Property> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| Property::text_value("COMMENT", t))
}

/// Replaces properties named `name` (all of them) with `new`, in place
/// of the first.
fn replace(c: &mut Component, name: &str, new: Option<Property>) {
    let at = c.properties.iter().position(|p| p.is(name));
    c.properties.retain(|p| !p.is(name));
    if let Some(new) = new {
        let at = at.unwrap_or(c.properties.len()).min(c.properties.len());
        c.properties.insert(at, new);
    }
}

/// The attendee's property from the source with a new `PARTSTAT`, or a
/// new one.
fn answering_attendee(source: &ItipComponent, a: &Attendee) -> Property {
    let Some(original) = source
        .component
        .properties_named("ATTENDEE")
        .find(|p| same(&p.value, &a.email))
    else {
        return attendee_property(a);
    };
    let mut p = clean(original);
    p.set_param("PARTSTAT", vec![partstat(a.status).to_owned()]);
    p.remove_param("RSVP");
    let uris = |v: &[String]| v.iter().map(|e| mailto(e)).collect::<Vec<_>>();
    p.set_param("DELEGATED-TO", uris(&a.delegated_to));
    if !a.delegated_from.is_empty() {
        p.set_param("DELEGATED-FROM", uris(&a.delegated_from));
    }
    if p.param("CN").is_none()
        && let Some(name) = &a.name
    {
        p.set_param("CN", vec![name.clone()]);
    }
    p
}

/// Every `TZID` the components use.
fn tzids(components: &[Component]) -> BTreeSet<String> {
    fn walk(c: &Component, out: &mut BTreeSet<String>) {
        for p in &c.properties {
            if let Some(tzid) = p.param("TZID") {
                out.insert(tzid.to_owned());
            }
        }
        for sub in &c.components {
            walk(sub, out);
        }
    }
    let mut out = BTreeSet::new();
    for c in components {
        walk(c, &mut out);
    }
    out
}

fn years(components: &[Component]) -> (i32, i32) {
    let mut years = Vec::new();
    for c in components {
        for p in &c.properties {
            if (p.is("DTSTART") || p.is("DTEND") || p.is("RECURRENCE-ID"))
                && let Some(y) = p.value.get(..4).and_then(|y| y.parse::<i32>().ok())
            {
                years.push(y);
            }
        }
    }
    let now = Utc::now().year();
    (
        years.iter().copied().min().unwrap_or(now),
        years.iter().copied().max().unwrap_or(now).max(now) + 1,
    )
}

/// The outgoing `VCALENDAR` text.
fn calendar(method: ItipMethod, zones: &[Component], events: Vec<Component>) -> String {
    let mut cal = Component::new("VCALENDAR");
    cal.properties
        .push(Property::new("PRODID", nail_ical::PRODID));
    cal.properties.push(Property::new("VERSION", "2.0"));
    cal.properties.push(Property::new("CALSCALE", "GREGORIAN"));
    cal.properties
        .push(Property::new("METHOD", method.as_ical()));
    let (from, to) = years(&events);
    for tzid in tzids(&events) {
        let own = zones.iter().find(|z| {
            z.property("TZID")
                .is_some_and(|p| p.text().trim() == tzid.trim())
        });
        match own {
            Some(zone) => cal.components.push(zone.clone()),
            None => {
                if let Some(zone) = nail_ical::tz::iana_for_name(&tzid)
                    .and_then(|tz| nail_ical::tz::vtimezone_for(tz, from, to))
                {
                    let mut zone = zone;
                    for p in &mut zone.properties {
                        if p.is("TZID") {
                            p.set_value(tzid.clone());
                        }
                    }
                    cal.components.push(zone);
                }
            }
        }
    }
    cal.components.extend(events);
    cal.to_text()
}

/// Options of [`build_request`].
#[derive(Debug, Clone)]
pub struct RequestOptions<'a> {
    /// `REQUEST`, `PUBLISH` or `ADD`.
    pub method: ItipMethod,
    /// The event's `UID`.
    pub uid: &'a str,
    /// Only this occurrence (its override must exist); `None` = the
    /// whole event.
    pub recurrence_id: Option<&'a PimTime>,
    /// `SEQUENCE` to write (the organizer's current one); `None` keeps
    /// the source's.
    pub sequence: Option<u32>,
    /// `DTSTAMP`.
    pub now: DateTime<Utc>,
}

/// `REQUEST` / `PUBLISH` / `ADD` from the organizer's stored copy: the
/// event's components without alarms, with `SEQUENCE` and `DTSTAMP` set.
pub fn build_request(
    source_text: &str,
    options: &RequestOptions<'_>,
) -> Result<String, BuildError> {
    let source = Source::read(source_text, options.uid)?;
    let chosen: Vec<&ItipComponent> = match options.recurrence_id {
        Some(rid) => vec![source.occurrence(rid).ok_or(BuildError::NoOccurrence)?],
        None => source.object.components.iter().collect(),
    };
    let mut out = Vec::new();
    for c in chosen {
        if c.organizer.is_none() && options.method != ItipMethod::Publish {
            return Err(BuildError::NoOrganizer);
        }
        let mut comp = c.component.clone();
        comp.components.retain(|sub| !sub.is("VALARM"));
        for p in &mut comp.properties {
            if p.is("ORGANIZER") || p.is("ATTENDEE") {
                *p = clean(p);
            }
        }
        if let Some(sequence) = options.sequence {
            replace(
                &mut comp,
                "SEQUENCE",
                Some(Property::new("SEQUENCE", sequence.to_string())),
            );
        }
        replace(&mut comp, "DTSTAMP", Some(stamp(options.now)));
        out.push(comp);
    }
    Ok(calendar(options.method, &source.zones(), out))
}

/// What a `CANCEL` cancels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelWhat<'a> {
    /// The whole meeting.
    Meeting,
    /// One occurrence.
    Occurrence(&'a PimTime),
    /// These attendees only (they were removed).
    Attendees(&'a [String]),
}

/// Options of [`build_cancel`].
#[derive(Debug, Clone)]
pub struct CancelOptions<'a> {
    /// The event's `UID`.
    pub uid: &'a str,
    /// What is cancelled.
    pub what: CancelWhat<'a>,
    /// `SEQUENCE` (one higher than the last one sent).
    pub sequence: u32,
    /// The organizer's comment.
    pub comment: Option<&'a str>,
    /// `DTSTAMP`.
    pub now: DateTime<Utc>,
}

/// `CANCEL` from the organizer's copy.
pub fn build_cancel(source_text: &str, options: &CancelOptions<'_>) -> Result<String, BuildError> {
    let source = Source::read(source_text, options.uid)?;
    let base = match options.what {
        CancelWhat::Occurrence(rid) => source.about(Some(rid)),
        _ => source.about(None),
    };
    if base.organizer.is_none() {
        return Err(BuildError::NoOrganizer);
    }
    let mut comp = Component::new("VEVENT");
    copy(&base.component, &mut comp, &["UID"]);
    comp.properties.push(stamp(options.now));
    comp.properties
        .push(Property::new("SEQUENCE", options.sequence.to_string()));
    if let CancelWhat::Occurrence(rid) = options.what {
        comp.properties.push(source.recurrence_id_property(rid));
        if source.occurrence(rid).is_some() {
            copy(
                &base.component,
                &mut comp,
                &["DTSTART", "DTEND", "DURATION"],
            );
        }
    } else {
        copy(
            &base.component,
            &mut comp,
            &["DTSTART", "DTEND", "DURATION"],
        );
    }
    copy(&base.component, &mut comp, &["SUMMARY", "LOCATION"]);
    comp.properties.extend(
        base.component
            .properties_named("ORGANIZER")
            .take(1)
            .map(clean),
    );
    let attendees = base.component.properties_named("ATTENDEE");
    match options.what {
        CancelWhat::Attendees(removed) => {
            for address in removed {
                let p = base
                    .component
                    .properties_named("ATTENDEE")
                    .find(|p| same(&p.value, address))
                    .map_or_else(
                        || attendee_property(&Attendee::new(normalize(address))),
                        clean,
                    );
                comp.properties.push(p);
            }
        }
        _ => comp.properties.extend(attendees.map(clean)),
    }
    comp.properties.push(Property::new("STATUS", "CANCELLED"));
    comp.properties.extend(comment(options.comment));
    Ok(calendar(ItipMethod::Cancel, &source.zones(), vec![comp]))
}

/// Options of [`build_reply`].
#[derive(Debug, Clone)]
pub struct ReplyOptions<'a> {
    /// The event's `UID`.
    pub uid: &'a str,
    /// The occurrence answered; `None` = the series.
    pub recurrence_id: Option<&'a PimTime>,
    /// The answering attendee with the new status (and `delegated_to`
    /// when delegating).
    pub attendee: &'a Attendee,
    /// Delegates (attendees with `DELEGATED-FROM` the answering one).
    pub delegates: &'a [Attendee],
    /// The attendee's comment.
    pub comment: Option<&'a str>,
    /// `DTSTAMP`.
    pub now: DateTime<Utc>,
}

/// `REPLY` (RFC 5546 §3.2.3): the answering attendee only (plus
/// delegates), the source's `SEQUENCE`, the occurrence's `RECURRENCE-ID`.
pub fn build_reply(source_text: &str, options: &ReplyOptions<'_>) -> Result<String, BuildError> {
    let source = Source::read(source_text, options.uid)?;
    let base = source.about(options.recurrence_id);
    if base.organizer.is_none() {
        return Err(BuildError::NoOrganizer);
    }
    let mut comp = Component::new("VEVENT");
    copy(&base.component, &mut comp, &["UID"]);
    comp.properties.push(stamp(options.now));
    comp.properties
        .push(Property::new("SEQUENCE", base.sequence.to_string()));
    if let Some(rid) = options.recurrence_id {
        comp.properties.push(source.recurrence_id_property(rid));
    }
    if options
        .recurrence_id
        .is_none_or(|rid| source.occurrence(rid).is_some())
    {
        copy(
            &base.component,
            &mut comp,
            &["DTSTART", "DTEND", "DURATION"],
        );
    }
    copy(&base.component, &mut comp, &["SUMMARY"]);
    comp.properties.extend(
        base.component
            .properties_named("ORGANIZER")
            .take(1)
            .map(clean),
    );
    comp.properties
        .push(answering_attendee(base, options.attendee));
    for d in options.delegates {
        let mut d = d.clone();
        if d.delegated_from.is_empty() {
            d.delegated_from = vec![normalize(&options.attendee.email)];
        }
        comp.properties.push(attendee_property(&d));
    }
    comp.properties.extend(comment(options.comment));
    Ok(calendar(ItipMethod::Reply, &source.zones(), vec![comp]))
}

/// Options of [`build_counter`].
#[derive(Debug, Clone)]
pub struct CounterOptions<'a> {
    /// The event's `UID`.
    pub uid: &'a str,
    /// The occurrence; `None` = the series.
    pub recurrence_id: Option<&'a PimTime>,
    /// The proposing attendee (one of the user's addresses).
    pub attendee: &'a str,
    /// The proposal (its `comment` becomes `COMMENT`).
    pub proposal: &'a CounterProposal,
    /// `DTSTAMP`.
    pub now: DateTime<Utc>,
}

/// `COUNTER` (RFC 5546 §3.2.7): the event as received with the proposed
/// time and location.
pub fn build_counter(
    source_text: &str,
    options: &CounterOptions<'_>,
) -> Result<String, BuildError> {
    let source = Source::read(source_text, options.uid)?;
    let base = source.about(options.recurrence_id);
    if base.organizer.is_none() {
        return Err(BuildError::NoOrganizer);
    }
    let mut comp = base.component.clone();
    comp.components.retain(|sub| !sub.is("VALARM"));
    for p in &mut comp.properties {
        if p.is("ORGANIZER") || p.is("ATTENDEE") {
            *p = clean(p);
        }
    }
    if let Some(rid) = options.recurrence_id {
        comp.properties
            .retain(|p| !(p.is("RRULE") || p.is("RDATE") || p.is("EXDATE")));
        replace(
            &mut comp,
            "RECURRENCE-ID",
            Some(source.recurrence_id_property(rid)),
        );
    }
    if base.attendee(options.attendee).is_none() {
        comp.properties
            .push(attendee_property(&Attendee::new(normalize(
                options.attendee,
            ))));
    }
    replace(&mut comp, "DURATION", None);
    replace(
        &mut comp,
        "DTSTART",
        Some(time_property("DTSTART", &options.proposal.start)),
    );
    replace(
        &mut comp,
        "DTEND",
        Some(time_property("DTEND", &options.proposal.end)),
    );
    if let Some(location) = options
        .proposal
        .location
        .as_deref()
        .filter(|l| !l.trim().is_empty())
    {
        replace(
            &mut comp,
            "LOCATION",
            Some(Property::text_value("LOCATION", location)),
        );
    }
    replace(
        &mut comp,
        "COMMENT",
        comment(options.proposal.comment.as_deref()),
    );
    replace(&mut comp, "DTSTAMP", Some(stamp(options.now)));
    Ok(calendar(ItipMethod::Counter, &source.zones(), vec![comp]))
}

/// Options of [`build_refresh`] and [`build_decline_counter`].
#[derive(Debug, Clone)]
pub struct MinimalOptions<'a> {
    /// The event's `UID`.
    pub uid: &'a str,
    /// The occurrence; `None` = the series.
    pub recurrence_id: Option<&'a PimTime>,
    /// The attendee (the user for `REFRESH`, the proposer for
    /// `DECLINECOUNTER`).
    pub attendee: &'a str,
    /// A comment.
    pub comment: Option<&'a str>,
    /// `SEQUENCE` for `DECLINECOUNTER` (the organizer's current one);
    /// `None` keeps the source's.
    pub sequence: Option<u32>,
    /// `DTSTAMP`.
    pub now: DateTime<Utc>,
}

fn build_minimal(
    method: ItipMethod,
    source_text: &str,
    options: &MinimalOptions<'_>,
) -> Result<String, BuildError> {
    let source = Source::read(source_text, options.uid)?;
    let base = source.about(options.recurrence_id);
    if base.organizer.is_none() {
        return Err(BuildError::NoOrganizer);
    }
    let mut comp = Component::new("VEVENT");
    copy(&base.component, &mut comp, &["UID"]);
    comp.properties.push(stamp(options.now));
    if method == ItipMethod::DeclineCounter {
        let sequence = options.sequence.unwrap_or(base.sequence);
        comp.properties
            .push(Property::new("SEQUENCE", sequence.to_string()));
    }
    if let Some(rid) = options.recurrence_id {
        comp.properties.push(source.recurrence_id_property(rid));
    }
    comp.properties.extend(
        base.component
            .properties_named("ORGANIZER")
            .take(1)
            .map(clean),
    );
    let attendee = base
        .component
        .properties_named("ATTENDEE")
        .find(|p| same(&p.value, options.attendee))
        .map_or_else(
            || attendee_property(&Attendee::new(normalize(options.attendee))),
            clean,
        );
    comp.properties.push(attendee);
    comp.properties.extend(comment(options.comment));
    Ok(calendar(method, &source.zones(), vec![comp]))
}

/// `REFRESH` (RFC 5546 §3.2.6): the attendee asks for the current version.
pub fn build_refresh(
    source_text: &str,
    options: &MinimalOptions<'_>,
) -> Result<String, BuildError> {
    build_minimal(ItipMethod::Refresh, source_text, options)
}

/// `DECLINECOUNTER` (RFC 5546 §3.2.8): the organizer refuses a proposal.
pub fn build_decline_counter(
    source_text: &str,
    options: &MinimalOptions<'_>,
) -> Result<String, BuildError> {
    build_minimal(ItipMethod::DeclineCounter, source_text, options)
}

/// The `REQUEST` a delegating attendee forwards to the delegate (RFC 5546
/// §3.2.2.3): the event as received, with the delegator `DELEGATED` /
/// `DELEGATED-TO` and the delegate `DELEGATED-FROM`, `RSVP=TRUE`.
pub fn build_delegated_request(
    source_text: &str,
    uid: &str,
    delegator: &str,
    delegate: &Attendee,
    now: DateTime<Utc>,
) -> Result<String, BuildError> {
    let source = Source::read(source_text, uid)?;
    let delegator = normalize(delegator);
    let delegate_email = normalize(&delegate.email);
    let mut out = Vec::new();
    for c in &source.object.components {
        if c.organizer.is_none() {
            return Err(BuildError::NoOrganizer);
        }
        let mut comp = c.component.clone();
        comp.components.retain(|sub| !sub.is("VALARM"));
        let role = c
            .attendee(&delegator)
            .map(|a| a.attendee.role)
            .unwrap_or_default();
        comp.properties
            .retain(|p| !(p.is("ATTENDEE") && same(&p.value, &delegate_email)));
        for p in &mut comp.properties {
            if p.is("ORGANIZER") || p.is("ATTENDEE") {
                *p = clean(p);
            }
            if p.is("ATTENDEE") && same(&p.value, &delegator) {
                p.set_param("PARTSTAT", vec!["DELEGATED".into()]);
                p.set_param("DELEGATED-TO", vec![mailto(&delegate_email)]);
                p.remove_param("RSVP");
            }
        }
        if c.attendee(&delegator).is_none() {
            let mut me = Attendee::new(delegator.clone());
            me.status = nail_api::pim::calendar::ParticipationStatus::Delegated;
            me.delegated_to = vec![delegate_email.clone()];
            comp.properties.push(attendee_property(&me));
        }
        let mut added = delegate.clone();
        added.email.clone_from(&delegate_email);
        added.role = role;
        added.status = nail_api::pim::calendar::ParticipationStatus::NeedsAction;
        added.rsvp = true;
        added.delegated_from = vec![delegator.clone()];
        comp.properties.push(attendee_property(&added));
        replace(&mut comp, "DTSTAMP", Some(stamp(now)));
        out.push(comp);
    }
    Ok(calendar(ItipMethod::Request, &source.zones(), out))
}

#[cfg(test)]
mod tests;
