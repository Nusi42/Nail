//! Recurrence rules (RFC 5545 section 3.3.10 and 3.8.5): `RRULE` text ↔
//! [`RecurrenceRule`], and the expansion of a rule — and of a whole
//! recurrence set (rules, `RDATE`, `EXDATE`) — into wall-clock
//! occurrences.
//!
//! Nail's own engine (ADR 0059): occurrences are generated in the
//! **wall-clock frame of the series start** (its zone, or floating / all
//! day), which is what RFC 5545 means by recurring at "the same local
//! time" across DST changes; callers turn them into instants with
//! [`crate::time::resolve_local`] (gaps shift forward, folds take the
//! first). Every `BY*` part is supported with RFC 5545's expand / limit
//! table, including `BYSETPOS`, `BYWEEKNO` with `WKST`, negative
//! ordinals, `COUNT` (the start always counts as the first occurrence)
//! and `UNTIL` (inclusive, compared as an instant when the start has a
//! zone). Invalid dates (`BYMONTHDAY=31` in April, February 29 in common
//! years) are skipped, as RFC 5545 requires.

use std::collections::{HashSet, VecDeque};

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use nail_api::pim::calendar::{Frequency, RecurrenceEnd, RecurrenceRule, WeekdayNum};
use nail_api::pim::{PimTime, Recurrence};
use nail_api::triage::Weekday;

use crate::time::{self, IcalValue};

/// Most empty periods in a row before a rule is considered exhausted
/// (rules that can never match, e.g. `BYMONTH=2;BYMONTHDAY=30`).
const MAX_IDLE_PERIODS: u32 = 200_000;

/// Most occurrences a `MINUTELY` or `SECONDLY` rule yields (counting from
/// the series start), whatever its `COUNT` / `UNTIL` say: no calendar
/// needs more, and hostile feeds use such rules to make every expansion,
/// reminder plan and edit scan millions of occurrences (ADR 0064).
pub const MAX_SUB_HOURLY_OCCURRENCES: u32 = 2_000;

/// chrono's weekday for the contract's.
pub fn chrono_weekday(day: Weekday) -> chrono::Weekday {
    match day {
        Weekday::Mon => chrono::Weekday::Mon,
        Weekday::Tue => chrono::Weekday::Tue,
        Weekday::Wed => chrono::Weekday::Wed,
        Weekday::Thu => chrono::Weekday::Thu,
        Weekday::Fri => chrono::Weekday::Fri,
        Weekday::Sat => chrono::Weekday::Sat,
        Weekday::Sun => chrono::Weekday::Sun,
    }
}

/// The contract's weekday for chrono's.
pub fn api_weekday(day: chrono::Weekday) -> Weekday {
    Weekday::from_monday(day.num_days_from_monday())
}

fn weekday_code(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn parse_weekday_code(code: &str) -> Option<Weekday> {
    Some(match code.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// RRULE text

fn parse_list<T: std::str::FromStr>(value: &str) -> Option<Vec<T>> {
    value
        .split(',')
        .map(|v| v.trim().trim_start_matches('+').parse().ok())
        .collect()
}

/// Parses an `UNTIL` value in the context of the series start: a date, a
/// UTC time, or a local time (read in the start's zone, a common
/// producer mistake).
pub fn parse_until(value: &str, start: &PimTime) -> Option<PimTime> {
    Some(match time::parse_ical_value(value)? {
        IcalValue::Date(date) => PimTime::date(time::format_date(date)),
        IcalValue::DateTime { local, utc: true } => PimTime::utc(time::format_local(local)),
        IcalValue::DateTime { local, utc: false } => match start {
            PimTime::Zoned { tz, .. } => PimTime::zoned(time::format_local(local), tz.clone()),
            _ => PimTime::floating(time::format_local(local)),
        },
    })
}

/// Parses an `RRULE` value (`FREQ=WEEKLY;BYDAY=MO,WE;COUNT=10`). `start`
/// is the series start (for local `UNTIL` values). Unknown parts are
/// ignored; `None` without a valid `FREQ` or with malformed parts.
pub fn parse_rrule(text: &str, start: &PimTime) -> Option<RecurrenceRule> {
    let mut freq = None;
    let mut rule = RecurrenceRule::new(Frequency::Daily);
    for part in text.trim().split(';').filter(|p| !p.trim().is_empty()) {
        let (name, value) = part.split_once('=')?;
        let value = value.trim();
        match name.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match value.to_ascii_uppercase().as_str() {
                    "SECONDLY" => Frequency::Secondly,
                    "MINUTELY" => Frequency::Minutely,
                    "HOURLY" => Frequency::Hourly,
                    "DAILY" => Frequency::Daily,
                    "WEEKLY" => Frequency::Weekly,
                    "MONTHLY" => Frequency::Monthly,
                    "YEARLY" => Frequency::Yearly,
                    _ => return None,
                });
            }
            "INTERVAL" => rule.interval = value.parse().ok().filter(|i| *i >= 1)?,
            "COUNT" => {
                rule.end = RecurrenceEnd::Count {
                    count: value.parse().ok()?,
                }
            }
            "UNTIL" => {
                rule.end = RecurrenceEnd::Until {
                    until: parse_until(value, start)?,
                }
            }
            "BYSECOND" => rule.by_second = parse_list(value)?,
            "BYMINUTE" => rule.by_minute = parse_list(value)?,
            "BYHOUR" => rule.by_hour = parse_list(value)?,
            "BYMONTHDAY" => rule.by_month_day = parse_list(value)?,
            "BYYEARDAY" => rule.by_year_day = parse_list(value)?,
            "BYWEEKNO" => rule.by_week_no = parse_list(value)?,
            "BYMONTH" => rule.by_month = parse_list(value)?,
            "BYSETPOS" => rule.by_set_pos = parse_list(value)?,
            "WKST" => rule.week_start = Some(parse_weekday_code(value)?),
            "BYDAY" => {
                rule.by_day = value
                    .split(',')
                    .map(|token| {
                        let token = token.trim();
                        let split = token.len().checked_sub(2)?;
                        let day = parse_weekday_code(token.get(split..)?)?;
                        let number = token[..split].trim_start_matches('+');
                        let ordinal = if number.is_empty() {
                            None
                        } else {
                            Some(number.parse::<i8>().ok()?)
                        };
                        Some(WeekdayNum { day, ordinal })
                    })
                    .collect::<Option<_>>()?;
            }
            _ => {} // RSCALE, SKIP (RFC 7529) and X- parts are ignored.
        }
    }
    rule.freq = freq?;
    Some(rule)
}

fn join<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Formats an `UNTIL` value: dates as dates, UTC as `…Z`, other zones
/// converted to UTC (RFC 5545 requires UTC with a zoned start), floating
/// as local.
pub fn format_until(until: &PimTime) -> String {
    match until {
        PimTime::Date { date } => time::parse_date(date)
            .map(time::ical_date)
            .unwrap_or_default(),
        PimTime::Floating { local } => time::parse_local(local)
            .map(time::ical_local)
            .unwrap_or_default(),
        PimTime::Zoned { .. } => time::instant(until, Tz::UTC)
            .map(time::ical_utc)
            .unwrap_or_default(),
    }
}

