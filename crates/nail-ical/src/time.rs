//! Dates and times: [`PimTime`] ↔ chrono, iCalendar `DATE` / `DATE-TIME`
//! values, and turning wall-clock times into instants with RFC 5545's
//! rules for DST gaps and folds (section 3.3.5): a time that falls into a
//! gap means the same time after the gap's offset change (02:30 on the
//! spring-forward night becomes 03:30), and a time that occurs twice
//! means the first (earlier) one.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use nail_api::pim::{PimTime, UTC_ZONE};

/// `YYYY-MM-DDTHH:MM:SS` → wall-clock time.
pub fn parse_local(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").ok()
}

/// `YYYY-MM-DD` → date.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// Wall-clock time → `YYYY-MM-DDTHH:MM:SS`.
pub fn format_local(time: NaiveDateTime) -> String {
    time.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Date → `YYYY-MM-DD`.
pub fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Instant → RFC 3339 UTC (`2026-10-01T07:00:00Z`), the contract's form.
pub fn format_instant(instant: DateTime<Utc>) -> String {
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// RFC 3339 instant (any offset, fractions allowed) → UTC.
pub fn parse_instant(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// An IANA zone by name (`UTC` included).
pub fn zone(name: &str) -> Option<Tz> {
    if name.eq_ignore_ascii_case(UTC_ZONE) || name == "Etc/UTC" || name == "Z" {
        return Some(Tz::UTC);
    }
    name.parse().ok()
}

/// The instant of a wall-clock time in `tz` by RFC 5545's rules: a time in
/// a DST gap is shifted forward by the gap, an ambiguous time takes the
/// earlier instant.
pub fn resolve_local(local: NaiveDateTime, tz: Tz) -> DateTime<Utc> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(t) => t.with_timezone(&Utc),
        LocalResult::Ambiguous(a, b) => a.min(b).with_timezone(&Utc),
        LocalResult::None => {
            // The offset in force before the gap applies.
            let before = local - Duration::hours(6);
            let offset = match tz.from_local_datetime(&before) {
                LocalResult::Single(t) => t.offset().fix(),
                LocalResult::Ambiguous(a, _) => a.offset().fix(),
                LocalResult::None => tz.offset_from_utc_datetime(&before).fix(),
            };
            Utc.from_utc_datetime(&(local - Duration::seconds(i64::from(offset.local_minus_utc()))))
        }
    }
}

/// The wall-clock time of an instant in `tz`.
pub fn to_local(instant: DateTime<Utc>, tz: Tz) -> NaiveDateTime {
    instant.with_timezone(&tz).naive_local()
}

/// The wall-clock part of a time (dates at midnight); `None` when
/// malformed.
pub fn wall(time: &PimTime) -> Option<NaiveDateTime> {
    match time {
        PimTime::Date { date } => parse_date(date).map(|d| d.and_time(NaiveTime::MIN)),
        PimTime::Floating { local } | PimTime::Zoned { local, .. } => parse_local(local),
    }
}

/// The instant of a time; dates (at midnight) and floating times are
/// placed in `viewer`. `None` when malformed or the zone is unknown.
pub fn instant(time: &PimTime, viewer: Tz) -> Option<DateTime<Utc>> {
    let local = wall(time)?;
    let tz = match time {
        PimTime::Zoned { tz, .. } => zone(tz)?,
        _ => viewer,
    };
    Some(resolve_local(local, tz))
}

/// A wall-clock time expressed like `like`: same kind and zone.
pub fn like(local: NaiveDateTime, like: &PimTime) -> PimTime {
    match like {
        PimTime::Date { .. } => PimTime::date(format_date(local.date())),
        PimTime::Floating { .. } => PimTime::floating(format_local(local)),
        PimTime::Zoned { tz, .. } => PimTime::zoned(format_local(local), tz.clone()),
    }
}

/// `time` converted into the wall-clock frame of `start` (the series
/// start): the same zone, so recurrence ids, `RDATE`s and `EXDATE`s given
/// in UTC or another zone compare with generated occurrences. Dates stay
/// dates (a date-time against a date start keeps its date). `None` when
/// malformed.
pub fn to_frame(time: &PimTime, start: &PimTime) -> Option<NaiveDateTime> {
    let local = wall(time)?;
    match (time, start) {
        (_, PimTime::Date { .. }) => Some(local.date().and_time(NaiveTime::MIN)),
        (PimTime::Zoned { tz: a, .. }, PimTime::Zoned { tz: b, .. }) if a != b => {
            let (from, to) = (zone(a)?, zone(b)?);
            Some(to_local(resolve_local(local, from), to))
        }
        _ => Some(local),
    }
}

/// Time of day of a wall-clock time as a duration since midnight.
pub fn time_of_day(local: NaiveDateTime) -> Duration {
    Duration::seconds(i64::from(local.num_seconds_from_midnight()))
}

/// A parsed iCalendar `DATE` or `DATE-TIME` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcalValue {
    /// `VALUE=DATE` (`20261001`).
    Date(NaiveDate),
    /// A date-time; `utc` when it ends with `Z`.
    DateTime {
        /// Wall-clock time.
        local: NaiveDateTime,
        /// UTC form.
        utc: bool,
    },
}

