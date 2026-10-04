//! The real-world-shaped corpus (`tests/corpus`, synthesized exports of
//! Google, Outlook, Apple and Thunderbird; see its README): every file is
//! read without unexpected problems, its items mean what the producer
//! meant, unchanged items are written back byte for byte, and an edit
//! keeps every other line of the original.
#![cfg(test)]

use std::fmt::Write as _;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use nail_api::pim::calendar::{AlarmRelated, AttendeeKind, EventStatus, Transparency, Visibility};
use nail_api::pim::contact::{ContactDate, VCardVersion};
use nail_api::pim::source::PimItemSync;
use nail_api::pim::{AlarmAction, AlarmTrigger, CalendarEvent, Contact, PimTime, Task, TaskStatus};
use nail_ical::alarm::{event_firings, task_firings};
use nail_ical::content::{self, Component};
use nail_ical::expand::{ExpandOptions, instances};
use nail_ical::vcard::{
    CardWriteOptions, GroupStyle, ParsedCards, parse_card_bytes, uid_key, write_contact,
    write_group,
};
use nail_ical::write::{WriteOptions, write_event, write_task};
use nail_ical::{IssueKind, ParsedCalendar, ParsedEvent, ParsedTask, feed, import};
use pretty_assertions::assert_eq;

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name)
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn calendar(name: &str) -> ParsedCalendar {
    match import::read_bytes(&bytes(name), Some(name)) {
        import::ImportData::Calendar(c) => c,
        other @ import::ImportData::Cards(_) => panic!("{name}: {other:?}"),
    }
}

fn cards(name: &str) -> ParsedCards {
    parse_card_bytes(&bytes(name))
}

fn utc(text: &str) -> DateTime<Utc> {
    nail_ical::time::parse_instant(text).unwrap()
}

fn now() -> DateTime<Utc> {
    utc("2026-10-01T12:00:00Z")
}

fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn event(parsed: &ParsedEvent) -> CalendarEvent {
    parsed
        .clone()
        .into_event("e".into(), "c".into(), false, PimItemSync::Synced)
}

fn task(parsed: &ParsedTask) -> Task {
    parsed.clone().into_task(
        "t".into(),
        "c".into(),
        None,
        vec![],
        false,
        PimItemSync::Synced,
    )
}

fn contact(cards: &ParsedCards, index: usize) -> Contact {
    let c = &cards.contacts[index];
    Contact {
        id: "c".into(),
        address_book_id: "b".into(),
        uid: c.uid.clone(),
        fields: c.fields.clone(),
        version: c.version,
        group_ids: vec![],
        linked_ids: vec![],
        updated_at: c.updated_at.clone(),
        read_only: false,
        sync: PimItemSync::Synced,
    }
}

fn find<'a>(cal: &'a ParsedCalendar, uid: &str) -> &'a ParsedEvent {
    cal.events
        .iter()
        .find(|e| e.uid == uid)
        .unwrap_or_else(|| panic!("no event {uid}"))
}

fn month(event: &CalendarEvent, from: &str, to: &str, viewer: Tz) -> Vec<(String, String)> {
    instances(event, &ExpandOptions::range(utc(from), utc(to), viewer))
        .into_iter()
        .map(|i| (i.start_utc, i.title))
        .collect()
}

/// Original text of every property (recursively) except `skip`.
fn original_lines(component: &Component, skip: &[&str], out: &mut Vec<String>) {
    for p in &component.properties {
        if !skip.iter().any(|s| p.is(s))
            && let Some(text) = p.original_text()
        {
            out.push(text.to_owned());
        }
    }
    for c in &component.components {
        original_lines(c, skip, out);
    }
}

