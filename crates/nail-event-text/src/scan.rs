//! Words and the grammar: the text is cut into words, and each grammar
//! rule tries to read a phrase starting at a word (a date, a time or a
//! time range, a length, "all day", a repetition). Nothing here knows the
//! result's shape; [`crate::parse`] puts the pieces together.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use nail_api::pim::calendar::{Frequency, RecurrenceRule, WeekdayNum};

use nail_util::time::DAYS_PER_YEAR;

use crate::keywords::{Keywords, WEEKDAYS};

/// One word of the text.
#[derive(Debug, Clone)]
pub(crate) struct Word {
    /// Lower case, trailing dots dropped ("Fri." → "fri").
    pub text: String,
    /// Byte offset of the first character (of the original word).
    pub start: usize,
    /// Byte offset after the last kept character.
    pub end: usize,
    /// Written with a capital first letter.
    pub capital: bool,
}

/// Characters that end a word and are not part of any phrase.
fn is_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | ';' | '!' | '?' | '(' | ')' | '[' | ']' | '"')
}

/// Cuts the text into words. A leading `@` is a word of its own
/// ("@Corner" → "@", "corner").
pub(crate) fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |from: usize, to: usize, out: &mut Vec<Word>| {
        let mut slice = &text[from..to];
        let mut from = from;
        if slice.len() > 1 && slice.starts_with('@') {
            out.push(word(text, from, from + 1));
            from += 1;
            slice = &text[from..to];
        }
        let kept = slice.trim_end_matches('.');
        if !kept.is_empty() {
            out.push(word(text, from, from + kept.len()));
        }
    };
    for (at, c) in text.char_indices() {
        if is_break(c) {
            if let Some(from) = start.take() {
                push(from, at, &mut out);
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    if let Some(from) = start {
        push(from, text.len(), &mut out);
    }
    out
}

fn word(text: &str, start: usize, end: usize) -> Word {
    let slice = &text[start..end];
    Word {
        text: slice.to_lowercase(),
        start,
        end,
        capital: slice.chars().next().is_some_and(char::is_uppercase),
    }
}

/// What a phrase means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    /// A day.
    Date(NaiveDate),
    /// A time, or a time range (the end may be before the start: past
    /// midnight).
    Time(NaiveTime, Option<NaiveTime>),
    /// A length.
    Length(Length),
    /// "all day".
    AllDay,
    /// A repetition; `days` are the weekdays it names (Monday = 0).
    Repeat {
        /// The rule.
        rule: Box<RecurrenceRule>,
        /// Weekdays named ("every Monday and Thursday").
        days: Vec<usize>,
    },
}

/// A length ("for 2 hours", "for 3 days").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Length {
    /// Minutes (a timed event).
    Minutes(u32),
    /// Whole days (an all-day event).
    Days(u32),
}

/// Longest length read: a year of days, a week of minutes.
const MAX_DAYS: u16 = DAYS_PER_YEAR + 1;
const MAX_MINUTES: u32 = 7 * 24 * 60;

/// Morning or afternoon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Half {
    Am,
    Pm,
}

impl Half {
    fn other(self) -> Self {
        match self {
            Self::Am => Self::Pm,
            Self::Pm => Self::Am,
        }
    }
}

/// A time of day as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Clock {
    hour: u32,
    minute: u32,
    half: Option<Half>,
    /// Written with minutes ("12:30") or as a word ("noon"): a time for
    /// sure, read on a 24-hour clock when it has no half.
    exact: bool,
}

impl Clock {
    /// On a 24-hour clock in `half` (or as written).
    fn time(self, half: Option<Half>) -> Option<NaiveTime> {
        let hour = match half {
            Some(h) => {
                if !(1..=12).contains(&self.hour) {
                    return None;
                }
                (self.hour % 12) + if h == Half::Pm { 12 } else { 0 }
            }
            None => self.hour,
        };
        NaiveTime::from_hms_opt(hour, self.minute, 0)
    }
}

