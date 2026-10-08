//! Events and tasks → iCalendar, **patching the original text**.
//!
//! The writer starts from the item's stored iCalendar (when there is one)
//! and, field by field, compares what the original means (read with the
//! same readers as [`crate::ical`]) with the edited model. Equal fields
//! keep their original properties byte for byte; changed fields are
//! rewritten, and a rewritten property keeps the parameters Nail does not
//! manage (`X-NUM-GUESTS`, `SCHEDULE-STATUS`, `LANGUAGE`, …). Unknown
//! properties, components (`VALARM`s with `ACTION:NONE`, vendor blocks)
//! and `VTIMEZONE`s stay as they are; time zones used for the first time
//! get a `VTIMEZONE` generated from `chrono-tz`. `DTSTAMP` is refreshed
//! when anything changed.

use std::cell::RefCell;

use base64::Engine as _;
use chrono::{DateTime, Datelike, Utc};
use nail_api::pim::calendar::{AlarmRelated, EventStatus, Transparency, Visibility};
use nail_api::pim::task::TaskStatus;
use nail_api::pim::{
    Alarm, AlarmAction, AlarmTrigger, Attendee, CalendarEvent, ConferenceLink, EventFields,
    Organizer, PimAttachment, PimLink, PimTime, Recurrence, RecurrenceEnd, Task, TaskFields,
};

use crate::content::{self, Component, Property, escape_text};
use crate::ical::{self, Attached, Ctx};
use crate::recur;
use crate::time;
use crate::tz::ZoneResolver;

/// Looks up stored bytes by SHA-256 (hex): the blob store.
pub type BlobLookup<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Options for writing.
#[derive(Clone, Copy)]
pub struct WriteOptions<'a> {
    /// "Now", for `DTSTAMP` (tests pass a fixed time).
    pub now: DateTime<Utc>,
    /// Bytes of an inline attachment by SHA-256, for attachments that are
    /// not in the original text (the blob store).
    pub blobs: Option<BlobLookup<'a>>,
    /// `RELATED-TO` for a series split off another one (the old `UID`).
    pub related_to: Option<&'a str>,
}

impl std::fmt::Debug for WriteOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteOptions")
            .field("now", &self.now)
            .field("blobs", &self.blobs.is_some())
            .field("related_to", &self.related_to)
            .finish()
    }
}

impl WriteOptions<'_> {
    /// Options with only a time.
    pub fn at(now: DateTime<Utc>) -> Self {
        WriteOptions {
            now,
            blobs: None,
            related_to: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Property helpers

/// Replaces every property named in `names` with `new`, at the position
/// of the first old one (or at the end).
pub(crate) fn replace(c: &mut Component, names: &[&str], new: Vec<Property>) {
    let position = c
        .properties
        .iter()
        .position(|p| names.iter().any(|n| p.is(n)));
    c.properties.retain(|p| !names.iter().any(|n| p.is(n)));
    let at = position
        .unwrap_or(c.properties.len())
        .min(c.properties.len());
    c.properties.splice(at..at, new);
}

/// `new` with the original's group and the original's parameters that
/// are not `managed` (and not set by `new`).
pub(crate) fn merged(original: Option<&Property>, mut new: Property, managed: &[&str]) -> Property {
    if let Some(original) = original {
        let mut params: Vec<content::Param> = original
            .params
            .iter()
            .filter(|p| !managed.iter().any(|m| p.is(m)))
            .filter(|p| !new.params.iter().any(|n| n.is(&p.name)))
            .cloned()
            .collect();
        params.append(&mut new.params);
        new.params = params;
        new.group.clone_from(&original.group);
        new.touch();
    }
    new
}

/// Rewrites a single-valued property when the meaning changed.
fn patch_one<T: PartialEq>(
    c: &mut Component,
    name: &str,
    current: &T,
    desired: &T,
    managed: &[&str],
    build: impl FnOnce(&T) -> Option<Property>,
) -> bool {
    if current == desired {
        return false;
    }
    let original = c.property(name).cloned();
    let new = build(desired).map(|p| merged(original.as_ref(), p, managed));
    replace(c, &[name], new.into_iter().collect());
    true
}

fn text_prop(name: &str, text: Option<&str>) -> Option<Property> {
    text.filter(|t| !t.is_empty())
        .map(|t| Property::text_value(name, t))
}

/// Picks `TZID` spellings: the original object's name for a zone when it
/// has one, else the IANA name; remembers what was used.
struct Zones<'a> {
    resolver: &'a ZoneResolver,
    used: RefCell<Vec<(String, String)>>,
}

impl Zones<'_> {
    fn tzid(&self, iana: &str) -> String {
        let tzid = self
            .resolver
            .original_tzid(iana)
            .unwrap_or_else(|| iana.to_owned());
        let mut used = self.used.borrow_mut();
        if !used.iter().any(|(t, _)| *t == tzid) {
            used.push((tzid.clone(), iana.to_owned()));
        }
        tzid
    }

    fn time_prop(&self, name: &str, value: &PimTime) -> Option<Property> {
        Some(match value {
            PimTime::Date { date } => Property::new(name, time::ical_date(time::parse_date(date)?))
                .with_param("VALUE", "DATE"),
            PimTime::Floating { local } => {
                Property::new(name, time::ical_local(time::parse_local(local)?))
            }
            PimTime::Zoned { local, tz } if tz.eq_ignore_ascii_case(nail_api::pim::UTC_ZONE) => {
                Property::new(name, time::ical_utc(time::parse_local(local)?.and_utc()))
            }
            PimTime::Zoned { local, tz } => {
                Property::new(name, time::ical_local(time::parse_local(local)?))
                    .with_param("TZID", self.tzid(tz))
            }
        })
    }
}

const TIME_PARAMS: &[&str] = &["VALUE", "TZID"];

fn utc_prop(name: &str, instant: Option<&str>) -> Option<Property> {
    let instant = time::parse_instant(instant?)?;
    Some(Property::new(name, time::ical_utc(instant)))
}

fn read_instant(c: &Component, name: &str) -> Option<String> {
    time::value_instant(&c.property(name)?.value)
}

fn normalized_instant(instant: Option<&str>) -> Option<String> {
    instant
        .and_then(time::parse_instant)
        .map(time::format_instant)
}

// ---------------------------------------------------------------------------
// Shared field patches

fn patch_text(c: &mut Component, name: &str, desired: Option<&str>, managed: &[&str]) -> bool {
    let current = ical::read_text(c, name);
    patch_one(c, name, &current.as_deref(), &desired, managed, |d| {
        text_prop(name, *d)
    })
}

fn patch_categories(c: &mut Component, desired: &[String]) {
    let current = ical::read_categories(c);
    if current != desired {
        let value = desired
            .iter()
            .map(|c| escape_text(c))
            .collect::<Vec<_>>()
            .join(",");
        let new = (!desired.is_empty()).then(|| Property::new("CATEGORIES", value));
        replace(c, &["CATEGORIES"], new.into_iter().collect());
    }
}

