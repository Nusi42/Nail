//! Calendar times as the PIM tables store them.
//!
//! Range columns and the instance cache hold **cache seconds**: Unix
//! seconds of the instant for times in a zone, and the wall clock read as
//! if it were UTC for all-day dates and floating times (`floating` = 1).
//! A floating value becomes an instant only when it is read in a viewer's
//! zone ([`place_wall`]); range queries widen by [`FLOATING_SLACK`] for
//! them because no zone is more than 14 hours away from UTC.
//!
//! Wall times that do not exist (spring forward) move forward by the gap;
//! wall times that exist twice (fall back) mean the earlier instant — the
//! same rules as `nail-triage`'s local times and RFC 5545 section 3.3.5.

use std::str::FromStr;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use nail_api::pim::{PimTime, UTC_ZONE};

/// How far a floating or all-day time can be from its UTC wall reading
/// once it is placed in a zone (UTC−12 … UTC+14).
pub const FLOATING_SLACK: i64 = 14 * 3600;

/// A calendar time in cache seconds (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheTime {
    /// Unix seconds (UTC), or the wall clock read as UTC when `floating`.
    pub secs: i64,
    /// A date or floating time (or a zone Nail does not know).
    pub floating: bool,
}

/// The IANA zone (or `UTC`) of a name; `None` when unknown.
pub fn parse_zone(name: &str) -> Option<Tz> {
    if name.eq_ignore_ascii_case(UTC_ZONE) || name == "Etc/UTC" || name == "Z" {
        return Some(chrono_tz::UTC);
    }
    Tz::from_str(name).ok()
}

/// Parses `YYYY-MM-DDTHH:MM:SS` (a leap second 60 reads as 59).
pub fn parse_local(text: &str) -> Option<NaiveDateTime> {
    let fixed;
    let text = if text.len() == 19 && text.ends_with(":60") {
        fixed = format!("{}59", &text[..17]);
        fixed.as_str()
    } else {
        text
    };
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").ok()
}

/// Parses `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// The instant of a wall-clock time in `tz` (gaps move forward, repeated
/// times take the earlier instant).
pub fn resolve_wall<Z: TimeZone>(tz: &Z, local: NaiveDateTime) -> DateTime<Z> {
    if let Some(earliest) = tz.from_local_datetime(&local).earliest() {
        return earliest;
    }
    let before = tz.offset_from_utc_datetime(&(local - Duration::days(1)));
    let utc = local - Duration::seconds(i64::from(before.fix().local_minus_utc()));
    tz.from_utc_datetime(&utc)
}

/// A calendar time in cache seconds; `None` when it is malformed.
pub fn cache_time(time: &PimTime) -> Option<CacheTime> {
    match time {
        PimTime::Date { date } => Some(CacheTime {
            secs: parse_date(date)?
                .and_hms_opt(0, 0, 0)?
                .and_utc()
                .timestamp(),
            floating: true,
        }),
        PimTime::Floating { local } => Some(CacheTime {
            secs: parse_local(local)?.and_utc().timestamp(),
            floating: true,
        }),
        PimTime::Zoned { local, tz } => {
            let local = parse_local(local)?;
            Some(match parse_zone(tz) {
                Some(zone) => CacheTime {
                    secs: resolve_wall(&zone, local).timestamp(),
                    floating: false,
                },
                // An unknown zone reads as floating rather than failing.
                None => CacheTime {
                    secs: local.and_utc().timestamp(),
                    floating: true,
                },
            })
        }
    }
}

/// The instant of a cache time in the viewer's zone.
pub fn place(time: CacheTime, viewer: &Tz) -> i64 {
    if time.floating {
        place_wall(time.secs, viewer)
    } else {
        time.secs
    }
}

/// The instant of a wall-clock reading (cache seconds of a floating time)
/// in `viewer`.
pub fn place_wall(secs: i64, viewer: &Tz) -> i64 {
    match DateTime::from_timestamp(secs, 0) {
        Some(wall) => resolve_wall(viewer, wall.naive_utc()).timestamp(),
        None => secs,
    }
}

