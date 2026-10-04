//! Property tests (proptest): round trips of the content layer, RRULE
//! text, events, tasks and contacts; invariants of recurrence expansion,
//! and Nail's recurrence engine against calcard's (a port of rust-rrule /
//! python-dateutil) as an independent oracle.
#![cfg(test)]

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use chrono_tz::Tz;
use nail_api::pim::calendar::{
    AlarmRelated, AttendeeRole, EventStatus, Frequency, ParticipationStatus, Transparency,
    Visibility, WeekdayNum,
};
use nail_api::pim::contact::{ContactName, VCardVersion};
use nail_api::pim::source::PimItemSync;
use nail_api::pim::{
    Alarm, AlarmTrigger, Attendee, CalendarEvent, Contact, ContactFields, EventFields,
    LabeledValue, PimTime, Recurrence, RecurrenceEnd, RecurrenceRule,
};
use nail_api::triage::Weekday;
use nail_ical::content::{self, Property};
use nail_ical::expand::{ExpandOptions, instances};
use nail_ical::recur::{Occurrences, RuleIter, chrono_weekday, format_rrule, parse_rrule};
use nail_ical::time;
use nail_ical::vcard::{CardWriteOptions, parse_cards, write_contact};
use nail_ical::write::{WriteOptions, write_event};
use nail_ical::{ParseOptions, parse_calendar};
use proptest::prelude::*;

/// `cases` cases, many rejects allowed (the
/// calcard comparison skips rules it rejects), and no regression files
/// written into the source tree (the repository lives in OneDrive).
fn config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        max_global_rejects: 1_000_000,
        failure_persistence: None,
        ..ProptestConfig::with_cases(cases)
    }
}

fn now() -> chrono::DateTime<chrono::Utc> {
    time::parse_instant("2026-10-01T12:00:00Z").unwrap()
}

// ---------------------------------------------------------------------------
// Strategies

fn text() -> impl Strategy<Value = String> {
    // Printable text incl. the characters that need escaping and non-ASCII.
    proptest::string::string_regex("[a-zA-Z0-9 ,;:\\\\\"äöüßéñ漢字😀\\n-]{1,90}")
        .unwrap()
        .prop_map(|s| s.trim().to_owned())
        .prop_filter("non-empty", |s| !s.is_empty())
}

fn weekday() -> impl Strategy<Value = Weekday> {
    prop::sample::select(Weekday::ALL.to_vec())
}

fn start_time() -> impl Strategy<Value = NaiveDateTime> {
    (
        2020i32..2030,
        1u32..=12,
        1u32..=28,
        0u32..24,
        prop::sample::select(vec![0u32, 15, 30, 45]),
    )
        .prop_map(|(y, m, d, h, min)| {
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_time(NaiveTime::from_hms_opt(h, min, 0).unwrap())
        })
}

fn small_vec<T: std::fmt::Debug + Clone>(
    s: impl Strategy<Value = T>,
    max: usize,
) -> impl Strategy<Value = Vec<T>> {
    prop::collection::vec(s, 0..=max)
}

fn sorted_dedup<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v.dedup();
    v
}