/// Writes a rule as `RRULE` text, parts in the customary order.
pub fn format_rrule(rule: &RecurrenceRule) -> String {
    let freq = match rule.freq {
        Frequency::Secondly => "SECONDLY",
        Frequency::Minutely => "MINUTELY",
        Frequency::Hourly => "HOURLY",
        Frequency::Daily => "DAILY",
        Frequency::Weekly => "WEEKLY",
        Frequency::Monthly => "MONTHLY",
        Frequency::Yearly => "YEARLY",
    };
    let mut parts = vec![format!("FREQ={freq}")];
    match &rule.end {
        RecurrenceEnd::Never => {}
        RecurrenceEnd::Count { count } => parts.push(format!("COUNT={count}")),
        RecurrenceEnd::Until { until } => parts.push(format!("UNTIL={}", format_until(until))),
    }
    if rule.interval > 1 {
        parts.push(format!("INTERVAL={}", rule.interval));
    }
    let lists: [(&str, String); 9] = [
        ("BYMONTH", join(&rule.by_month)),
        ("BYWEEKNO", join(&rule.by_week_no)),
        ("BYYEARDAY", join(&rule.by_year_day)),
        ("BYMONTHDAY", join(&rule.by_month_day)),
        (
            "BYDAY",
            rule.by_day
                .iter()
                .map(|d| match d.ordinal {
                    Some(n) => format!("{n}{}", weekday_code(d.day)),
                    None => weekday_code(d.day).to_owned(),
                })
                .collect::<Vec<_>>()
                .join(","),
        ),
        ("BYHOUR", join(&rule.by_hour)),
        ("BYMINUTE", join(&rule.by_minute)),
        ("BYSECOND", join(&rule.by_second)),
        ("BYSETPOS", join(&rule.by_set_pos)),
    ];
    for (name, value) in lists {
        if !value.is_empty() {
            parts.push(format!("{name}={value}"));
        }
    }
    if let Some(day) = rule.week_start {
        parts.push(format!("WKST={}", weekday_code(day)));
    }
    parts.join(";")
}

// ---------------------------------------------------------------------------
// UNTIL

/// How `UNTIL` is compared with candidates in the start's frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntilBound {
    /// Wall-clock comparison (floating start).
    Wall(NaiveDateTime),
    /// Date comparison (all-day start, or a date `UNTIL`).
    Date(NaiveDate),
    /// Instant comparison: candidates are wall-clock times in the zone.
    Instant(DateTime<Utc>, Tz),
}

impl UntilBound {
    /// Builds the bound for `until` against a series starting at `start`;
    /// floating starts compare floating / UTC bounds by wall clock.
    pub fn new(until: &PimTime, start: &PimTime) -> Option<Self> {
        let until_wall = time::wall(until)?;
        Some(match (start, until) {
            (PimTime::Date { .. }, _) | (_, PimTime::Date { .. }) => Self::Date(until_wall.date()),
            (PimTime::Zoned { tz, .. }, PimTime::Floating { .. }) => {
                let zone = time::zone(tz)?;
                Self::Instant(time::resolve_local(until_wall, zone), zone)
            }
            (PimTime::Zoned { tz, .. }, PimTime::Zoned { .. }) => {
                let zone = time::zone(tz)?;
                Self::Instant(time::instant(until, zone)?, zone)
            }
            (PimTime::Floating { .. }, _) => Self::Wall(until_wall),
        })
    }

    /// Whether `candidate` lies after the bound.
    pub fn passed(&self, candidate: NaiveDateTime) -> bool {
        match *self {
            Self::Wall(wall) => candidate > wall,
            Self::Date(date) => candidate.date() > date,
            Self::Instant(instant, zone) => time::resolve_local(candidate, zone) > instant,
        }
    }
}

// ---------------------------------------------------------------------------
// One rule

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrdinalScope {
    Month,
    Year,
    Ignore,
}

/// Iterates the occurrences of one rule, in ascending wall-clock order,
/// starting with the series start itself (RFC 5545: the start always
/// counts as the first occurrence).
#[derive(Debug, Clone)]
pub struct RuleIter {
    freq: Frequency,
    interval: i64,
    start: NaiveDateTime,
    all_day: bool,
    count: Option<u32>,
    until: Option<UntilBound>,
    by_second: Vec<u32>,
    by_minute: Vec<u32>,
    by_hour: Vec<u32>,
    by_day: Vec<WeekdayNum>,
    by_month_day: Vec<i32>,
    by_year_day: Vec<i32>,
    by_week_no: Vec<i32>,
    by_month: Vec<u32>,
    by_set_pos: Vec<i32>,
    week_start: chrono::Weekday,
    ordinal_scope: OrdinalScope,
    period: i64,
    buffer: VecDeque<NaiveDateTime>,
    last: Option<NaiveDateTime>,
    emitted: u32,
    idle: u32,
    finished: bool,
    /// Every period yields exactly one occurrence (no `BY*` parts,
    /// `DAILY` or finer, or `WEEKLY`): skipping periods keeps `COUNT`
    /// exact.
    uniform: bool,
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    match (next, NaiveDate::from_ymd_opt(year, month, 1)) {
        (Some(next), Some(first)) => u32::try_from((next - first).num_days()).unwrap_or(30),
        _ => 30,
    }
}

fn days_in_year(year: i32) -> i32 {
    // At most 366: always fits.
    i32::try_from(nail_util::time::days_in_year(i64::from(year))).unwrap_or(366)
}

/// First day of week 1 of `year` (the first week with at least four days
/// in the year, weeks starting on `week_start`).
fn week_one_start(year: i32, week_start: chrono::Weekday) -> Option<NaiveDate> {
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1)?;
    let offset = (7 + i64::from(jan1.weekday().num_days_from_monday())
        - i64::from(week_start.num_days_from_monday()))
        % 7;
    let week_of_jan1 = jan1 - Duration::days(offset);
    Some(if 7 - offset >= 4 {
        week_of_jan1
    } else {
        week_of_jan1 + Duration::days(7)
    })
}

/// Week number (1-based) of `day` and the number of weeks in its week
/// year.
fn week_number(day: NaiveDate, week_start: chrono::Weekday) -> Option<(i32, i32)> {
    let year = day.year();
    let this = week_one_start(year, week_start)?;
    let next = week_one_start(year + 1, week_start)?;
    let (begin, end) = if day < this {
        (week_one_start(year - 1, week_start)?, this)
    } else if day >= next {
        (next, week_one_start(year + 2, week_start)?)
    } else {
        (this, next)
    };
    let number = (day - begin).num_days() / 7 + 1;
    let weeks = (end - begin).num_days() / 7;
    Some((i32::try_from(number).ok()?, i32::try_from(weeks).ok()?))
}