/// Unchanged items are written back exactly; a changed title changes
/// only `SUMMARY` / `DTSTAMP` (and keeps everything else as written).
fn check_calendar_items(cal: &ParsedCalendar) {
    for parsed in &cal.events {
        let e = event(parsed);
        let options = WriteOptions::at(now());
        assert_eq!(
            write_event(&e, Some(&parsed.raw), &options),
            crlf(&parsed.raw),
            "{} is not written back unchanged",
            parsed.uid
        );
        let mut edited = e.clone();
        edited.fields.title = format!("{} (edited)", e.fields.title);
        let out = write_event(&edited, Some(&parsed.raw), &options);
        let mut lines = Vec::new();
        for c in content::parse(&parsed.raw).components {
            original_lines(&c, &["SUMMARY", "DTSTAMP"], &mut lines);
        }
        for line in lines {
            assert!(out.contains(&line), "{}: lost {line:?}\n{out}", parsed.uid);
        }
        let again = nail_ical::parse_calendar(&out, nail_ical::ParseOptions::default());
        let back = find(&again, &parsed.uid);
        assert_eq!(back.fields, edited.fields, "{}", parsed.uid);
        assert_eq!(back.recurrence, edited.recurrence);
        assert_eq!(back.overrides, edited.overrides);
        assert_eq!(back.this_and_future, parsed.this_and_future);
    }
    for parsed in &cal.tasks {
        let t = task(parsed);
        let options = WriteOptions::at(now());
        let parent = parsed.parent_uid.as_deref();
        assert_eq!(
            write_task(&t, parent, Some(&parsed.raw), &options),
            crlf(&parsed.raw),
            "{}",
            parsed.uid
        );
        let mut edited = t.clone();
        edited.fields.percent_complete = Some(50);
        let out = write_task(&edited, parent, Some(&parsed.raw), &options);
        let back = nail_ical::parse_calendar(&out, nail_ical::ParseOptions::default())
            .tasks
            .remove(0);
        assert_eq!(back.fields, edited.fields);
        assert_eq!(back.parent_uid, parsed.parent_uid);
    }
}

fn check_cards(parsed: &ParsedCards) {
    for (i, c) in parsed.contacts.iter().enumerate() {
        let original = contact(parsed, i);
        let options = CardWriteOptions::at(now());
        assert_eq!(
            write_contact(&original, Some(&c.raw), &options),
            c.raw,
            "{}",
            c.uid
        );
        let mut edited = original.clone();
        edited.fields.notes = Some("Edited in Nail.".into());
        let out = write_contact(&edited, Some(&c.raw), &options);
        let back = nail_ical::vcard::parse_cards(&out).contacts.remove(0);
        assert_eq!(back.fields, edited.fields, "{out}");
        assert_eq!(back.version, c.version);
        if !c.raw.contains("VERSION:2.1") {
            let mut lines = Vec::new();
            for card in content::parse(&c.raw).components {
                original_lines(&card, &["NOTE", "REV"], &mut lines);
            }
            for line in lines {
                assert!(out.contains(&line), "{}: lost {line:?}\n{out}", c.uid);
            }
        }
    }
    for g in &parsed.groups {
        assert_eq!(
            write_group(&g.group, Some(&g.raw), &CardWriteOptions::at(now())),
            g.raw
        );
    }
}

#[test]
fn google_calendar() {
    let cal = calendar("google/calendar-export.ics");
    assert!(cal.issues.is_empty(), "{:?}", cal.issues);
    assert_eq!(cal.name.as_deref(), Some("ada@example.org"));
    assert_eq!(cal.method.as_deref(), Some("PUBLISH"));
    assert_eq!(cal.events.len(), 4);
    let sync = find(&cal, "7kukuqrfedlm2f9t2vepo8v3bk@google.com");
    assert_eq!(
        sync.fields.start,
        PimTime::zoned("2026-10-05T09:30:00", "Europe/Berlin")
    );
    assert_eq!(sync.fields.attendees.len(), 2);
    assert!(sync.fields.attendees[1].rsvp);
    assert_eq!(
        sync.fields.conferences.len(),
        1,
        "vendor and text link are one"
    );
    assert_eq!(
        sync.fields.alarms[0].description.as_deref(),
        Some("This is an event reminder")
    );
    assert_eq!(sync.overrides.len(), 1);
    assert_eq!(sync.recurrence.as_ref().unwrap().exceptions.len(), 2);
    assert_eq!(sync.sequence, 2);
    let october = month(
        &event(sync),
        "2026-10-01T00:00:00Z",
        "2026-11-01T00:00:00Z",
        Tz::UTC,
    );
    assert_eq!(october.len(), 10);
    assert_eq!(
        october[1],
        (
            "2026-10-12T09:00:00Z".to_owned(),
            "Engine sync (moved)".to_owned()
        )
    );
    // UNTIL=20261221T225959Z: the last one is Monday Dec 21.
    let december = month(
        &event(sync),
        "2026-12-01T00:00:00Z",
        "2027-01-01T00:00:00Z",
        Tz::UTC,
    );
    assert_eq!(december.last().unwrap().0, "2026-12-21T08:30:00Z");
    let birthday = find(&cal, "0bm3f9d1jj3p4n2s7mv4q8q0rv@google.com");
    assert!(birthday.fields.is_all_day());
    assert_eq!(birthday.fields.transparency, Transparency::Free);
    assert_eq!(
        birthday.fields.alarms[0].trigger,
        AlarmTrigger::Relative {
            offset_secs: -15 * 3600,
            related: AlarmRelated::Start
        }
    );
    let firings = event_firings(
        &event(birthday),
        &[],
        utc("2026-12-01T00:00:00Z"),
        utc("2027-01-01T00:00:00Z"),
        chrono_tz::Europe::Berlin,
    );
    assert_eq!(firings.len(), 1);
    assert_eq!(
        nail_ical::time::format_instant(firings[0].fire_at),
        "2026-12-04T08:00:00Z"
    );
    let vendor = find(&cal, "3d1qq5v6h2f0n3kq0kpd2m2a8t@google.com");
    assert_eq!(vendor.fields.status, EventStatus::Tentative);
    assert_eq!(
        vendor.fields.attachments[0].name.as_deref(),
        Some("Agenda.pdf")
    );
    assert!(
        vendor.fields.attachments[0]
            .url
            .as_deref()
            .unwrap()
            .starts_with("https://drive.google.com/file/d/")
    );
    assert_eq!(
        vendor.fields.conferences[0].uri,
        "https://zoom.us/j/95551234567?pwd=abcDEF"
    );
    check_calendar_items(&cal);
}

