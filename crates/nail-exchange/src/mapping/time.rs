//! Times between Exchange (`dateTimeTimeZone`: a wall time and a Windows or
//! IANA zone name) and Nail ([`PimTime`]: dates, floating times, wall
//! times in an IANA zone or UTC). Windows zone names map to IANA through
//! `nail_ical::tz` (CLDR's table); unknown zones are read as UTC with a
//! note.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use nail_api::exchange::pim::ExchangeDateTime;
use nail_api::localized::LocalizedMessage;
use nail_api::pim::{PimTime, UTC_ZONE};

use crate::texts;

/// `YYYY-MM-DDTHH:MM:SS` of a server time (fractions and a trailing `Z`
/// or offset dropped; a bare date gets midnight).
pub fn normalize_local(text: &str) -> String {
    let text = text.trim();
    if text.len() == 10 {
        return format!("{text}T00:00:00");
    }
    let main = text.get(..19).unwrap_or(text);
    main.to_owned()
}

fn is_utc_name(zone: &str) -> bool {
    let zone = zone.trim();
    [
        "UTC",
        "Etc/UTC",
        "GMT",
        "Etc/GMT",
        "Z",
        "tzone://Microsoft/Utc",
        "Coordinated Universal Time",
    ]
    .iter()
    .any(|u| u.eq_ignore_ascii_case(zone))
}

/// The IANA zone of an Exchange zone name.
pub fn zone_of(name: &str) -> Option<Tz> {
    if is_utc_name(name) {
        return Some(Tz::UTC);
    }
    nail_ical::tz::iana_for_name(name)
}

/// A server time as a [`PimTime`]: the date for all-day items, else the
/// wall time in the mapped zone (unknown zones: UTC plus
/// `exchange-pim-reason-zone`).
pub fn pim_time(time: &ExchangeDateTime, all_day: bool) -> (PimTime, Option<LocalizedMessage>) {
    if all_day {
        return (PimTime::date(time.date()), None);
    }
    let local = normalize_local(&time.date_time);
    match zone_of(&time.time_zone) {
        Some(Tz::UTC) => (PimTime::utc(local), None),
        Some(tz) => (PimTime::zoned(local, tz.name()), None),
        None => (
            PimTime::utc(local),
            Some(LocalizedMessage::new(texts::PIM_ZONE).arg("zone", time.time_zone.clone())),
        ),
    }
}

/// The instant of a server time (unknown zones: UTC).
pub fn instant(time: &ExchangeDateTime) -> Option<DateTime<Utc>> {
    let local =
        NaiveDateTime::parse_from_str(&normalize_local(&time.date_time), "%Y-%m-%dT%H:%M:%S")
            .ok()?;
    let tz = zone_of(&time.time_zone).unwrap_or(Tz::UTC);
    Some(nail_ical::time::resolve_local(local, tz))
}