/// Valid rules of the daily-and-coarser frequencies (what calendars use).
fn rule() -> impl Strategy<Value = RecurrenceRule> {
    let freq = prop::sample::select(vec![
        Frequency::Daily,
        Frequency::Weekly,
        Frequency::Monthly,
        Frequency::Yearly,
    ]);
    (
        freq,
        1u32..4,
        small_vec(1u8..=12, 2),
        small_vec(prop_oneof![1i8..=28, -3i8..=-1], 2),
        small_vec(
            (weekday(), prop::option::of(prop_oneof![1i8..=4, -2i8..=-1])),
            3,
        ),
        small_vec(prop_oneof![1i16..=4, -2i16..=-1], 1),
        small_vec(1i8..=52, 1),
        small_vec(1i16..=200, 1),
        prop::option::of(weekday()),
        small_vec(prop::sample::select(vec![8u8, 12, 17]), 2),
    )
        .prop_map(
            |(
                freq,
                interval,
                by_month,
                by_month_day,
                by_day,
                by_set_pos,
                week_no,
                year_day,
                wkst,
                by_hour,
            )| {
                let mut rule = RecurrenceRule::new(freq);
                rule.interval = interval;
                rule.by_month = sorted_dedup(by_month);
                rule.by_month_day = sorted_dedup(by_month_day);
                rule.by_day = by_day
                    .into_iter()
                    .map(|(day, ordinal)| WeekdayNum {
                        day,
                        ordinal: if matches!(freq, Frequency::Monthly | Frequency::Yearly) {
                            ordinal
                        } else {
                            None
                        },
                    })
                    .collect();
                rule.by_day.sort_by_key(|d| (d.day, d.ordinal));
                rule.by_day.dedup();
                if !rule.by_day.is_empty() || !rule.by_month_day.is_empty() {
                    rule.by_set_pos = sorted_dedup(by_set_pos);
                }
                if freq == Frequency::Yearly
                    && rule.by_month.is_empty()
                    && rule.by_month_day.is_empty()
                {
                    if rule.by_day.iter().all(|d| d.ordinal.is_none()) {
                        rule.by_week_no = sorted_dedup(week_no);
                    }
                    if rule.by_day.is_empty() {
                        rule.by_year_day = sorted_dedup(year_day);
                    }
                }
                rule.week_start = wkst;
                rule.by_hour = sorted_dedup(by_hour);
                rule
            },
        )
        .prop_filter("valid", RecurrenceRule::is_valid)
}

// ---------------------------------------------------------------------------
// Content layer

proptest! {
    #![proptest_config(config(200))]

    #[test]
    fn text_values_survive_escaping_and_folding(value in text(), param in text()) {
        let p = Property::text_value("X-TEST", &value).with_param("X-P", param.replace('\n', " "));
        let mut out = String::new();
        p.write(&mut out);
        for line in out.split("\r\n") {
            prop_assert!(line.len() <= content::FOLD_OCTETS, "{line:?}");
        }
        let doc = content::parse(&format!("BEGIN:X\r\n{out}END:X\r\n"));
        prop_assert!(doc.issues.is_empty(), "{:?} in {out:?}", doc.issues);
        let back = doc.components[0].property("X-TEST").unwrap();
        prop_assert_eq!(back.text(), value.replace("\r\n", "\n"));
        let expected_param = param.replace('\n', " ");
        prop_assert_eq!(back.param("X-P"), Some(expected_param.as_str()));
        // And the parsed text is written back unchanged.
        prop_assert_eq!(doc.to_text(), format!("BEGIN:X\r\n{out}END:X\r\n"));
    }

    #[test]
    fn rrule_text_round_trips(rule in rule()) {
        let text = format_rrule(&rule);
        let back = parse_rrule(&text, &PimTime::floating("2026-01-01T00:00:00")).unwrap();
        prop_assert_eq!(back, rule);
    }
}

// ---------------------------------------------------------------------------
// Recurrence

fn occurrences(rule: &RecurrenceRule, start: NaiveDateTime, limit: usize) -> Vec<NaiveDateTime> {
    RuleIter::new(rule, start, false, None)
        .take(limit)
        .collect()
}

fn matches_filters(rule: &RecurrenceRule, t: NaiveDateTime) -> bool {
    (rule.by_month.is_empty() || rule.by_month.contains(&u8::try_from(t.month()).unwrap()))
        && (rule.by_hour.is_empty() || rule.by_hour.contains(&u8::try_from(t.hour()).unwrap()))
        && (rule.by_day.is_empty()
            || rule
                .by_day
                .iter()
                .any(|d| chrono_weekday(d.day) == t.weekday()))
}