/// The reader over the words of one text.
#[derive(Debug)]
pub(crate) struct Reader<'a> {
    pub words: Vec<Word>,
    k: &'a Keywords,
    today: NaiveDate,
    day_first: bool,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(text: &str, k: &'a Keywords, today: NaiveDate, day_first: bool) -> Self {
        Self {
            words: words(text),
            k,
            today,
            day_first,
        }
    }

    fn text(&self, i: usize) -> Option<&str> {
        self.words.get(i).map(|w| w.text.as_str())
    }

    fn is(&self, i: usize, list: &[&str]) -> bool {
        self.text(i).is_some_and(|t| list.contains(&t))
    }

    /// Whether the words from `i` are one of the sequences; the length.
    fn phrase(&self, i: usize, sequences: &[&[&str]]) -> Option<usize> {
        sequences
            .iter()
            .find(|seq| {
                seq.iter()
                    .enumerate()
                    .all(|(n, w)| self.text(i + n) == Some(*w))
            })
            .map(|seq| seq.len())
    }

    /// Tries every rule at word `i`: the phrase's length and meaning.
    pub(crate) fn read_at(&self, i: usize) -> Option<(usize, Piece)> {
        self.repeat_at(i)
            .or_else(|| self.phrase(i, self.k.all_day).map(|n| (n, Piece::AllDay)))
            .or_else(|| self.date_at(i).map(|(n, d)| (n, Piece::Date(d))))
            .or_else(|| self.length_at(i).map(|(n, l)| (n, Piece::Length(l))))
            .or_else(|| self.time_at(i))
    }

    // ----------------------------------------------------------------- dates

    /// The weekday a word names (Monday = 0). An abbreviation that is also
    /// a word needs `prefixed` (after "on", "next", "every", …) or a
    /// capital letter.
    fn weekday(&self, i: usize, prefixed: bool) -> Option<usize> {
        let w = self.words.get(i)?;
        let day = self
            .k
            .weekdays
            .iter()
            .position(|names| names.contains(&w.text.as_str()))?;
        let ambiguous = self.k.ambiguous_weekdays.contains(&w.text.as_str());
        (!ambiguous || prefixed || w.capital).then_some(day)
    }

    fn month(&self, i: usize) -> Option<u32> {
        let text = self.text(i)?;
        let index = self
            .k
            .months
            .iter()
            .position(|names| names.contains(&text))?;
        u32::try_from(index + 1).ok()
    }

    /// A day of the month: `4`, `4th`.
    fn day_number(&self, i: usize) -> Option<u32> {
        let text = self.text(i)?;
        let digits = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let suffix = &text[digits.len()..];
        if !(1..=2).contains(&digits.len())
            || !digits.chars().all(|c| c.is_ascii_digit())
            || !(suffix.is_empty() || self.k.ordinal_suffixes.contains(&suffix))
        {
            return None;
        }
        digits.parse().ok().filter(|d| (1..=31).contains(d))
    }

    fn year(&self, i: usize) -> Option<i32> {
        let text = self.text(i)?;
        (text.len() == 4 && text.chars().all(|c| c.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    }

    /// A date starting at `i`, with an optional "on" / "next" / "this".
    pub(crate) fn date_at(&self, i: usize) -> Option<(usize, NaiveDate)> {
        let mut j = i;
        let mut prefixed = false;
        if self.is(j, self.k.on) {
            j += 1;
            prefixed = true;
        }
        let next = self.is(j, self.k.next);
        if next || self.is(j, self.k.this) {
            j += 1;
            let day = self.weekday(j, true)?;
            let date = self.coming(day) + Duration::days(if next { 7 } else { 0 });
            return Some((j + 1 - i, date));
        }
        let (n, date) = self
            .weekday(j, prefixed)
            .map_or_else(|| self.plain_date_at(j), |day| Some((1, self.coming(day))))?;
        Some((j + n - i, date))
    }

    /// The first `weekday` on or after today.
    pub(crate) fn coming(&self, weekday: usize) -> NaiveDate {
        coming_from(self.today, weekday)
    }

    fn plain_date_at(&self, i: usize) -> Option<(usize, NaiveDate)> {
        if self.is(i, self.k.today) {
            return Some((1, self.today));
        }
        if self.is(i, self.k.tomorrow) {
            return Some((1, self.today + Duration::days(1)));
        }
        if let Some(n) = self.phrase(i, self.k.day_after_tomorrow) {
            return Some((n, self.today + Duration::days(2)));
        }
        if self.is(i, self.k.in_)
            && let Some(count) = self.text(i + 1).and_then(|t| t.parse::<u32>().ok())
            && count <= u32::from(MAX_DAYS)
        {
            let per = if self.is(i + 2, self.k.days) {
                1
            } else if self.is(i + 2, self.k.weeks) {
                7
            } else {
                return None;
            };
            return Some((3, self.today + Duration::days(i64::from(count * per))));
        }
        if let Some(found) = self.numeric_date(i) {
            return Some((1, found));
        }
        // "Dec 4", "Dec 4th 2027", "4 Dec", "4th of December 2027".
        let (n, month, day) =
            if let (Some(month), Some(day)) = (self.month(i), self.day_number(i + 1)) {
                (2, month, day)
            } else {
                let day = self.day_number(i)?;
                let of = usize::from(self.is(i + 1, self.k.of));
                (2 + of, self.month(i + 1 + of)?, day)
            };
        match self.year(i + n) {
            Some(year) => NaiveDate::from_ymd_opt(year, month, day).map(|d| (n + 1, d)),
            None => self.next_date(month, day).map(|d| (n, d)),
        }
    }

    /// `month` / `day` this year, or the next year that has it when it
    /// has passed (or does not exist, like 29 February).
    fn next_date(&self, month: u32, day: u32) -> Option<NaiveDate> {
        (0..=8)
            .filter_map(|ahead| NaiveDate::from_ymd_opt(self.today.year() + ahead, month, day))
            .find(|d| *d >= self.today)
    }

    /// `2026-12-04`, `12/4`, `12/4/2026`, `12/4/26` (month first, or day
    /// first in the regions that write it so).
    fn numeric_date(&self, i: usize) -> Option<NaiveDate> {
        let text = self.text(i)?;
        let numbers = |sep: char| -> Option<Vec<u32>> {
            let parts: Vec<&str> = text.split(sep).collect();
            (parts.len() >= 2
                && parts.len() <= 3
                && parts.iter().all(|p| {
                    !p.is_empty() && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit())
                }))
            .then(|| parts.iter().filter_map(|p| p.parse().ok()).collect())
        };
        if let Some(parts) = numbers('-')
            && let [year, month, day] = parts[..]
            && text.len() == 10
        {
            return NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day);
        }
        let parts = numbers('/')?;
        let (first, second) = (*parts.first()?, *parts.get(1)?);
        let (month, day) = if self.day_first {
            (second, first)
        } else {
            (first, second)
        };
        match parts.get(2) {
            Some(&year) => {
                // Two digits: this century.
                let century = u32::try_from(self.today.year() / 100 * 100).ok()?;
                let year = if year < 100 { century + year } else { year };
                NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)
            }
            None => self.next_date(month, day),
        }
    }

    // ----------------------------------------------------------------- times

    /// A time of day as written in one word (`12:30`, `3pm`, `noon`).
    fn clock(&self, text: &str) -> Option<Clock> {
        if self.k.noon.contains(&text) {
            return Some(Clock {
                hour: 12,
                minute: 0,
                half: None,
                exact: true,
            });
        }
        if self.k.midnight.contains(&text) {
            return Some(Clock {
                hour: 0,
                minute: 0,
                half: None,
                exact: true,
            });
        }
        let digits_end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (hour, mut rest) = text.split_at(digits_end);
        if hour.is_empty() || hour.len() > 2 {
            return None;
        }
        let mut minute = 0;
        let mut exact = false;
        if let Some(after) = rest.strip_prefix([':', '.']) {
            let (m, tail) = after.split_at(
                after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len()),
            );
            if m.len() != 2 {
                return None;
            }
            minute = m.parse().ok().filter(|m| *m < 60)?;
            exact = true;
            rest = tail;
        }
        let half = if rest.is_empty() {
            None
        } else {
            Some(self.half(rest)?)
        };
        let hour: u32 = hour.parse().ok()?;
        (hour < 24).then_some(Clock {
            hour,
            minute,
            half,
            exact,
        })
    }

    fn half(&self, text: &str) -> Option<Half> {
        if self.k.am.contains(&text) {
            Some(Half::Am)
        } else if self.k.pm.contains(&text) {
            Some(Half::Pm)
        } else {
            None
        }
    }

    /// A clock at word `j`, with a following "pm" word; the words read.
    fn clock_at(&self, j: usize) -> Option<(usize, Clock)> {
        let mut clock = self.clock(self.text(j)?)?;
        if clock.half.is_none()
            && let Some(half) = self.text(j + 1).and_then(|t| self.half(t))
            // A lone "a" after a number is an article ("at 3 a …"), not a.m.
            && self.text(j + 1).is_some_and(|t| t.len() > 1)
        {
            clock.half = Some(half);
            return Some((2, clock));
        }
        Some((1, clock))
    }

    /// A time or a time range at `i`: "at 3pm", "12:30", "3-4pm", "from 3
    /// to 4:30 pm", "noon".
    pub(crate) fn time_at(&self, i: usize) -> Option<(usize, Piece)> {
        let mut j = i;
        let prefixed = self.is(j, self.k.at) || self.is(j, self.k.from);
        if prefixed {
            j += 1;
        }
        let text = self.text(j)?;
        let dashes: &[char] = &['-', '\u{2013}', '\u{2014}'];
        // "3-4pm" in one word.
        if let Some((a, b)) = text.split_once(dashes)
            && !a.is_empty()
            && !b.is_empty()
        {
            let first = self.clock(a)?;
            let (n, second) = match self.clock(b) {
                Some(c) if c.half.is_none() => match self.text(j + 1).and_then(|t| self.half(t)) {
                    Some(half) if self.text(j + 1).is_some_and(|t| t.len() > 1) => (
                        2,
                        Clock {
                            half: Some(half),
                            ..c
                        },
                    ),
                    _ => (1, c),
                },
                Some(c) => (1, c),
                None => return None,
            };
            let (start, end) = range(first, second, prefixed)?;
            return Some((j + n - i, Piece::Time(start, Some(end))));
        }
        let (n, first) = self.clock_at(j)?;
        j += n;
        // "3pm -4pm": the dash glued to the second time.
        let glued = self
            .text(j)
            .and_then(|t| t.strip_prefix(dashes))
            .filter(|rest| !rest.is_empty())
            .and_then(|rest| self.clock(rest));
        let second = if let Some(clock) = glued {
            Some((1, clock))
        } else if self.is(j, self.k.range_to) {
            self.clock_at(j + 1).map(|(n, c)| (n + 1, c))
        } else {
            None
        };
        if let Some((m, second)) = second
            && let Some((start, end)) = range(first, second, prefixed)
        {
            return Some((j + m - i, Piece::Time(start, Some(end))));
        }
        // A bare number is a time only after "at" / "from".
        if !(first.exact || first.half.is_some() || prefixed) {
            return None;
        }
        let start = single(first, prefixed)?;
        Some((j - i, Piece::Time(start, None)))
    }

    // ---------------------------------------------------------------- lengths

    /// A length at `i`: "for 30 min", "for 2 hours", "for an hour", "for
    /// half an hour", "for 3 days", "90 minutes", "1h30".
    pub(crate) fn length_at(&self, i: usize) -> Option<(usize, Length)> {
        let has_for = self.is(i, self.k.for_);
        let j = i + usize::from(has_for);
        let (n, length) = self.amount_at(j, has_for)?;
        Some((j + n - i, length))
    }

    fn amount_at(&self, j: usize, has_for: bool) -> Option<(usize, Length)> {
        // "half an hour", "an hour", "a day".
        let half = self.is(j, self.k.half);
        let article = j + usize::from(half);
        if has_for && self.is(article, self.k.one) {
            let unit = article + 1;
            let minutes = if self.is(unit, self.k.hours) {
                if half { 30 } else { 60 }
            } else if !half && self.is(unit, self.k.days) {
                return Some((unit + 1 - j, Length::Days(1)));
            } else {
                return None;
            };
            return Some((unit + 1 - j, Length::Minutes(minutes)));
        }
        let text = self.text(j)?;
        // "1h30" / "1h30m".
        if let Some((h, m)) = text.split_once('h')
            && !h.is_empty()
            && h.chars().all(|c| c.is_ascii_digit())
            && let m = m.trim_end_matches('m')
            && m.len() == 2
            && m.chars().all(|c| c.is_ascii_digit())
        {
            let minutes = h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?;
            return minutes_length(minutes).map(|l| (1, l));
        }
        // A number glued to its unit ("30min", "2h", "1.5h").
        let number_end = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(number_end);
        if number.is_empty() {
            return None;
        }
        let (n, unit) = if unit.is_empty() {
            (2, self.text(j + 1)?)
        } else {
            (1, unit)
        };
        if self.k.minutes.contains(&unit) {
            let minutes = scaled(number, 1)?;
            // A bare "m" after a separate number is too likely metres.
            if n == 2 && unit == "m" {
                return None;
            }
            minutes_length(minutes).map(|l| (n, l))
        } else if self.k.hours.contains(&unit) {
            if n == 2 && unit == "h" {
                return None;
            }
            minutes_length(scaled(number, 60)?).map(|l| (n, l))
        } else if has_for && self.k.days.contains(&unit) {
            let days: u32 = number.parse().ok()?;
            (1..=u32::from(MAX_DAYS))
                .contains(&days)
                .then_some((n, Length::Days(days)))
        } else {
            None
        }
    }

    // ------------------------------------------------------------ repetitions

    /// A repetition at `i`: "every Monday", "every Mon and Thu", "every
    /// weekday", "every other week", "every 2 days", "daily", ….
    pub(crate) fn repeat_at(&self, i: usize) -> Option<(usize, Piece)> {
        for (list, freq) in [
            (self.k.daily, Frequency::Daily),
            (self.k.weekly, Frequency::Weekly),
            (self.k.monthly, Frequency::Monthly),
            (self.k.yearly, Frequency::Yearly),
        ] {
            if self.is(i, list) {
                return Some((1, repeat(freq, 1, Vec::new())));
            }
        }
        if !self.is(i, self.k.every) {
            return None;
        }
        let mut j = i + 1;
        let mut interval = 1;
        if self.is(j, self.k.other) {
            interval = 2;
            j += 1;
        } else if let Some(n) = self
            .text(j)
            .and_then(|t| t.parse::<u32>().ok())
            .filter(|n| (1..=u32::from(MAX_DAYS)).contains(n))
        {
            interval = n;
            j += 1;
        }
        if self.is(j, self.k.weekday) {
            return Some((
                j + 1 - i,
                repeat(Frequency::Weekly, interval, (0..5).collect()),
            ));
        }
        if let Some(first) = self.weekday(j, true) {
            let mut days = vec![first];
            j += 1;
            loop {
                let after_and = j + usize::from(self.is(j, self.k.and));
                match self.weekday(after_and, true) {
                    Some(day) => {
                        if !days.contains(&day) {
                            days.push(day);
                        }
                        j = after_and + 1;
                    }
                    None => break,
                }
            }
            days.sort_unstable();
            return Some((j - i, repeat(Frequency::Weekly, interval, days)));
        }
        let freq = if self.is(j, self.k.days) {
            Frequency::Daily
        } else if self.is(j, self.k.weeks) {
            Frequency::Weekly
        } else if self.is(j, self.k.months_unit) {
            Frequency::Monthly
        } else if self.is(j, self.k.years) {
            Frequency::Yearly
        } else {
            return None;
        };
        Some((j + 1 - i, repeat(freq, interval, Vec::new())))
    }
}