/// RFC 3339 UTC text of a server time.
pub fn utc_text(time: &ExchangeDateTime) -> Option<String> {
    instant(time).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// A server time from an RFC 3339 instant (as UTC).
pub fn exchange_utc(instant: &str) -> Option<ExchangeDateTime> {
    let t = DateTime::parse_from_rfc3339(instant)
        .ok()?
        .with_timezone(&Utc);
    Some(ExchangeDateTime::utc(
        t.format("%Y-%m-%dT%H:%M:%S").to_string(),
    ))
}

/// A Nail time for Exchange. Dates become midnight in `default_zone` (the
/// mailbox's zone; all-day items carry it), floating times are read in
/// `default_zone` (`exchange-pim-reason-floating`).
pub fn exchange_time(
    time: &PimTime,
    default_zone: &str,
) -> (ExchangeDateTime, Option<LocalizedMessage>) {
    match time {
        PimTime::Date { date } => (
            ExchangeDateTime {
                date_time: format!("{date}T00:00:00"),
                time_zone: default_zone.to_owned(),
            },
            None,
        ),
        PimTime::Floating { local } => (
            ExchangeDateTime {
                date_time: normalize_local(local),
                time_zone: default_zone.to_owned(),
            },
            Some(LocalizedMessage::new(texts::PIM_FLOATING)),
        ),
        PimTime::Zoned { local, tz } => (
            ExchangeDateTime {
                date_time: normalize_local(local),
                time_zone: if tz == UTC_ZONE {
                    "UTC".to_owned()
                } else {
                    tz.clone()
                },
            },
            None,
        ),
    }
}

/// `instant` as a wall time in `like`'s kind and zone (dates: the date in
/// that zone, floating: UTC wall time).
pub fn like(instant: DateTime<Utc>, like: &PimTime) -> PimTime {
    match like {
        PimTime::Date { .. } => PimTime::date(instant.format("%Y-%m-%d").to_string()),
        PimTime::Floating { .. } => {
            PimTime::floating(instant.format("%Y-%m-%dT%H:%M:%S").to_string())
        }
        PimTime::Zoned { tz, .. } => {
            let zone = zone_of(tz).unwrap_or(Tz::UTC);
            let local = zone.from_utc_datetime(&instant.naive_utc());
            PimTime::zoned(local.format("%Y-%m-%dT%H:%M:%S").to_string(), tz.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(local: &str, zone: &str) -> ExchangeDateTime {
        ExchangeDateTime {
            date_time: local.into(),
            time_zone: zone.into(),
        }
    }

    #[test]
    fn server_times_to_nail() {
        assert_eq!(
            pim_time(
                &dt("2026-10-05T09:00:00.0000000", "W. Europe Standard Time"),
                false
            )
            .0,
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            pim_time(&dt("2026-10-05T07:00:00", "UTC"), false).0,
            PimTime::utc("2026-10-05T07:00:00")
        );
        assert_eq!(
            pim_time(&dt("2026-10-05T09:00:00", "America/New_York"), false).0,
            PimTime::zoned("2026-10-05T09:00:00", "America/New_York")
        );
        assert_eq!(
            pim_time(&dt("2026-10-05T00:00:00", "Pacific Standard Time"), true).0,
            PimTime::date("2026-10-05")
        );
        let (time, note) = pim_time(&dt("2026-10-05T09:00:00", "Mars Standard Time"), false);
        assert_eq!(time, PimTime::utc("2026-10-05T09:00:00"));
        assert_eq!(note.unwrap().id, texts::PIM_ZONE);
    }

    #[test]
    fn instants() {
        assert_eq!(
            utc_text(&dt("2026-10-05T09:00:00", "W. Europe Standard Time")).as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(
            exchange_utc("2026-10-05T09:00:00+02:00").unwrap(),
            ExchangeDateTime::utc("2026-10-05T07:00:00")
        );
        assert_eq!(normalize_local("2026-10-05"), "2026-10-05T00:00:00");
        assert_eq!(
            normalize_local("2026-10-05T09:00:00Z"),
            "2026-10-05T09:00:00"
        );
        assert!(instant(&dt("garbage", "UTC")).is_none());
    }

    #[test]
    fn nail_times_to_server() {
        assert_eq!(
            exchange_time(
                &PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
                "UTC"
            )
            .0,
            dt("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            exchange_time(&PimTime::utc("2026-10-05T09:00:00"), "Europe/Berlin").0,
            dt("2026-10-05T09:00:00", "UTC")
        );
        assert_eq!(
            exchange_time(&PimTime::date("2026-10-05"), "Europe/Berlin").0,
            dt("2026-10-05T00:00:00", "Europe/Berlin")
        );
        let (time, note) =
            exchange_time(&PimTime::floating("2026-10-05T09:00:00"), "Europe/Berlin");
        assert_eq!(time, dt("2026-10-05T09:00:00", "Europe/Berlin"));
        assert_eq!(note.unwrap().id, texts::PIM_FLOATING);
    }

    #[test]
    fn like_converts_into_the_series_zone() {
        let instant = Utc.with_ymd_and_hms(2026, 10, 12, 7, 0, 0).unwrap();
        assert_eq!(
            like(
                instant,
                &PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
            ),
            PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            like(instant, &PimTime::date("2026-10-05")),
            PimTime::date("2026-10-12")
        );
        assert_eq!(
            like(instant, &PimTime::utc("2026-10-05T07:00:00")),
            PimTime::utc("2026-10-12T07:00:00")
        );
    }
}
