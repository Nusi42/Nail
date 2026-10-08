//! The iCalendar (RFC 5545) value texts several crates read and write
//! without the full `nail-ical` model: UTC `DATE-TIME` stamps
//! (`YYYYMMDDTHHMMSSZ`) and `DURATION`s. One definition for `nail-ical`,
//! the DAV client's free/busy code and the mock DAV server (wave Q2,
//! ADR 0171; before, each had its own copy).

use crate::time::{SECS_PER_DAY, civil_from_days, days_from_civil, days_in_month};

/// Seconds per hour.
const SECS_PER_HOUR: i64 = 3_600;
/// Seconds per minute.
const SECS_PER_MINUTE: i64 = 60;
/// Days per week.
const DAYS_PER_WEEK: i64 = 7;

/// Unix seconds → `YYYYMMDDTHHMMSSZ` (proleptic Gregorian, UTC).
#[must_use]
pub fn utc_stamp(secs: i64) -> String {
    let (year, month, day) = civil_from_days(secs.div_euclid(SECS_PER_DAY));
    let rem = secs.rem_euclid(SECS_PER_DAY);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / SECS_PER_HOUR,
        rem % SECS_PER_HOUR / SECS_PER_MINUTE,
        rem % SECS_PER_MINUTE
    )
}

/// A `DATE-TIME` (`YYYYMMDDTHHMMSS`, a trailing `Z` optional: the value is
/// taken as UTC) or a `DATE` (`YYYYMMDD`, midnight UTC) → Unix seconds;
/// `None` for anything else or an impossible date / time.
#[must_use]
pub fn parse_utc_stamp(value: &str) -> Option<i64> {
    let v = value.trim();
    let v = v.strip_suffix(['Z', 'z']).unwrap_or(v);
    let b = v.as_bytes();
    let number = |r: std::ops::Range<usize>| -> Option<u32> {
        let digits = b.get(r)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    if day == 0 || day > days_in_month(i64::from(year), month)? {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    let (hour, minute, second) = match b.len() {
        8 => (0, 0, 0),
        15 if b[8].eq_ignore_ascii_case(&b'T') => {
            (number(9..11)?, number(11..13)?, number(13..15)?)
        }
        _ => return None,
    };
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(
        days * SECS_PER_DAY
            + i64::from(hour) * SECS_PER_HOUR
            + i64::from(minute) * SECS_PER_MINUTE
            + i64::from(second),
    )
}

/// A leading `+` / `-` of `text` as a sign (`1` / `-1`; none: `1`) and the
/// rest; `None` for empty text.
#[must_use]
pub fn split_sign(text: &str) -> Option<(i64, &str)> {
    Some(match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    })
}

/// Parses a `DURATION` (`-PT15M`, `P1DT2H`, `P2W`) into nominal days
/// (weeks × 7 + days) and exact seconds, both with the sign applied.
#[must_use]
pub fn parse_duration_parts(text: &str) -> Option<(i64, i64)> {
    let (sign, rest) = split_sign(text.trim())?;
    let rest = rest.strip_prefix(['P', 'p'])?;
    let mut days = 0i64;
    let mut seconds = 0i64;
    let mut number = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => number.push(c),
            'T' if number.is_empty() && !in_time => in_time = true,
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                any = true;
                match (unit, in_time) {
                    ('W', false) => days = days.checked_add(n.checked_mul(DAYS_PER_WEEK)?)?,
                    ('D', false) => days = days.checked_add(n)?,
                    ('H', true) => seconds = seconds.checked_add(n.checked_mul(SECS_PER_HOUR)?)?,
                    ('M', true) => {
                        seconds = seconds.checked_add(n.checked_mul(SECS_PER_MINUTE)?)?;
                    }
                    ('S', true) => seconds = seconds.checked_add(n)?,
                    _ => return None,
                }
            }
        }
    }
    (any && number.is_empty()).then_some((sign * days, sign * seconds))
}

/// Parses a `DURATION` into seconds (days count 24 hours; see
/// [`parse_duration_parts`] for nominal days).
#[must_use]
pub fn parse_duration(text: &str) -> Option<i64> {
    let (days, seconds) = parse_duration_parts(text)?;
    days.checked_mul(SECS_PER_DAY)?.checked_add(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_round_trip() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(1_788_998_400 + 3_723), "20260910T010203Z");
        assert_eq!(utc_stamp(-1), "19691231T235959Z");
        for secs in [0, 59, 1_788_998_400, -86_401, 4_102_444_799] {
            assert_eq!(parse_utc_stamp(&utc_stamp(secs)), Some(secs), "{secs}");
        }
        assert_eq!(parse_utc_stamp("20260910"), Some(1_788_998_400));
        assert_eq!(
            parse_utc_stamp(" 20260910t010203 "),
            Some(1_788_998_400 + 3_723)
        );
        for bad in [
            "",
            "2026091",
            "20261301",
            "20260230",
            "20260910T250000Z",
            "20260910X010203",
            "2026-09-10",
            "2026091aT000000",
        ] {
            assert_eq!(parse_utc_stamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn signs() {
        assert_eq!(split_sign("-0100"), Some((-1, "0100")));
        assert_eq!(split_sign("+02:00"), Some((1, "02:00")));
        assert_eq!(split_sign("0300"), Some((1, "0300")));
        assert_eq!(split_sign(""), None);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT15M"), Some(900));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("+P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("p2w"), Some(1_209_600));
        assert_eq!(parse_duration_parts("-P1DT1S"), Some((-1, -1)));
        for bad in ["", "P", "PT", "1H", "P1H", "PT1D", "PTT1H", "P1", "P1X"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }
}