proptest! {
    #![proptest_config(config(300))]

    #[test]
    fn expansion_invariants(rule in rule(), start in start_time(), count in 1u32..30) {
        let mut counted = rule.clone();
        counted.end = RecurrenceEnd::Count { count };
        let all = occurrences(&counted, start, 1000);
        prop_assert!(!all.is_empty() && all[0] == start, "the start comes first");
        prop_assert!(all.len() <= count as usize);
        prop_assert!(all.windows(2).all(|w| w[0] < w[1]), "strictly increasing");
        for t in &all[1..] {
            prop_assert!(matches_filters(&rule, *t), "{t} does not match {}", format_rrule(&rule));
        }
        // UNTIL cuts the same sequence.
        if all.len() > 2 {
            let cut = all[all.len() / 2];
            let mut until = rule.clone();
            until.end = RecurrenceEnd::Until { until: PimTime::floating(time::format_local(cut)) };
            let start_time = PimTime::floating(time::format_local(start));
            let bound = nail_ical::recur::UntilBound::new(
                &PimTime::floating(time::format_local(cut)), &start_time);
            let cut_list: Vec<_> = RuleIter::new(&until, start, false, bound).take(1000).collect();
            prop_assert_eq!(&cut_list[..], &all[..=all.len() / 2]);
        }
        // A recurrence set without exceptions yields the same.
        let set: Vec<_> = Occurrences::new(
            &PimTime::floating(time::format_local(start)),
            Some(&Recurrence::rule(counted.clone())),
        ).unwrap().take(1000).collect();
        prop_assert_eq!(set, all);
    }

    #[test]
    fn agrees_with_calcard(rule in rule(), start in start_time()) {
        // dateutil (and so calcard) ANDs plain and ordinal BYDAY entries;
        // RFC 5545 lists any match. Compare only rules where both agree.
        prop_assume!(rule.by_day.iter().all(|d| d.ordinal.is_some()) || rule.by_day.iter().all(|d| d.ordinal.is_none()));
        let ics = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:p\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:{}\r\nRRULE:{}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            time::ical_local(start),
            format_rrule(&rule)
        );
        let calendar = calcard::icalendar::ICalendar::parse(&ics).unwrap();
        let expanded = calendar.expand_dates(calcard::common::timezone::Tz::from(Tz::UTC), 40);
        prop_assume!(expanded.errors.is_empty());
        // rust-rrule drops candidates before DTSTART *before* applying
        // BYSETPOS in the first period; RFC 5545 selects from the whole
        // period first (so a period can end up empty). Compare BYSETPOS
        // rules from the second period on.
        let first_period = chrono::Duration::days(i64::from(rule.interval) * match rule.freq {
            Frequency::Yearly => 367,
            Frequency::Monthly => 32,
            Frequency::Weekly => 8,
            _ => 2,
        });
        let cutoff = if rule.by_set_pos.is_empty() { start } else { start + first_period };
        let keep = |t: &NaiveDateTime| *t != start && *t >= cutoff;
        let mut theirs: Vec<NaiveDateTime> =
            expanded.events.iter().map(|e| e.start.naive_local()).filter(keep).collect();
        theirs.sort();
        theirs.dedup();
        let ours: Vec<NaiveDateTime> = occurrences(&rule, start, 80)
            .into_iter()
            .filter(keep)
            .take(theirs.len())
            .collect();
        let n = theirs.len().min(ours.len()).min(30);
        prop_assert_eq!(&ours[..n], &theirs[..n], "{}", format_rrule(&rule));
    }
}

// ---------------------------------------------------------------------------
// Events, instances

fn zone() -> impl Strategy<Value = Option<&'static str>> {
    prop::sample::select(vec![
        None,
        Some("UTC"),
        Some("Europe/Berlin"),
        Some("America/New_York"),
        Some("Australia/Lord_Howe"),
        Some("Asia/Kolkata"),
    ])
}

fn pim(t: NaiveDateTime, zone: Option<&str>, all_day: bool) -> PimTime {
    if all_day {
        PimTime::date(time::format_date(t.date()))
    } else {
        match zone {
            Some(z) => PimTime::zoned(time::format_local(t), z),
            None => PimTime::floating(time::format_local(t)),
        }
    }
}