#[test]
fn google_contacts() {
    let parsed = cards("google/contacts.vcf");
    assert!(
        parsed
            .issues
            .iter()
            .all(|i| i.kind == IssueKind::MissingUid),
        "{:?}",
        parsed.issues
    );
    assert_eq!(parsed.contacts.len(), 2);
    let charles = &parsed.contacts[0].fields;
    assert_eq!(charles.emails[0].kinds, ["work"]);
    assert_eq!(charles.urls[0].value, "https://babbage.example");
    assert_eq!(charles.urls[0].label.as_deref(), Some("PROFILE"));
    assert_eq!(
        charles.birthday,
        Some(ContactDate {
            year: Some(1791),
            month: 12,
            day: 26
        })
    );
    assert_eq!(
        charles.notes.as_deref(),
        Some("Met at the Royal Society.\nPrefers letters.")
    );
    assert_eq!(
        charles.organization.as_ref().unwrap().name,
        "Difference Engines"
    );
    let mary = &parsed.contacts[1].fields;
    assert_eq!(mary.related[0].kinds, ["spouse"]);
    assert_eq!(mary.custom[0].name, "X-ABDATE");
    // Derived UIDs are stable across reads.
    assert_eq!(
        cards("google/contacts.vcf").contacts[0].uid,
        parsed.contacts[0].uid
    );
    check_cards(&parsed);
}

#[test]
fn outlook_calendar() {
    let cal = calendar("outlook/calendar-export.ics");
    assert!(cal.issues.is_empty(), "{:?}", cal.issues);
    assert_eq!(cal.events.len(), 3);
    let review = &cal.events[0];
    assert_eq!(
        review.fields.start,
        PimTime::zoned("2026-10-20T14:00:00", "Europe/Berlin")
    );
    assert_eq!(review.fields.title, "Budget review Q4");
    assert_eq!(review.fields.priority, Some(5));
    assert_eq!(review.fields.attendees[1].kind, AttendeeKind::Resource);
    assert_eq!(review.fields.conferences.len(), 1);
    assert!(
        review.fields.conferences[0]
            .uri
            .starts_with("https://teams.microsoft.com/l/meetup-join/")
    );
    assert_eq!(
        review.fields.alarms[0].description.as_deref(),
        Some("Reminder")
    );
    assert!(review.uid.starts_with("040000008200E00074C5B7101A82E008"));
    // "Customized Time Zone" has New York's rules.
    let check_in = &cal.events[1];
    assert_eq!(
        check_in.fields.start,
        PimTime::zoned("2026-11-03T09:00:00", "America/New_York")
    );
    assert_eq!(check_in.fields.visibility, Visibility::Private);
    let firsts: Vec<String> = month(
        &event(check_in),
        "2026-11-01T00:00:00Z",
        "2027-06-01T00:00:00Z",
        Tz::UTC,
    )
    .into_iter()
    .map(|(s, _)| s)
    .collect();
    assert_eq!(
        firsts,
        [
            "2026-11-03T14:00:00Z",
            "2026-12-01T14:00:00Z",
            "2027-01-05T14:00:00Z",
            "2027-02-02T14:00:00Z",
            "2027-03-02T14:00:00Z",
            "2027-04-06T13:00:00Z"
        ]
    );
    let home = &cal.events[2];
    assert_eq!(home.fields.start, PimTime::date("2026-11-02"));
    assert_eq!(home.fields.transparency, Transparency::Free);
    check_calendar_items(&cal);
    // A changed description drops Outlook's stale HTML copy.
    let mut edited = event(review);
    edited.fields.description = Some("New agenda.".into());
    let out = write_event(&edited, Some(&review.raw), &WriteOptions::at(now()));
    assert!(!out.contains("X-ALT-DESC") && out.contains("X-MICROSOFT-CDO-BUSYSTATUS:BUSY"));
}

