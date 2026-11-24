//! Table-driven tests of the reader: "now" is Wednesday 25 November 2026,
//! 10:40 in Berlin.

use super::*;
use nail_api::pim::calendar::{Frequency, WeekdayNum};
use nail_api::triage::Weekday;

const TZ: &str = "Europe/Berlin";

fn context(locale: &str) -> Context {
    Context {
        now: NaiveDate::from_ymd_opt(2026, 11, 25)
            .and_then(|d| d.and_hms_opt(10, 40, 0))
            .unwrap(),
        tz: TZ.into(),
        locale: locale.into(),
    }
}

fn read(text: &str) -> ParsedEventText {
    parse(text, &context("en"))
}

/// `2026-11-26` = a date, `2026-11-26T12:30` = a time in Berlin.
fn at(text: &str) -> PimTime {
    if text.len() == 10 {
        PimTime::date(text)
    } else {
        PimTime::zoned(format!("{text}:00"), TZ)
    }
}

/// (text, title, start, end, place)
type Case = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

#[rustfmt::skip]
const CASES: &[Case] = &[
    // The gate's sentence.
    (
        "Lunch with Bob tomorrow 12:30 at Corner café",
        "Lunch with Bob",
        Some("2026-11-26T12:30"),
        Some("2026-11-26T13:30"),
        Some("Corner café"),
    ),
    // Weekdays: the coming one; "next" a week later; today counts.
    ("Dentist Friday 3pm", "Dentist", Some("2026-11-27T15:00"), Some("2026-11-27T16:00"), None),
    ("Review next Friday 9am", "Review", Some("2026-12-04T09:00"), Some("2026-12-04T10:00"), None),
    ("Retro this Wednesday", "Retro", Some("2026-11-25"), Some("2026-11-26"), None),
    ("Retro Wednesday", "Retro", Some("2026-11-25"), Some("2026-11-26"), None),
    ("Lunch on Sun", "Lunch", Some("2026-11-29"), Some("2026-11-30"), None),
    ("Brunch Sun 11am", "Brunch", Some("2026-11-29T11:00"), Some("2026-11-29T12:00"), None),
    // Abbreviations that are words stay words without a hint.
    ("sun cream and hat", "sun cream and hat", None, None, None),
    ("Report fri. 4pm", "Report", Some("2026-11-27T16:00"), Some("2026-11-27T17:00"), None),
    // Dates.
    ("Party Dec 4", "Party", Some("2026-12-04"), Some("2026-12-05"), None),
    ("Party 4th of December", "Party", Some("2026-12-04"), Some("2026-12-05"), None),
    ("Party 4 dec", "Party", Some("2026-12-04"), Some("2026-12-05"), None),
    ("Party on December 4th", "Party", Some("2026-12-04"), Some("2026-12-05"), None),
    // A passed day of the year is next year's; 29 February the next leap year.
    ("Taxes Mar 1", "Taxes", Some("2027-03-01"), Some("2027-03-02"), None),
    ("Leap Feb 29", "Leap", Some("2028-02-29"), Some("2028-03-01"), None),
    (
        "Board May 5 2027 10:00-11:30",
        "Board",
        Some("2027-05-05T10:00"),
        Some("2027-05-05T11:30"),
        None,
    ),
    ("Trip 2026-12-24", "Trip", Some("2026-12-24"), Some("2026-12-25"), None),
    ("Release 12/4", "Release", Some("2026-12-04"), Some("2026-12-05"), None),
    ("Release 12/4/27", "Release", Some("2027-12-04"), Some("2027-12-05"), None),
    ("Planning in 2 weeks", "Planning", Some("2026-12-09"), Some("2026-12-10"), None),
    ("Visit the day after tomorrow", "Visit", Some("2026-11-27"), Some("2026-11-28"), None),
    ("Pick up today", "Pick up", Some("2026-11-25"), Some("2026-11-26"), None),
    // Month names need a day number.
    ("May the force be with you", "May the force be with you", None, None, None),
    ("March for climate", "March for climate", None, None, None),
    // Times: today, or tomorrow once passed.
    ("Standup 9:30", "Standup", Some("2026-11-26T09:30"), Some("2026-11-26T10:30"), None),
    ("Coffee at noon", "Coffee", Some("2026-11-25T12:00"), Some("2026-11-25T13:00"), None),
    ("Deploy at midnight", "Deploy", Some("2026-11-26T00:00"), Some("2026-11-26T01:00"), None),
    ("Talk tomorrow 3 pm", "Talk", Some("2026-11-26T15:00"), Some("2026-11-26T16:00"), None),
    ("Talk tomorrow 3 p.m.", "Talk", Some("2026-11-26T15:00"), Some("2026-11-26T16:00"), None),
    ("Tea 16.30", "Tea", Some("2026-11-25T16:30"), Some("2026-11-25T17:30"), None),
    // A bare number is a time only after at / from; 1 to 6 mean afternoon.
    ("Meet Ana at 3", "Meet Ana", Some("2026-11-25T15:00"), Some("2026-11-25T16:00"), None),
    ("Breakfast at 8", "Breakfast", Some("2026-11-26T08:00"), Some("2026-11-26T09:00"), None),
    ("Room 4 booking", "Room 4 booking", None, None, None),
    ("Score 3-4", "Score 3-4", None, None, None),
    // Ranges: the half on one end applies to the other.
    ("Call 3-4pm", "Call", Some("2026-11-25T15:00"), Some("2026-11-25T16:00"), None),
    ("Workshop 11-1pm", "Workshop", Some("2026-11-25T11:00"), Some("2026-11-25T13:00"), None),
    ("Shift 9am-5", "Shift", Some("2026-11-26T09:00"), Some("2026-11-26T17:00"), None),
    ("Class 3pm - 4:30pm", "Class", Some("2026-11-25T15:00"), Some("2026-11-25T16:30"), None),
    ("Class 3pm \u{2013} 4:30pm", "Class", Some("2026-11-25T15:00"), Some("2026-11-25T16:30"), None),
    ("Class from 3 to 4", "Class", Some("2026-11-25T15:00"), Some("2026-11-25T16:00"), None),
    ("Class from 11 to 1", "Class", Some("2026-11-25T11:00"), Some("2026-11-25T13:00"), None),
    ("Class 14:00 until 15:15", "Class", Some("2026-11-25T14:00"), Some("2026-11-25T15:15"), None),
    // Past midnight ends the next day.
    ("Late show 11pm-1am", "Late show", Some("2026-11-25T23:00"), Some("2026-11-26T01:00"), None),
    // Lengths.
    ("Sync for 30 min at 2pm", "Sync", Some("2026-11-25T14:00"), Some("2026-11-25T14:30"), None),
    ("Sync 2pm for an hour", "Sync", Some("2026-11-25T14:00"), Some("2026-11-25T15:00"), None),
    ("Sync 2pm for half an hour", "Sync", Some("2026-11-25T14:00"), Some("2026-11-25T14:30"), None),
    ("Sync 2pm 90 minutes", "Sync", Some("2026-11-25T14:00"), Some("2026-11-25T15:30"), None),
    ("Sync 2pm for 1h30", "Sync", Some("2026-11-25T14:00"), Some("2026-11-25T15:30"), None),
    ("Focus for 1.5 hours", "Focus", Some("2026-11-25T10:45"), Some("2026-11-25T12:15"), None),
    ("Talk 2h", "Talk", Some("2026-11-25T10:45"), Some("2026-11-25T12:45"), None),
    ("Run 5 m", "Run 5 m", None, None, None),
    ("Conference Dec 7 for 3 days", "Conference", Some("2026-12-07"), Some("2026-12-10"), None),
    // A range wins over a length.
    ("Call 3-4pm for 2 hours", "Call", Some("2026-11-25T15:00"), Some("2026-11-25T16:00"), None),
    // All day wins over a time.
    ("Offsite all day tomorrow", "Offsite", Some("2026-11-26"), Some("2026-11-27"), None),
    ("Offsite tomorrow all-day 9am", "Offsite", Some("2026-11-26"), Some("2026-11-27"), None),
    ("Ski trip all day", "Ski trip", Some("2026-11-25"), Some("2026-11-26"), None),
    // Places.
    (
        "Lunch at Corner café tomorrow at 1pm",
        "Lunch",
        Some("2026-11-26T13:00"),
        Some("2026-11-26T14:00"),
        Some("Corner café"),
    ),
    ("Call @ 4pm", "Call", Some("2026-11-25T16:00"), Some("2026-11-25T17:00"), None),
    ("Drinks @Bar Luce", "Drinks", None, None, Some("Bar Luce")),
    ("Dinner, tomorrow, at The Ivy.", "Dinner", Some("2026-11-26"), Some("2026-11-27"), Some("The Ivy")),
    ("Meet at", "Meet at", None, None, None),
    // Only the first of a kind counts.
    ("Move Monday to Tuesday", "Move to Tuesday", Some("2026-11-30"), Some("2026-12-01"), None),
    // Nothing recognised.
    ("  Call mum ", "Call mum", None, None, None),
    ("", "", None, None, None),
];