/// A calendar time of the same kind and zone as `like` at cache seconds
/// `secs` (an occurrence of a series in its own zone).
pub fn time_like(like: &PimTime, secs: i64) -> PimTime {
    let wall = |s: i64| {
        DateTime::from_timestamp(s, 0)
            .unwrap_or_default()
            .naive_utc()
    };
    match like {
        PimTime::Date { .. } => PimTime::date(wall(secs).date().format("%Y-%m-%d").to_string()),
        PimTime::Floating { .. } => PimTime::floating(format_local(wall(secs))),
        PimTime::Zoned { tz, .. } => match parse_zone(tz) {
            Some(zone) => {
                let local = DateTime::from_timestamp(secs, 0)
                    .unwrap_or_default()
                    .with_timezone(&zone)
                    .naive_local();
                PimTime::zoned(format_local(local), tz.clone())
            }
            None => PimTime::zoned(format_local(wall(secs)), tz.clone()),
        },
    }
}

/// `YYYY-MM-DDTHH:MM:SS`.
pub fn format_local(local: NaiveDateTime) -> String {
    local.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// RFC 3339 in UTC as the contract writes instants (`…Z`).
pub fn utc_string(secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(secs, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Unix seconds of an RFC 3339 instant (any offset); `None` when malformed.
pub fn parse_instant(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(text: &str) -> i64 {
        parse_instant(text).unwrap()
    }

    #[test]
    fn dates_and_floating_times_are_wall_clock() {
        let date = cache_time(&PimTime::date("2026-10-01")).unwrap();
        assert_eq!(
            date,
            CacheTime {
                secs: secs("2026-10-01T00:00:00Z"),
                floating: true
            }
        );
        let floating = cache_time(&PimTime::floating("2026-10-01T09:30:00")).unwrap();
        assert_eq!(floating.secs, secs("2026-10-01T09:30:00Z"));
        assert!(floating.floating);
        // Placed in a viewer's zone they become instants.
        let berlin = parse_zone("Europe/Berlin").unwrap();
        assert_eq!(place(floating, &berlin), secs("2026-10-01T07:30:00Z"));
        let tokyo = parse_zone("Asia/Tokyo").unwrap();
        assert_eq!(place(date, &tokyo), secs("2026-09-30T15:00:00Z"));
        assert!((place(date, &tokyo) - date.secs).abs() <= FLOATING_SLACK);
    }

    #[test]
    fn zoned_times_are_instants_across_dst() {
        let summer = cache_time(&PimTime::zoned("2026-03-28T09:00:00", "Europe/Berlin")).unwrap();
        let after = cache_time(&PimTime::zoned("2026-03-30T09:00:00", "Europe/Berlin")).unwrap();
        assert_eq!(summer.secs, secs("2026-03-28T08:00:00Z"));
        assert_eq!(after.secs, secs("2026-03-30T07:00:00Z"));
        assert!(!summer.floating);
        // 02:30 does not exist on 2026-03-29 in Berlin: moves to 03:30.
        let gap = cache_time(&PimTime::zoned("2026-03-29T02:30:00", "Europe/Berlin")).unwrap();
        assert_eq!(gap.secs, secs("2026-03-29T01:30:00Z"));
        // 02:30 exists twice on 2026-10-25: the earlier one.
        let twice = cache_time(&PimTime::zoned("2026-10-25T02:30:00", "Europe/Berlin")).unwrap();
        assert_eq!(twice.secs, secs("2026-10-25T00:30:00Z"));
        let utc = cache_time(&PimTime::utc("2026-10-01T09:00:00")).unwrap();
        assert_eq!(utc.secs, secs("2026-10-01T09:00:00Z"));
        // Unknown zones read as floating.
        let unknown = cache_time(&PimTime::zoned("2026-10-01T09:00:00", "Mars/Olympus")).unwrap();
        assert!(unknown.floating);
        assert_eq!(cache_time(&PimTime::date("2026-02-30")), None);
        // A leap second reads as :59.
        assert!(cache_time(&PimTime::floating("2026-12-31T23:59:60")).is_some());
    }

    #[test]
    fn occurrences_keep_kind_and_zone() {
        let start = PimTime::zoned("2026-03-28T09:00:00", "Europe/Berlin");
        let next = secs("2026-03-30T07:00:00Z");
        assert_eq!(
            time_like(&start, next),
            PimTime::zoned("2026-03-30T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            time_like(&PimTime::date("2026-01-01"), secs("2026-01-05T00:00:00Z")),
            PimTime::date("2026-01-05")
        );
        assert_eq!(
            time_like(
                &PimTime::floating("2026-01-01T08:00:00"),
                secs("2026-01-02T08:00:00Z")
            ),
            PimTime::floating("2026-01-02T08:00:00")
        );
        assert_eq!(utc_string(next), "2026-03-30T07:00:00Z");
        assert_eq!(parse_instant("2026-03-30T09:00:00+02:00"), Some(next));
        assert_eq!(parse_instant("yesterday"), None);
    }
}