#[test]
fn outlook_vcard21() {
    let parsed = cards("outlook/contacts-vcard21.vcf");
    assert_eq!(parsed.contacts.len(), 1);
    let c = &parsed.contacts[0];
    assert_eq!(c.version, VCardVersion::V3);
    assert_eq!(c.fields.display_name, "Jürgen Müller");
    assert_eq!(
        c.fields.notes.as_deref(),
        Some("Grüße aus der Entwicklung.")
    );
    assert_eq!(
        c.fields.addresses[0].street.as_deref(),
        Some("Hauptstraße 1")
    );
    assert!(c.fields.addresses[0].preferred);
    assert_eq!(c.fields.phones[1].kinds, ["cell", "voice"]);
    assert_eq!(c.updated_at.as_deref(), Some("2026-09-15T08:15:00Z"));
    let names: Vec<&str> = c.fields.custom.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["X-MS-OL-DEFAULT-POSTAL-ADDRESS", "X-MS-OL-DESIGN"]);
    check_cards(&parsed);
    // Edited, it is a clean vCard 3.0 that keeps the unknown LABEL.
    let mut edited = contact(&parsed, 0);
    edited.fields.title = Some("Chefingenieur".into());
    let out = write_contact(&edited, Some(&c.raw), &CardWriteOptions::at(now()));
    assert!(out.contains("VERSION:3.0\r\n"));
    assert!(
        out.contains("LABEL;TYPE=WORK;TYPE=PREF:Hauptstraße 1\\n"),
        "{out}"
    );
    assert!(!out.contains("QUOTED-PRINTABLE"));
}