fn patch_visibility(c: &mut Component, desired: Visibility) {
    let current = ical::read_visibility(c);
    patch_one(c, "CLASS", &current, &desired, &[], |v| {
        Some(Property::new(
            "CLASS",
            match v {
                Visibility::Public => "PUBLIC",
                Visibility::Private => "PRIVATE",
                Visibility::Confidential => "CONFIDENTIAL",
            },
        ))
    });
}

fn patch_priority(c: &mut Component, desired: Option<u8>) {
    let current = ical::read_priority(c);
    patch_one(c, "PRIORITY", &current, &desired, &[], |p| {
        p.map(|p| Property::new("PRIORITY", p.to_string()))
    });
}

fn alarm_component(alarm: &Alarm, title: &str, original: Option<&Component>) -> Component {
    let mut c = original
        .cloned()
        .unwrap_or_else(|| Component::new("VALARM"));
    let current = ical::read_alarm(&c);
    let action = |a: AlarmAction| match a {
        AlarmAction::Display => "DISPLAY",
        AlarmAction::Audio => "AUDIO",
        AlarmAction::Email => "EMAIL",
    };
    patch_one(
        &mut c,
        "ACTION",
        &current.as_ref().map(|a| a.action),
        &Some(alarm.action),
        &[],
        |a| a.map(|a| Property::new("ACTION", action(a))),
    );
    patch_one(
        &mut c,
        "TRIGGER",
        &current.as_ref().map(|a| a.trigger.clone()),
        &Some(alarm.trigger.clone()),
        &["VALUE", "RELATED"],
        |t| {
            Some(match t.as_ref()? {
                AlarmTrigger::Relative {
                    offset_secs,
                    related,
                } => {
                    let p = Property::new("TRIGGER", time::format_duration(*offset_secs));
                    if *related == AlarmRelated::End {
                        p.with_param("RELATED", "END")
                    } else {
                        p
                    }
                }
                AlarmTrigger::Absolute { at } => {
                    Property::new("TRIGGER", time::ical_utc(time::parse_instant(at)?))
                        .with_param("VALUE", "DATE-TIME")
                }
            })
        },
    );
    // DISPLAY and EMAIL alarms need a DESCRIPTION; none means the title.
    let description = alarm
        .description
        .clone()
        .or_else(|| (alarm.action != AlarmAction::Audio).then(|| title.to_owned()));
    let current_description = ical::read_text(&c, "DESCRIPTION");
    if current_description != description
        && !(alarm.description.is_none() && current_description.is_some())
    {
        let new = text_prop("DESCRIPTION", description.as_deref());
        let original = c.property("DESCRIPTION").cloned();
        replace(
            &mut c,
            &["DESCRIPTION"],
            new.map(|p| merged(original.as_ref(), p, &[]))
                .into_iter()
                .collect(),
        );
    }
    if alarm.action == AlarmAction::Email && c.property("SUMMARY").is_none() {
        c.properties.push(Property::text_value("SUMMARY", title));
    }
    let current_repeat = current.as_ref().and_then(|a| a.repeat);
    if current_repeat != alarm.repeat {
        let (repeat, duration) = match alarm.repeat {
            Some(r) => (
                Some(Property::new("REPEAT", r.count.to_string())),
                Some(Property::new(
                    "DURATION",
                    time::format_duration(i64::from(r.interval_secs)),
                )),
            ),
            None => (None, None),
        };
        replace(&mut c, &["REPEAT"], repeat.into_iter().collect());
        replace(&mut c, &["DURATION"], duration.into_iter().collect());
    }
    let current_uid = ical::read_text(&c, "UID").or_else(|| ical::read_text(&c, "X-WR-ALARMUID"));
    if current_uid != alarm.uid {
        let new = text_prop("UID", alarm.uid.as_deref());
        replace(&mut c, &["UID"], new.into_iter().collect());
    }
    let current_ack = read_instant(&c, "ACKNOWLEDGED");
    if current_ack != normalized_instant(alarm.acknowledged.as_deref()) {
        let new = utc_prop("ACKNOWLEDGED", alarm.acknowledged.as_deref());
        replace(&mut c, &["ACKNOWLEDGED"], new.into_iter().collect());
    }
    c
}

fn patch_alarms(c: &mut Component, desired: &[Alarm], title: &str) {
    if ical::read_alarms(c) == desired {
        return;
    }
    let originals: Vec<Component> = c
        .components
        .iter()
        .filter(|s| s.is("VALARM"))
        .cloned()
        .collect();
    let mut used = vec![false; originals.len()];
    let mut alarms = Vec::new();
    for alarm in desired {
        let readable = |i: usize| {
            ical::read_alarm(&originals[i]).map(|mut a| {
                if a.description.as_deref() == Some(title) {
                    a.description = None;
                }
                a
            })
        };
        // An identical original stays as written; else the one with the
        // same UID is patched; else a new one is built.
        let exact = (0..originals.len()).find(|&i| !used[i] && readable(i).as_ref() == Some(alarm));
        let by_uid = || {
            (0..originals.len()).find(|&i| {
                !used[i] && alarm.uid.is_some() && readable(i).is_some_and(|a| a.uid == alarm.uid)
            })
        };
        match exact.or_else(by_uid) {
            Some(i) => {
                used[i] = true;
                alarms.push(if exact == Some(i) {
                    originals[i].clone()
                } else {
                    alarm_component(alarm, title, Some(&originals[i]))
                });
            }
            None => alarms.push(alarm_component(alarm, title, None)),
        }
    }
    // Alarms Nail does not read (ACTION:NONE, broken ones) stay.
    for (i, original) in originals.iter().enumerate() {
        if !used[i] && ical::read_alarm(original).is_none() {
            alarms.push(original.clone());
        }
    }
    let first = c.components.iter().position(|s| s.is("VALARM"));
    c.components.retain(|s| !s.is("VALARM"));
    let at = first.unwrap_or(c.components.len()).min(c.components.len());
    c.components.splice(at..at, alarms);
}

fn link_prop(link: &PimLink) -> Option<Property> {
    let PimLink::Message {
        rfc822_id, subject, ..
    } = link;
    let id = rfc822_id.as_deref()?;
    let p = Property::new("ATTACH", format!("mid:{}", ical::percent_encode(id)))
        .with_param("FMTTYPE", "message/rfc822");
    Some(match subject {
        Some(subject) => p.with_param("X-NAIL-SUBJECT", subject.clone()),
        None => p,
    })
}

fn without_local_ids(links: &[PimLink]) -> Vec<PimLink> {
    links
        .iter()
        .map(
            |PimLink::Message {
                 rfc822_id, subject, ..
             }| PimLink::Message {
                message_id: None,
                rfc822_id: rfc822_id.clone(),
                subject: subject.clone(),
            },
        )
        .collect()
}