/// The first `weekday` (Monday = 0) on or after `from`.
pub(crate) fn coming_from(from: NaiveDate, weekday: usize) -> NaiveDate {
    let today = from.weekday().num_days_from_monday();
    let wanted = u32::try_from(weekday).unwrap_or(0);
    from + Duration::days(i64::from((wanted + 7 - today) % 7))
}

fn repeat(freq: Frequency, interval: u32, days: Vec<usize>) -> Piece {
    let mut rule = RecurrenceRule::new(freq);
    rule.interval = interval;
    rule.by_day = days
        .iter()
        .filter_map(|d| WEEKDAYS.get(*d))
        .map(|day| WeekdayNum {
            day: *day,
            ordinal: None,
        })
        .collect();
    Piece::Repeat {
        rule: Box::new(rule),
        days,
    }
}

/// `number` (`2`, `1.5`, `0.25`) times `per`, in whole units.
fn scaled(number: &str, per: u32) -> Option<u32> {
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && fraction.is_empty() || fraction.len() > 2 {
        return None;
    }
    let whole: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut total = whole.checked_mul(per)?;
    if !fraction.is_empty() {
        let digits: u32 = fraction.parse().ok()?;
        let scale = 10_u32.pow(u32::try_from(fraction.len()).ok()?);
        total = total.checked_add(digits * per / scale)?;
    }
    Some(total)
}

