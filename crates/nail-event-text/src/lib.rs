//! Quick create from text (R7, ADR 0246): reads a sentence such as "Lunch
//! with Bob tomorrow 12:30 at Corner café" as an event — dates ("today",
//! "tomorrow", weekdays, "Dec 4"), times and time ranges ("12:30",
//! "3-4pm"), lengths ("for 2 hours"), repetitions ("every Monday"), "all
//! day" and a place ("at …") — and says which parts of the text it
//! recognised, so the UI can mark them.
//!
//! The core answers [`nail_api::Request::ParseEventText`] with [`parse`];
//! the calendar's quick-add field, the command palette and the MCP surface
//! share it. English comes first; the keywords of other languages come
//! from a per-language table (unknown languages fall back to English).
//! Nothing here reads the clock: "now" is part of the [`Context`], so a
//! test gets the same answer every day.
//!
//! Rules of the result (`nail_api::pim::ParsedEventText`): start and end
//! are both set or both `None`; a date without a time is all-day (end the
//! next day, exclusive); a start without an end or length lasts
//! [`nail_api::pim::calendar_view::DEFAULT_EVENT_MINUTES`]; times are
//! zoned in the context's zone; spans are UTF-16 offsets, in text order,
//! not overlapping; the title is the text without the recognised parts.

use chrono::NaiveDateTime;
use nail_api::pim::ParsedEventText;

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

/// Reads `text` as an event.
///
/// Contract stub: answers with the whole text as the title and nothing
/// recognised; the parser itself is the core package's work in R7.
pub fn parse(text: &str, context: &Context) -> ParsedEventText {
    let _ = context;
    ParsedEventText::title_only(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            now: chrono::NaiveDate::from_ymd_opt(2026, 11, 25)
                .and_then(|d| d.and_hms_opt(10, 40, 0))
                .unwrap(),
            tz: "Europe/Berlin".into(),
            locale: "en".into(),
        }
    }

    #[test]
    fn plain_text_is_the_title() {
        let parsed = parse("  Call the plumber ", &context());
        assert_eq!(parsed.title, "Call the plumber");
        assert!(parsed.start.is_none() && parsed.end.is_none());
        assert_eq!(parsed.spans, []);
        assert_eq!(
            serde_json::to_value(&parsed).unwrap()["spans"],
            serde_json::json!([])
        );
    }
}
