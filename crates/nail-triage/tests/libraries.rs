//! Smoke tests of the libraries WP52 builds on (WP50, ADR 0043).
#![cfg(test)]

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use unicode_segmentation::UnicodeSegmentation;

#[test]
fn words_split_on_unicode_boundaries() {
    let words: Vec<&str> = "Grüße, CHEAP pills!! 50% off".unicode_words().collect();
    assert_eq!(words, ["Grüße", "CHEAP", "pills", "50", "off"]);
}

#[test]
fn local_times_and_weekdays() {
    let day = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    assert_eq!(day.weekday(), chrono::Weekday::Wed);
    let t = NaiveTime::parse_from_str("22:30", "%H:%M").unwrap();
    assert_eq!(t.hour() * 60 + t.minute(), 1350);
    assert_eq!(
        nail_api::triage::parse_hhmm("22:30"),
        Some(t.hour() * 60 + t.minute())
    );
}