/// Rewrites `ATTACH` properties. Tasks manage only message links
/// (`files` = `None`): their other attachments stay as they are.
fn patch_attachments(
    c: &mut Component,
    files: Option<&[PimAttachment]>,
    links: &[PimLink],
    options: &WriteOptions<'_>,
) {
    let (current_files, current_links, _) = ical::read_attachments(c);
    let desired_links = without_local_ids(links);
    if files.is_none_or(|f| f == current_files) && current_links == desired_links {
        return;
    }
    let originals: Vec<Property> = c.properties_named("ATTACH").cloned().collect();
    let mut used = vec![false; originals.len()];
    let mut new = Vec::new();
    match files {
        Some(files) => {
            for file in files {
                let same = (0..originals.len()).find(|&i| {
                    !used[i]
                        && matches!(ical::read_attach(&originals[i]), Some(Attached::Attachment(a, _)) if a == *file)
                });
                if let Some(i) = same {
                    used[i] = true;
                    new.push(originals[i].clone());
                    continue;
                }
                let names = |p: Property| {
                    let p = match &file.name {
                        Some(name) => p.with_param("FILENAME", name.clone()),
                        None => p,
                    };
                    match &file.mime_type {
                        Some(mime) => p.with_param("FMTTYPE", mime.clone()),
                        None => p,
                    }
                };
                if let Some(url) = &file.url {
                    let p = names(Property::new("ATTACH", url.clone()));
                    new.push(match file.size {
                        Some(size) => p.with_param("SIZE", size.to_string()),
                        None => p,
                    });
                } else if let Some(sha) = &file.sha256 {
                    // The same data under another name: re-label it.
                    let same_data = (0..originals.len()).find(|&i| {
                        !used[i]
                            && matches!(ical::read_attach(&originals[i]), Some(Attached::Attachment(a, _)) if a.sha256.as_ref() == Some(sha))
                    });
                    let bytes = match same_data {
                        Some(i) => {
                            used[i] = true;
                            match ical::read_attach(&originals[i]) {
                                Some(Attached::Attachment(_, Some(data))) => Some(data.bytes),
                                _ => None,
                            }
                        }
                        None => options.blobs.and_then(|blobs| blobs(sha)),
                    };
                    if let Some(bytes) = bytes {
                        new.push(names(
                            Property::new(
                                "ATTACH",
                                base64::engine::general_purpose::STANDARD.encode(bytes),
                            )
                            .with_param("ENCODING", "BASE64")
                            .with_param("VALUE", "BINARY"),
                        ));
                    }
                }
            }
        }
        None => {
            for (i, original) in originals.iter().enumerate() {
                if !matches!(ical::read_attach(original), Some(Attached::Link(_))) {
                    used[i] = true;
                    new.push(original.clone());
                }
            }
        }
    }
    for link in &desired_links {
        let same = (0..originals.len()).find(|&i| {
            !used[i]
                && matches!(ical::read_attach(&originals[i]), Some(Attached::Link(l)) if l == *link)
        });
        match same {
            Some(i) => {
                used[i] = true;
                new.push(originals[i].clone());
            }
            None => new.extend(link_prop(link)),
        }
    }
    replace(c, &["ATTACH"], new);
}

fn patch_recurrence(
    c: &mut Component,
    start: &PimTime,
    desired: Option<&Recurrence>,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
) {
    let empty = Recurrence::default();
    let desired = desired.unwrap_or(&empty);
    let current: Vec<_> = c
        .properties_named("RRULE")
        .filter_map(|p| recur::parse_rrule(&p.value, start))
        .filter(nail_api::pim::RecurrenceRule::is_valid)
        .collect();
    if current != desired.rules {
        let new = desired
            .rules
            .iter()
            .map(|rule| {
                let mut rule = rule.clone();
                // UNTIL in UTC when the start has a zone (RFC 5545).
                if let (RecurrenceEnd::Until { until }, PimTime::Zoned { .. }) = (&rule.end, start)
                    && let Some(instant) = time::instant(until, chrono_tz::Tz::UTC)
                    && !until.is_date()
                {
                    rule.end = RecurrenceEnd::Until {
                        until: PimTime::utc(time::format_local(instant.naive_utc())),
                    };
                }
                Property::new("RRULE", recur::format_rrule(&rule))
            })
            .collect();
        replace(c, &["RRULE"], new);
    }
    for (name, values) in [("RDATE", &desired.dates), ("EXDATE", &desired.exceptions)] {
        let current: Vec<PimTime> = c
            .properties_named(name)
            .flat_map(|p| ctx.times(p))
            .collect();
        if current != *values {
            let new = values
                .iter()
                .filter_map(|v| zones.time_prop(name, v))
                .collect();
            replace(c, &[name], new);
        }
    }
}

fn patch_meta(
    c: &mut Component,
    uid: &str,
    sequence: u32,
    created: Option<&str>,
    updated: Option<&str>,
) {
    let current_uid = ical::read_text(c, "UID");
    patch_one(c, "UID", &current_uid.as_deref(), &Some(uid), &[], |u| {
        text_prop("UID", *u)
    });
    let current = ical::read_sequence(c);
    patch_one(c, "SEQUENCE", &current, &sequence, &[], |s| {
        (*s > 0).then(|| Property::new("SEQUENCE", s.to_string()))
    });
    for (name, value) in [("CREATED", created), ("LAST-MODIFIED", updated)] {
        let current = read_instant(c, name);
        patch_one(c, name, &current, &normalized_instant(value), &[], |v| {
            utc_prop(name, v.as_deref())
        });
    }
}

fn finish(c: &mut Component, before: &str, options: &WriteOptions<'_>) {
    if c.to_text() != before || c.property("DTSTAMP").is_none() {
        let stamp = Property::new("DTSTAMP", time::ical_utc(options.now));
        if c.property("DTSTAMP").is_some() {
            replace(c, &["DTSTAMP"], vec![stamp]);
        } else {
            // After UID, where most producers put it.
            let at = c
                .properties
                .iter()
                .position(|p| p.is("UID"))
                .map_or(0, |i| i + 1);
            c.properties.insert(at, stamp);
        }
    }
}

// ---------------------------------------------------------------------------
// Events

fn attendee_prop(a: &Attendee) -> Property {
    crate::attendee::property(a, |e| format!("mailto:{e}"))
}

const ATTENDEE_PARAMS: &[&str] = &[
    "CN",
    "ROLE",
    "PARTSTAT",
    "CUTYPE",
    "RSVP",
    "DELEGATED-TO",
    "DELEGATED-FROM",
];

