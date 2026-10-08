//! Calendar arithmetic for `before:` / `after:` (proleptic Gregorian,
//! no dependency needed for days ↔ Unix time).

use crate::query::DateValue;

/// Whether `year-month-day` is a real calendar day.
pub(crate) fn is_valid_day(year: i32, month: u8, day: u8) -> bool {
    (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    nail_util::time::days_from_civil(i64::from(year), u32::from(month), u32::from(day))
}

/// When a [`DateValue`] starts, in Unix seconds: local midnight of a day
/// (`utc_offset_secs` east of UTC), or `now` minus an age.
pub(crate) fn resolve(date: DateValue, now: i64, utc_offset_secs: i32) -> i64 {
    match date {
        DateValue::Day { year, month, day } => {
            days_from_civil(year, month, day) * 86_400 - i64::from(utc_offset_secs)
        }
        DateValue::Ago { amount, unit } => now - i64::from(amount) * unit.seconds(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::AgeUnit;

    #[test]
    fn validates_days() {
        assert!(is_valid_day(2026, 9, 30));
        assert!(!is_valid_day(2026, 9, 31));
        assert!(!is_valid_day(2026, 2, 29));
        assert!(is_valid_day(2028, 2, 29));
        assert!(!is_valid_day(2100, 2, 29));
        assert!(is_valid_day(2000, 2, 29));
        assert!(!is_valid_day(2026, 13, 1));
        assert!(!is_valid_day(2026, 0, 1));
        assert!(!is_valid_day(2026, 1, 0));
    }

    #[test]
    fn converts_days_to_unix_time() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 10) * 86_400, 1_788_998_400);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn resolves_dates() {
        let day = DateValue::Day {
            year: 2026,
            month: 9,
            day: 10,
        };
        assert_eq!(resolve(day, 0, 0), 1_788_998_400);
        // Midnight in UTC+2 is 22:00 UTC the day before.
        assert_eq!(resolve(day, 0, 7_200), 1_788_998_400 - 7_200);
        let ago = DateValue::Ago {
            amount: 7,
            unit: AgeUnit::Days,
        };
        assert_eq!(resolve(ago, 1_000_000, 0), 1_000_000 - 7 * 86_400);
    }
}