/// Parses `20261001`, `20261001T090000` or `20261001T090000Z` (also
/// tolerates `2026-10-01T09:00:00` and missing seconds).
pub fn parse_ical_value(text: &str) -> Option<IcalValue> {
    let text = text.trim();
    let compact: String = text.chars().filter(|c| *c != '-' && *c != ':').collect();
    let (body, utc) = match compact.strip_suffix(['Z', 'z']) {
        Some(body) => (body, true),
        None => (compact.as_str(), false),
    };
    match body.split_once(['T', 't']) {
        None => {
            if body.len() != 8 || utc {
                return None;
            }
            NaiveDate::parse_from_str(body, "%Y%m%d")
                .ok()
                .map(IcalValue::Date)
        }
        Some((date, time)) => {
            let date = NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
            let time = match time.len() {
                6 => NaiveTime::parse_from_str(time, "%H%M%S").ok().or_else(|| {
                    // Second 60 (leap second) is read as 59.
                    time.strip_suffix("60")
                        .and_then(|hm| NaiveTime::parse_from_str(&format!("{hm}59"), "%H%M%S").ok())
                })?,
                4 => NaiveTime::parse_from_str(time, "%H%M").ok()?,
                _ => {
                    // Fractions (`090000.000`) are dropped.
                    let main = time.split('.').next()?;
                    NaiveTime::parse_from_str(main, "%H%M%S").ok()?
                }
            };
            Some(IcalValue::DateTime {
                local: date.and_time(time),
                utc,
            })
        }
    }
}

/// `20261001`.
pub fn ical_date(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

/// `20261001T090000`.
pub fn ical_local(local: NaiveDateTime) -> String {
    local.format("%Y%m%dT%H%M%S").to_string()
}

/// `20261001T070000Z`.
pub fn ical_utc(instant: DateTime<Utc>) -> String {
    instant.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Parses an RFC 5545 `DURATION` (`-PT15M`, `P1DT2H`, `P2W`) into
/// seconds. Days count 24 hours here; [`parse_duration_parts`] keeps
/// nominal days apart.
pub fn parse_duration(text: &str) -> Option<i64> {
    let (days, seconds) = parse_duration_parts(text)?;
    days.checked_mul(86_400)?.checked_add(seconds)
}

/// Parses a `DURATION` into nominal days (weeks × 7 + days) and exact
/// seconds, both with the sign applied.
pub fn parse_duration_parts(text: &str) -> Option<(i64, i64)> {
    let text = text.trim();
    let (sign, rest) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    let rest = rest.strip_prefix(['P', 'p'])?;
    let mut days = 0i64;
    let mut seconds = 0i64;
    let mut number = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => number.push(c),
            'T' if number.is_empty() => in_time = true,
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                any = true;
                match (unit, in_time) {
                    ('W', false) => days = days.checked_add(n.checked_mul(7)?)?,
                    ('D', false) => days = days.checked_add(n)?,
                    ('H', true) => seconds = seconds.checked_add(n.checked_mul(3600)?)?,
                    ('M', true) => seconds = seconds.checked_add(n.checked_mul(60)?)?,
                    ('S', true) => seconds = seconds.checked_add(n)?,
                    _ => return None,
                }
            }
        }
    }
    (any && number.is_empty()).then_some((sign * days, sign * seconds))
}

/// Writes seconds as a `DURATION` (`-PT15M`, `P1D`, `P1DT2H30M`, `PT0S`);
/// whole weeks as `P1W`.
pub fn format_duration(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let mut rest = seconds.unsigned_abs();
    if rest == 0 {
        return "PT0S".to_owned();
    }
    if rest.is_multiple_of(7 * 86_400) {
        return format!("{sign}P{}W", rest / (7 * 86_400));
    }
    let days = rest / 86_400;
    rest %= 86_400;
    let mut out = format!("{sign}P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if rest > 0 {
        out.push('T');
        let (h, m, s) = (rest / 3600, rest % 3600 / 60, rest % 60);
        if h > 0 {
            out.push_str(&format!("{h}H"));
        }
        if m > 0 {
            out.push_str(&format!("{m}M"));
        }
        if s > 0 {
            out.push_str(&format!("{s}S"));
        }
    }
    out
}

