//! The words the reader knows, one table per language (ADR 0246: they are
//! grammar, not UI texts, so they live here and not in a Fluent catalog).
//!
//! Every entry is lower case; the reader folds the text before it
//! compares. A language without a table reads as English.

use nail_api::triage::Weekday;

/// One language's keywords.
#[derive(Debug)]
pub(crate) struct Keywords {
    /// "today".
    pub today: &'static [&'static str],
    /// "tomorrow".
    pub tomorrow: &'static [&'static str],
    /// "the day after tomorrow" (word sequences).
    pub day_after_tomorrow: &'static [&'static [&'static str]],
    /// Weekdays, Monday first: full names and abbreviations.
    pub weekdays: [&'static [&'static str]; 7],
    /// Abbreviations that are also ordinary words ("sat", "sun", "wed"):
    /// read as a weekday only after a date word ("on", "next", "every")
    /// or when written with a capital letter.
    pub ambiguous_weekdays: &'static [&'static str],
    /// Months, January first: full names and abbreviations. A month is
    /// read only next to a day number, so "may" and "march" stay words.
    pub months: [&'static [&'static str]; 12],
    /// Ordinal suffixes after a day number ("4th").
    pub ordinal_suffixes: &'static [&'static str],
    /// "of" in "4th of December".
    pub of: &'static [&'static str],
    /// "on" before a date.
    pub on: &'static [&'static str],
    /// "next" before a weekday (the one a week later).
    pub next: &'static [&'static str],
    /// "this" before a weekday (the coming one).
    pub this: &'static [&'static str],
    /// "in" in "in 3 days".
    pub in_: &'static [&'static str],
    /// "at" before a time or a place.
    pub at: &'static [&'static str],
    /// "from" before a time range.
    pub from: &'static [&'static str],
    /// Words between the ends of a range ("to", "until").
    pub range_to: &'static [&'static str],
    /// Morning / afternoon markers.
    pub am: &'static [&'static str],
    /// Afternoon markers.
    pub pm: &'static [&'static str],
    /// 12:00.
    pub noon: &'static [&'static str],
    /// 00:00.
    pub midnight: &'static [&'static str],
    /// "for" before a length.
    pub for_: &'static [&'static str],
    /// "an hour", "a day": the article that means one.
    pub one: &'static [&'static str],
    /// "half" in "half an hour".
    pub half: &'static [&'static str],
    /// Minute units (words and the compact suffix).
    pub minutes: &'static [&'static str],
    /// Hour units.
    pub hours: &'static [&'static str],
    /// Day units.
    pub days: &'static [&'static str],
    /// Week units.
    pub weeks: &'static [&'static str],
    /// Month units.
    pub months_unit: &'static [&'static str],
    /// Year units.
    pub years: &'static [&'static str],
    /// "all day" (word sequences; a joined form is one word).
    pub all_day: &'static [&'static [&'static str]],
    /// "every".
    pub every: &'static [&'static str],
    /// "other" in "every other week".
    pub other: &'static [&'static str],
    /// "weekday" in "every weekday" (Monday to Friday).
    pub weekday: &'static [&'static str],
    /// "and" between weekdays.
    pub and: &'static [&'static str],
    /// One-word repetitions.
    pub daily: &'static [&'static str],
    /// "weekly".
    pub weekly: &'static [&'static str],
    /// "monthly".
    pub monthly: &'static [&'static str],
    /// "yearly".
    pub yearly: &'static [&'static str],
    /// Regions that write numeric dates day first (`4/12` = 4 December);
    /// the others (and a bare language) write the month first.
    pub day_first_regions: &'static [&'static str],
}