impl RuleIter {
    /// The occurrences of `rule` for a series starting at `start` (wall
    /// clock in the series' frame). `all_day` ignores time parts.
    pub fn new(
        rule: &RecurrenceRule,
        start: NaiveDateTime,
        all_day: bool,
        until: Option<UntilBound>,
    ) -> Self {
        let mut freq = rule.freq;
        if all_day
            && matches!(
                freq,
                Frequency::Hourly | Frequency::Minutely | Frequency::Secondly
            )
        {
            freq = Frequency::Daily;
        }
        let mut by_day = rule.by_day.clone();
        let mut by_month: Vec<u32> = rule.by_month.iter().map(|&m| u32::from(m)).collect();
        let mut by_month_day: Vec<i32> = rule.by_month_day.iter().map(|&d| i32::from(d)).collect();
        let by_year_day: Vec<i32> = rule.by_year_day.iter().map(|&d| i32::from(d)).collect();
        let by_week_no: Vec<i32> = rule.by_week_no.iter().map(|&w| i32::from(w)).collect();
        let ordinal_scope = match freq {
            Frequency::Monthly => OrdinalScope::Month,
            Frequency::Yearly if !by_week_no.is_empty() => OrdinalScope::Ignore,
            Frequency::Yearly if !by_month.is_empty() => OrdinalScope::Month,
            Frequency::Yearly => OrdinalScope::Year,
            _ => OrdinalScope::Ignore,
        };
        // Defaults from the start (RFC 5545: missing parts come from
        // DTSTART).
        if by_week_no.is_empty()
            && by_year_day.is_empty()
            && by_month_day.is_empty()
            && by_day.is_empty()
        {
            match freq {
                Frequency::Yearly => {
                    if by_month.is_empty() {
                        by_month.push(start.month());
                    }
                    by_month_day.push(i32::try_from(start.day()).unwrap_or(1));
                }
                Frequency::Monthly => by_month_day.push(i32::try_from(start.day()).unwrap_or(1)),
                Frequency::Weekly => by_day.push(WeekdayNum {
                    day: api_weekday(start.weekday()),
                    ordinal: None,
                }),
                _ => {}
            }
        }
        let sorted = |values: &[u8]| {
            let mut v: Vec<u32> = values.iter().map(|&x| u32::from(x)).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let by_hour = sorted(&rule.by_hour);
        let by_minute = sorted(&rule.by_minute);
        // Second 60 (leap seconds) cannot be represented; ignored.
        let by_second: Vec<u32> = sorted(&rule.by_second)
            .into_iter()
            .filter(|&s| s < 60)
            .collect();
        let dropped_all_seconds = !rule.by_second.is_empty() && by_second.is_empty();
        let mut count = match rule.end {
            RecurrenceEnd::Count { count } => Some(count),
            _ if dropped_all_seconds => Some(1),
            _ => None,
        };
        if matches!(freq, Frequency::Minutely | Frequency::Secondly) {
            count = Some(count.map_or(MAX_SUB_HOURLY_OCCURRENCES, |c| {
                c.min(MAX_SUB_HOURLY_OCCURRENCES)
            }));
        }
        let uniform = !matches!(freq, Frequency::Monthly | Frequency::Yearly)
            && rule.by_second.is_empty()
            && rule.by_minute.is_empty()
            && rule.by_hour.is_empty()
            && rule.by_day.is_empty()
            && rule.by_month_day.is_empty()
            && rule.by_year_day.is_empty()
            && rule.by_week_no.is_empty()
            && rule.by_month.is_empty()
            && rule.by_set_pos.is_empty();
        Self {
            freq,
            interval: i64::from(rule.interval.max(1)),
            start,
            all_day,
            count,
            until,
            by_second,
            by_minute,
            by_hour,
            by_day,
            by_month_day,
            by_year_day,
            by_week_no,
            by_month,
            by_set_pos: rule.by_set_pos.iter().map(|&p| i32::from(p)).collect(),
            week_start: chrono_weekday(rule.week_start.unwrap_or(Weekday::Mon)),
            ordinal_scope,
            period: 0,
            buffer: VecDeque::new(),
            last: None,
            emitted: 0,
            idle: 0,
            finished: false,
            uniform,
        }
    }

    /// The start of the first sub-daily period (the start truncated to the
    /// frequency's unit) and the unit in seconds.
    fn sub_daily_base(&self) -> (NaiveDateTime, i64) {
        let unit = match self.freq {
            Frequency::Hourly => 3600,
            Frequency::Minutely => 60,
            _ => 1,
        };
        let base = self.start.date().and_time(NaiveTime::MIN)
            + Duration::seconds(i64::from(self.start.num_seconds_from_midnight()) / unit * unit);
        (base, unit)
    }

    /// The first day (per `WKST`) of the week the series starts in.
    fn first_week(&self) -> Option<NaiveDate> {
        let start_day = self.start.date();
        let back = (7 + i64::from(start_day.weekday().num_days_from_monday())
            - i64::from(self.week_start.num_days_from_monday()))
            % 7;
        start_day.checked_sub_signed(Duration::days(back))
    }

    /// The index of the period that contains `target` (negative for
    /// targets before the start).
    fn period_of(&self, target: NaiveDateTime) -> Option<i64> {
        let units = match self.freq {
            Frequency::Yearly => i64::from(target.year()) - i64::from(self.start.year()),
            Frequency::Monthly => {
                (i64::from(target.year()) * 12 + i64::from(target.month0()))
                    - (i64::from(self.start.year()) * 12 + i64::from(self.start.month0()))
            }
            Frequency::Weekly => (target.date() - self.first_week()?)
                .num_days()
                .div_euclid(7),
            Frequency::Daily => (target.date() - self.start.date()).num_days(),
            Frequency::Hourly | Frequency::Minutely | Frequency::Secondly => {
                let (base, unit) = self.sub_daily_base();
                (target - base).num_seconds().div_euclid(unit)
            }
        };
        Some(units.div_euclid(self.interval.max(1)))
    }

    /// How many occurrences were yielded or skipped so far (exact while a
    /// `COUNT` applies).
    pub fn emitted(&self) -> u32 {
        self.emitted
    }

    /// Jumps ahead so that iteration continues shortly before `target`
    /// instead of walking every period from the start: whole periods that
    /// end before `target` are skipped (one more period is kept as a
    /// margin, so the next occurrence may still lie a little before
    /// `target`). Occurrences after `target` are unaffected. Does nothing
    /// for rules with a `COUNT` whose periods vary in size (the count
    /// needs every occurrence before). Returns whether it skipped.
    pub fn skip_before(&mut self, target: NaiveDateTime) -> bool {
        if self.finished || (self.count.is_some() && !self.uniform) {
            return false;
        }
        let Some(period) = self.period_of(target) else {
            return false;
        };
        let k = period - 1;
        if k <= self.period || k <= 0 {
            return false;
        }
        if self.count.is_some() {
            // One occurrence per period; the start is the one of period 0.
            if let Ok(emitted) = u32::try_from(k) {
                self.emitted = emitted;
            } else {
                self.finished = true;
                return true;
            }
        } else if self.last.is_none() {
            self.emitted = 1;
        }
        // The start lies before the target: it is skipped too.
        if self.last.is_none() {
            self.last = Some(self.start);
        }
        self.period = k;
        self.buffer.clear();
        self.idle = 0;
        true
    }

    fn day_matches(&self, day: NaiveDate) -> bool {
        if !self.by_month.is_empty() && !self.by_month.contains(&day.month()) {
            return false;
        }
        if !self.by_week_no.is_empty() {
            let Some((week, weeks)) = week_number(day, self.week_start) else {
                return false;
            };
            if !self
                .by_week_no
                .iter()
                .any(|&w| w == week || w == week - weeks - 1)
            {
                return false;
            }
        }
        if !self.by_year_day.is_empty() {
            let yd = i32::try_from(day.ordinal()).unwrap_or(0);
            let len = days_in_year(day.year());
            if !self
                .by_year_day
                .iter()
                .any(|&d| d == yd || d == yd - len - 1)
            {
                return false;
            }
        }
        if !self.by_month_day.is_empty() {
            let md = i32::try_from(day.day()).unwrap_or(0);
            let len = i32::try_from(days_in_month(day.year(), day.month())).unwrap_or(30);
            if !self
                .by_month_day
                .iter()
                .any(|&d| d == md || d == md - len - 1)
            {
                return false;
            }
        }
        if !self.by_day.is_empty() && !self.by_day.iter().any(|wd| self.weekday_matches(*wd, day)) {
            return false;
        }
        true
    }

    fn weekday_matches(&self, wd: WeekdayNum, day: NaiveDate) -> bool {
        if chrono_weekday(wd.day) != day.weekday() {
            return false;
        }
        let Some(n) = wd.ordinal else {
            return true;
        };
        let n = i32::from(n);
        let (index, len) = match self.ordinal_scope {
            OrdinalScope::Ignore => return true,
            OrdinalScope::Month => (
                i32::try_from(day.day0()).unwrap_or(0),
                i32::try_from(days_in_month(day.year(), day.month())).unwrap_or(30),
            ),
            OrdinalScope::Year => (
                i32::try_from(day.ordinal0()).unwrap_or(0),
                days_in_year(day.year()),
            ),
        };
        let from_start = index / 7 + 1;
        let from_end = -((len - 1 - index) / 7 + 1);
        n == from_start || n == from_end
    }

    fn times(&self, values: &[u32], from_start: u32) -> Vec<u32> {
        if self.all_day {
            vec![0]
        } else if values.is_empty() {
            vec![from_start]
        } else {
            values.to_vec()
        }
    }

    fn expand_day(&self, day: NaiveDate, out: &mut Vec<NaiveDateTime>) {
        let hours = self.times(&self.by_hour, self.start.hour());
        let minutes = self.times(&self.by_minute, self.start.minute());
        let seconds = self.times(&self.by_second, self.start.second());
        for &h in &hours {
            for &m in &minutes {
                for &s in &seconds {
                    if let Some(t) = NaiveTime::from_hms_opt(h, m, s) {
                        out.push(day.and_time(t));
                    }
                }
            }
        }
    }

    fn apply_set_pos(&self, mut candidates: Vec<NaiveDateTime>) -> Vec<NaiveDateTime> {
        candidates.sort_unstable();
        candidates.dedup();
        if self.by_set_pos.is_empty() {
            return candidates;
        }
        let len = i32::try_from(candidates.len()).unwrap_or(i32::MAX);
        let mut picked: Vec<NaiveDateTime> = self
            .by_set_pos
            .iter()
            .filter_map(|&p| {
                let index = if p > 0 { p - 1 } else { len + p };
                usize::try_from(index)
                    .ok()
                    .and_then(|i| candidates.get(i).copied())
            })
            .collect();
        picked.sort_unstable();
        picked.dedup();
        picked
    }

    /// Candidates of the current period; `None` past the supported range.
    fn next_period(&mut self) -> Option<Vec<NaiveDateTime>> {
        let k = self.period;
        let step = k.checked_mul(self.interval)?;
        let mut candidates = Vec::new();
        match self.freq {
            Frequency::Yearly => {
                let year = i32::try_from(i64::from(self.start.year()).checked_add(step)?).ok()?;
                if year > 9999 {
                    return None;
                }
                let mut day = NaiveDate::from_ymd_opt(year, 1, 1)?;
                while day.year() == year {
                    if self.day_matches(day) {
                        self.expand_day(day, &mut candidates);
                    }
                    day = day.succ_opt()?;
                }
                self.period += 1;
            }
            Frequency::Monthly => {
                let index = i64::from(self.start.year()) * 12 + i64::from(self.start.month0());
                let index = index.checked_add(step)?;
                let year = i32::try_from(index.div_euclid(12)).ok()?;
                let month = u32::try_from(index.rem_euclid(12)).ok()? + 1;
                if year > 9999 {
                    return None;
                }
                let mut day = NaiveDate::from_ymd_opt(year, month, 1)?;
                while day.month() == month {
                    if self.day_matches(day) {
                        self.expand_day(day, &mut candidates);
                    }
                    day = day.succ_opt()?;
                }
                self.period += 1;
            }
            Frequency::Weekly => {
                let week = self
                    .first_week()?
                    .checked_add_signed(Duration::days(step.checked_mul(7)?))?;
                if week.year() > 9999 {
                    return None;
                }
                for offset in 0..7 {
                    let day = week.checked_add_signed(Duration::days(offset))?;
                    if self.day_matches(day) {
                        self.expand_day(day, &mut candidates);
                    }
                }
                self.period += 1;
            }
            Frequency::Daily => {
                let day = self.start.date().checked_add_signed(Duration::days(step))?;
                if day.year() > 9999 {
                    return None;
                }
                if self.day_matches(day) {
                    self.expand_day(day, &mut candidates);
                }
                self.period += 1;
            }
            Frequency::Hourly | Frequency::Minutely | Frequency::Secondly => {
                let (base, unit) = self.sub_daily_base();
                let at = base.checked_add_signed(Duration::seconds(step.checked_mul(unit)?))?;
                if at.year() > 9999 {
                    return None;
                }
                if !self.day_matches(at.date()) {
                    // Jump to the first period of the next day.
                    let midnight = at.date().succ_opt()?.and_time(NaiveTime::MIN);
                    let per = self.interval * unit;
                    let needed = (midnight - base).num_seconds();
                    self.period = (needed + per - 1) / per;
                    return Some(Vec::new());
                }
                self.period += 1;
                let hour_ok = self.by_hour.is_empty() || self.by_hour.contains(&at.hour());
                if !hour_ok {
                    return Some(Vec::new());
                }
                let minute_fixed = matches!(self.freq, Frequency::Minutely | Frequency::Secondly);
                if minute_fixed
                    && !(self.by_minute.is_empty() || self.by_minute.contains(&at.minute()))
                {
                    return Some(Vec::new());
                }
                if self.freq == Frequency::Secondly
                    && !(self.by_second.is_empty() || self.by_second.contains(&at.second()))
                {
                    return Some(Vec::new());
                }
                let minutes = if minute_fixed {
                    vec![at.minute()]
                } else {
                    self.times(&self.by_minute, self.start.minute())
                };
                let seconds = if self.freq == Frequency::Secondly {
                    vec![at.second()]
                } else {
                    self.times(&self.by_second, self.start.second())
                };
                for &m in &minutes {
                    for &s in &seconds {
                        if let Some(t) = NaiveTime::from_hms_opt(at.hour(), m, s) {
                            candidates.push(at.date().and_time(t));
                        }
                    }
                }
            }
        }
        Some(self.apply_set_pos(candidates))
    }
}

impl Iterator for RuleIter {
    type Item = NaiveDateTime;

