//! Calendar arithmetic (proleptic Gregorian) for date conditions.

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
pub fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    nail_util::time::days_from_civil(i64::from(year), u32::from(month), u32::from(day))
}

/// Parses `YYYY-MM-DD` into days since 1970-01-01; `None` unless it is a
/// real calendar day.
pub fn parse_day(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let digits = |p: &str| p.bytes().all(|c| c.is_ascii_digit());
    if !(digits(y) && digits(m) && digits(d)) {
        return None;
    }
    let (year, month, day): (i32, u8, u8) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=max)
        .contains(&day)
        .then(|| days_from_civil(year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 10) * 86_400, 1_788_998_400);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }

    #[test]
    fn parsing_days() {
        assert_eq!(parse_day("2026-09-10"), Some(days_from_civil(2026, 9, 10)));
        assert_eq!(parse_day("2028-02-29"), Some(days_from_civil(2028, 2, 29)));
        for bad in ["2026-02-29", "2026-9-10", "2026-09-10-1", "abcd-09-10", ""] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
    }
}
