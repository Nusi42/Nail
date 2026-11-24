//! Free time within hours of each day (R8, ADR 0251): `FindFreeTime` with
//! [`nail_api::pim::FreeTimeHours`] other than `Any` cuts the free stretches
//! to one day's hours on the chosen days, so neither the UI nor the MCP
//! tool cut them again. The working day comes from the calendar
//! preferences (the one source of truth the views shade, ADR 0250).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use nail_api::pim::WorkHours;
use nail_api::pim::calendar_view::MINUTES_PER_DAY;
use nail_api::triage::Weekday;

/// Longest summer-time gap looked across for the first instant after a
/// skipped local time (minutes).
const MAX_GAP_MINUTES: i64 = 180;

/// Cuts free stretches `[start, end)` (Unix seconds) to `hours` of each of
/// `days` in `tz`, keeping pieces of at least `min_secs`, by start.
pub(crate) fn within_hours(
    slots: &[(i64, i64)],
    tz: Tz,
    hours: WorkHours,
    days: &[Weekday],
    min_secs: i64,
) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for &(start, end) in slots {
        let (Some(first), Some(last)) = (
            DateTime::<Utc>::from_timestamp(start, 0),
            DateTime::<Utc>::from_timestamp(end, 0),
        ) else {
            continue;
        };
        let mut day = first.with_timezone(&tz).date_naive();
        let last = last.with_timezone(&tz).date_naive();
        while day <= last {
            if is_one_of(day, days)
                && let (Some(open), Some(close)) = (
                    local_minute(tz, day, hours.start_minute),
                    local_minute(tz, day, hours.end_minute),
                )
            {
                let (a, b) = (start.max(open), end.min(close));
                if b - a >= min_secs.max(1) {
                    out.push((a, b));
                }
            }
            let Some(next) = day.succ_opt() else { break };
            day = next;
        }
    }
    out
}

/// Whether `day` is one of `days`.
fn is_one_of(day: NaiveDate, days: &[Weekday]) -> bool {
    usize::try_from(day.weekday().num_days_from_monday())
        .ok()
        .and_then(|i| Weekday::ALL.get(i))
        .is_some_and(|d| days.contains(d))
}