    fn next(&mut self) -> Option<NaiveDateTime> {
        if self.finished {
            return None;
        }
        if self.last.is_none() {
            // The start always counts as the first occurrence.
            if self.count == Some(0) {
                self.finished = true;
                return None;
            }
            self.last = Some(self.start);
            self.emitted = 1;
            return Some(self.start);
        }
        loop {
            if self.count.is_some_and(|c| self.emitted >= c) {
                self.finished = true;
                return None;
            }
            if let Some(candidate) = self.buffer.pop_front() {
                if self.last.is_some_and(|last| candidate <= last) {
                    continue;
                }
                if self.until.is_some_and(|u| u.passed(candidate)) {
                    self.finished = true;
                    return None;
                }
                self.last = Some(candidate);
                self.emitted += 1;
                return Some(candidate);
            }
            let Some(candidates) = self.next_period() else {
                self.finished = true;
                return None;
            };
            if candidates.is_empty() {
                self.idle += 1;
                if self.idle > MAX_IDLE_PERIODS {
                    self.finished = true;
                    return None;
                }
            } else {
                self.idle = 0;
                self.buffer.extend(candidates);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Recurrence sets

/// Iterates a recurrence set (start, rules, `RDATE`s, minus `EXDATE`s) in
/// the wall-clock frame of the start, ascending, without duplicates.
#[derive(Debug, Clone)]
pub struct Occurrences {
    start: NaiveDateTime,
    rules: Vec<RuleIter>,
    /// The next occurrence of each rule (`None`: exhausted).
    heads: Vec<Option<NaiveDateTime>>,
    dates: Vec<NaiveDateTime>,
    next_date: usize,
    exceptions: HashSet<NaiveDateTime>,
    exception_days: HashSet<NaiveDate>,
    started: bool,
    last: Option<NaiveDateTime>,
}

impl Occurrences {
    /// The occurrences of a series starting at `start`; `None` when the
    /// start is malformed. Without a recurrence it yields the start once.
    pub fn new(start: &PimTime, recurrence: Option<&Recurrence>) -> Option<Self> {
        let first = time::wall(start)?;
        let all_day = start.is_date();
        let mut rules = Vec::new();
        let mut dates = Vec::new();
        let mut exceptions = HashSet::new();
        let mut exception_days = HashSet::new();
        if let Some(recurrence) = recurrence {
            for rule in &recurrence.rules {
                let until = match &rule.end {
                    RecurrenceEnd::Until { until } => UntilBound::new(until, start),
                    _ => None,
                };
                rules.push(RuleIter::new(rule, first, all_day, until));
            }
            dates = recurrence
                .dates
                .iter()
                .filter_map(|d| time::to_frame(d, start))
                .collect();
            dates.sort_unstable();
            dates.dedup();
            for exception in &recurrence.exceptions {
                if exception.is_date() && !all_day {
                    if let Some(day) = time::wall(exception) {
                        exception_days.insert(day.date());
                    }
                } else if let Some(frame) = time::to_frame(exception, start) {
                    exceptions.insert(frame);
                }
            }
        }
        let heads = rules.iter_mut().map(Iterator::next).collect();
        Some(Self {
            start: first,
            rules,
            heads,
            dates,
            next_date: 0,
            exceptions,
            exception_days,
            started: false,
            last: None,
        })
    }

    /// Jumps ahead to shortly before `target` (see
    /// [`RuleIter::skip_before`]): the following occurrences are the same
    /// as without the skip from `target` on, but earlier ones may be left
    /// out. Cheap for any distance.
    pub fn skip_before(&mut self, target: NaiveDateTime) {
        if target <= self.start {
            return;
        }
        // The start lies before the target: skipped as well.
        self.started = true;
        for (rule, head) in self.rules.iter_mut().zip(self.heads.iter_mut()) {
            if head.is_some_and(|h| h < target) && rule.skip_before(target) {
                *head = rule.next();
            }
        }
        while self.dates.get(self.next_date).is_some_and(|&d| d < target) {
            self.next_date += 1;
        }
    }

    /// Whether an occurrence (in the start's frame) was excluded.
    pub fn is_excluded(&self, occurrence: NaiveDateTime) -> bool {
        self.exceptions.contains(&occurrence) || self.exception_days.contains(&occurrence.date())
    }

    fn next_raw(&mut self) -> Option<NaiveDateTime> {
        if !self.started {
            self.started = true;
            // Rules yield the start first themselves; without rules the
            // start is its own first occurrence.
            if self.rules.is_empty() {
                return Some(self.start);
            }
        }
        let rule_min = self.heads.iter().flatten().min().copied();
        let date = self.dates.get(self.next_date).copied();
        let next = match (rule_min, date) {
            (Some(r), Some(d)) => r.min(d),
            (Some(r), None) => r,
            (None, Some(d)) => d,
            (None, None) => return None,
        };
        for (rule, head) in self.rules.iter_mut().zip(self.heads.iter_mut()) {
            while head.is_some_and(|r| r <= next) {
                *head = rule.next();
            }
        }
        while self.dates.get(self.next_date).is_some_and(|&d| d <= next) {
            self.next_date += 1;
        }
        Some(next)
    }
}

impl Iterator for Occurrences {
    type Item = NaiveDateTime;

    fn next(&mut self) -> Option<NaiveDateTime> {
        loop {
            let next = self.next_raw()?;
            if self.last.is_some_and(|last| next <= last) {
                continue;
            }
            self.last = Some(next);
            if self.is_excluded(next) {
                continue;
            }
            return Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        time::parse_local(text).unwrap()
    }

    fn rule(text: &str) -> RecurrenceRule {
        parse_rrule(text, &PimTime::floating("2000-01-01T00:00:00")).unwrap()
    }

    fn expand(text: &str, start: &str, limit: usize) -> Vec<String> {
        let start_time = PimTime::floating(start);
        let r = parse_rrule(text, &start_time).unwrap();
        let until = match &r.end {
            RecurrenceEnd::Until { until } => UntilBound::new(until, &start_time),
            _ => None,
        };
        RuleIter::new(&r, at(start), false, until)
            .take(limit)
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .collect()
    }

    #[test]
    fn rrule_text_round_trips() {
        for text in [
            "FREQ=DAILY;COUNT=10",
            "FREQ=WEEKLY;UNTIL=20261224T000000Z;INTERVAL=2;BYDAY=MO,WE,FR;WKST=SU",
            "FREQ=MONTHLY;BYDAY=-1FR",
            "FREQ=YEARLY;BYMONTH=3;BYDAY=2SU;BYHOUR=2",
            "FREQ=MONTHLY;BYMONTHDAY=-3;BYSETPOS=1,-1",
            "FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO",
            "FREQ=YEARLY;BYYEARDAY=1,100,-1",
            "FREQ=MINUTELY;INTERVAL=15;BYHOUR=9,10;BYSECOND=0",
        ] {
            let parsed = rule(text);
            assert!(parsed.is_valid(), "{text}");
            assert_eq!(format_rrule(&parsed), text);
        }
        let lower = rule("freq=weekly;byday=+2mo;count=3");
        assert_eq!(lower.by_day[0].ordinal, Some(2));
        assert_eq!(format_rrule(&lower), "FREQ=WEEKLY;COUNT=3;BYDAY=2MO");
        for bad in [
            "",
            "COUNT=3",
            "FREQ=FORTNIGHTLY",
            "FREQ=DAILY;INTERVAL=0",
            "FREQ=DAILY;BYDAY=XX",
            "FREQ=DAILY;COUNT",
        ] {
            assert!(
                parse_rrule(bad, &PimTime::date("2026-01-01")).is_none(),
                "{bad}"
            );
        }
        // Unknown parts (RFC 7529) are ignored.
        assert!(
            parse_rrule(
                "FREQ=YEARLY;RSCALE=GREGORIAN;SKIP=OMIT",
                &PimTime::date("2026-01-01")
            )
            .is_some()
        );
        // A local UNTIL takes the start's zone.
        let zoned = parse_rrule(
            "FREQ=DAILY;UNTIL=20261001T090000",
            &PimTime::zoned("2026-09-01T09:00:00", "Europe/Berlin"),
        )
        .unwrap();
        assert_eq!(
            zoned.end,
            RecurrenceEnd::Until {
                until: PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin")
            }
        );
    }

    // Examples from RFC 5545 section 3.8.5.3 (start times as in the RFC,
    // floating; zones do not change wall-clock results).

    #[test]
    fn rfc_daily_examples() {
        assert_eq!(
            expand("FREQ=DAILY;COUNT=10", "1997-09-02T09:00:00", 20).len(),
            10
        );
        let until = expand(
            "FREQ=DAILY;UNTIL=19971224T000000",
            "1997-09-02T09:00:00",
            500,
        );
        assert_eq!(until.len(), 113);
        assert_eq!(until.last().unwrap(), "1997-12-23 09:00");
        assert_eq!(
            expand("FREQ=DAILY;INTERVAL=10;COUNT=5", "1997-09-02T09:00:00", 10),
            [
                "1997-09-02 09:00",
                "1997-09-12 09:00",
                "1997-09-22 09:00",
                "1997-10-02 09:00",
                "1997-10-12 09:00"
            ]
        );
        // Every day in January, for 3 years.
        let january = expand(
            "FREQ=YEARLY;UNTIL=20000131T140000Z;BYMONTH=1;BYDAY=SU,MO,TU,WE,TH,FR,SA",
            "1998-01-01T09:00:00",
            200,
        );
        assert_eq!(january.len(), 93);
        assert_eq!(
            january,
            expand(
                "FREQ=DAILY;UNTIL=20000131T140000Z;BYMONTH=1",
                "1998-01-01T09:00:00",
                200
            )
        );
    }

    #[test]
    fn rfc_weekly_examples() {
        assert_eq!(
            expand("FREQ=WEEKLY;INTERVAL=2;WKST=SU", "1997-09-02T09:00:00", 4),
            [
                "1997-09-02 09:00",
                "1997-09-16 09:00",
                "1997-09-30 09:00",
                "1997-10-14 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=WEEKLY;UNTIL=19971007T000000Z;WKST=SU;BYDAY=TU,TH",
                "1997-09-02T09:00:00",
                50
            ),
            [
                "1997-09-02 09:00",
                "1997-09-04 09:00",
                "1997-09-09 09:00",
                "1997-09-11 09:00",
                "1997-09-16 09:00",
                "1997-09-18 09:00",
                "1997-09-23 09:00",
                "1997-09-25 09:00",
                "1997-09-30 09:00",
                "1997-10-02 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=WEEKLY;INTERVAL=2;COUNT=8;WKST=SU;BYDAY=TU,TH",
                "1997-09-02T09:00:00",
                50
            ),
            [
                "1997-09-02 09:00",
                "1997-09-04 09:00",
                "1997-09-16 09:00",
                "1997-09-18 09:00",
                "1997-09-30 09:00",
                "1997-10-02 09:00",
                "1997-10-14 09:00",
                "1997-10-16 09:00"
            ]
        );
        // WKST changes the result.
        assert_eq!(
            expand(
                "FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=MO",
                "1997-08-05T09:00:00",
                10
            ),
            [
                "1997-08-05 09:00",
                "1997-08-10 09:00",
                "1997-08-19 09:00",
                "1997-08-24 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=SU",
                "1997-08-05T09:00:00",
                10
            ),
            [
                "1997-08-05 09:00",
                "1997-08-17 09:00",
                "1997-08-19 09:00",
                "1997-08-31 09:00"
            ]
        );
    }

    #[test]
    fn rfc_monthly_examples() {
        assert_eq!(
            expand("FREQ=MONTHLY;COUNT=10;BYDAY=1FR", "1997-09-05T09:00:00", 20),
            [
                "1997-09-05 09:00",
                "1997-10-03 09:00",
                "1997-11-07 09:00",
                "1997-12-05 09:00",
                "1998-01-02 09:00",
                "1998-02-06 09:00",
                "1998-03-06 09:00",
                "1998-04-03 09:00",
                "1998-05-01 09:00",
                "1998-06-05 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=MONTHLY;INTERVAL=2;COUNT=10;BYDAY=1SU,-1SU",
                "1997-09-07T09:00:00",
                20
            ),
            [
                "1997-09-07 09:00",
                "1997-09-28 09:00",
                "1997-11-02 09:00",
                "1997-11-30 09:00",
                "1998-01-04 09:00",
                "1998-01-25 09:00",
                "1998-03-01 09:00",
                "1998-03-29 09:00",
                "1998-05-03 09:00",
                "1998-05-31 09:00"
            ]
        );
        assert_eq!(
            expand("FREQ=MONTHLY;COUNT=6;BYDAY=-2MO", "1997-09-22T09:00:00", 20),
            [
                "1997-09-22 09:00",
                "1997-10-20 09:00",
                "1997-11-17 09:00",
                "1997-12-22 09:00",
                "1998-01-19 09:00",
                "1998-02-16 09:00"
            ]
        );
        assert_eq!(
            expand("FREQ=MONTHLY;BYMONTHDAY=-3", "1997-09-28T09:00:00", 6),
            [
                "1997-09-28 09:00",
                "1997-10-29 09:00",
                "1997-11-28 09:00",
                "1997-12-29 09:00",
                "1998-01-29 09:00",
                "1998-02-26 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=MONTHLY;COUNT=10;BYMONTHDAY=1,-1",
                "1997-09-30T09:00:00",
                20
            ),
            [
                "1997-09-30 09:00",
                "1997-10-01 09:00",
                "1997-10-31 09:00",
                "1997-11-01 09:00",
                "1997-11-30 09:00",
                "1997-12-01 09:00",
                "1997-12-31 09:00",
                "1998-01-01 09:00",
                "1998-01-31 09:00",
                "1998-02-01 09:00"
            ]
        );
        // Friday the 13th.
        assert_eq!(
            expand(
                "FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13",
                "1997-09-02T09:00:00",
                6
            ),
            [
                "1997-09-02 09:00",
                "1998-02-13 09:00",
                "1998-03-13 09:00",
                "1998-11-13 09:00",
                "1999-08-13 09:00",
                "2000-10-13 09:00"
            ]
        );
        // The first Saturday that follows the first Sunday of the month.
        assert_eq!(
            expand(
                "FREQ=MONTHLY;BYDAY=SA;BYMONTHDAY=7,8,9,10,11,12,13",
                "1997-09-13T09:00:00",
                5
            ),
            [
                "1997-09-13 09:00",
                "1997-10-11 09:00",
                "1997-11-08 09:00",
                "1997-12-13 09:00",
                "1998-01-10 09:00"
            ]
        );
        // BYSETPOS: the third instance of Tue, Wed or Thu, for 3 months.
        assert_eq!(
            expand(
                "FREQ=MONTHLY;COUNT=3;BYDAY=TU,WE,TH;BYSETPOS=3",
                "1997-09-04T09:00:00",
                5
            ),
            ["1997-09-04 09:00", "1997-10-07 09:00", "1997-11-06 09:00"]
        );
        // The second-to-last weekday of the month.
        assert_eq!(
            expand(
                "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-2",
                "1997-09-29T09:00:00",
                7
            ),
            [
                "1997-09-29 09:00",
                "1997-10-30 09:00",
                "1997-11-27 09:00",
                "1997-12-30 09:00",
                "1998-01-29 09:00",
                "1998-02-26 09:00",
                "1998-03-30 09:00"
            ]
        );
        // Invalid dates are skipped.
        assert_eq!(
            expand(
                "FREQ=MONTHLY;BYMONTHDAY=15,30;COUNT=5",
                "2007-01-15T09:00:00",
                10
            ),
            [
                "2007-01-15 09:00",
                "2007-01-30 09:00",
                "2007-02-15 09:00",
                "2007-03-15 09:00",
                "2007-03-30 09:00"
            ]
        );
        assert_eq!(
            expand("FREQ=MONTHLY;COUNT=4", "2026-01-31T10:00:00", 10),
            [
                "2026-01-31 10:00",
                "2026-03-31 10:00",
                "2026-05-31 10:00",
                "2026-07-31 10:00"
            ]
        );
    }

    #[test]
    fn rfc_yearly_examples() {
        assert_eq!(
            expand(
                "FREQ=YEARLY;COUNT=10;BYMONTH=6,7",
                "1997-06-10T09:00:00",
                20
            ),
            [
                "1997-06-10 09:00",
                "1997-07-10 09:00",
                "1998-06-10 09:00",
                "1998-07-10 09:00",
                "1999-06-10 09:00",
                "1999-07-10 09:00",
                "2000-06-10 09:00",
                "2000-07-10 09:00",
                "2001-06-10 09:00",
                "2001-07-10 09:00"
            ]
        );
        assert_eq!(
            expand(
                "FREQ=YEARLY;INTERVAL=3;COUNT=10;BYYEARDAY=1,100,200",
                "1997-01-01T09:00:00",
                20
            ),
            [
                "1997-01-01 09:00",
                "1997-04-10 09:00",
                "1997-07-19 09:00",
                "2000-01-01 09:00",
                "2000-04-09 09:00",
                "2000-07-18 09:00",
                "2003-01-01 09:00",
                "2003-04-10 09:00",
                "2003-07-19 09:00",
                "2006-01-01 09:00"
            ]
        );
        assert_eq!(
            expand("FREQ=YEARLY;BYDAY=20MO", "1997-05-19T09:00:00", 3),
            ["1997-05-19 09:00", "1998-05-18 09:00", "1999-05-17 09:00"]
        );
        assert_eq!(
            expand("FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO", "1997-05-12T09:00:00", 3),
            ["1997-05-12 09:00", "1998-05-11 09:00", "1999-05-17 09:00"]
        );
        assert_eq!(
            expand("FREQ=YEARLY;BYMONTH=3;BYDAY=TH", "1997-03-13T09:00:00", 11),
            [
                "1997-03-13 09:00",
                "1997-03-20 09:00",
                "1997-03-27 09:00",
                "1998-03-05 09:00",
                "1998-03-12 09:00",
                "1998-03-19 09:00",
                "1998-03-26 09:00",
                "1999-03-04 09:00",
                "1999-03-11 09:00",
                "1999-03-18 09:00",
                "1999-03-25 09:00"
            ]
        );
        // US presidential election day.
        assert_eq!(
            expand(
                "FREQ=YEARLY;INTERVAL=4;BYMONTH=11;BYDAY=TU;BYMONTHDAY=2,3,4,5,6,7,8",
                "1996-11-05T09:00:00",
                3
            ),
            ["1996-11-05 09:00", "2000-11-07 09:00", "2004-11-02 09:00"]
        );
        // February 29 only in leap years.
        assert_eq!(
            expand("FREQ=YEARLY;COUNT=3", "2024-02-29T08:00:00", 5),
            ["2024-02-29 08:00", "2028-02-29 08:00", "2032-02-29 08:00"]
        );
        // Last week of the year (BYWEEKNO=-1) and week 53.
        assert_eq!(
            expand(
                "FREQ=YEARLY;BYWEEKNO=-1;BYDAY=MO;COUNT=3",
                "2026-12-28T09:00:00",
                5
            ),
            ["2026-12-28 09:00", "2027-12-27 09:00", "2028-12-25 09:00"]
        );
        assert_eq!(
            expand(
                "FREQ=YEARLY;BYWEEKNO=53;BYDAY=MO;COUNT=3",
                "2020-12-28T09:00:00",
                5
            ),
            ["2020-12-28 09:00", "2026-12-28 09:00", "2032-12-27 09:00"]
        );
        // Week 1 can start in December of the previous year.
        assert_eq!(
            expand(
                "FREQ=YEARLY;BYWEEKNO=1;BYDAY=MO;COUNT=3",
                "2024-12-30T09:00:00",
                5
            ),
            ["2024-12-30 09:00", "2025-12-29 09:00", "2027-01-04 09:00"]
        );
    }

    #[test]
    fn rfc_sub_daily_examples() {
        assert_eq!(
            expand(
                "FREQ=HOURLY;INTERVAL=3;UNTIL=19970902T170000",
                "1997-09-02T09:00:00",
                10
            ),
            ["1997-09-02 09:00", "1997-09-02 12:00", "1997-09-02 15:00"]
        );
        assert_eq!(
            expand(
                "FREQ=MINUTELY;INTERVAL=15;COUNT=6",
                "1997-09-02T09:00:00",
                10
            ),
            [
                "1997-09-02 09:00",
                "1997-09-02 09:15",
                "1997-09-02 09:30",
                "1997-09-02 09:45",
                "1997-09-02 10:00",
                "1997-09-02 10:15"
            ]
        );
        // Every 20 minutes from 9:00 to 16:40, two ways.
        let a = expand(
            "FREQ=DAILY;BYHOUR=9,10,11,12,13,14,15,16;BYMINUTE=0,20,40",
            "1997-09-02T09:00:00",
            48,
        );
        let b = expand(
            "FREQ=MINUTELY;INTERVAL=20;BYHOUR=9,10,11,12,13,14,15,16",
            "1997-09-02T09:00:00",
            48,
        );
        assert_eq!(a, b);
        assert_eq!(a[23], "1997-09-02 16:40");
        assert_eq!(a[24], "1997-09-03 09:00");
        // Day masks skip whole days for sub-daily rules.
        let weekdays = expand("FREQ=HOURLY;INTERVAL=5;BYDAY=SA", "2026-10-01T00:00:00", 3);
        assert_eq!(
            weekdays,
            ["2026-10-01 00:00", "2026-10-03 02:00", "2026-10-03 07:00"]
        );
    }

    #[test]
    fn impossible_rules_end() {
        assert_eq!(
            expand(
                "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30",
                "2026-01-01T09:00:00",
                5
            ),
            ["2026-01-01 09:00"]
        );
        assert_eq!(
            expand("FREQ=DAILY;BYSECOND=60", "2026-01-01T09:00:00", 5),
            ["2026-01-01 09:00"]
        );
    }

    #[test]
    fn until_bounds() {
        let start = PimTime::zoned("2026-10-23T09:00:00", "Europe/Berlin");
        // 07:00Z is 09:00 in Berlin on Oct 24 (summer time): included.
        let bound = UntilBound::new(&PimTime::utc("2026-10-24T07:00:00"), &start).unwrap();
        assert!(!bound.passed(at("2026-10-24T09:00:00")));
        assert!(bound.passed(at("2026-10-25T09:00:00")));
        let date = UntilBound::new(&PimTime::date("2026-10-24"), &start).unwrap();
        assert!(!date.passed(at("2026-10-24T23:00:00")));
        assert!(date.passed(at("2026-10-25T00:00:00")));
        let floating = UntilBound::new(
            &PimTime::floating("2026-10-24T09:00:00"),
            &PimTime::floating("2026-10-01T09:00:00"),
        )
        .unwrap();
        assert!(!floating.passed(at("2026-10-24T09:00:00")));
        assert!(floating.passed(at("2026-10-24T09:00:01")));
    }

    #[test]
    fn recurrence_sets_merge_rules_dates_and_exceptions() {
        let start = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin");
        let recurrence = Recurrence {
            rules: vec![
                rule("FREQ=WEEKLY;COUNT=4"),
                rule("FREQ=MONTHLY;COUNT=2;BYMONTHDAY=20"),
            ],
            dates: vec![
                PimTime::utc("2026-10-07T08:00:00"), // 10:00 Berlin
                PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin"), // duplicate
            ],
            exceptions: vec![PimTime::utc("2026-10-19T07:00:00")], // 09:00 Berlin (CEST)
        };
        let all: Vec<String> = Occurrences::new(&start, Some(&recurrence))
            .unwrap()
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .collect();
        assert_eq!(
            all,
            [
                "10-05 09:00",
                "10-07 10:00",
                "10-12 09:00",
                "10-20 09:00",
                "10-26 09:00"
            ]
        );
        // A date exception removes the occurrence on that day.
        let by_day = Recurrence {
            rules: vec![rule("FREQ=DAILY;COUNT=3")],
            exceptions: vec![PimTime::date("2026-10-06")],
            ..Recurrence::default()
        };
        assert_eq!(Occurrences::new(&start, Some(&by_day)).unwrap().count(), 2);
        // The start itself can be an exception.
        let no_start = Recurrence {
            rules: vec![rule("FREQ=DAILY;COUNT=3")],
            exceptions: vec![start.clone()],
            ..Recurrence::default()
        };
        assert_eq!(
            Occurrences::new(&start, Some(&no_start)).unwrap().count(),
            2
        );
        // No recurrence: just the start.
        assert_eq!(Occurrences::new(&start, None).unwrap().count(), 1);
        // All-day series ignore times.
        let days = Recurrence::rule(rule("FREQ=DAILY;COUNT=3;BYHOUR=10"));
        let all_day: Vec<NaiveDateTime> =
            Occurrences::new(&PimTime::date("2026-02-27"), Some(&days))
                .unwrap()
                .collect();
        assert_eq!(
            all_day,
            [
                at("2026-02-27T00:00:00"),
                at("2026-02-28T00:00:00"),
                at("2026-03-01T00:00:00")
            ]
        );
    }

    #[test]
    fn unsynchronized_start_counts_first() {
        // RFC 5545: DTSTART is always the first instance and counts.
        assert_eq!(
            expand("FREQ=WEEKLY;BYDAY=FR;COUNT=3", "2026-10-01T09:00:00", 5),
            ["2026-10-01 09:00", "2026-10-02 09:00", "2026-10-09 09:00"]
        );
    }

    #[test]
    fn year_limit_stops_iteration() {
        let all = expand("FREQ=YEARLY", "9997-06-01T00:00:00", 10);
        assert_eq!(all.len(), 3);
    }

    /// Skipping ahead yields exactly the occurrences a full walk yields
    /// from the target on, for every frequency, with and without COUNT.
    #[test]
    fn skip_before_matches_a_full_walk() {
        let rules = [
            "FREQ=DAILY",
            "FREQ=DAILY;INTERVAL=3",
            "FREQ=DAILY;COUNT=500",
            "FREQ=DAILY;INTERVAL=2;COUNT=100",
            "FREQ=DAILY;BYDAY=MO,WE",
            "FREQ=WEEKLY",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH",
            "FREQ=WEEKLY;INTERVAL=3;COUNT=50",
            "FREQ=WEEKLY;WKST=SU;BYDAY=SU,SA",
            "FREQ=MONTHLY;BYDAY=-1FR",
            "FREQ=MONTHLY;BYMONTHDAY=31",
            "FREQ=MONTHLY;INTERVAL=5;BYSETPOS=2;BYDAY=MO,TU",
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29",
            "FREQ=YEARLY;INTERVAL=2",
            "FREQ=HOURLY;INTERVAL=7",
            "FREQ=HOURLY;COUNT=3000",
            "FREQ=HOURLY;BYHOUR=9,17",
            "FREQ=MINUTELY;INTERVAL=13",
            "FREQ=MINUTELY;INTERVAL=90;COUNT=900",
            "FREQ=SECONDLY;INTERVAL=1000",
        ];
        let start = PimTime::floating("2020-02-29T09:30:15");
        for text in rules {
            let r = parse_rrule(text, &start).unwrap();
            for target in [
                "2020-02-29T09:30:15",
                "2020-03-01T00:00:00",
                "2020-03-11T13:05:00",
                "2020-06-30T23:59:59",
                "2021-01-01T00:00:00",
                "2024-02-29T09:30:15",
            ] {
                let target = at(target);
                let full: Vec<NaiveDateTime> =
                    RuleIter::new(&r, at("2020-02-29T09:30:15"), false, None)
                        .take(20_000)
                        .filter(|o| *o >= target)
                        .take(40)
                        .collect();
                let mut skipped = RuleIter::new(&r, at("2020-02-29T09:30:15"), false, None);
                skipped.skip_before(target);
                let skipped: Vec<NaiveDateTime> = skipped
                    .filter(|o| *o >= target)
                    .take(full.len().max(1))
                    .collect();
                if full.len() < 40 && !full.is_empty() {
                    // The walk above was cut by `take(20_000)`: compare its part.
                    assert_eq!(skipped[..full.len()], full[..], "{text} from {target}");
                } else {
                    assert_eq!(skipped, full, "{text} from {target}");
                }
            }
        }
    }

    /// A COUNT stays exact across a skip (the series still ends where it
    /// ended), and a series in the 1970s is cheap to reach in 2026.
    #[test]
    fn skip_keeps_counts_and_reaches_far_targets() {
        let start = PimTime::floating("2026-10-01T09:00:00");
        let r = parse_rrule("FREQ=DAILY;COUNT=10", &start).unwrap();
        let mut iter = RuleIter::new(&r, at("2026-10-01T09:00:00"), false, None);
        assert!(iter.skip_before(at("2026-10-08T00:00:00")));
        let rest: Vec<String> = iter.map(|t| t.format("%m-%d").to_string()).collect();
        // One period before the target is kept as a margin.
        assert_eq!(rest, ["10-07", "10-08", "10-09", "10-10"]);

        let old = PimTime::floating("1970-01-01T00:00:00");
        let hourly = parse_rrule("FREQ=HOURLY", &old).unwrap();
        let mut set = Occurrences::new(&old, Some(&Recurrence::rule(hourly))).unwrap();
        let begun = std::time::Instant::now();
        set.skip_before(at("2026-10-01T00:00:00"));
        let next = set.find(|t| *t >= at("2026-10-01T00:00:00")).unwrap();
        assert_eq!(next, at("2026-10-01T00:00:00"));
        assert!(begun.elapsed() < std::time::Duration::from_secs(1));
    }

    /// MINUTELY / SECONDLY rules end after `MAX_SUB_HOURLY_OCCURRENCES`
    /// whatever their COUNT or UNTIL say; smaller counts stay.
    #[test]
    fn sub_hourly_rules_are_capped() {
        let start = PimTime::floating("2026-10-01T00:00:00");
        for text in [
            "FREQ=SECONDLY",
            "FREQ=MINUTELY;COUNT=4000000000",
            "FREQ=SECONDLY;UNTIL=20991231T000000",
        ] {
            let r = parse_rrule(text, &start).unwrap();
            let until = match &r.end {
                RecurrenceEnd::Until { until } => UntilBound::new(until, &start),
                _ => None,
            };
            let n = RuleIter::new(&r, at("2026-10-01T00:00:00"), false, until).count();
            assert_eq!(n, MAX_SUB_HOURLY_OCCURRENCES as usize, "{text}");
        }
        let small = parse_rrule("FREQ=MINUTELY;COUNT=6", &start).unwrap();
        assert_eq!(
            RuleIter::new(&small, at("2026-10-01T00:00:00"), false, None).count(),
            6
        );
        // HOURLY is not capped.
        let hourly = parse_rrule("FREQ=HOURLY;COUNT=5000", &start).unwrap();
        assert_eq!(
            RuleIter::new(&hourly, at("2026-10-01T00:00:00"), false, None).count(),
            5000
        );
    }
}