fn email() -> impl Strategy<Value = String> {
    "[a-z]{1,8}@[a-z]{1,8}\\.example".prop_map(|s| s)
}

prop_compose! {
    fn event()(
        title in text(),
        start in start_time(),
        minutes in 0i64..(3 * 24 * 60),
        zone in zone(),
        all_day in any::<bool>(),
        location in prop::option::of(text()),
        description in prop::option::of(text()),
        status in prop::sample::select(vec![EventStatus::Confirmed, EventStatus::Tentative, EventStatus::Cancelled]),
        free in any::<bool>(),
        private in any::<bool>(),
        priority in prop::option::of(1u8..=9),
        categories in small_vec(text().prop_map(|t| t.replace(',', " ").trim().to_owned()).prop_filter("x", |t| !t.is_empty()), 3),
        attendees in small_vec((email(), prop::option::of(text()), any::<bool>()), 3),
        alarm_minutes in small_vec(-2000i64..2000, 2),
        rule in prop::option::of(rule()),
        exception_days in small_vec(1i64..60, 2),
    ) -> CalendarEvent {
        let end = if all_day {
            start + chrono::Duration::days(1 + minutes / 1440)
        } else {
            start + chrono::Duration::minutes(minutes)
        };
        let s = pim(start, zone, all_day);
        let mut fields = EventFields::new(title, s.clone(), pim(end, zone, all_day));
        fields.location = location;
        fields.description = description;
        fields.status = status;
        fields.transparency = if free { Transparency::Free } else { Transparency::Busy };
        fields.visibility = if private { Visibility::Private } else { Visibility::Public };
        fields.priority = priority;
        fields.categories = categories;
        let mut seen = Vec::new();
        fields.attendees = attendees
            .into_iter()
            .filter(|(e, _, _)| { let fresh = !seen.contains(e); seen.push(e.clone()); fresh })
            .map(|(email, name, rsvp)| Attendee {
                name,
                rsvp,
                role: if rsvp { AttendeeRole::Optional } else { AttendeeRole::Required },
                status: if rsvp { ParticipationStatus::Accepted } else { ParticipationStatus::NeedsAction },
                ..Attendee::new(email)
            })
            .collect();
        fields.alarms = alarm_minutes
            .into_iter()
            .map(|m| Alarm {
                trigger: AlarmTrigger::Relative {
                    offset_secs: m * 60,
                    related: if m > 0 { AlarmRelated::End } else { AlarmRelated::Start },
                },
                ..Alarm::minutes_before(0)
            })
            .collect();
        let recurrence = rule.map(|mut rule| {
            rule.by_hour.clear();
            Recurrence {
                rules: vec![rule],
                dates: vec![],
                exceptions: exception_days
                    .iter()
                    .map(|d| pim(start + chrono::Duration::days(*d), zone, all_day))
                    .collect(),
            }
        });
        CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "prop@nail".into(),
            fields,
            recurrence,
            overrides: vec![],
            sequence: 1,
            created_at: Some("2026-09-01T10:00:00Z".into()),
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }
}

