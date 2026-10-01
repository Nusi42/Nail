//! EWS recurrences (`t:Recurrence`: a `*Recurrence` pattern and a range)
//! ↔ [`ExchangeRecurrence`].
//!
//! EWS yearly patterns have no interval (always 1); `DaysOfWeek` may say
//! `Day`, `Weekday` or `WeekendDay`, which become the matching weekdays;
//! task regeneration patterns (`DailyRegeneration`, …) have no
//! provider-neutral shape and are read as `None`.

use nail_api::exchange::pim::{
    ExchangeDayOfWeek, ExchangeRecurrence, ExchangeRecurrencePattern, ExchangeRecurrenceRange,
    PatternKind, WeekIndex,
};

use crate::error::EwsError;
use crate::xml::{Element, el};

const DAYS: [(ExchangeDayOfWeek, &str); 7] = [
    (ExchangeDayOfWeek::Sunday, "Sunday"),
    (ExchangeDayOfWeek::Monday, "Monday"),
    (ExchangeDayOfWeek::Tuesday, "Tuesday"),
    (ExchangeDayOfWeek::Wednesday, "Wednesday"),
    (ExchangeDayOfWeek::Thursday, "Thursday"),
    (ExchangeDayOfWeek::Friday, "Friday"),
    (ExchangeDayOfWeek::Saturday, "Saturday"),
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The EWS name of a weekday.
pub fn day_name(day: ExchangeDayOfWeek) -> &'static str {
    DAYS.iter()
        .find(|(d, _)| *d == day)
        .map_or("Monday", |(_, n)| n)
}

/// The weekdays an EWS `DaysOfWeek` value names (a space-separated list).
pub fn parse_days(text: &str) -> Vec<ExchangeDayOfWeek> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let days: Vec<ExchangeDayOfWeek> = match word {
            "Day" => DAYS.iter().map(|(d, _)| *d).collect(),
            "Weekday" => DAYS[1..6].iter().map(|(d, _)| *d).collect(),
            "WeekendDay" => vec![ExchangeDayOfWeek::Saturday, ExchangeDayOfWeek::Sunday],
            name => DAYS
                .iter()
                .filter(|(_, n)| n.eq_ignore_ascii_case(name))
                .map(|(d, _)| *d)
                .collect(),
        };
        for day in days {
            if !out.contains(&day) {
                out.push(day);
            }
        }
    }
    out
}

fn week_index(text: &str) -> Option<WeekIndex> {
    Some(match text.trim() {
        "First" => WeekIndex::First,
        "Second" => WeekIndex::Second,
        "Third" => WeekIndex::Third,
        "Fourth" => WeekIndex::Fourth,
        "Last" => WeekIndex::Last,
        _ => return None,
    })
}

fn week_index_name(index: WeekIndex) -> &'static str {
    match index {
        WeekIndex::First => "First",
        WeekIndex::Second => "Second",
        WeekIndex::Third => "Third",
        WeekIndex::Fourth => "Fourth",
        WeekIndex::Last => "Last",
    }
}

fn month(text: &str) -> Option<u32> {
    MONTHS
        .iter()
        .position(|m| m.eq_ignore_ascii_case(text.trim()))
        .and_then(|i| u32::try_from(i + 1).ok())
}

fn date(text: &str) -> String {
    text.trim().get(..10).unwrap_or(text.trim()).to_owned()
}