/// The instant `minute` minutes after midnight of `day` in `tz` (Unix
/// seconds): [`MINUTES_PER_DAY`] is the next midnight; a time a summer-time
/// change skips is the first instant after the gap, a repeated one its
/// earlier instant.
fn local_minute(tz: Tz, day: NaiveDate, minute: u16) -> Option<i64> {
    let (day, minute) = if minute >= MINUTES_PER_DAY {
        (day.succ_opt()?, 0)
    } else {
        (day, minute)
    };
    let local = day.and_time(NaiveTime::from_hms_opt(
        u32::from(minute / 60),
        u32::from(minute % 60),
        0,
    )?);
    (0..=MAX_GAP_MINUTES)
        .find_map(|m| {
            tz.from_local_datetime(&(local + Duration::minutes(m)))
                .earliest()
        })
        .map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text)
            .map(|t| t.timestamp())
            .unwrap_or_default()
    }

    fn texts(slots: &[(i64, i64)]) -> Vec<(String, String)> {
        let show = |s: i64| {
            DateTime::<Utc>::from_timestamp(s, 0)
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default()
        };
        slots.iter().map(|&(a, b)| (show(a), show(b))).collect()
    }

    const NINE_TO_FIVE: WorkHours = WorkHours {
        start_minute: 9 * 60,
        end_minute: 17 * 60,
    };

    #[test]
    fn stretches_are_cut_per_day_and_short_pieces_dropped() {
        // Monday 2026-10-05 06:00 UTC to Tuesday 20:00 UTC, Berlin (UTC+2).
        let slots = [(at("2026-10-05T06:00:00Z"), at("2026-10-06T20:00:00Z"))];
        let cut = within_hours(
            &slots,
            chrono_tz::Europe::Berlin,
            NINE_TO_FIVE,
            &Weekday::ALL,
            30 * 60,
        );
        assert_eq!(
            texts(&cut),
            [
                ("2026-10-05T07:00:00Z".into(), "2026-10-05T15:00:00Z".into()),
                ("2026-10-06T07:00:00Z".into(), "2026-10-06T15:00:00Z".into())
            ]
        );
        // A piece shorter than the length is left out.
        let short = [(at("2026-10-05T14:45:00Z"), at("2026-10-05T20:00:00Z"))];
        let cut = within_hours(
            &short,
            chrono_tz::Europe::Berlin,
            NINE_TO_FIVE,
            &Weekday::ALL,
            30 * 60,
        );
        assert_eq!(cut, Vec::<(i64, i64)>::new());
    }

    #[test]
    fn only_the_chosen_days_count() {
        // 2026-10-03 is a Saturday, 2026-10-05 a Monday.
        let slots = [(at("2026-10-03T00:00:00Z"), at("2026-10-06T00:00:00Z"))];
        let cut = within_hours(&slots, chrono_tz::UTC, NINE_TO_FIVE, &[Weekday::Mon], 60);
        assert_eq!(
            texts(&cut),
            [("2026-10-05T09:00:00Z".into(), "2026-10-05T17:00:00Z".into())]
        );
        assert_eq!(
            within_hours(&slots, chrono_tz::UTC, NINE_TO_FIVE, &[], 60),
            Vec::<(i64, i64)>::new()
        );
    }

    /// R8: one table over zones east and west of UTC, a half-hour zone,
    /// both hemispheres' summer-time changes (a gap and a repeated hour)
    /// and a 30-minute change: each case's free stretch cut to 09:00-17:00
    /// local time on the chosen days.
    #[test]
    fn working_hours_in_many_zones() {
        use chrono_tz::{America, Asia, Australia, Europe};
        struct Case {
            what: &'static str,
            zone: Tz,
            days: &'static [Weekday],
            free: (&'static str, &'static str),
            want: &'static [(&'static str, &'static str)],
        }
        let cases = [
            Case {
                what: "New York, a winter Monday",
                zone: America::New_York,
                days: &[Weekday::Mon],
                free: ("2026-12-07T00:00:00Z", "2026-12-08T12:00:00Z"),
                want: &[("2026-12-07T14:00:00Z", "2026-12-07T22:00:00Z")],
            },
            Case {
                what: "Tokyo, a day that starts on the UTC day before",
                zone: Asia::Tokyo,
                days: &Weekday::ALL,
                free: ("2026-10-04T15:00:00Z", "2026-10-05T15:00:00Z"),
                want: &[("2026-10-05T00:00:00Z", "2026-10-05T08:00:00Z")],
            },
            Case {
                what: "Kolkata, half an hour off UTC",
                zone: Asia::Kolkata,
                days: &Weekday::ALL,
                free: ("2026-10-05T00:00:00Z", "2026-10-05T23:59:00Z"),
                want: &[("2026-10-05T03:30:00Z", "2026-10-05T11:30:00Z")],
            },
            Case {
                what: "New York, the Sunday clocks go forward (a gap at 02:00)",
                zone: America::New_York,
                days: &[Weekday::Sun, Weekday::Mon],
                free: ("2026-03-08T00:00:00Z", "2026-03-10T00:00:00Z"),
                want: &[
                    ("2026-03-08T13:00:00Z", "2026-03-08T21:00:00Z"),
                    ("2026-03-09T13:00:00Z", "2026-03-09T21:00:00Z"),
                ],
            },
            Case {
                what: "Berlin, the Sunday clocks go back (an hour twice)",
                zone: Europe::Berlin,
                days: &[Weekday::Sat, Weekday::Sun],
                free: ("2026-10-24T00:00:00Z", "2026-10-26T00:00:00Z"),
                want: &[
                    ("2026-10-24T07:00:00Z", "2026-10-24T15:00:00Z"),
                    ("2026-10-25T08:00:00Z", "2026-10-25T16:00:00Z"),
                ],
            },
            Case {
                what: "Sydney, southern summer time starts in October",
                zone: Australia::Sydney,
                days: &[Weekday::Fri, Weekday::Mon],
                free: ("2026-10-01T00:00:00Z", "2026-10-06T00:00:00Z"),
                want: &[
                    ("2026-10-01T23:00:00Z", "2026-10-02T07:00:00Z"),
                    ("2026-10-04T22:00:00Z", "2026-10-05T06:00:00Z"),
                ],
            },
            Case {
                what: "Lord Howe, a summer-time change of 30 minutes",
                zone: Australia::Lord_Howe,
                days: &[Weekday::Sat, Weekday::Mon],
                free: ("2026-10-02T12:00:00Z", "2026-10-05T12:00:00Z"),
                want: &[
                    ("2026-10-02T22:30:00Z", "2026-10-03T06:30:00Z"),
                    ("2026-10-04T22:00:00Z", "2026-10-05T06:00:00Z"),
                ],
            },
        ];
        for case in cases {
            let slots = [(at(case.free.0), at(case.free.1))];
            let cut = within_hours(&slots, case.zone, NINE_TO_FIVE, case.days, 30 * 60);
            let want: Vec<(String, String)> = case
                .want
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect();
            assert_eq!(texts(&cut), want, "{}", case.what);
        }
    }

    #[test]
    fn midnight_end_and_summer_time_days() {
        let whole = WorkHours {
            start_minute: 0,
            end_minute: MINUTES_PER_DAY,
        };
        // Berlin's clocks go back on 2026-10-25: that day has 25 hours.
        let slots = [(at("2026-10-24T22:00:00Z"), at("2026-10-26T00:00:00Z"))];
        let cut = within_hours(
            &slots,
            chrono_tz::Europe::Berlin,
            whole,
            &[Weekday::Sun],
            60,
        );
        assert_eq!(
            texts(&cut),
            [("2026-10-24T22:00:00Z".into(), "2026-10-25T23:00:00Z".into())]
        );
        // On 2026-03-29 02:00 does not exist in Berlin: a day starting at
        // 02:30 opens at 03:00 local (01:00 UTC).
        let early = WorkHours {
            start_minute: 2 * 60 + 30,
            end_minute: 4 * 60,
        };
        let slots = [(at("2026-03-28T23:00:00Z"), at("2026-03-29T05:00:00Z"))];
        let cut = within_hours(
            &slots,
            chrono_tz::Europe::Berlin,
            early,
            &[Weekday::Sun],
            60,
        );
        assert_eq!(
            texts(&cut),
            [("2026-03-29T01:00:00Z".into(), "2026-03-29T02:00:00Z".into())]
        );
    }
}
