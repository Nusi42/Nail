//! Calendar arithmetic (proleptic Gregorian, Howard Hinnant's civil-date
//! algorithms) and Unix time, one definition for every crate (before Q2 it
//! was copied into eleven files; ADR 0171).

/// Seconds per day.
pub const SECS_PER_DAY: i64 = 86_400;

/// A fixed instant for tests and fixtures, after every fixture key's
/// creation: 2026-09-21T14:13:20Z (Unix seconds).
pub const FIXTURE_NOW: i64 = 1_790_000_000;

/// A second fixed instant for tests, months after [`FIXTURE_NOW`]:
/// 2027-01-15T08:00:00Z.
pub const FIXTURE_LATER: i64 = 1_800_000_000;

/// Days since 1970-01-01 of a civil date (`month` 1–12, `day` 1–31; not
/// validated, out-of-range values roll over like the algorithm does).
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of a day number (days since
/// 1970-01-01).
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // `day` is 1..=31 and `month` 1..=12 by construction.
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// Days of a common (non-leap) year: a year's span in day-based windows.
pub const DAYS_PER_YEAR: u16 = 365;

/// The number of days of `year`.
#[must_use]
pub fn days_in_year(year: i64) -> i64 {
    i64::from(DAYS_PER_YEAR) + i64::from(is_leap_year(year))
}

/// Whether `year` is a leap year.
#[must_use]
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days of `month` (1–12) in `year`; `None` for another month.
#[must_use]
pub fn days_in_month(year: i64, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

/// The weekday of a day number (days since 1970-01-01), counted from
/// Monday (0 = Monday … 6 = Sunday).
#[must_use]
pub fn weekday_from_monday(days: i64) -> u32 {
    // 1970-01-01 was a Thursday; `rem_euclid` is 0..=6.
    u32::try_from((days + 3).rem_euclid(7)).unwrap_or_default()
}

/// The day of the month of the last `weekday` (0 = Monday … 6 = Sunday) of
/// `month` in `year` (the last Sunday of March: DST rules); `None` for an
/// invalid month.
#[must_use]
pub fn last_weekday_of_month(year: i64, month: u32, weekday: u32) -> Option<u32> {
    let last = days_in_month(year, month)?;
    let last_weekday = weekday_from_monday(days_from_civil(year, month, last));
    Some(last - (last_weekday + 7 - weekday % 7) % 7)
}

/// The current Unix time in seconds (0 when the clock is before 1970).
#[must_use]
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `YYYY-MM-DD` of a day number.
#[must_use]
pub fn iso_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Unix seconds as RFC 3339 UTC with whole seconds
/// (`1970-01-01T00:00:00Z`).
#[must_use]
pub fn rfc3339_utc(secs: i64) -> String {
    let rem = secs.rem_euclid(SECS_PER_DAY);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        iso_date(secs.div_euclid(SECS_PER_DAY)),
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2026, 9, 10) * SECS_PER_DAY, 1_788_998_400);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for days in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
        assert_eq!(civil_from_days(days_from_civil(2028, 2, 29)), (2028, 2, 29));
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
    }

    #[test]
    fn months_and_leap_years() {
        assert!(is_leap_year(2000) && is_leap_year(2028) && !is_leap_year(1900));
        assert_eq!(days_in_year(2028) - days_in_year(2027), 1);
        assert_eq!(days_in_year(1900), i64::from(DAYS_PER_YEAR));
        assert_eq!(days_in_month(2026, 2), Some(28));
        assert_eq!(days_in_month(2028, 2), Some(29));
        assert_eq!(days_in_month(2026, 4), Some(30));
        assert_eq!(days_in_month(2026, 13), None);
    }

    #[test]
    fn weekdays_and_last_weekdays() {
        assert_eq!(weekday_from_monday(0), 3, "1970-01-01 was a Thursday");
        assert_eq!(weekday_from_monday(-1), 2);
        assert_eq!(weekday_from_monday(days_from_civil(2026, 10, 5)), 0);
        // EU DST 2026: 29 March and 25 October.
        assert_eq!(last_weekday_of_month(2026, 3, 6), Some(29));
        assert_eq!(last_weekday_of_month(2026, 10, 6), Some(25));
        // The month's last day itself.
        assert_eq!(last_weekday_of_month(2026, 1, 5), Some(31));
        assert_eq!(last_weekday_of_month(2026, 13, 6), None);
    }

    #[test]
    fn fixture_instants() {
        assert_eq!(rfc3339_utc(FIXTURE_NOW), "2026-09-21T14:13:20Z");
        assert_eq!(rfc3339_utc(FIXTURE_LATER), "2027-01-15T08:00:00Z");
    }

    #[test]
    fn text_forms() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_788_998_400 + 3_723), "2026-09-10T01:02:03Z");
        assert_eq!(rfc3339_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn the_clock_is_after_2026() {
        assert!(unix_now() > 1_788_998_400);
    }
}
