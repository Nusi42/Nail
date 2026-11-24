//! Quick create from text (R7, ADR 0246): reads a sentence such as "Lunch
//! with Bob tomorrow 12:30 at Corner café" as an event — dates ("today",
//! "tomorrow", weekdays, "next Friday", "Dec 4"), times and time ranges
//! ("12:30", "3pm", "3-4pm"), lengths ("for 30 min"), repetitions ("every
//! Monday"), "all day" and a place ("at …") — and says which parts of the
//! text it recognised, so the UI can mark them.
//!
//! The core answers [`nail_api::Request::ParseEventText`] with [`parse`].
//! English comes first; the keywords of a language come from its table
//! in `keywords` (unknown languages read as English). Nothing here reads
//! the clock: "now" is part of the [`Context`], so a test gets the same
//! answer every day.
//!
//! Rules of the result (`nail_api::pim::ParsedEventText`):
//!
//! * Start and end are both set or both `None` (nothing about time).
//! * A date without a time is all-day (end the next day, exclusive; "for
//!   3 days" makes it three days). "all day" wins over a time.
//! * A start without an end or a length lasts
//!   [`nail_api::pim::calendar_view::DEFAULT_EVENT_MINUTES`]; a range
//!   whose end is not after its start ends the next day ("11pm-1am").
//! * A time without a date is today, or tomorrow when it has passed.
//! * A plain weekday is the coming one (today counts); "next Friday" is a
//!   week after that; a month and day without a year is the next one.
//! * A repetition without a date starts on its first day ("every Monday"
//!   = the coming Monday); with no time it is all-day.
//! * A length alone starts at the next quarter hour.
//! * A bare number is a time only after "at" / "from", and 1 to 6 then
//!   mean the afternoon ("at 3" = 15:00); `12/4` is month first, or day
//!   first in regions that write dates so (`en-GB`).
//! * Of each kind only the first phrase counts; a second one stays in the
//!   title. The place is the text after an "at" that is not a time, up to
//!   the next recognised part.
//! * Times are zoned in the context's zone; spans are UTF-16 offsets, in
//!   text order, not overlapping; the title is the text without them.

mod keywords;
mod scan;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use nail_api::pim::calendar::{Recurrence, RecurrenceRule};
use nail_api::pim::calendar_view::{DEFAULT_EVENT_MINUTES, ParsedPart, ParsedSpan};
use nail_api::pim::{ParsedEventText, PimTime};

use scan::{Length, Piece, Reader, Word};

/// What the text is read against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// "Now" as a wall-clock time in [`Self::tz`] (relative words count
    /// from it).
    pub now: NaiveDateTime,
    /// The IANA zone the times are meant in (the view's zone).
    pub tz: String,
    /// The language of the keywords (BCP 47, e.g. `en`, `en-GB`).
    pub locale: String,
}

/// The pieces found, the first of each kind.
#[derive(Debug, Default)]
struct Found {
    date: Option<NaiveDate>,
    time: Option<(NaiveTime, Option<NaiveTime>)>,
    length: Option<Length>,
    all_day: bool,
    repeat: Option<(RecurrenceRule, Vec<usize>)>,
    location: Option<String>,
}

/// A read part: words `[from, to)` and their kind.
type Part = (usize, usize, ParsedPart);

/// Reads `text` as an event.
pub fn parse(text: &str, context: &Context) -> ParsedEventText {
    let k = keywords::for_locale(&context.locale);
    let day_first = keywords::day_first(k, &context.locale);
    let reader = Reader::new(text, k, context.now.date(), day_first);
    let mut found = Found::default();
    let mut parts: Vec<Part> = Vec::new();
    let mut i = 0;
    while i < reader.words.len() {
        if let Some((n, piece)) = reader.read_at(i)
            && let Some(kind) = take(&mut found, piece)
        {
            parts.push((i, i + n, kind));
            i += n;
        } else {
            i += 1;
        }
    }
    if let Some((from, to, place)) = place(text, &reader.words, &parts, k.at) {
        found.location = Some(place);
        parts.push((from, to, ParsedPart::Location));
    }
    parts.sort_by_key(|(from, ..)| *from);
    let bytes: Vec<(usize, usize, ParsedPart)> = parts
        .iter()
        .map(|(from, to, kind)| (reader.words[*from].start, reader.words[to - 1].end, *kind))
        .collect();
    let spans = bytes
        .iter()
        .map(|(start, end, kind)| ParsedSpan {
            kind: *kind,
            start: utf16_offset(text, *start),
            end: utf16_offset(text, *end),
        })
        .collect();
    let (start, end) = when(&found, context).unzip();
    ParsedEventText {
        title: title(text, &bytes),
        start,
        end,
        location: found.location,
        recurrence: found.repeat.map(|(rule, _)| Recurrence::rule(rule)),
        spans,
    }
}

