//! Smoke tests of the libraries WP71 builds on (WP70, ADR 0058):
//! calcard parses and writes iCalendar and vCard and expands recurrence
//! rules in IANA zones on Windows MSVC.

use calcard::common::timezone::Tz;
use calcard::icalendar::ICalendar;
use calcard::vcard::VCard;
use chrono::{Datelike, TimeZone, Timelike};

const DAILY: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Nail//Test//EN\r\n\
BEGIN:VEVENT\r\n\
UID:standup@nail.example\r\n\
DTSTAMP:20261001T080000Z\r\n\
DTSTART;TZID=Europe/Berlin:20261023T090000\r\n\
DTEND;TZID=Europe/Berlin:20261023T091500\r\n\
RRULE:FREQ=DAILY;COUNT=4\r\n\
SUMMARY:Standup\r\n\
X-NAIL-UNKNOWN:kept\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn icalendar_parses_expands_across_dst_and_round_trips() {
    let calendar = ICalendar::parse(DAILY).expect("valid iCalendar");
    let expanded = calendar.expand_dates(Tz::from(chrono_tz::Europe::Berlin), 100);
    assert!(expanded.errors.is_empty(), "{:?}", expanded.errors);
    assert_eq!(expanded.events.len(), 4);
    // 9:00 in Berlin each day, across the end of summer time (Oct 25).
    let utc_hours: Vec<u32> = expanded
        .events
        .iter()
        .map(|e| e.start.with_timezone(&chrono::Utc).hour())
        .collect();
    assert_eq!(utc_hours, [7, 7, 8, 8]);
    assert_eq!(expanded.events[0].start.day(), 23);

    // Unknown properties survive a round trip.
    let written = calendar.to_string();
    assert!(written.contains("X-NAIL-UNKNOWN:kept"), "{written}");
    assert!(written.contains("RRULE:FREQ=DAILY;COUNT=4"), "{written}");
}

#[test]
fn vcard_three_and_four_parse() {
    let v3 = VCard::parse(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Lovelace\r\nN:Lovelace;Ada;;;\r\n\
         EMAIL;TYPE=INTERNET,WORK:ada@work.example\r\nEND:VCARD\r\n",
    )
    .expect("valid vCard 3.0");
    let mut out = String::new();
    v3.write_to(&mut out, calcard::vcard::VCardVersion::V3_0)
        .unwrap();
    assert!(out.contains("FN:Ada Lovelace"), "{out}");
    assert!(out.contains("ada@work.example"), "{out}");

    let v4 = VCard::parse(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nFN:Team\r\n\
         MEMBER:urn:uuid:03a0e51f-d1aa-4385-8a53-e29025acd8af\r\nEND:VCARD\r\n",
    )
    .expect("valid vCard 4.0");
    let mut out = String::new();
    v4.write_to(&mut out, calcard::vcard::VCardVersion::V4_0)
        .unwrap();
    // calcard writes enumerated values in upper case (`KIND:GROUP`);
    // WP71 keeps the original text where lossless output matters.
    assert!(out.contains("KIND:GROUP"), "{out}");
    assert!(out.contains("MEMBER:urn:uuid:03a0e51f"), "{out}");
}

#[test]
fn iana_zones_cover_dst_gaps() {
    // 2026-03-29 02:30 does not exist in Berlin (spring forward).
    let berlin = chrono_tz::Europe::Berlin;
    assert!(
        berlin
            .with_ymd_and_hms(2026, 3, 29, 2, 30, 0)
            .single()
            .is_none()
    );
    // 2026-10-25 02:30 exists twice (fall back).
    assert!(matches!(
        berlin.with_ymd_and_hms(2026, 10, 25, 2, 30, 0),
        chrono::LocalResult::Ambiguous(_, _)
    ));
}