/// Reads a `t:Recurrence` element; `time_zone` is the series' zone.
pub fn parse_recurrence(
    recurrence: &Element,
    time_zone: Option<&str>,
) -> Option<ExchangeRecurrence> {
    let mut pattern = None;
    let mut range = None;
    for part in &recurrence.children {
        let interval = part.child_number("Interval").unwrap_or(1u32).max(1);
        let days = part
            .child_text("DaysOfWeek")
            .map(parse_days)
            .unwrap_or_default();
        let base = |kind| ExchangeRecurrencePattern {
            kind,
            interval,
            days_of_week: Vec::new(),
            day_of_month: None,
            month: None,
            index: None,
            first_day_of_week: None,
        };
        match part.name.as_str() {
            "DailyRecurrence" => pattern = Some(base(PatternKind::Daily)),
            "WeeklyRecurrence" => {
                pattern = Some(ExchangeRecurrencePattern {
                    days_of_week: days,
                    first_day_of_week: part
                        .child_text("FirstDayOfWeek")
                        .and_then(|d| parse_days(d).first().copied()),
                    ..base(PatternKind::Weekly)
                });
            }
            "AbsoluteMonthlyRecurrence" => {
                pattern = Some(ExchangeRecurrencePattern {
                    day_of_month: part.child_number("DayOfMonth"),
                    ..base(PatternKind::AbsoluteMonthly)
                });
            }
            "RelativeMonthlyRecurrence" => {
                pattern = Some(ExchangeRecurrencePattern {
                    days_of_week: days,
                    index: part.child_text("DayOfWeekIndex").and_then(week_index),
                    ..base(PatternKind::RelativeMonthly)
                });
            }
            "AbsoluteYearlyRecurrence" => {
                pattern = Some(ExchangeRecurrencePattern {
                    interval: 1,
                    day_of_month: part.child_number("DayOfMonth"),
                    month: part.child_text("Month").and_then(month),
                    ..base(PatternKind::AbsoluteYearly)
                });
            }
            "RelativeYearlyRecurrence" => {
                pattern = Some(ExchangeRecurrencePattern {
                    interval: 1,
                    days_of_week: days,
                    index: part.child_text("DayOfWeekIndex").and_then(week_index),
                    month: part.child_text("Month").and_then(month),
                    ..base(PatternKind::RelativeYearly)
                });
            }
            "NoEndRecurrence" => {
                range = Some(ExchangeRecurrenceRange::NoEnd {
                    start_date: date(part.child_text("StartDate")?),
                });
            }
            "EndDateRecurrence" => {
                range = Some(ExchangeRecurrenceRange::EndDate {
                    start_date: date(part.child_text("StartDate")?),
                    end_date: date(part.child_text("EndDate")?),
                });
            }
            "NumberedRecurrence" => {
                range = Some(ExchangeRecurrenceRange::Numbered {
                    start_date: date(part.child_text("StartDate")?),
                    occurrences: part.child_number("NumberOfOccurrences")?,
                });
            }
            _ => {}
        }
    }
    Some(ExchangeRecurrence {
        pattern: pattern?,
        range: range?,
        time_zone: time_zone.map(str::to_owned),
    })
}