#[test]
fn apple_calendar_and_reminders() {
    let cal = calendar("apple/icloud-calendar.ics");
    assert!(cal.issues.is_empty(), "{:?}", cal.issues);
    assert_eq!(cal.color.as_deref(), Some("#1BADF8"));
    let night = &cal.events[0];
    // 01:00 on the night the clocks go back: the first (PDT) one.
    let shift = month(
        &event(night),
        "2026-11-01T00:00:00Z",
        "2026-11-02T00:00:00Z",
        Tz::UTC,
    );
    assert_eq!(shift[0].0, "2026-11-01T08:00:00Z");
    assert_eq!(
        night.fields.alarms.len(),
        1,
        "ACTION:NONE is not a reminder"
    );
    assert_eq!(night.fields.alarms[0].action, AlarmAction::Audio);
    assert_eq!(
        night.fields.alarms[0].uid.as_deref(),
        Some("6D1F7C2A-1B2C-4D3E-9F8A-7B6C5D4E3F2A")
    );
    // Acknowledged on another device: it does not fire again.
    assert!(
        event_firings(
            &event(night),
            &[],
            utc("2026-10-31T00:00:00Z"),
            utc("2026-11-02T00:00:00Z"),
            Tz::UTC
        )
        .is_empty()
    );
    assert!(
        night
            .fields
            .location
            .as_deref()
            .unwrap()
            .starts_with("Infinite Loop\n")
    );
    let rent = &cal.events[1];
    let rents: Vec<String> = month(
        &event(rent),
        "2026-04-01T00:00:00Z",
        "2026-07-01T00:00:00Z",
        Tz::UTC,
    )
    .into_iter()
    .map(|(s, _)| s)
    .collect();
    assert_eq!(
        rents,
        [
            "2026-04-30T00:00:00Z",
            "2026-05-31T00:00:00Z",
            "2026-06-30T00:00:00Z"
        ]
    );
    check_calendar_items(&cal);
    // The NONE alarm and the structured location survive an edit.
    let mut edited = event(night);
    edited.fields.alarms.clear();
    let out = write_event(&edited, Some(&night.raw), &WriteOptions::at(now()));
    assert!(out.contains("ACTION:NONE") && out.contains("X-APPLE-STRUCTURED-LOCATION"));
    assert!(!out.contains("ACTION:AUDIO"));

    let reminders = calendar("apple/reminders.ics");
    assert!(reminders.issues.is_empty(), "{:?}", reminders.issues);
    assert_eq!(reminders.tasks.len(), 3);
    let plan = &reminders.tasks[0];
    assert_eq!(plan.fields.order, Some(741_258_963));
    assert_eq!(plan.fields.priority, Some(1));
    let fired = task_firings(
        &task(plan),
        utc("2026-10-01T00:00:00Z"),
        utc("2026-11-01T00:00:00Z"),
        Tz::UTC,
    );
    assert_eq!(
        nail_ical::time::format_instant(fired[0].fire_at),
        "2026-10-15T07:00:00Z"
    );
    let flights = &reminders.tasks[1];
    assert_eq!(flights.parent_uid.as_deref(), Some(plan.uid.as_str()));
    assert_eq!(flights.fields.status, TaskStatus::Completed);
    let plants = &reminders.tasks[2];
    assert_eq!(
        plants.fields.due,
        Some(PimTime::zoned("2026-10-03T08:00:00", "America/Los_Angeles"))
    );
    let (next, _) =
        nail_ical::edit::advance_task(&plants.fields, plants.recurrence.as_ref().unwrap()).unwrap();
    assert_eq!(
        next.due,
        Some(PimTime::zoned("2026-10-10T08:00:00", "America/Los_Angeles"))
    );
    check_calendar_items(&reminders);
}

#[test]
fn apple_contacts() {
    let parsed = cards("apple/contacts.vcf");
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    let grace = &parsed.contacts[0];
    assert_eq!(grace.fields.emails[0].label.as_deref(), Some("Navy"));
    assert!(grace.fields.emails[0].preferred);
    assert_eq!(grace.fields.phones[1].kinds, ["cell"]);
    assert_eq!(
        grace.fields.birthday,
        Some(ContactDate {
            year: None,
            month: 12,
            day: 9
        })
    );
    assert_eq!(grace.fields.related[0].value, "Vincent Hopper");
    assert_eq!(
        grace.photo.as_ref().unwrap().mime_type.as_deref(),
        Some("image/jpeg")
    );
    assert_eq!(grace.fields.nicknames, ["Amazing Grace"]);
    let group = &parsed.groups[0].group;
    assert_eq!(group.style, GroupStyle::Apple);
    assert_eq!(group.member_uids.len(), 2);
    assert!(group.member_uids.contains(&uid_key(&grace.uid)));
    check_cards(&parsed);
}