/// English (every region).
pub(crate) static ENGLISH: Keywords = Keywords {
    today: &["today", "tdy"],
    tomorrow: &["tomorrow", "tmrw", "tmr"],
    day_after_tomorrow: &[
        &["the", "day", "after", "tomorrow"],
        &["day", "after", "tomorrow"],
    ],
    weekdays: [
        &["monday", "mon"],
        &["tuesday", "tue", "tues"],
        &["wednesday", "wed", "weds"],
        &["thursday", "thu", "thur", "thurs"],
        &["friday", "fri"],
        &["saturday", "sat"],
        &["sunday", "sun"],
    ],
    ambiguous_weekdays: &["wed", "sat", "sun"],
    months: [
        &["january", "jan"],
        &["february", "feb"],
        &["march", "mar"],
        &["april", "apr"],
        &["may"],
        &["june", "jun"],
        &["july", "jul"],
        &["august", "aug"],
        &["september", "sep", "sept"],
        &["october", "oct"],
        &["november", "nov"],
        &["december", "dec"],
    ],
    ordinal_suffixes: &["st", "nd", "rd", "th"], // spellchecker:disable-line
    of: &["of"],
    on: &["on"],
    next: &["next"],
    this: &["this", "coming"],
    in_: &["in"],
    at: &["at", "@"],
    from: &["from"],
    range_to: &["-", "\u{2013}", "\u{2014}", "to", "until", "till", "til"],
    am: &["am", "a.m", "a"],
    pm: &["pm", "p.m", "p"],
    noon: &["noon", "midday"],
    midnight: &["midnight"],
    for_: &["for"],
    one: &["an", "a", "one"],
    half: &["half"],
    minutes: &["m", "min", "mins", "minute", "minutes"],
    hours: &["h", "hr", "hrs", "hour", "hours"],
    days: &["day", "days"],
    weeks: &["week", "weeks", "wk", "wks"],
    months_unit: &["month", "months"],
    years: &["year", "years", "yr", "yrs"],
    all_day: &[
        &["all", "day"],
        &["all-day"],
        &["allday"],
        &["whole", "day"],
    ],
    every: &["every", "each"],
    other: &["other"],
    weekday: &["weekday", "workday"],
    and: &["and", "&", "+"],
    daily: &["daily"],
    weekly: &["weekly"],
    monthly: &["monthly"],
    yearly: &["yearly", "annually"],
    day_first_regions: &["gb", "au", "nz", "ie", "in", "za"],
};

/// The table of a language (BCP 47 tag; region and script ignored for
/// the choice of table). Unknown languages read as English.
pub(crate) fn for_locale(locale: &str) -> &'static Keywords {
    let _language = language_of(locale);
    // English is the only table in R7; new languages add a `match` arm.
    &ENGLISH
}

/// Whether numeric dates in this locale are written day first.
pub(crate) fn day_first(keywords: &Keywords, locale: &str) -> bool {
    region_of(locale).is_some_and(|r| keywords.day_first_regions.contains(&r.as_str()))
}

fn language_of(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn region_of(locale: &str) -> Option<String> {
    locale
        .split(['-', '_'])
        .skip(1)
        .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_lowercase)
}

/// The weekday at a position of [`Keywords::weekdays`].
pub(crate) const WEEKDAYS: [Weekday; 7] = Weekday::ALL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_pick_the_numeric_date_order() {
        assert!(!day_first(&ENGLISH, "en"));
        assert!(!day_first(&ENGLISH, "en-US"));
        assert!(day_first(&ENGLISH, "en-GB"));
        assert!(day_first(&ENGLISH, "en_au"));
        assert!(!day_first(&ENGLISH, "en-Latn-US"));
        assert!(day_first(&ENGLISH, "en-Latn-GB"));
        assert_eq!(language_of("EN-gb"), "en");
        // Every language reads as English for now.
        assert!(std::ptr::eq(
            for_locale("de-DE"),
            std::ptr::from_ref(&ENGLISH)
        ));
    }

    #[test]
    fn tables_are_lower_case() {
        let k = &ENGLISH;
        let words = k
            .weekdays
            .iter()
            .chain(k.months.iter())
            .flat_map(|w| w.iter())
            .chain(k.today)
            .chain(k.tomorrow);
        for word in words {
            assert_eq!(*word, word.to_lowercase());
        }
    }
}
