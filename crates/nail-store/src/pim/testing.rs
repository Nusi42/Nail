//! Test fixtures for the PIM store: collections in the local source and a
//! CalDAV / CardDAV source, item builders, and a small recurrence expander
//! (daily / weekly rules in the event's zone; the real one is `nail-ical`).

use chrono::{Duration, NaiveDateTime};
use nail_api::pim::calendar::Frequency;
use nail_api::pim::contact::VCardVersion;
use nail_api::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarComponent, CalendarEvent, CalendarSettings,
    Contact, ContactFields, ContactGroup, EventFields, LabeledValue, PimItemSync, PimSourceKind,
    PimTime, Recurrence, RecurrenceEnd, RecurrenceRule, Task, TaskFields,
};
use nail_api::{AddressBookId, CalendarId, PimSourceId};

use super::instances::{ExpandedInstance, InstanceExpander};
use super::time::{cache_time, parse_local, parse_zone, resolve_wall, time_like};
use crate::store::Store;

/// Local calendar.
pub const CAL: &str = "cal";
/// Local address book.
pub const BOOK: &str = "book";
/// CalDAV calendar of source [`DAV`].
pub const DAV_CAL: &str = "dcal";
/// CardDAV address book of source [`DAV`].
pub const DAV_BOOK: &str = "dbook";
/// A CalDAV source (holds both kinds of collection in tests).
pub const DAV: &str = "dav";
/// A subscription calendar (read-only).
pub const ICS_CAL: &str = "ics";

/// Builds items.
pub struct Fixtures;

impl Fixtures {
    pub fn new() -> Self {
        Self
    }

    /// A one-hour event in the local calendar on 2026-10-01 09:00 Berlin.
    pub fn event(&self, id: &str, uid: &str, title: &str) -> CalendarEvent {
        event_at(
            id,
            uid,
            title,
            PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-01T10:00:00", "Europe/Berlin"),
        )
    }
}

/// An event in [`CAL`].
pub fn event_at(id: &str, uid: &str, title: &str, start: PimTime, end: PimTime) -> CalendarEvent {
    CalendarEvent {
        id: id.into(),
        calendar_id: CAL.into(),
        uid: uid.into(),
        fields: EventFields::new(title, start, end),
        recurrence: None,
        overrides: Vec::new(),
        sequence: 0,
        created_at: None,
        updated_at: None,
        read_only: false,
        sync: PimItemSync::Synced,
    }
}

/// A daily rule.
pub fn daily(end: RecurrenceEnd) -> Recurrence {
    let mut rule = RecurrenceRule::new(Frequency::Daily);
    rule.end = end;
    Recurrence::rule(rule)
}

/// A task in [`CAL`].
pub fn task(id: &str, uid: &str, title: &str) -> Task {
    Task {
        id: id.into(),
        calendar_id: CAL.into(),
        uid: uid.into(),
        fields: TaskFields::titled(title),
        recurrence: None,
        subtask_ids: Vec::new(),
        created_at: None,
        updated_at: None,
        read_only: false,
        sync: PimItemSync::Synced,
    }
}

/// A contact in [`BOOK`] with emails.
pub fn contact(id: &str, uid: &str, name: &str, emails: &[&str]) -> Contact {
    let mut fields = ContactFields::named(name);
    fields.emails = emails.iter().map(|e| LabeledValue::new(*e)).collect();
    Contact {
        id: id.into(),
        address_book_id: BOOK.into(),
        uid: uid.into(),
        fields,
        version: VCardVersion::V4,
        group_ids: Vec::new(),
        linked_ids: Vec::new(),
        updated_at: None,
        read_only: false,
        sync: PimItemSync::Synced,
    }
}

/// A group in [`BOOK`].
pub fn group(id: &str, uid: &str, name: &str, member_ids: &[&str]) -> ContactGroup {
    ContactGroup {
        id: id.into(),
        address_book_id: BOOK.into(),
        uid: uid.into(),
        name: name.into(),
        member_ids: member_ids.iter().map(|m| (*m).into()).collect(),
        member_emails: Vec::new(),
        read_only: false,
        sync: PimItemSync::Synced,
    }
}

fn calendar(id: &str, source: &str, read_only: bool) -> Calendar {
    Calendar {
        id: CalendarId::new(id),
        source_id: PimSourceId::new(source),
        account_id: None,
        settings: CalendarSettings::named(id),
        components: vec![CalendarComponent::Event, CalendarComponent::Task],
        read_only,
        timezone: None,
        exchange: None,
    }
}

fn book(id: &str, source: &str) -> AddressBook {
    AddressBook {
        id: AddressBookId::new(id),
        source_id: PimSourceId::new(source),
        account_id: None,
        settings: AddressBookSettings::named(id),
        read_only: false,
        role: None,
        exchange: None,
    }
}