/// Keeps a piece unless its kind was already found; its kind.
fn take(found: &mut Found, piece: Piece) -> Option<ParsedPart> {
    match piece {
        Piece::Date(d) if found.date.is_none() => {
            found.date = Some(d);
            Some(ParsedPart::Date)
        }
        Piece::Time(s, e) if found.time.is_none() => {
            found.time = Some((s, e));
            Some(ParsedPart::Time)
        }
        Piece::Length(l) if found.length.is_none() => {
            found.length = Some(l);
            Some(ParsedPart::Duration)
        }
        Piece::AllDay if !found.all_day => {
            found.all_day = true;
            Some(ParsedPart::AllDay)
        }
        Piece::Repeat { rule, days } if found.repeat.is_none() => {
            found.repeat = Some((*rule, days));
            Some(ParsedPart::Recurrence)
        }
        _ => None,
    }
}

/// The place: after the first unread "at" / "@" followed by an unread
/// word, up to the next read part (or the end). Its word range and text.
fn place(
    text: &str,
    words: &[Word],
    parts: &[Part],
    at: &[&str],
) -> Option<(usize, usize, String)> {
    let read = |i: usize| parts.iter().any(|(from, to, _)| (*from..*to).contains(&i));
    let keyword = (0..words.len()).find(|&i| {
        !read(i) && at.contains(&words[i].text.as_str()) && i + 1 < words.len() && !read(i + 1)
    })?;
    let last = (keyword + 1..words.len())
        .take_while(|&i| !read(i))
        .last()?;
    let place = text[words[keyword + 1].start..words[last].end]
        .trim_end_matches(['-', '\u{2013}', '\u{2014}', ':'])
        .trim();
    (!place.is_empty()).then(|| (keyword, last + 1, place.to_owned()))
}

/// The text without the read parts (byte ranges): words that are only
/// punctuation are dropped, the rest joined by single spaces and trimmed
/// of separators.
fn title(text: &str, parts: &[(usize, usize, ParsedPart)]) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    for (from, to, _) in parts {
        if *from >= at {
            kept.push_str(&text[at..*from]);
            kept.push(' ');
            at = *to;
        }
    }
    kept.push_str(&text[at..]);
    let separator = |c: char| matches!(c, ',' | '.' | ';' | ':' | '-' | '\u{2013}' | '\u{2014}');
    kept.split_whitespace()
        .filter(|w| !w.chars().all(separator))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| separator(c) || c.is_whitespace())
        .to_owned()
}

/// UTF-16 code units before byte `at`.
fn utf16_offset(text: &str, at: usize) -> u32 {
    u32::try_from(text[..at].encode_utf16().count()).unwrap_or(u32::MAX)
}

/// Start and end of the event, if anything about time was found.
fn when(found: &Found, context: &Context) -> Option<(PimTime, PimTime)> {
    let today = context.now.date();
    let now = context.now.time();
    let zoned =
        |at: NaiveDateTime| PimTime::zoned(at.format("%Y-%m-%dT%H:%M:%S").to_string(), &context.tz);
    let date = |d: NaiveDate| PimTime::date(d.format("%Y-%m-%d").to_string());
    // The day of a time without a date: today, or tomorrow once passed; a
    // weekly repetition moves on to its first weekday.
    let first_day = |time: Option<NaiveTime>| -> NaiveDate {
        if let Some(day) = found.date {
            return day;
        }
        let from = match time {
            Some(t) if t < now => today + Duration::days(1),
            _ => today,
        };
        match &found.repeat {
            Some((_, days)) if !days.is_empty() => days
                .iter()
                .map(|d| scan::coming_from(from, *d))
                .min()
                .unwrap_or(from),
            _ => from,
        }
    };
    let days = match found.length {
        Some(Length::Days(n)) => Some(n),
        _ => None,
    };
    if let Some((start_time, end_time)) = found.time
        && !found.all_day
        && days.is_none()
    {
        let start = first_day(Some(start_time)).and_time(start_time);
        let end = match (end_time, found.length) {
            (Some(end_time), _) => {
                let end = start.date().and_time(end_time);
                if end <= start {
                    end + Duration::days(1)
                } else {
                    end
                }
            }
            (None, Some(Length::Minutes(m))) => start + Duration::minutes(i64::from(m)),
            _ => start + Duration::minutes(i64::from(DEFAULT_EVENT_MINUTES)),
        };
        return Some((zoned(start), zoned(end)));
    }
    if found.date.is_some() || found.all_day || found.repeat.is_some() || days.is_some() {
        let day = first_day(None);
        let end = day + Duration::days(i64::from(days.unwrap_or(1)));
        return Some((date(day), date(end)));
    }
    if let Some(Length::Minutes(m)) = found.length {
        let start = next_quarter(context.now);
        return Some((zoned(start), zoned(start + Duration::minutes(i64::from(m)))));
    }
    None
}

/// The next quarter hour at or after `now` (seconds dropped).
fn next_quarter(now: NaiveDateTime) -> NaiveDateTime {
    let start = now
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(now);
    let late = start < now;
    let rest = (15 - start.minute() % 15) % 15;
    let bump = if rest == 0 && late { 15 } else { rest };
    start + Duration::minutes(i64::from(bump))
}

#[cfg(test)]
mod tests;