fn patch_people(c: &mut Component, organizer: Option<&Organizer>, attendees: &[Attendee]) {
    let current = ical::read_organizer(c);
    patch_one(
        c,
        "ORGANIZER",
        &current.as_ref(),
        &organizer,
        &["CN"],
        |o| {
            o.map(|o| {
                let p = Property::new("ORGANIZER", format!("mailto:{}", o.email));
                match &o.name {
                    Some(name) => p.with_param("CN", name.clone()),
                    None => p,
                }
            })
        },
    );
    if ical::read_attendees(c) == attendees {
        return;
    }
    let originals: Vec<Property> = c.properties_named("ATTENDEE").cloned().collect();
    let mut used = vec![false; originals.len()];
    let new = attendees
        .iter()
        .map(|a| {
            let same = (0..originals.len()).find(|&i| {
                !used[i]
                    && ical::read_attendee(&originals[i])
                        .email
                        .eq_ignore_ascii_case(&a.email)
            });
            match same {
                Some(i) => {
                    used[i] = true;
                    if ical::read_attendee(&originals[i]) == *a {
                        originals[i].clone()
                    } else {
                        merged(Some(&originals[i]), attendee_prop(a), ATTENDEE_PARAMS)
                    }
                }
                None => attendee_prop(a),
            }
        })
        .collect();
    replace(c, &["ATTENDEE"], new);
}

fn conference_prop(link: &ConferenceLink) -> Property {
    let p = Property::new("CONFERENCE", link.uri.clone())
        .with_param("VALUE", "URI")
        .with_param_values(
            "FEATURE",
            link.features
                .iter()
                .map(|f| f.to_ascii_uppercase())
                .collect(),
        );
    match &link.label {
        Some(label) => p.with_param("LABEL", label.clone()),
        None => p,
    }
}

fn patch_conferences(c: &mut Component, desired: &[ConferenceLink]) {
    if ical::read_conferences(c) == desired {
        return;
    }
    let originals: Vec<Property> = c.properties_named("CONFERENCE").cloned().collect();
    // Vendor properties stay while their link does.
    for name in ical::CONFERENCE_VENDOR_PROPERTIES {
        c.properties
            .retain(|p| !p.is(name) || desired.iter().any(|d| d.uri == p.text().trim()));
    }
    let mut implicit: Vec<String> = Vec::new();
    for name in ical::CONFERENCE_VENDOR_PROPERTIES {
        implicit.extend(c.properties_named(name).map(|p| p.text().trim().to_owned()));
    }
    for name in ["LOCATION", "URL", "DESCRIPTION"] {
        for p in c.properties_named(name) {
            implicit.extend(ical::conference_links_in_text(&p.text()));
        }
    }
    let new = desired
        .iter()
        .filter(|d| !implicit.contains(&d.uri))
        .map(|d| {
            let original = originals.iter().find(|o| o.value.trim() == d.uri);
            let same = original.is_some_and(|o| {
                ical::read_conferences(&{
                    let mut one = Component::new("VEVENT");
                    one.properties.push(o.clone());
                    one
                })
                .first()
                    == Some(d)
            });
            match original {
                Some(o) if same => o.clone(),
                o => merged(o, conference_prop(d), &["VALUE", "FEATURE", "LABEL"]),
            }
        })
        .collect();
    replace(c, &["CONFERENCE"], new);
}

fn patch_end(
    c: &mut Component,
    start: &PimTime,
    end: &PimTime,
    end_name: &str,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
) {
    if ical::read_end(c, start, end_name, ctx) == *end {
        return;
    }
    let had_duration = c.property("DURATION").is_some() && c.property(end_name).is_none();
    let same_frame =
        std::mem::discriminant(start) == std::mem::discriminant(end) && start.tz() == end.tz();
    if had_duration
        && same_frame
        && let (Some(s), Some(e)) = (time::wall(start), time::wall(end))
    {
        let original = c.property("DURATION").cloned();
        let p = Property::new("DURATION", time::format_duration((e - s).num_seconds()));
        replace(c, &["DURATION"], vec![merged(original.as_ref(), p, &[])]);
        return;
    }
    let original = c.property(end_name).cloned();
    let new = zones
        .time_prop(end_name, end)
        .map(|p| merged(original.as_ref(), p, TIME_PARAMS));
    replace(c, &[end_name, "DURATION"], new.into_iter().collect());
}

fn patch_event_fields(
    c: &mut Component,
    fields: &EventFields,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
    options: &WriteOptions<'_>,
) {
    let changed_title = patch_text(
        c,
        "SUMMARY",
        Some(fields.title.as_str()).filter(|t| !t.is_empty()),
        &[],
    );
    let _ = changed_title;
    let current_start = c.property("DTSTART").and_then(|p| ctx.time(p));
    patch_one(
        c,
        "DTSTART",
        &current_start,
        &Some(fields.start.clone()),
        TIME_PARAMS,
        |s| s.as_ref().and_then(|s| zones.time_prop("DTSTART", s)),
    );
    patch_end(c, &fields.start, &fields.end, "DTEND", zones, ctx);
    patch_text(c, "LOCATION", fields.location.as_deref(), &["ALTREP"]);
    if patch_text(c, "DESCRIPTION", fields.description.as_deref(), &["ALTREP"]) {
        // An HTML copy of the old description would contradict the new one.
        replace(c, &["X-ALT-DESC"], Vec::new());
    }
    let status = ical::read_status(c);
    patch_one(c, "STATUS", &status, &fields.status, &[], |s| {
        Some(Property::new(
            "STATUS",
            match s {
                EventStatus::Tentative => "TENTATIVE",
                EventStatus::Confirmed => "CONFIRMED",
                EventStatus::Cancelled => "CANCELLED",
            },
        ))
    });
    if ical::read_transparency(c) != fields.transparency {
        let value = match fields.transparency {
            Transparency::Busy => "OPAQUE",
            Transparency::Free => "TRANSPARENT",
        };
        replace(
            c,
            &["TRANSP", "X-MICROSOFT-CDO-BUSYSTATUS"],
            vec![Property::new("TRANSP", value)],
        );
    }
    patch_visibility(c, fields.visibility);
    patch_priority(c, fields.priority);
    patch_categories(c, &fields.categories);
    patch_text(c, "COLOR", fields.color.as_deref(), &[]);
    patch_text(c, "URL", fields.url.as_deref(), &[]);
    patch_people(c, fields.organizer.as_ref(), &fields.attendees);
    patch_attachments(c, Some(&fields.attachments), &fields.links, options);
    patch_conferences(c, &fields.conferences);
    patch_alarms(c, &fields.alarms, &fields.title);
}

fn calendar_from(original: Option<&str>) -> Component {
    original
        .map(content::parse)
        .and_then(|doc| doc.components.into_iter().find(|c| c.is("VCALENDAR")))
        .unwrap_or_else(|| {
            let mut c = Component::new("VCALENDAR");
            c.properties.push(Property::new("VERSION", "2.0"));
            c.properties.push(Property::new("PRODID", crate::PRODID));
            c
        })
}

fn years(times: impl Iterator<Item = PimTime>) -> (i32, i32) {
    let years: Vec<i32> = times
        .filter_map(|t| time::wall(&t))
        .map(|w| w.year())
        .collect();
    let min = years.iter().copied().min().unwrap_or(2026);
    let max = years.iter().copied().max().unwrap_or(min);
    (min, max.max(min))
}