fn minutes_length(minutes: u32) -> Option<Length> {
    (1..=MAX_MINUTES)
        .contains(&minutes)
        .then_some(Length::Minutes(minutes))
}

/// The hour a bare number after "at" / "from" means: 1 to 6 are taken as
/// the afternoon ("at 3" = 15:00; nobody plans a meeting at 3 in the
/// night without saying so).
fn afternoon(clock: Clock, prefixed: bool) -> Clock {
    if prefixed && clock.half.is_none() && !clock.exact && (1..=6).contains(&clock.hour) {
        Clock {
            hour: clock.hour + 12,
            ..clock
        }
    } else {
        clock
    }
}

fn single(clock: Clock, prefixed: bool) -> Option<NaiveTime> {
    let clock = afternoon(clock, prefixed);
    clock.time(clock.half)
}

/// The two ends of a range. A half written on one end applies to the
/// other ("3-4pm"); when that would put the start after the end, the
/// start is the other half ("11-1pm" = 11:00 to 13:00). A range needs at
/// least one end that is surely a time (minutes, a half, a word) or a
/// leading "at" / "from".
fn range(first: Clock, second: Clock, prefixed: bool) -> Option<(NaiveTime, NaiveTime)> {
    let sure = |c: Clock| c.exact || c.half.is_some();
    if !(sure(first) || sure(second) || prefixed) {
        return None;
    }
    match (first.half, second.half) {
        (Some(a), Some(b)) => Some((first.time(Some(a))?, second.time(Some(b))?)),
        (None, Some(b)) => {
            let end = second.time(Some(b))?;
            if first.hour > 12 || first.hour == 0 {
                return Some((first.time(None)?, end));
            }
            let start = first.time(Some(b))?;
            if start > end {
                Some((first.time(Some(b.other()))?, end))
            } else {
                Some((start, end))
            }
        }
        (Some(a), None) => {
            let start = first.time(Some(a))?;
            if second.hour > 12 || second.hour == 0 {
                return Some((start, second.time(None)?));
            }
            let end = second.time(Some(a))?;
            if end <= start {
                Some((start, second.time(Some(a.other()))?))
            } else {
                Some((start, end))
            }
        }
        (None, None) => {
            let start = single(first, prefixed)?;
            let mut end = single(second, prefixed)?;
            // "from 11 to 1": the end is after the start the same day.
            if end <= start && !second.exact && second.hour < 12 {
                end = second.time(Some(Half::Pm))?;
            }
            Some((start, end))
        }
    }
}