/// A store with [`CAL`], [`BOOK`], [`DAV_CAL`], [`DAV_BOOK`] and
/// [`ICS_CAL`].
pub fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .ensure_pim_source(&PimSourceId::new(DAV), PimSourceKind::Caldav, 0)
        .unwrap();
    store
        .ensure_pim_source(&PimSourceId::new("subs"), PimSourceKind::Ics, 0)
        .unwrap();
    store
        .save_calendar(&calendar(CAL, "local", false), None)
        .unwrap();
    store
        .save_calendar(&calendar(DAV_CAL, DAV, false), Some("/cal/"))
        .unwrap();
    store
        .save_calendar(
            &calendar(ICS_CAL, "subs", true),
            Some("https://example.org/h.ics"),
        )
        .unwrap();
    store.save_address_book(&book(BOOK, "local"), None).unwrap();
    store
        .save_address_book(&book(DAV_BOOK, DAV), Some("/card/"))
        .unwrap();
    store
}

/// Raw text standing in for the iCalendar / vCard of an item.
pub fn raw(tag: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nX-TEST:{tag}\r\nEND:VCALENDAR\r\n")
}

/// Unix seconds of an RFC 3339 instant.
pub fn at(text: &str) -> i64 {
    super::time::parse_instant(text).unwrap()
}

/// Expands daily / weekly rules (interval, COUNT / UNTIL), RDATEs,
/// EXDATEs and overrides, stepping the wall clock in the event's zone so
/// DST is right. Counts expansions.
#[derive(Default)]
pub struct TestExpander {
    pub calls: std::sync::atomic::AtomicUsize,
}

impl InstanceExpander for TestExpander {
    fn expand(&self, event: &CalendarEvent, from: i64, until: i64) -> Vec<ExpandedInstance> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let start = &event.fields.start;
        let duration =
            cache_time(&event.fields.end).unwrap().secs - cache_time(start).unwrap().secs;
        let wall_of = |time: &PimTime| -> NaiveDateTime {
            match time {
                PimTime::Date { date } => super::time::parse_date(date)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                PimTime::Floating { local } | PimTime::Zoned { local, .. } => {
                    parse_local(local).unwrap()
                }
            }
        };
        let at_wall = |wall: NaiveDateTime| -> PimTime {
            match start {
                PimTime::Date { .. } => PimTime::date(wall.format("%Y-%m-%d").to_string()),
                PimTime::Floating { .. } => {
                    PimTime::floating(wall.format("%Y-%m-%dT%H:%M:%S").to_string())
                }
                PimTime::Zoned { tz, .. } => {
                    let zone = parse_zone(tz).unwrap();
                    let instant = resolve_wall(&zone, wall).timestamp();
                    time_like(start, instant)
                }
            }
        };
        let recurrence = event.recurrence.clone().unwrap_or_default();
        let mut walls: Vec<NaiveDateTime> = Vec::new();
        let first = wall_of(start);
        walls.push(first);
        for rule in &recurrence.rules {
            let step = match rule.freq {
                Frequency::Daily => Duration::days(i64::from(rule.interval)),
                Frequency::Weekly => Duration::weeks(i64::from(rule.interval)),
                _ => panic!("test expander: daily / weekly only"),
            };
            let (count, until_wall) = match &rule.end {
                RecurrenceEnd::Never => (u32::MAX, None),
                RecurrenceEnd::Count { count } => (*count, None),
                RecurrenceEnd::Until { until } => (u32::MAX, Some(cache_time(until).unwrap())),
            };
            let mut wall = first;
            for _ in 1..count {
                wall += step;
                let time = at_wall(wall);
                let cached = cache_time(&time).unwrap();
                if let Some(limit) = until_wall
                    && cached.secs > limit.secs
                {
                    break;
                }
                if cached.secs >= until + 86_400 * 2 {
                    break;
                }
                walls.push(wall);
            }
        }
        walls.extend(recurrence.dates.iter().map(wall_of));
        walls.sort();
        walls.dedup();
        let mut out = Vec::new();
        for wall in walls {
            let rid = at_wall(wall);
            if recurrence.exceptions.iter().any(|x| wall_of(x) == wall) {
                continue;
            }
            let (start_time, end_time, overridden, cancelled) = match event
                .overrides
                .iter()
                .find(|o| wall_of(&o.recurrence_id) == wall)
            {
                Some(o) => (
                    o.fields.start.clone(),
                    o.fields.end.clone(),
                    true,
                    o.fields.status == nail_api::pim::calendar::EventStatus::Cancelled,
                ),
                None => {
                    let s = cache_time(&rid).unwrap().secs;
                    (rid.clone(), time_like(start, s + duration), false, false)
                }
            };
            let s = cache_time(&start_time).unwrap().secs;
            if s < from || s >= until {
                continue;
            }
            out.push(ExpandedInstance {
                recurrence_id: event.is_recurring().then_some(rid),
                start: start_time,
                end: end_time,
                overridden,
                cancelled,
            });
        }
        out
    }
}