/// Whether a year is within what Nail stores (`PimTime` years 1–9999).
pub fn year_in_range(local: NaiveDateTime) -> bool {
    (1..=9999).contains(&local.year())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        parse_local(text).unwrap()
    }

    #[test]
    fn gaps_shift_forward_and_folds_take_the_first() {
        let berlin = chrono_tz::Europe::Berlin;
        // Spring forward: 02:30 does not exist → 03:30 CEST = 01:30Z.
        assert_eq!(
            format_instant(resolve_local(at("2026-03-29T02:30:00"), berlin)),
            "2026-03-29T01:30:00Z"
        );
        // Fall back: 02:30 twice → the first (CEST) = 00:30Z.
        assert_eq!(
            format_instant(resolve_local(at("2026-10-25T02:30:00"), berlin)),
            "2026-10-25T00:30:00Z"
        );
        let new_york = chrono_tz::America::New_York;
        assert_eq!(
            format_instant(resolve_local(at("2026-03-08T02:15:00"), new_york)),
            "2026-03-08T07:15:00Z"
        );
        // Lord Howe: a 30-minute DST shift.
        let lord_howe = chrono_tz::Australia::Lord_Howe;
        assert_eq!(
            format_instant(resolve_local(at("2026-10-04T02:15:00"), lord_howe)),
            "2026-10-03T15:45:00Z"
        );
    }

    #[test]
    fn instants_of_pim_times() {
        let tokyo = chrono_tz::Asia::Tokyo;
        assert_eq!(
            instant(&PimTime::date("2026-10-01"), tokyo).map(format_instant),
            Some("2026-09-30T15:00:00Z".into())
        );
        assert_eq!(
            instant(&PimTime::floating("2026-10-01T09:00:00"), Tz::UTC).map(format_instant),
            Some("2026-10-01T09:00:00Z".into())
        );
        assert_eq!(
            instant(
                &PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin"),
                tokyo
            )
            .map(format_instant),
            Some("2026-10-01T07:00:00Z".into())
        );
        assert_eq!(
            instant(&PimTime::zoned("2026-10-01T09:00:00", "Mars/Base"), tokyo),
            None
        );
        assert_eq!(
            to_frame(
                &PimTime::utc("2026-10-01T07:00:00"),
                &PimTime::zoned("2026-01-01T09:00:00", "Europe/Berlin")
            ),
            Some(at("2026-10-01T09:00:00"))
        );
        assert_eq!(
            to_frame(
                &PimTime::utc("2026-10-01T07:00:00"),
                &PimTime::date("2026-01-01")
            ),
            Some(at("2026-10-01T00:00:00"))
        );
        assert_eq!(
            like(
                at("2026-10-02T10:00:00"),
                &PimTime::zoned("x", "Asia/Tokyo")
            ),
            PimTime::zoned("2026-10-02T10:00:00", "Asia/Tokyo")
        );
    }

    #[test]
    fn ical_values() {
        assert_eq!(
            parse_ical_value("20261001"),
            Some(IcalValue::Date(
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
            ))
        );
        assert_eq!(
            parse_ical_value("20261001T090000Z"),
            Some(IcalValue::DateTime {
                local: at("2026-10-01T09:00:00"),
                utc: true
            })
        );
        assert_eq!(
            parse_ical_value("2026-10-01T09:00:00"),
            Some(IcalValue::DateTime {
                local: at("2026-10-01T09:00:00"),
                utc: false
            })
        );
        assert_eq!(
            parse_ical_value("20261001T0900"),
            Some(IcalValue::DateTime {
                local: at("2026-10-01T09:00:00"),
                utc: false
            })
        );
        assert!(parse_ical_value("20261301").is_none());
        assert!(parse_ical_value("hello").is_none());
        assert_eq!(ical_local(at("2026-10-01T09:00:00")), "20261001T090000");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("P2W"), Some(1_209_600));
        assert_eq!(parse_duration("PT0S"), Some(0));
        assert_eq!(parse_duration_parts("P1DT1H"), Some((1, 3600)));
        assert_eq!(parse_duration("P"), None);
        assert_eq!(parse_duration("PT1D"), None);
        assert_eq!(parse_duration("15M"), None);
        for secs in [-900, 0, 86_400, 93_600, 1_209_600, 3_725, -604_800 * 2] {
            assert_eq!(parse_duration(&format_duration(secs)), Some(secs), "{secs}");
        }
        assert_eq!(format_duration(-900), "-PT15M");
        assert_eq!(format_duration(86_400), "P1D");
    }
}
