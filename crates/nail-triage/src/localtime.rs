//! Local wall-clock time: resolving a local date and time to an instant
//! across daylight-saving changes, and weekday conversion.
//!
//! Everything is generic over [`chrono::TimeZone`]; the core passes
//! `chrono::Local` (the OS time zone), tests pass fixed or rule-based
//! zones. Rules for the two DST edge cases:
//!
//! * a local time that does not exist (spring forward, e.g. 02:30 in
//!   Berlin on the last Sunday of March) moves forward by the length of
//!   the gap (→ 03:30);
//! * a local time that exists twice (fall back, 02:30 on the last Sunday
//!   of October) means the **earlier** instant.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone};
use nail_api::triage::Weekday;

/// The instant of a local date and time (DST gaps move forward, repeated
/// times take the earlier instant).
pub fn resolve_local<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime) -> DateTime<Tz> {
    if let Some(earliest) = tz.from_local_datetime(&local).earliest() {
        return earliest;
    }
    // In a gap: the offset before the gap applied to the local time gives
    // the instant the wall clock would have shown it without the jump.
    let before = tz.offset_from_utc_datetime(&(local - Duration::days(1)));
    let utc = local - Duration::seconds(i64::from(before.fix().local_minus_utc()));
    tz.from_utc_datetime(&utc)
}

/// The instant of a local time on a date.
pub fn at_local<Tz: TimeZone>(tz: &Tz, date: NaiveDate, time: NaiveTime) -> DateTime<Tz> {
    resolve_local(tz, date.and_time(time))
}

/// The instant `days` calendar days after `from` (in its zone) at the local
/// `time` — "tomorrow at 08:00" is `days = 1` whatever the length of the
/// day in between.
pub fn days_later_at<Tz: TimeZone>(
    from: &DateTime<Tz>,
    days: i64,
    time: NaiveTime,
) -> DateTime<Tz> {
    let tz = from.timezone();
    let date = from.date_naive() + Duration::days(days);
    at_local(&tz, date, time)
}

/// The contract's weekday of a chrono weekday.
pub fn weekday(day: chrono::Weekday) -> Weekday {
    match day {
        chrono::Weekday::Mon => Weekday::Mon,
        chrono::Weekday::Tue => Weekday::Tue,
        chrono::Weekday::Wed => Weekday::Wed,
        chrono::Weekday::Thu => Weekday::Thu,
        chrono::Weekday::Fri => Weekday::Fri,
        chrono::Weekday::Sat => Weekday::Sat,
        chrono::Weekday::Sun => Weekday::Sun,
    }
}

/// The weekday of a local date.
pub fn weekday_of(date: NaiveDate) -> Weekday {
    weekday(date.weekday())
}

/// A rule-based time zone for tests: standard offset plus one hour of DST
/// between two transition instants per year (EU or US rules).
#[cfg(test)]
pub(crate) mod testzone {
    use std::fmt;

    use chrono::{
        Datelike, Duration, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, NaiveTime,
        Offset, TimeZone, Weekday,
    };

