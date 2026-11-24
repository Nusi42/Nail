//! The calendar preferences and quick create from text (R7, ADR 0246).
//!
//! The preferences live in the app settings under
//! [`CALENDAR_PREFERENCES_KEY`] (defaults until first saved); saving them
//! announces [`Event::CalendarPreferencesChanged`]. Text is read by
//! `nail-event-text` against "now" in the request's zone.

use chrono::{DateTime, Utc};
use nail_api::pim::calendar_view::MAX_EVENT_TEXT;
use nail_api::pim::{CalendarPreferences, ParsedEventText};
use nail_api::{ApiError, Event};

use super::{check, request_zone};
use crate::error::Result;
use crate::service::Service;

/// The app-settings key of [`CalendarPreferences`].
pub(crate) const CALENDAR_PREFERENCES_KEY: &str = "calendar-preferences";

/// Language of the keywords when the request names none.
const DEFAULT_TEXT_LOCALE: &str = "en";

impl Service {
    /// The calendar preferences (defaults when never set).
    pub(crate) async fn calendar_preferences(&self) -> Result<CalendarPreferences> {
        Ok(self
            .store()
            .call(|s| s.setting::<CalendarPreferences>(CALENDAR_PREFERENCES_KEY))
            .await?
            .unwrap_or_default())
    }

    /// Checks and stores the calendar preferences, then announces them.
    pub(crate) async fn set_calendar_preferences(
        &self,
        preferences: CalendarPreferences,
    ) -> Result<CalendarPreferences> {
        check(&preferences.issues())?;
        for zone in [&preferences.time_zone, &preferences.second_time_zone]
            .into_iter()
            .flatten()
        {
            request_zone(zone)?;
        }
        let stored = preferences.clone();
        self.store()
            .call(move |s| s.set_setting(CALENDAR_PREFERENCES_KEY, &stored))
            .await?;
        self.broadcast(Event::CalendarPreferencesChanged);
        Ok(preferences)
    }
}

/// Answers `ParseEventText`: checks the text and the zone, places "now" in
/// the zone and lets `nail-event-text` read the text.
pub(super) fn parse_event_text(
    text: &str,
    tz: &str,
    now: Option<&str>,
    locale: Option<&str>,
) -> Result<ParsedEventText> {
    if text.chars().count() > MAX_EVENT_TEXT {
        return Err(ApiError::invalid_request(format!(
            "the text must be at most {MAX_EVENT_TEXT} characters"
        ))
        .into());
    }
    let zone = request_zone(tz)?;
    let now = match now {
        Some(text) => DateTime::parse_from_rfc3339(text)
            .map_err(|_| ApiError::invalid_request(format!("invalid time {text:?}")))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let context = nail_event_text::Context {
        now: now.with_timezone(&zone).naive_local(),
        tz: tz.to_owned(),
        locale: locale
            .filter(|l| !l.trim().is_empty())
            .unwrap_or(DEFAULT_TEXT_LOCALE)
            .to_owned(),
    };
    Ok(nail_event_text::parse(text, &context))
}