#[test]
fn cases() {
    let mut failures = Vec::new();
    for (text, title, start, end, place) in CASES {
        let parsed = read(text);
        let want = (
            title.to_string(),
            start.map(at),
            end.map(at),
            place.map(str::to_owned),
        );
        let got = (
            parsed.title.clone(),
            parsed.start.clone(),
            parsed.end.clone(),
            parsed.location.clone(),
        );
        if got != want {
            failures.push(format!("{text:?}: got {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn weekly(days: &[Weekday], interval: u32) -> RecurrenceRule {
    let mut rule = RecurrenceRule::new(Frequency::Weekly);
    rule.interval = interval;
    rule.by_day = days
        .iter()
        .map(|d| WeekdayNum {
            day: *d,
            ordinal: None,
        })
        .collect();
    rule
}

#[test]
fn repetitions() {
    let simple = |freq, interval| {
        let mut rule = RecurrenceRule::new(freq);
        rule.interval = interval;
        rule
    };
    let cases: Vec<(&str, RecurrenceRule, &str, &str, &str)> = vec![
        (
            "Yoga every Monday 7pm",
            weekly(&[Weekday::Mon], 1),
            "Yoga",
            "2026-11-30T19:00",
            "2026-11-30T20:00",
        ),
        // Seven o'clock has passed today; Thursday is the next weekday.
        (
            "Gym every weekday 7am",
            weekly(
                &[
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                1,
            ),
            "Gym",
            "2026-11-26T07:00",
            "2026-11-26T08:00",
        ),
        (
            "Swim every Fri and Tue",
            weekly(&[Weekday::Tue, Weekday::Fri], 1),
            "Swim",
            "2026-11-27",
            "2026-11-28",
        ),
        (
            "Team call every other week Tue 4pm",
            weekly(&[], 2),
            "Team call",
            "2026-12-01T16:00",
            "2026-12-01T17:00",
        ),
        (
            "Water plants every 3 days",
            simple(Frequency::Daily, 3),
            "Water plants",
            "2026-11-25",
            "2026-11-26",
        ),
        (
            "Pay rent monthly",
            simple(Frequency::Monthly, 1),
            "Pay rent",
            "2026-11-25",
            "2026-11-26",
        ),
        (
            "Standup daily at 9:15",
            simple(Frequency::Daily, 1),
            "Standup",
            "2026-11-26T09:15",
            "2026-11-26T10:15",
        ),
        (
            "Birthday Dec 4 yearly",
            simple(Frequency::Yearly, 1),
            "Birthday",
            "2026-12-04",
            "2026-12-05",
        ),
        (
            "Review every month",
            simple(Frequency::Monthly, 1),
            "Review",
            "2026-11-25",
            "2026-11-26",
        ),
    ];
    for (text, rule, title, start, end) in cases {
        let parsed = read(text);
        assert_eq!(parsed.recurrence, Some(Recurrence::rule(rule)), "{text}");
        assert_eq!(parsed.title, title, "{text}");
        assert_eq!(parsed.start, Some(at(start)), "{text}");
        assert_eq!(parsed.end, Some(at(end)), "{text}");
    }
    // "every" without a unit is a word.
    let plain = read("Thanks for every thing");
    assert!(plain.recurrence.is_none() && plain.start.is_none());
}

#[test]
fn spans_mark_the_parts_in_utf16() {
    let parsed = read("Lunch with Bob tomorrow 12:30 at Corner café");
    let spans: Vec<(ParsedPart, u32, u32)> = parsed
        .spans
        .iter()
        .map(|s| (s.kind, s.start, s.end))
        .collect();
    assert_eq!(
        spans,
        [
            (ParsedPart::Date, 15, 23),
            (ParsedPart::Time, 24, 29),
            (ParsedPart::Location, 30, 44),
        ]
    );
    // An emoji outside the BMP is two code units.
    let parsed = read("\u{1F600} Party tomorrow");
    assert_eq!(parsed.spans[0].start, 9);
    assert_eq!(parsed.spans[0].end, 17);
    assert_eq!(parsed.title, "\u{1F600} Party");
    let kinds =
        |text: &str| -> Vec<ParsedPart> { read(text).spans.iter().map(|s| s.kind).collect() };
    assert_eq!(
        kinds("Yoga every Monday 7pm for 45 min all day"),
        [
            ParsedPart::Recurrence,
            ParsedPart::Time,
            ParsedPart::Duration,
            ParsedPart::AllDay
        ]
    );
    // The "on" before a date belongs to the date's span.
    let parsed = read("Party on Friday");
    assert_eq!((parsed.spans[0].start, parsed.spans[0].end), (6, 15));
}

#[test]
fn numeric_dates_follow_the_region() {
    let gb = parse("Release 12/4", &context("en-GB"));
    assert_eq!(gb.start, Some(at("2027-04-12")));
    let us = parse("Release 12/4", &context("en-US"));
    assert_eq!(us.start, Some(at("2026-12-04")));
    // Unknown languages read as English.
    let other = parse("Party tomorrow", &context("xx"));
    assert_eq!(other.start, Some(at("2026-11-26")));
}

#[test]
fn times_are_zoned_in_the_context_zone() {
    let mut ctx = context("en");
    ctx.tz = "America/New_York".into();
    let parsed = parse("Call 3pm", &ctx);
    assert_eq!(
        parsed.start,
        Some(PimTime::zoned("2026-11-25T15:00:00", "America/New_York"))
    );
}

#[test]
fn next_quarter_rounds_up() {
    let t = |h, m, s| {
        NaiveDate::from_ymd_opt(2026, 11, 25)
            .and_then(|d| d.and_hms_opt(h, m, s))
            .unwrap()
    };
    assert_eq!(next_quarter(t(10, 40, 0)), t(10, 45, 0));
    assert_eq!(next_quarter(t(10, 45, 0)), t(10, 45, 0));
    assert_eq!(next_quarter(t(10, 45, 1)), t(11, 0, 0));
    assert_eq!(next_quarter(t(23, 50, 0)), t(0, 0, 0) + Duration::days(1));
}

/// Property: whatever the text, the result keeps the contract's rules.
#[test]
fn any_text_keeps_the_rules() {
    const POOL: &[&str] = &[
        "Lunch",
        "with",
        "Bob",
        "tomorrow",
        "today",
        "12:30",
        "3pm",
        "3-4pm",
        "at",
        "@",
        "for",
        "30",
        "min",
        "hours",
        "an",
        "hour",
        "half",
        "every",
        "other",
        "Monday",
        "and",
        "Fri",
        "next",
        "this",
        "on",
        "Dec",
        "4th",
        "of",
        "May",
        "2027",
        "all",
        "day",
        "all-day",
        "from",
        "to",
        "-",
        "\u{2013}",
        "noon",
        "midnight",
        "in",
        "2",
        "weeks",
        "12/4",
        "2026-12-24",
        "café",
        "\u{1F600}",
        ",",
        ".",
        "p.m.",
        "am",
        "1h30",
        "1.5h",
        "daily",
        "weekday",
        "Sun",
        "sat",
        "",
        "  ",
    ];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..3000 {
        let count = next() % 9;
        let words: Vec<&str> = (0..count)
            .map(|_| POOL[usize::try_from(next() % POOL.len() as u64).unwrap()])
            .collect();
        let text = words.join(" ");
        let parsed = read(&text);
        let units = u32::try_from(text.encode_utf16().count()).unwrap();
        assert_eq!(parsed.start.is_some(), parsed.end.is_some(), "{text:?}");
        if let (Some(s), Some(e)) = (&parsed.start, &parsed.end) {
            assert_eq!(s.is_date(), e.is_date(), "{text:?}");
            assert!(e.sort_key() > s.sort_key(), "{text:?}: {s:?} {e:?}");
        }
        let mut last = 0;
        for span in &parsed.spans {
            assert!(
                span.start >= last && span.end > span.start && span.end <= units,
                "{text:?}"
            );
            last = span.end;
        }
        assert_eq!(parsed.title, parsed.title.trim(), "{text:?}");
    }
}