/// Adds `VTIMEZONE`s for zones that are used but not defined.
fn add_zones(calendar: &mut Component, zones: &Zones<'_>, from_year: i32, to_year: i32) {
    let defined: Vec<String> = calendar
        .components_named("VTIMEZONE")
        .filter_map(|z| z.text("TZID"))
        .collect();
    let mut referenced = Vec::new();
    for c in &calendar.components {
        if !c.is("VTIMEZONE") {
            ical::tzids(c, &mut referenced);
        }
    }
    let insert_at = calendar
        .components
        .iter()
        .position(|c| !c.is("VTIMEZONE"))
        .unwrap_or(calendar.components.len());
    let mut new = Vec::new();
    for (tzid, iana) in zones.used.borrow().iter() {
        if defined.contains(tzid) || !referenced.contains(tzid) {
            continue;
        }
        let Some(tz) = time::zone(iana) else {
            continue;
        };
        if let Some(mut zone) = crate::tz::vtimezone_for(tz, from_year - 1, to_year + 1) {
            for p in &mut zone.properties {
                if p.is("TZID") {
                    p.set_value(escape_text(tzid));
                }
            }
            new.push(zone);
        }
    }
    calendar.components.splice(insert_at..insert_at, new);
}

/// Writes an event series as iCalendar, patching `original` (the item's
/// stored text) when given.
pub fn write_event(
    event: &CalendarEvent,
    original: Option<&str>,
    options: &WriteOptions<'_>,
) -> String {
    let mut calendar = calendar_from(original);
    let resolver = ZoneResolver::new(
        &calendar,
        time::wall(&event.fields.start).map_or(2026, |w| w.year()),
    );
    let ctx = Ctx::new(&resolver, None);
    let zones = Zones {
        resolver: &resolver,
        used: RefCell::new(Vec::new()),
    };
    let start = &event.fields.start;
    let mut placed = vec![false; event.overrides.len()];
    let mut master_done = false;
    let mut components = Vec::new();
    for c in std::mem::take(&mut calendar.components) {
        let ours =
            c.is("VEVENT") && ical::read_text(&c, "UID").as_deref() == Some(event.uid.as_str());
        if !ours {
            components.push(c);
            continue;
        }
        match ical::read_recurrence_id(&c, Some(start), &ctx) {
            None if !master_done => {
                master_done = true;
                components.push(master_component(c, event, &zones, &ctx, options));
            }
            None => {} // A duplicate master is dropped.
            Some((rid, _)) => {
                if let Some(i) = event
                    .overrides
                    .iter()
                    .position(|o| o.recurrence_id == rid)
                    .filter(|&i| !placed[i])
                {
                    placed[i] = true;
                    components.push(override_component(c, event, i, &zones, &ctx, options));
                }
            }
        }
    }
    if !master_done {
        let fresh = Component::new("VEVENT");
        components.push(master_component(fresh, event, &zones, &ctx, options));
    }
    for (i, done) in placed.iter().enumerate() {
        if !done {
            let fresh = Component::new("VEVENT");
            components.push(override_component(fresh, event, i, &zones, &ctx, options));
        }
    }
    calendar.components = components;
    let (from, to) = years(
        std::iter::once(start.clone())
            .chain(event.overrides.iter().map(|o| o.fields.start.clone()))
            .chain(event.recurrence.iter().flat_map(|r| {
                r.rules.iter().filter_map(|rule| match &rule.end {
                    RecurrenceEnd::Until { until } => Some(until.clone()),
                    _ => None,
                })
            })),
    );
    add_zones(&mut calendar, &zones, from, to);
    calendar.to_text()
}

fn master_component(
    mut c: Component,
    event: &CalendarEvent,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
    options: &WriteOptions<'_>,
) -> Component {
    let before = c.to_text();
    patch_meta(
        &mut c,
        &event.uid,
        event.sequence,
        event.created_at.as_deref(),
        event.updated_at.as_deref(),
    );
    patch_event_fields(&mut c, &event.fields, zones, ctx, options);
    patch_recurrence(
        &mut c,
        &event.fields.start,
        event.recurrence.as_ref(),
        zones,
        ctx,
    );
    if let Some(related) = options.related_to
        && !c
            .properties_named("RELATED-TO")
            .any(|p| p.text() == related)
    {
        c.properties
            .push(Property::text_value("RELATED-TO", related).with_param("RELTYPE", "SIBLING"));
    }
    finish(&mut c, &before, options);
    c
}

fn override_component(
    mut c: Component,
    event: &CalendarEvent,
    index: usize,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
    options: &WriteOptions<'_>,
) -> Component {
    let before = c.to_text();
    let o = &event.overrides[index];
    // An override keeps its own CREATED / LAST-MODIFIED; its SEQUENCE
    // follows the series when that was bumped.
    let created = read_instant(&c, "CREATED");
    let updated = read_instant(&c, "LAST-MODIFIED");
    let sequence = ical::read_sequence(&c).max(event.sequence);
    patch_meta(
        &mut c,
        &event.uid,
        sequence,
        created.as_deref(),
        updated.as_deref(),
    );
    if c.property("RECURRENCE-ID").is_none()
        && let Some(p) = zones.time_prop("RECURRENCE-ID", &o.recurrence_id)
    {
        c.properties.push(p);
    }
    patch_event_fields(&mut c, &o.fields, zones, ctx, options);
    finish(&mut c, &before, options);
    c
}

// ---------------------------------------------------------------------------
// Tasks