#[test]
fn thunderbird_calendar_and_address_book() {
    let cal = calendar("thunderbird/calendar-export.ics");
    assert!(cal.issues.is_empty(), "{:?}", cal.issues);
    let choir = &cal.events[0];
    let rehearsals = month(
        &event(choir),
        "2026-10-01T00:00:00Z",
        "2027-01-01T00:00:00Z",
        Tz::UTC,
    );
    assert_eq!(rehearsals.len(), 9, "{rehearsals:?}");
    assert_eq!(
        rehearsals[2],
        (
            "2026-10-15T18:00:00Z".to_owned(),
            "Choir rehearsal (concert hall)".to_owned()
        )
    );
    assert!(
        !rehearsals.iter().any(|(s, _)| s.starts_with("2026-10-29")),
        "EXDATE in UTC applies"
    );
    let vienna = &cal.events[1];
    assert_eq!(
        vienna.fields.start,
        PimTime::zoned("2026-11-10T09:00:00", "Europe/Vienna")
    );
    assert_eq!(vienna.fields.visibility, Visibility::Confidential);
    assert_eq!(cal.tasks.len(), 2);
    let slides = &cal.tasks[0];
    assert_eq!(
        slides.fields.alarms[0].trigger,
        AlarmTrigger::Relative {
            offset_secs: -86_400,
            related: AlarmRelated::End
        }
    );
    assert_eq!(
        cal.tasks[1].parent_uid.as_deref(),
        Some(slides.uid.as_str())
    );
    check_calendar_items(&cal);

    let book = cards("thunderbird/address-book.vcf");
    assert!(book.issues.is_empty(), "{:?}", book.issues);
    let alan = &book.contacts[0];
    assert_eq!(alan.version, VCardVersion::V4);
    assert!(alan.fields.emails[0].preferred);
    assert_eq!(alan.fields.impp[0].value, "xmpp:alan@bletchley.example");
    assert_eq!(
        alan.fields.anniversary,
        Some(ContactDate {
            year: None,
            month: 6,
            day: 12
        })
    );
    assert_eq!(alan.fields.organization.as_ref().unwrap().units, ["Hut 8"]);
    let group = &book.groups[0].group;
    assert_eq!(group.member_uids, [uid_key(&alan.uid)]);
    assert_eq!(group.member_emails, ["joan@bletchley.example"]);
    check_cards(&book);
}

#[test]
fn holiday_feed() {
    let parsed = feed::parse_feed_bytes(&bytes("feeds/holidays.ics"));
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert_eq!(parsed.name.as_deref(), Some("Holidays in Example Land"));
    assert_eq!(
        parsed.description.as_deref(),
        Some("Public holidays, published weekly")
    );
    assert_eq!(parsed.color.as_deref(), Some("crimson"));
    assert_eq!(feed::refresh_minutes(None, &parsed), 7 * 24 * 60);
    let fireworks = &parsed.events[1];
    assert_eq!(
        fireworks.fields.start,
        PimTime::zoned("2026-12-31T23:00:00", "Europe/Berlin")
    );
    assert_eq!(parsed.events[2].fields.end, PimTime::date("2027-08-16"));
    let new_year = month(
        &event(&parsed.events[0]),
        "2027-01-01T00:00:00Z",
        "2029-01-02T00:00:00Z",
        chrono_tz::Europe::Berlin,
    );
    assert_eq!(new_year.len(), 3);
}

#[test]
fn rfc_edge_cases() {
    let cal = calendar("edge/rfc-edge-cases.ics");
    let kinds: Vec<&IssueKind> = cal.issues.iter().map(|i| &i.kind).collect();
    assert_eq!(
        kinds,
        [&IssueKind::UnknownTimeZone {
            tzid: "Mars Standard Time".into()
        }]
    );
    assert_eq!(cal.issues[0].item.as_deref(), Some("Unknown zone"));
    assert_eq!(cal.events.len(), 5, "the VJOURNAL is not an event");
    let future = find(&cal, "future@edge.example");
    let options = ExpandOptions {
        this_and_future: &future.this_and_future,
        ..ExpandOptions::range(
            utc("2026-11-01T00:00:00Z"),
            utc("2027-01-01T00:00:00Z"),
            Tz::UTC,
        )
    };
    let list = instances(&event(future), &options);
    let titles: Vec<(&str, &str)> = list
        .iter()
        .map(|i| (&i.start_utc[..16], i.title.as_str()))
        .collect();
    assert_eq!(
        titles,
        [
            ("2026-11-02T13:30", "Status meeting"),
            ("2026-11-09T13:30", "Status meeting"),
            ("2026-11-16T13:30", "Status meeting"),
            ("2026-11-23T15:00", "Status meeting (new slot)"),
            ("2026-11-30T15:00", "Status meeting (new slot)"),
            ("2026-12-07T15:00", "Status meeting (new slot)"),
            ("2026-12-14T15:00", "Status meeting (new slot)"),
            ("2026-12-21T15:00", "Status meeting (new slot)"),
        ]
    );
    assert_eq!(list[4].end_utc, "2026-11-30T16:00:00Z");
    let gap = find(&cal, "gap@edge.example");
    let runs = month(
        &event(gap),
        "2026-03-01T00:00:00Z",
        "2026-05-01T00:00:00Z",
        Tz::UTC,
    );
    let starts: Vec<&str> = runs.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        starts,
        [
            "2026-03-29T01:30:00Z",
            "2026-03-30T00:30:00Z",
            "2026-03-31T00:30:00Z",
            "2026-04-05T10:00:00Z"
        ]
    );
    let setpos = find(&cal, "setpos@edge.example");
    let last_workdays: Vec<String> = month(
        &event(setpos),
        "2026-10-01T00:00:00Z",
        "2027-03-01T00:00:00Z",
        Tz::UTC,
    )
    .into_iter()
    .map(|(s, _)| s[..10].to_owned())
    .collect();
    assert_eq!(
        last_workdays,
        ["2026-10-30", "2026-11-30", "2026-12-31", "2027-01-29"]
    );
    let lower = find(&cal, "lower@edge.example");
    assert_eq!(lower.fields.start, PimTime::date("2026-12-24"));
    assert_eq!(lower.fields.title, "lower-case names");
    let unknown = find(&cal, "unknown-zone@edge.example");
    assert_eq!(
        unknown.fields.start,
        PimTime::floating("2026-10-05T12:00:00")
    );
    check_calendar_items(&cal);
    let mut edited = event(lower);
    edited.fields.location = Some("North Pole".into());
    let out = write_event(&edited, Some(&lower.raw), &WriteOptions::at(now()));
    assert!(
        out.contains("x-vendor-flag;X-Param=\"a;b:c\":kept\r\n"),
        "{out}"
    );
    assert!(
        out.contains("BEGIN:X-NAIL-VENDOR-BLOCK\r\nX-INSIDE:value\r\nEND:X-NAIL-VENDOR-BLOCK\r\n")
    );
    assert!(out.contains("LOCATION:North Pole\r\n"));
}