/// The `t:Recurrence` element of a recurrence.
pub fn recurrence_xml(recurrence: &ExchangeRecurrence) -> Result<String, EwsError> {
    let p = &recurrence.pattern;
    let interval = el("t:Interval", &p.interval.max(1).to_string());
    let days = || {
        let names: Vec<&str> = p.days_of_week.iter().map(|d| day_name(*d)).collect();
        if names.is_empty() {
            Err(EwsError::Unsupported(
                "a recurrence pattern without weekdays".into(),
            ))
        } else {
            Ok(el("t:DaysOfWeek", &names.join(" ")))
        }
    };
    let index = || {
        p.index
            .map(|i| el("t:DayOfWeekIndex", week_index_name(i)))
            .ok_or_else(|| EwsError::Unsupported("a relative pattern without a week".into()))
    };
    let day_of_month = || {
        p.day_of_month
            .filter(|d| (1..=31).contains(d))
            .map(|d| el("t:DayOfMonth", &d.to_string()))
            .ok_or_else(|| EwsError::Unsupported("a pattern without a day of the month".into()))
    };
    let month = || {
        p.month
            .and_then(|m| MONTHS.get(usize::try_from(m).ok()?.checked_sub(1)?))
            .map(|m| el("t:Month", m))
            .ok_or_else(|| EwsError::Unsupported("a yearly pattern without a month".into()))
    };
    let pattern = match p.kind {
        PatternKind::Daily => format!("<t:DailyRecurrence>{interval}</t:DailyRecurrence>"),
        PatternKind::Weekly => {
            let first = p
                .first_day_of_week
                .map(|d| el("t:FirstDayOfWeek", day_name(d)))
                .unwrap_or_default();
            format!(
                "<t:WeeklyRecurrence>{interval}{}{first}</t:WeeklyRecurrence>",
                days()?
            )
        }
        PatternKind::AbsoluteMonthly => format!(
            "<t:AbsoluteMonthlyRecurrence>{interval}{}</t:AbsoluteMonthlyRecurrence>",
            day_of_month()?
        ),
        PatternKind::RelativeMonthly => format!(
            "<t:RelativeMonthlyRecurrence>{interval}{}{}</t:RelativeMonthlyRecurrence>",
            days()?,
            index()?
        ),
        PatternKind::AbsoluteYearly => format!(
            "<t:AbsoluteYearlyRecurrence>{}{}</t:AbsoluteYearlyRecurrence>",
            day_of_month()?,
            month()?
        ),
        PatternKind::RelativeYearly => format!(
            "<t:RelativeYearlyRecurrence>{}{}{}</t:RelativeYearlyRecurrence>",
            days()?,
            index()?,
            month()?
        ),
    };
    let range = match &recurrence.range {
        ExchangeRecurrenceRange::NoEnd { start_date } => format!(
            "<t:NoEndRecurrence>{}</t:NoEndRecurrence>",
            el("t:StartDate", start_date)
        ),
        ExchangeRecurrenceRange::EndDate {
            start_date,
            end_date,
        } => format!(
            "<t:EndDateRecurrence>{}{}</t:EndDateRecurrence>",
            el("t:StartDate", start_date),
            el("t:EndDate", end_date)
        ),
        ExchangeRecurrenceRange::Numbered {
            start_date,
            occurrences,
        } => format!(
            "<t:NumberedRecurrence>{}{}</t:NumberedRecurrence>",
            el("t:StartDate", start_date),
            el("t:NumberOfOccurrences", &occurrences.max(&1).to_string())
        ),
    };
    Ok(format!("<t:Recurrence>{pattern}{range}</t:Recurrence>"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    fn round_trip(recurrence: &ExchangeRecurrence) -> ExchangeRecurrence {
        let xml = recurrence_xml(recurrence).unwrap();
        let wrapped = format!(r#"<x xmlns:t="t">{xml}</x>"#);
        let element = parse(&wrapped).unwrap();
        parse_recurrence(
            element.child("Recurrence").unwrap(),
            recurrence.time_zone.as_deref(),
        )
        .unwrap()
    }

    fn pattern(kind: PatternKind) -> ExchangeRecurrencePattern {
        ExchangeRecurrencePattern {
            kind,
            interval: 1,
            days_of_week: Vec::new(),
            day_of_month: None,
            month: None,
            index: None,
            first_day_of_week: None,
        }
    }

    #[test]
    fn every_pattern_round_trips() {
        let start = "2026-10-05".to_owned();
        let cases = vec![
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    interval: 2,
                    ..pattern(PatternKind::Daily)
                },
                range: ExchangeRecurrenceRange::NoEnd {
                    start_date: start.clone(),
                },
                time_zone: Some("W. Europe Standard Time".into()),
            },
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    days_of_week: vec![ExchangeDayOfWeek::Monday, ExchangeDayOfWeek::Wednesday],
                    first_day_of_week: Some(ExchangeDayOfWeek::Monday),
                    ..pattern(PatternKind::Weekly)
                },
                range: ExchangeRecurrenceRange::Numbered {
                    start_date: start.clone(),
                    occurrences: 10,
                },
                time_zone: None,
            },
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    day_of_month: Some(15),
                    interval: 3,
                    ..pattern(PatternKind::AbsoluteMonthly)
                },
                range: ExchangeRecurrenceRange::EndDate {
                    start_date: start.clone(),
                    end_date: "2027-10-05".into(),
                },
                time_zone: None,
            },
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    days_of_week: vec![ExchangeDayOfWeek::Friday],
                    index: Some(WeekIndex::Last),
                    ..pattern(PatternKind::RelativeMonthly)
                },
                range: ExchangeRecurrenceRange::NoEnd {
                    start_date: start.clone(),
                },
                time_zone: None,
            },
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    day_of_month: Some(24),
                    month: Some(12),
                    ..pattern(PatternKind::AbsoluteYearly)
                },
                range: ExchangeRecurrenceRange::NoEnd {
                    start_date: start.clone(),
                },
                time_zone: None,
            },
            ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    days_of_week: vec![ExchangeDayOfWeek::Thursday],
                    index: Some(WeekIndex::Fourth),
                    month: Some(11),
                    ..pattern(PatternKind::RelativeYearly)
                },
                range: ExchangeRecurrenceRange::NoEnd { start_date: start },
                time_zone: None,
            },
        ];
        for case in cases {
            assert_eq!(round_trip(&case), case);
        }
    }

    #[test]
    fn server_forms() {
        let element = parse(concat!(
            r#"<Recurrence><WeeklyRecurrence><Interval>1</Interval><DaysOfWeek>Weekday</DaysOfWeek>"#,
            r#"</WeeklyRecurrence><NumberedRecurrence><StartDate>2026-10-05+02:00</StartDate>"#,
            r#"<NumberOfOccurrences>5</NumberOfOccurrences></NumberedRecurrence></Recurrence>"#
        ))
        .unwrap();
        let r = parse_recurrence(&element, None).unwrap();
        assert_eq!(r.pattern.days_of_week.len(), 5);
        assert_eq!(
            r.range,
            ExchangeRecurrenceRange::Numbered {
                start_date: "2026-10-05".into(),
                occurrences: 5
            }
        );
        assert_eq!(
            parse_days("WeekendDay Monday"),
            [
                ExchangeDayOfWeek::Saturday,
                ExchangeDayOfWeek::Sunday,
                ExchangeDayOfWeek::Monday
            ]
        );
        let regenerating = parse(
            "<Recurrence><DailyRegeneration><Interval>2</Interval></DailyRegeneration><NoEndRecurrence><StartDate>2026-10-05</StartDate></NoEndRecurrence></Recurrence>",
        )
        .unwrap();
        assert!(parse_recurrence(&regenerating, None).is_none());
        let incomplete = ExchangeRecurrence {
            pattern: pattern(PatternKind::Weekly),
            range: ExchangeRecurrenceRange::NoEnd {
                start_date: "2026-10-05".into(),
            },
            time_zone: None,
        };
        assert!(matches!(
            recurrence_xml(&incomplete),
            Err(EwsError::Unsupported(_))
        ));
    }
}