fn patch_task_fields(
    c: &mut Component,
    fields: &TaskFields,
    parent_uid: Option<&str>,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
    options: &WriteOptions<'_>,
) {
    patch_text(
        c,
        "SUMMARY",
        Some(fields.title.as_str()).filter(|t| !t.is_empty()),
        &[],
    );
    patch_text(c, "DESCRIPTION", fields.description.as_deref(), &["ALTREP"]);
    let current_start = c.property("DTSTART").and_then(|p| ctx.time(p));
    patch_one(
        c,
        "DTSTART",
        &current_start,
        &fields.start,
        TIME_PARAMS,
        |s| s.as_ref().and_then(|s| zones.time_prop("DTSTART", s)),
    );
    match (&fields.start, &fields.due) {
        (Some(start), Some(due)) => patch_end(c, start, due, "DUE", zones, ctx),
        (None, Some(due)) => {
            let current = c.property("DUE").and_then(|p| ctx.time(p));
            if current.as_ref() != Some(due) {
                let original = c.property("DUE").cloned();
                let new = zones
                    .time_prop("DUE", due)
                    .map(|p| merged(original.as_ref(), p, TIME_PARAMS));
                replace(c, &["DUE", "DURATION"], new.into_iter().collect());
            }
        }
        (_, None) => replace(c, &["DUE", "DURATION"], Vec::new()),
    }
    let status = ical::read_task_status(c);
    patch_one(c, "STATUS", &status, &fields.status, &[], |s| {
        Some(Property::new(
            "STATUS",
            match s {
                TaskStatus::NeedsAction => "NEEDS-ACTION",
                TaskStatus::InProcess => "IN-PROCESS",
                TaskStatus::Completed => "COMPLETED",
                TaskStatus::Cancelled => "CANCELLED",
            },
        ))
    });
    let percent = c
        .property("PERCENT-COMPLETE")
        .and_then(|p| p.value.trim().parse::<u8>().ok())
        .map(|p| p.min(100));
    patch_one(
        c,
        "PERCENT-COMPLETE",
        &percent,
        &fields.percent_complete,
        &[],
        |p| p.map(|p| Property::new("PERCENT-COMPLETE", p.to_string())),
    );
    patch_priority(c, fields.priority);
    let completed = read_instant(c, "COMPLETED");
    patch_one(
        c,
        "COMPLETED",
        &completed,
        &normalized_instant(fields.completed_at.as_deref()),
        &[],
        |v| utc_prop("COMPLETED", v.as_deref()),
    );
    patch_categories(c, &fields.categories);
    patch_text(c, "LOCATION", fields.location.as_deref(), &["ALTREP"]);
    patch_text(c, "URL", fields.url.as_deref(), &[]);
    patch_visibility(c, fields.visibility);
    let order = c
        .property("X-APPLE-SORT-ORDER")
        .and_then(|p| p.value.trim().parse::<i64>().ok());
    patch_one(c, "X-APPLE-SORT-ORDER", &order, &fields.order, &[], |o| {
        o.map(|o| Property::new("X-APPLE-SORT-ORDER", o.to_string()))
    });
    let current_parent = ical::read_parent_uid(c);
    if current_parent.as_deref() != parent_uid {
        // Only the parent link is Nail's; other relations stay.
        let is_parent = |p: &Property| {
            p.is("RELATED-TO")
                && p.param("RELTYPE")
                    .is_none_or(|r| r.eq_ignore_ascii_case("PARENT"))
        };
        let position = c.properties.iter().position(is_parent);
        c.properties.retain(|p| !is_parent(p));
        if let Some(parent) = parent_uid {
            let at = position
                .unwrap_or(c.properties.len())
                .min(c.properties.len());
            c.properties
                .insert(at, Property::text_value("RELATED-TO", parent));
        }
    }
    patch_attachments(c, None, &fields.links, options);
    patch_alarms(c, &fields.alarms, &fields.title);
}

/// Writes a task as iCalendar, patching `original` when given.
/// `parent_uid` is the parent task's `UID` (the store maps
/// [`TaskFields::parent_id`] to it).
pub fn write_task(
    task: &Task,
    parent_uid: Option<&str>,
    original: Option<&str>,
    options: &WriteOptions<'_>,
) -> String {
    let mut calendar = calendar_from(original);
    let anchor = task
        .fields
        .start
        .clone()
        .or_else(|| task.fields.due.clone());
    let year = anchor
        .as_ref()
        .and_then(time::wall)
        .map_or(2026, |w| w.year());
    let resolver = ZoneResolver::new(&calendar, year);
    let ctx = Ctx::new(&resolver, None);
    let zones = Zones {
        resolver: &resolver,
        used: RefCell::new(Vec::new()),
    };
    let mut done = false;
    let mut components = Vec::new();
    for c in std::mem::take(&mut calendar.components) {
        let ours =
            c.is("VTODO") && ical::read_text(&c, "UID").as_deref() == Some(task.uid.as_str());
        if !ours || c.property("RECURRENCE-ID").is_some() {
            components.push(c);
            continue;
        }
        if !done {
            done = true;
            components.push(task_component(
                c,
                task,
                parent_uid,
                anchor.as_ref(),
                &zones,
                &ctx,
                options,
            ));
        }
    }
    if !done {
        let fresh = Component::new("VTODO");
        components.push(task_component(
            fresh,
            task,
            parent_uid,
            anchor.as_ref(),
            &zones,
            &ctx,
            options,
        ));
    }
    calendar.components = components;
    add_zones(&mut calendar, &zones, year, year + 1);
    calendar.to_text()
}