#[test]
fn bare_lf_cards() {
    let parsed = cards("edge/lf-line-endings.vcf");
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert_eq!(parsed.contacts[0].fields.display_name, "Line Feed Only");
    let c = contact(&parsed, 0);
    // Unchanged: returned as it was (LF and all).
    assert_eq!(
        write_contact(
            &c,
            Some(&parsed.contacts[0].raw),
            &CardWriteOptions::at(now())
        ),
        parsed.contacts[0].raw
    );
}

#[test]
fn every_corpus_file_previews() {
    for name in [
        "google/calendar-export.ics",
        "google/contacts.vcf",
        "outlook/calendar-export.ics",
        "outlook/contacts-vcard21.vcf",
        "apple/icloud-calendar.ics",
        "apple/reminders.ics",
        "apple/contacts.vcf",
        "thunderbird/calendar-export.ics",
        "thunderbird/address-book.vcf",
        "feeds/holidays.ics",
        "edge/rfc-edge-cases.ics",
        "edge/lf-line-endings.vcf",
    ] {
        let data = import::read_bytes(&bytes(name), Some(name));
        let preview = import::preview(&data);
        assert!(
            preview.events + preview.tasks + preview.contacts + preview.groups > 0,
            "{name}"
        );
        assert!(import::fatal_issue(&data).is_none(), "{name}");
        assert!(preview.issues.iter().all(|i| i.localized.is_valid()));
    }
}

#[test]
fn large_imports_stay_linear() {
    let mut text = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Custom Berlin\r\nBEGIN:STANDARD\r\nDTSTART:16011028T030000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010325T020000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n",
    );
    for i in 0..3000 {
        write!(
            text,
            "BEGIN:VEVENT\r\nUID:bulk-{i}@t\r\nDTSTAMP:20261001T000000Z\r\nDTSTART;TZID=Custom Berlin:20261005T{:02}0000\r\nDURATION:PT1H\r\nSUMMARY:Item {i}\r\nEND:VEVENT\r\n",
            i % 24
        ).unwrap();
    }
    text.push_str("END:VCALENDAR\r\n");
    let started = std::time::Instant::now();
    let parsed = nail_ical::parse_calendar(&text, nail_ical::ParseOptions::default());
    assert_eq!(parsed.events.len(), 3000);
    assert!(parsed.issues.is_empty(), "{:?}", &parsed.issues[..1]);
    assert_eq!(parsed.events[0].fields.start.tz(), Some("Europe/Berlin"));
    assert!(parsed.events[1].raw.contains("TZID:Custom Berlin"));
    // Generous bound for debug builds on CI; quadratic behaviour would
    // take far longer.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}