    /// DST rules.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Rule {
        /// EU: last Sunday of March 01:00 UTC → last Sunday of October
        /// 01:00 UTC.
        Eu,
        /// US: second Sunday of March 02:00 local → first Sunday of
        /// November 02:00 local (daylight).
        Us,
    }

    /// A zone.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Zone {
        /// Standard offset in hours.
        pub std_hours: i32,
        /// Its rules.
        pub rule: Rule,
    }

    /// Europe/Berlin.
    pub(crate) const BERLIN: Zone = Zone {
        std_hours: 1,
        rule: Rule::Eu,
    };
    /// `America/New_York`.
    pub(crate) const NEW_YORK: Zone = Zone {
        std_hours: -5,
        rule: Rule::Us,
    };

    /// An offset of a [`Zone`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct ZoneOffset {
        zone: Zone,
        dst: bool,
    }

    impl Offset for ZoneOffset {
        fn fix(&self) -> FixedOffset {
            let hours = self.zone.std_hours + i32::from(self.dst);
            FixedOffset::east_opt(hours * 3600).unwrap()
        }
    }

    impl fmt::Display for ZoneOffset {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.fix())
        }
    }

    fn nth_sunday(year: i32, month: u32, n: u32) -> NaiveDate {
        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let offset = (7 - first.weekday().num_days_from_sunday()) % 7;
        first + Duration::days(i64::from(offset + 7 * (n - 1)))
    }

    fn last_sunday(year: i32, month: u32) -> NaiveDate {
        let next = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)
        }
        .unwrap();
        let mut d = next - Duration::days(1);
        while d.weekday() != Weekday::Sun {
            d -= Duration::days(1);
        }
        d
    }

    impl Zone {
        /// DST start and end as UTC instants.
        fn window(self, year: i32) -> (NaiveDateTime, NaiveDateTime) {
            let t = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
            match self.rule {
                Rule::Eu => (
                    last_sunday(year, 3).and_time(t(1)),
                    last_sunday(year, 10).and_time(t(1)),
                ),
                Rule::Us => {
                    // 02:00 local standard = 02:00 - std; 02:00 local
                    // daylight = 02:00 - (std + 1).
                    let start = nth_sunday(year, 3, 2).and_time(t(2))
                        - Duration::hours(i64::from(self.std_hours));
                    let end = nth_sunday(year, 11, 1).and_time(t(2))
                        - Duration::hours(i64::from(self.std_hours + 1));
                    (start, end)
                }
            }
        }

        fn is_dst_utc(self, utc: NaiveDateTime) -> bool {
            let (start, end) = self.window(utc.year());
            utc >= start && utc < end
        }
    }

    impl TimeZone for Zone {
        type Offset = ZoneOffset;

        fn from_offset(offset: &ZoneOffset) -> Self {
            offset.zone
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<ZoneOffset> {
            self.offset_from_local_datetime(&local.and_time(NaiveTime::MIN))
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<ZoneOffset> {
            let mut found: Vec<ZoneOffset> = Vec::new();
            // Daylight first: the earlier instant of a repeated time.
            for dst in [true, false] {
                let off = ZoneOffset { zone: *self, dst };
                let utc = *local - Duration::seconds(i64::from(off.fix().local_minus_utc()));
                if self.is_dst_utc(utc) == dst {
                    found.push(off);
                }
            }
            match found.as_slice() {
                [] => MappedLocalTime::None,
                [one] => MappedLocalTime::Single(*one),
                [a, b, ..] => MappedLocalTime::Ambiguous(*a, *b),
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> ZoneOffset {
            self.offset_from_utc_datetime(&utc.and_time(NaiveTime::MIN))
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> ZoneOffset {
            ZoneOffset {
                zone: *self,
                dst: self.is_dst_utc(*utc),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testzone::{BERLIN, NEW_YORK};
    use super::*;
    use chrono::{FixedOffset, Timelike, Utc};
    use pretty_assertions::assert_eq;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn test_zone_transitions() {
        // 2026: EU DST from Mar 29 01:00 UTC to Oct 25 01:00 UTC.
        let before = BERLIN.from_utc_datetime(&dt(2026, 3, 29, 0, 59));
        let after = BERLIN.from_utc_datetime(&dt(2026, 3, 29, 1, 0));
        assert_eq!((before.hour(), before.minute()), (1, 59));
        assert_eq!((after.hour(), after.minute()), (3, 0));
        // US: Mar 8 2026 02:00 EST → 03:00 EDT; Nov 1 02:00 EDT → 01:00 EST.
        let ny = NEW_YORK.from_utc_datetime(&dt(2026, 3, 8, 7, 0));
        assert_eq!(ny.hour(), 3);
        let ny = NEW_YORK.from_utc_datetime(&dt(2026, 11, 1, 6, 0));
        assert_eq!(ny.hour(), 1);
    }

    #[test]
    fn gaps_move_forward() {
        let at = resolve_local(&BERLIN, dt(2026, 3, 29, 2, 30));
        assert_eq!(at.naive_utc(), dt(2026, 3, 29, 1, 30));
        assert_eq!((at.hour(), at.minute()), (3, 30));
        let at = resolve_local(&NEW_YORK, dt(2026, 3, 8, 2, 15));
        assert_eq!((at.hour(), at.minute()), (3, 15));
    }

    #[test]
    fn repeated_times_take_the_earlier_instant() {
        let at = resolve_local(&BERLIN, dt(2026, 10, 25, 2, 30));
        // 02:30 CEST = 00:30 UTC (the later 02:30 CET would be 01:30 UTC).
        assert_eq!(at.naive_utc(), dt(2026, 10, 25, 0, 30));
        let at = resolve_local(&NEW_YORK, dt(2026, 11, 1, 1, 30));
        assert_eq!(at.naive_utc(), dt(2026, 11, 1, 5, 30));
    }

    #[test]
    fn days_later_keeps_the_wall_clock_across_dst() {
        // Saturday before the spring change, 21:00 CET → "tomorrow 08:00"
        // is 08:00 CEST, only 10 hours later (not 11).
        let from = resolve_local(&BERLIN, dt(2026, 3, 28, 21, 0));
        let wake = days_later_at(&from, 1, t(8, 0));
        assert_eq!(wake.naive_local(), dt(2026, 3, 29, 8, 0));
        assert_eq!((wake - from).num_hours(), 10);
        // Autumn: 12 hours.
        let from = resolve_local(&BERLIN, dt(2026, 10, 24, 21, 0));
        let wake = days_later_at(&from, 1, t(8, 0));
        assert_eq!((wake - from).num_hours(), 12);
        // A wake time inside the gap moves forward.
        let from = resolve_local(&BERLIN, dt(2026, 3, 28, 12, 0));
        let wake = days_later_at(&from, 1, t(2, 30));
        assert_eq!(wake.naive_local(), dt(2026, 3, 29, 3, 30));
        // Fixed zones and UTC work the same.
        let fixed = FixedOffset::east_opt(9 * 3600).unwrap();
        let from = fixed.from_utc_datetime(&dt(2026, 1, 1, 20, 0));
        assert_eq!(
            days_later_at(&from, 1, t(8, 0)).naive_local(),
            dt(2026, 1, 3, 8, 0)
        );
        assert_eq!(
            at_local(&Utc, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(), t(0, 0)).naive_utc(),
            dt(2026, 2, 28, 0, 0)
        );
    }

    #[test]
    fn weekdays() {
        assert_eq!(
            weekday_of(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()),
            Weekday::Wed
        );
        let all: Vec<Weekday> = [
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
            chrono::Weekday::Sat,
            chrono::Weekday::Sun,
        ]
        .into_iter()
        .map(weekday)
        .collect();
        assert_eq!(all, Weekday::ALL);
    }
}