proptest! {
    #![proptest_config(config(150))]

    #[test]
    fn events_round_trip_through_icalendar(event in event()) {
        let text = write_event(&event, None, &WriteOptions::at(now()));
        let parsed = parse_calendar(&text, ParseOptions::default());
        prop_assert!(parsed.issues.is_empty(), "{:?}\n{text}", parsed.issues);
        let back = parsed.events.into_iter().next().unwrap()
            .into_event("e".into(), "c".into(), false, PimItemSync::Synced);
        prop_assert_eq!(&back.fields, &event.fields, "{}", text);
        prop_assert_eq!(&back.recurrence, &event.recurrence);
        // Writing what was read changes nothing.
        prop_assert_eq!(write_event(&back, Some(&text), &WriteOptions::at(now())), text);
    }

    #[test]
    fn instances_are_sorted_and_in_range(event in event(), offset_days in 0i64..400, viewer in zone()) {
        let viewer: Tz = viewer.and_then(time::zone).unwrap_or(Tz::UTC);
        let from = time::parse_instant("2020-01-01T00:00:00Z").unwrap() + chrono::Duration::days(offset_days * 7);
        let to = from + chrono::Duration::days(60);
        let list = instances(&event, &ExpandOptions::range(from, to, viewer));
        prop_assert!(list.windows(2).all(|w| w[0].start_utc <= w[1].start_utc));
        for i in &list {
            let s = time::parse_instant(&i.start_utc).unwrap();
            let e = time::parse_instant(&i.end_utc).unwrap();
            prop_assert!(s <= e);
            prop_assert!(s < to && (e > from || (s == e && s >= from)));
            prop_assert!(i.status != EventStatus::Cancelled);
            prop_assert_eq!(i.all_day, i.start.is_date());
        }
        // Every instance of a series is an occurrence of it.
        if event.recurrence.is_some() {
            for i in &list {
                let rid = i.recurrence_id.as_ref().unwrap();
                prop_assert!(nail_ical::edit::is_occurrence(&event, rid), "{rid:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Contacts

prop_compose! {
    fn contact()(
        display in text(),
        given in prop::option::of(text()),
        family in prop::option::of(text()),
        emails in small_vec((email(), any::<bool>(), prop::option::of(text())), 3),
        phones in small_vec(("[+0-9 ]{3,15}", prop::sample::select(vec!["cell", "work", "home"])), 2),
        note in prop::option::of(text()),
        v4 in any::<bool>(),
    ) -> Contact {
        let mut fields = ContactFields::named(display);
        if given.is_some() || family.is_some() {
            fields.name = Some(ContactName { given, family, ..ContactName::default() });
        }
        let mut seen = Vec::new();
        fields.emails = emails
            .into_iter()
            .filter(|(e, _, _)| { let fresh = !seen.contains(e); seen.push(e.clone()); fresh })
            .map(|(value, preferred, label)| LabeledValue { value, kinds: vec!["work".into()], label, preferred })
            .collect();
        fields.phones = phones
            .into_iter()
            .filter(|(p, _)| !p.trim().is_empty())
            .map(|(value, kind)| LabeledValue { value: value.trim().to_owned(), kinds: vec![kind.into()], label: None, preferred: false })
            .collect();
        fields.notes = note;
        Contact {
            id: "c".into(),
            address_book_id: "b".into(),
            uid: "urn:uuid:prop".into(),
            fields,
            version: if v4 { VCardVersion::V4 } else { VCardVersion::V3 },
            group_ids: vec![],
            linked_ids: vec![],
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }
}

fn normalized(mut fields: ContactFields) -> ContactFields {
    // Names normalize whitespace (structured parts are trimmed).
    if let Some(name) = &mut fields.name {
        for part in [&mut name.given, &mut name.family] {
            *part = part
                .as_ref()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty());
        }
        if name.is_empty() {
            fields.name = None;
        }
    }
    fields.display_name = fields.display_name.trim().to_owned();
    fields.notes = fields
        .notes
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty());
    for e in &mut fields.emails {
        e.label = e
            .label
            .as_ref()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty());
    }
    fields
}

proptest! {
    #![proptest_config(config(150))]

    #[test]
    fn contacts_round_trip_through_vcard(contact in contact()) {
        let text = write_contact(&contact, None, &CardWriteOptions::at(now()));
        let parsed = parse_cards(&text);
        prop_assert!(parsed.issues.is_empty(), "{:?}\n{text}", parsed.issues);
        let back = &parsed.contacts[0];
        prop_assert_eq!(back.version, contact.version);
        prop_assert_eq!(&back.fields, &normalized(contact.fields.clone()), "{}", text);
        // Writing what was read changes nothing.
        let again = Contact { fields: back.fields.clone(), ..contact.clone() };
        prop_assert_eq!(write_contact(&again, Some(&text), &CardWriteOptions::at(now())), text);
    }
}