fn task_component(
    mut c: Component,
    task: &Task,
    parent_uid: Option<&str>,
    anchor: Option<&PimTime>,
    zones: &Zones<'_>,
    ctx: &Ctx<'_>,
    options: &WriteOptions<'_>,
) -> Component {
    let before = c.to_text();
    let sequence = ical::read_sequence(&c);
    patch_meta(
        &mut c,
        &task.uid,
        sequence,
        task.created_at.as_deref(),
        task.updated_at.as_deref(),
    );
    patch_task_fields(&mut c, &task.fields, parent_uid, zones, ctx, options);
    if let Some(anchor) = anchor {
        patch_recurrence(&mut c, anchor, task.recurrence.as_ref(), zones, ctx);
    } else {
        replace(&mut c, &["RRULE", "RDATE", "EXDATE"], Vec::new());
    }
    finish(&mut c, &before, options);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::{ParseOptions, parse_calendar};
    use nail_api::pim::calendar::{Frequency, ParticipationStatus};
    use nail_api::pim::source::PimItemSync;
    use nail_api::pim::{EventOverride, RecurrenceRule};
    use pretty_assertions::assert_eq;

    fn now() -> DateTime<Utc> {
        time::parse_instant("2026-10-01T12:00:00Z").unwrap()
    }

    fn event_of(text: &str) -> CalendarEvent {
        let parsed = parse_calendar(text, ParseOptions::default());
        assert_eq!(parsed.events.len(), 1, "{:?}", parsed.issues);
        parsed.events.into_iter().next().unwrap().into_event(
            "e1".into(),
            "c1".into(),
            false,
            PimItemSync::Synced,
        )
    }

    const GOOGLE: &str = concat!(
        "BEGIN:VCALENDAR\r\n\
PRODID:-//Google Inc//Google Calendar 70.9054//EN\r\n\
VERSION:2.0\r\n\
CALSCALE:GREGORIAN\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:W. Europe Standard Time\r\n\
",
        crate::cet_rules!(),
        "END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
DTSTART;TZID=W. Europe Standard Time:20261005T090000\r\n\
DTEND;TZID=W. Europe Standard Time:20261005T093000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
DTSTAMP:20261001T080000Z\r\n\
UID:abc123@google.com\r\n\
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=ada@exa\r\n\
\x20mple.org;X-NUM-GUESTS=0:mailto:ada@example.org\r\n\
X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\n\
CREATED:20260901T100000Z\r\n\
DESCRIPTION:Weekly sync\r\n\
LAST-MODIFIED:20260902T100000Z\r\n\
LOCATION:\r\n\
SEQUENCE:0\r\n\
STATUS:CONFIRMED\r\n\
SUMMARY:Sync\r\n\
TRANSP:OPAQUE\r\n\
X-UNKNOWN;x-param=lower:kept as is\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
DESCRIPTION:This is an event reminder\r\n\
TRIGGER:-P0DT0H10M0S\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n"
    );

    #[test]
    fn unchanged_events_are_written_byte_for_byte() {
        let event = event_of(GOOGLE);
        assert_eq!(
            write_event(&event, Some(GOOGLE), &WriteOptions::at(now())),
            GOOGLE
        );
    }

    #[test]
    fn edits_touch_only_changed_properties() {
        let mut event = event_of(GOOGLE);
        event.fields.title = "Sync, renamed".into();
        event.fields.start = PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin");
        event.fields.end = PimTime::zoned("2026-10-05T10:30:00", "Europe/Berlin");
        event.fields.attendees[0].status = ParticipationStatus::Declined;
        event.sequence = 1;
        let text = write_event(&event, Some(GOOGLE), &WriteOptions::at(now()));
        // Kept: unknown property, vendor conference, the zone name, the alarm.
        assert!(
            text.contains("X-UNKNOWN;x-param=lower:kept as is\r\n"),
            "{text}"
        );
        assert!(text.contains("X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\n"));
        assert!(
            text.contains("DTSTART;TZID=W. Europe Standard Time:20261005T100000\r\n"),
            "{text}"
        );
        assert!(text.contains("DTEND;TZID=W. Europe Standard Time:20261005T103000\r\n"));
        assert!(text.contains("TRIGGER:-P0DT0H10M0S\r\n"));
        assert!(text.contains("SUMMARY:Sync\\, renamed\r\n"));
        assert!(text.contains("SEQUENCE:1\r\n"));
        assert!(text.contains("DTSTAMP:20261001T120000Z\r\n"));
        // The attendee keeps X-NUM-GUESTS, PARTSTAT changes.
        let doc = content::parse(&text);
        let vevent = doc.components[0].components_named("VEVENT").next().unwrap();
        let attendee = vevent.property("ATTENDEE").unwrap();
        assert_eq!(attendee.param("X-NUM-GUESTS"), Some("0"));
        assert_eq!(attendee.param("PARTSTAT"), Some("DECLINED"));
        // No duplicate VTIMEZONE for Berlin: the original one is reused.
        assert_eq!(text.matches("BEGIN:VTIMEZONE").count(), 1);
        // And it reads back as the edited event.
        let again = event_of(&text);
        assert_eq!(again.fields, event.fields);
        assert_eq!(again.sequence, 1);
        // Writing that again changes nothing.
        assert_eq!(
            write_event(&again, Some(&text), &WriteOptions::at(now())),
            text
        );
    }

    #[test]
    fn new_events_are_complete() {
        let mut fields = EventFields::new(
            "Kick-off",
            PimTime::zoned("2026-11-02T09:00:00", "America/New_York"),
            PimTime::zoned("2026-11-02T10:00:00", "America/New_York"),
        );
        fields.alarms.push(Alarm::minutes_before(15));
        fields.attendees.push(Attendee::new("bob@example.org"));
        fields.conferences.push(ConferenceLink {
            uri: "https://zoom.us/j/9".into(),
            label: None,
            features: vec!["video".into()],
        });
        fields.links.push(PimLink::Message {
            message_id: Some("local-1".into()),
            rfc822_id: Some("m1@example.org".into()),
            subject: Some("Plan".into()),
        });
        let mut rule = RecurrenceRule::new(Frequency::Weekly);
        rule.end = RecurrenceEnd::Until {
            until: PimTime::zoned("2026-12-31T09:00:00", "America/New_York"),
        };
        let event = CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "new-1@nail".into(),
            fields,
            recurrence: Some(Recurrence {
                rules: vec![rule],
                dates: vec![],
                exceptions: vec![PimTime::zoned("2026-11-09T09:00:00", "America/New_York")],
            }),
            overrides: vec![EventOverride {
                recurrence_id: PimTime::zoned("2026-11-16T09:00:00", "America/New_York"),
                fields: EventFields::new(
                    "Kick-off (moved)",
                    PimTime::zoned("2026-11-17T09:00:00", "America/New_York"),
                    PimTime::zoned("2026-11-17T10:00:00", "America/New_York"),
                ),
                this_and_future: false,
            }],
            sequence: 0,
            created_at: Some("2026-10-01T12:00:00Z".into()),
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Pending,
        };
        let options = WriteOptions {
            related_to: Some("old@nail"),
            ..WriteOptions::at(now())
        };
        let text = write_event(&event, None, &options);
        assert!(
            text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Nail PIM//EN\r\n"),
            "{text}"
        );
        assert!(
            text.contains("BEGIN:VTIMEZONE\r\nTZID:America/New_York\r\n"),
            "{text}"
        );
        assert!(
            text.contains("RRULE:FREQ=WEEKLY;UNTIL=20261231T140000Z\r\n"),
            "{text}"
        );
        assert!(text.contains("EXDATE;TZID=America/New_York:20261109T090000\r\n"));
        assert!(text.contains("RECURRENCE-ID;TZID=America/New_York:20261116T090000\r\n"));
        assert!(
            text.contains(
                "ATTACH;FMTTYPE=message/rfc822;X-NAIL-SUBJECT=Plan:mid:m1@example.org\r\n"
            )
        );
        assert!(text.contains("CONFERENCE;VALUE=URI;FEATURE=VIDEO:https://zoom.us/j/9\r\n"));
        assert!(text.contains("RELATED-TO;RELTYPE=SIBLING:old@nail\r\n"));
        assert!(
            text.contains("DESCRIPTION:Kick-off\r\n"),
            "display alarms get the title"
        );
        assert_eq!(text.matches("DTSTAMP:20261001T120000Z").count(), 2);
        let back = event_of(&text);
        let mut expected = event.clone();
        expected.fields.links = without_local_ids(&event.fields.links);
        expected.recurrence.as_mut().unwrap().rules[0].end = RecurrenceEnd::Until {
            until: PimTime::utc("2026-12-31T14:00:00"),
        };
        assert_eq!(back.fields, expected.fields);
        assert_eq!(back.recurrence, expected.recurrence);
        assert_eq!(back.overrides, expected.overrides);
        assert_eq!(back.created_at, event.created_at);
        for line in text.split("\r\n") {
            assert!(line.len() <= content::FOLD_OCTETS);
        }
    }

    #[test]
    fn removing_things_removes_their_properties() {
        let mut event = event_of(GOOGLE);
        event.fields.alarms.clear();
        event.fields.attendees.clear();
        event.fields.conferences.clear();
        event.fields.description = None;
        event.recurrence = None;
        let text = write_event(&event, Some(GOOGLE), &WriteOptions::at(now()));
        let vevent = &text[text.find("BEGIN:VEVENT").unwrap()..];
        for gone in [
            "VALARM",
            "ATTENDEE",
            "X-GOOGLE-CONFERENCE",
            "DESCRIPTION",
            "RRULE",
        ] {
            assert!(!vevent.contains(gone), "{gone} in {text}");
        }
        assert!(vevent.contains("DTSTAMP:20261001T120000Z"), "{text}");
        assert!(text.contains("X-UNKNOWN"));
        assert_eq!(event_of(&text).fields, event.fields);
    }

    #[test]
    fn durations_stay_durations() {
        let original = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:d\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261005T090000Z\r\nDURATION:PT1H\r\nSUMMARY:x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut event = event_of(original);
        event.fields.start = PimTime::utc("2026-10-06T09:00:00");
        event.fields.end = PimTime::utc("2026-10-06T10:00:00");
        let text = write_event(&event, Some(original), &WriteOptions::at(now()));
        assert!(
            text.contains("DURATION:PT1H\r\n") && !text.contains("DTEND"),
            "{text}"
        );
        event.fields.end = PimTime::utc("2026-10-06T11:30:00");
        let text = write_event(&event, Some(original), &WriteOptions::at(now()));
        assert!(text.contains("DURATION:PT2H30M\r\n"), "{text}");
        assert_eq!(event_of(&text).fields.end, event.fields.end);
    }

    #[test]
    fn overrides_are_added_kept_and_removed() {
        let original = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:s\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\nRRULE:FREQ=DAILY;COUNT=10\r\nSUMMARY:x\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nDTSTAMP:20261001T000000Z\r\nRECURRENCE-ID:20261006T090000Z\r\nDTSTART:20261006T110000Z\r\nDTEND:20261006T120000Z\r\nSUMMARY:moved\r\nX-KEEP:1\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nDTSTAMP:20261001T000000Z\r\nRECURRENCE-ID:20261007T090000Z\r\nDTSTART:20261007T110000Z\r\nDTEND:20261007T120000Z\r\nSUMMARY:gone\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut event = event_of(original);
        assert_eq!(event.overrides.len(), 2);
        event.overrides.remove(1);
        let mut extra = event.fields.clone();
        extra.title = "third".into();
        extra.start = PimTime::utc("2026-10-08T15:00:00");
        extra.end = PimTime::utc("2026-10-08T16:00:00");
        event.overrides.push(EventOverride {
            recurrence_id: PimTime::utc("2026-10-08T09:00:00"),
            fields: extra,
            this_and_future: false,
        });
        let text = write_event(&event, Some(original), &WriteOptions::at(now()));
        assert!(text.contains("SUMMARY:moved\r\nX-KEEP:1\r\n"), "{text}");
        assert!(!text.contains("gone"));
        assert!(text.contains("RECURRENCE-ID:20261008T090000Z\r\n"));
        assert_eq!(event_of(&text).overrides, event.overrides);
    }

    #[test]
    fn tasks_round_trip_and_patch() {
        let original = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//iOS 17//EN\r\nBEGIN:VTODO\r\nUID:t1\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Buy milk\r\nDUE;VALUE=DATE:20261010\r\nRELATED-TO:parent-1\r\nRELATED-TO;RELTYPE=SIBLING:other\r\nX-APPLE-SORT-ORDER:5\r\nATTACH:https://example.org/list.pdf\r\nX-APPLE-UNKNOWN:1\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let parsed = parse_calendar(original, ParseOptions::default());
        let p = parsed.tasks[0].clone();
        let parent_uid = p.parent_uid.clone();
        let task = p.into_task(
            "t".into(),
            "c".into(),
            None,
            vec![],
            false,
            PimItemSync::Synced,
        );
        assert_eq!(
            write_task(
                &task,
                parent_uid.as_deref(),
                Some(original),
                &WriteOptions::at(now())
            ),
            original
        );
        let mut edited = task.clone();
        edited.fields.status = TaskStatus::Completed;
        edited.fields.completed_at = Some("2026-10-02T08:00:00Z".into());
        edited.fields.percent_complete = Some(100);
        edited.fields.links.push(PimLink::Message {
            message_id: None,
            rfc822_id: Some("x@y".into()),
            subject: None,
        });
        let text = write_task(&edited, None, Some(original), &WriteOptions::at(now()));
        assert!(
            text.contains("STATUS:COMPLETED\r\n")
                && text.contains("COMPLETED:20261002T080000Z\r\n")
        );
        assert!(
            text.contains("ATTACH:https://example.org/list.pdf\r\n"),
            "{text}"
        );
        assert!(text.contains("ATTACH;FMTTYPE=message/rfc822:mid:x@y\r\n"));
        assert!(!text.contains("RELATED-TO:parent-1"));
        assert!(text.contains("RELATED-TO;RELTYPE=SIBLING:other\r\n"));
        assert!(text.contains("X-APPLE-UNKNOWN:1\r\n"));
        let back = parse_calendar(&text, ParseOptions::default())
            .tasks
            .remove(0);
        assert_eq!(back.fields, edited.fields);
        assert_eq!(back.parent_uid, None);
        // A new task.
        let mut fresh = task.clone();
        fresh.uid = "t-new".into();
        fresh.fields = TaskFields::titled("New");
        fresh.fields.start = Some(PimTime::zoned("2026-10-01T09:00:00", "Asia/Tokyo"));
        fresh.fields.due = Some(PimTime::zoned("2026-10-02T09:00:00", "Asia/Tokyo"));
        fresh.recurrence = Some(Recurrence::rule(RecurrenceRule::new(Frequency::Monthly)));
        let text = write_task(&fresh, Some("p"), None, &WriteOptions::at(now()));
        assert!(text.contains("TZID:Asia/Tokyo"), "{text}");
        let back = parse_calendar(&text, ParseOptions::default())
            .tasks
            .remove(0);
        assert_eq!(back.fields, fresh.fields);
        assert_eq!(back.parent_uid.as_deref(), Some("p"));
        assert_eq!(back.recurrence, fresh.recurrence);
    }

    #[test]
    fn inline_attachments_need_bytes() {
        let mut event = event_of(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261005T090000Z\r\nSUMMARY:x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let sha = ical::sha256_hex(b"data");
        event.fields.attachments.push(PimAttachment {
            name: Some("d.bin".into()),
            mime_type: None,
            size: Some(4),
            url: None,
            sha256: Some(sha.clone()),
        });
        let without = write_event(&event, None, &WriteOptions::at(now()));
        assert!(!without.contains("ATTACH"));
        let blobs = |s: &str| (s == sha).then(|| b"data".to_vec());
        let options = WriteOptions {
            blobs: Some(&blobs),
            ..WriteOptions::at(now())
        };
        let with = write_event(&event, None, &options);
        assert!(
            with.contains("ATTACH;ENCODING=BASE64;VALUE=BINARY;FILENAME=d.bin:ZGF0YQ==\r\n"),
            "{with}"
        );
        assert_eq!(event_of(&with).fields.attachments, event.fields.attachments);
    }
}
