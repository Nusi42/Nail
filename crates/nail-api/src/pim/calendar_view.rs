//! How the calendar reads (R7, ADR 0246): the colour palette and the rule
//! that gives new and discovered calendars distinct colours, the user's
//! calendar preferences (working hours, work days, week start, week
//! numbers, view zones), and the result of quick create from text
//! ([`crate::Request::ParseEventText`]).
//!
//! The UI renders these facts; it never decides them on its own (the
//! default colour, the working day, what a sentence means). Every value
//! the UI needs as a constant is exported to TypeScript
//! (`crate::ts_constants`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::calendar::Recurrence;
use super::{PimIssue, PimTime, is_time_zone_name, push_issue};
use crate::triage::Weekday;

// ---------------------------------------------------------------------------
// Calendar colours

/// The palette of calendar colours (the calendar list's "…" menu, the
/// calendar dialogs, and the defaults the core assigns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarColor {
    /// Blue.
    Blue,
    /// Cyan.
    Cyan,
    /// Green.
    Green,
    /// Amber.
    Amber,
    /// Orange.
    Orange,
    /// Red.
    Red,
    /// Pink.
    Pink,
    /// Purple.
    Purple,
    /// Slate.
    Slate,
}

impl CalendarColor {
    /// Every colour, in palette order (the order defaults are handed out).
    pub const ALL: [CalendarColor; 9] = [
        Self::Blue,
        Self::Cyan,
        Self::Green,
        Self::Amber,
        Self::Orange,
        Self::Red,
        Self::Pink,
        Self::Purple,
        Self::Slate,
    ];

    /// The colour's value (`#rrggbb`, lower case), as stored in
    /// [`super::CalendarSettings::color`].
    pub fn value(self) -> &'static str {
        match self {
            Self::Blue => "#2563eb",
            Self::Cyan => "#0891b2",
            Self::Green => "#16a34a",
            Self::Amber => "#ca8a04",
            Self::Orange => "#ea580c",
            Self::Red => "#dc2626",
            Self::Pink => "#db2777",
            Self::Purple => "#9333ea",
            Self::Slate => "#64748b",
        }
    }

    /// The palette colour with this value (case-insensitive), if any.
    pub fn of_value(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL
            .into_iter()
            .find(|c| c.value().eq_ignore_ascii_case(value))
    }
}

/// One palette entry as the UI sees it (`CALENDAR_PALETTE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaletteColor {
    /// The colour's id (`pim-color-<id>` names it).
    pub id: CalendarColor,
    /// Its value (`#rrggbb`).
    pub value: String,
}

/// The palette in order, for the UI.
pub fn calendar_palette() -> Vec<PaletteColor> {
    CalendarColor::ALL
        .into_iter()
        .map(|id| PaletteColor {
            id,
            value: id.value().to_owned(),
        })
        .collect()
}

/// The default colour of a new or discovered calendar that has none (no
/// colour chosen, no CalDAV `calendar-color`, no Graph `hexColor`), given
/// the colours the user's other calendars already have: the first palette
/// colour nobody uses; when every one is taken, the least used one (the
/// earliest in palette order on a tie). Values outside the palette count
/// for nothing. Deterministic, so a test (and every client) gets the same
/// colour for the same calendars.
pub fn next_calendar_color<'a>(used: impl IntoIterator<Item = &'a str>) -> CalendarColor {
    let mut counts = [0_usize; CalendarColor::ALL.len()];
    for value in used {
        if let Some(color) = CalendarColor::of_value(value)
            && let Some(slot) = CalendarColor::ALL.iter().position(|c| *c == color)
            && let Some(count) = counts.get_mut(slot)
        {
            *count += 1;
        }
    }
    let least = counts.iter().copied().min().unwrap_or(0);
    CalendarColor::ALL
        .into_iter()
        .zip(counts)
        .find(|(_, count)| *count == least)
        .map_or(CalendarColor::Blue, |(color, _)| color)
}

// ---------------------------------------------------------------------------
// Preferences

/// Minutes in a day: the end of [`WorkHours`] may be this (midnight).
pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// The working day, in minutes after midnight in the view's zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct WorkHours {
    /// First working minute (default 480 = 8:00).
    pub start_minute: u16,
    /// End of the working day, exclusive (default 1080 = 18:00); after
    /// `start_minute`, at most [`MINUTES_PER_DAY`].
    pub end_minute: u16,
}

impl Default for WorkHours {
    fn default() -> Self {
        Self {
            start_minute: 8 * 60,
            end_minute: 18 * 60,
        }
    }
}

impl WorkHours {
    /// Whether the hours are usable (start before end, end by midnight).
    pub fn is_valid(self) -> bool {
        self.start_minute < self.end_minute && self.end_minute <= MINUTES_PER_DAY
    }
}

/// The user's calendar preferences, kept in the app settings
/// ([`crate::Request::GetCalendarPreferences`]). The calendar views shade
/// time outside the working hours and days, number the weeks, start the
/// week and place events in the view zone from here; the free-time view
/// and scheduling use the same working day. A layout option of a calendar
/// widget (`time-zone`, `secondary-zone`, `work-start`, `work-end`) still
/// wins for that widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct CalendarPreferences {
    /// The working day.
    pub work_hours: WorkHours,
    /// Working days (default Monday to Friday); empty = none (every day is
    /// shaded).
    pub work_days: Vec<Weekday>,
    /// First day of the week; `null` = as the UI locale says.
    pub week_start: Option<Weekday>,
    /// Show week numbers (ISO 8601) in the views and the mini-month.
    pub show_week_numbers: bool,
    /// The zone the views show; `null` = the system's.
    pub time_zone: Option<String>,
    /// A second zone shown in the time grid's gutter; `null` = none.
    pub second_time_zone: Option<String>,
    /// Show events the user declined (struck through); else they are left
    /// out of the views.
    pub show_declined: bool,
}

impl Default for CalendarPreferences {
    fn default() -> Self {
        Self {
            work_hours: WorkHours::default(),
            work_days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ],
            week_start: None,
            show_week_numbers: false,
            time_zone: None,
            second_time_zone: None,
            show_declined: true,
        }
    }
}

/// Which hours of each day [`crate::Request::FindFreeTime`] searches (R8,
/// ADR 0251).
///
/// With any value other than [`FreeTimeHours::Any`] the core cuts every
/// free stretch to one day's hours on one of the days, in the request's
/// zone (summer-time days included), and drops pieces shorter than the
/// request's length: the UI shows the slots as they come, without cutting
/// them per day.
///
/// JSON: `{"kind": "any"}`, `{"kind": "working-hours"}` or
/// `{"kind": "custom", "work_hours": {…}, "work_days": ["mon", …]}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FreeTimeHours {
    /// Around the clock on every day: a stretch may span several days
    /// (the default; older clients).
    #[default]
    Any,
    /// The working hours on the work days of the calendar preferences
    /// ([`CalendarPreferences::work_hours`] / [`CalendarPreferences::work_days`]):
    /// the same working day the views shade.
    WorkingHours,
    /// The given hours on the given days (the MCP tool's `day_start` /
    /// `day_end`, a widget's own working hours).
    Custom {
        /// The hours of each day.
        work_hours: WorkHours,
        /// The days; empty = none (no slots).
        work_days: Vec<Weekday>,
    },
}

impl FreeTimeHours {
    /// Problems (empty = valid): custom hours out of order or past
    /// midnight, a day listed twice.
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if let Self::Custom {
            work_hours,
            work_days,
        } = self
        {
            if !work_hours.is_valid() {
                push_issue(&mut out, PimIssue::InvalidWorkHours);
            }
            if has_repeated_day(work_days) {
                push_issue(&mut out, PimIssue::TooMany);
            }
        }
        out
    }

    /// The hours and days to cut to, given the preferences; `None` for
    /// [`FreeTimeHours::Any`] (no cutting).
    pub fn resolve(&self, preferences: &CalendarPreferences) -> Option<(WorkHours, Vec<Weekday>)> {
        match self {
            Self::Any => None,
            Self::WorkingHours => Some((preferences.work_hours, preferences.work_days.clone())),
            Self::Custom {
                work_hours,
                work_days,
            } => Some((*work_hours, work_days.clone())),
        }
    }
}

/// Whether a day is listed more than once.
fn has_repeated_day(days: &[Weekday]) -> bool {
    let mut sorted = days.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted.len() != days.len()
}

impl CalendarPreferences {
    /// Problems with the preferences (empty = valid): working hours out of
    /// order or past midnight, a work day listed twice, a zone that is not
    /// an IANA name (the core also checks that it knows the zone).
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if !self.work_hours.is_valid() {
            push_issue(&mut out, PimIssue::InvalidWorkHours);
        }
        if has_repeated_day(&self.work_days) {
            push_issue(&mut out, PimIssue::TooMany);
        }
        for zone in [&self.time_zone, &self.second_time_zone]
            .into_iter()
            .flatten()
        {
            if !is_time_zone_name(zone) {
                push_issue(&mut out, PimIssue::InvalidTimeZone);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Quick create from text

/// Longest text [`crate::Request::ParseEventText`] reads (characters);
/// longer text is refused with `invalid_request`.
pub const MAX_EVENT_TEXT: usize = 500;

/// Length of a new event when only its start is known (minutes): quick
/// create from text, a click on the time grid, "New event".
pub const DEFAULT_EVENT_MINUTES: u32 = 60;

/// What a part of the parsed text was read as (the UI marks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ParsedPart {
    /// A day ("tomorrow", "Fri", "Dec 4").
    Date,
    /// A time or a time range ("12:30", "3-4pm").
    Time,
    /// A length ("for 2 hours").
    Duration,
    /// A repetition ("every Monday").
    Recurrence,
    /// A place ("at Corner café").
    Location,
    /// "all day".
    AllDay,
}

/// One recognised part of the text: `[start, end)` in UTF-16 code units
/// (JavaScript string indices), so the UI can mark it without converting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ParsedSpan {
    /// What the part means.
    pub kind: ParsedPart,
    /// First code unit.
    pub start: u32,
    /// One past the last code unit.
    pub end: u32,
}

/// What a sentence describes ([`crate::Request::ParseEventText`]). Nothing
/// is saved: the UI shows it as a preview and creates the event with
/// [`crate::Request::CreateEvent`] (or opens the editor with it).
///
/// `start` / `end` are both set or both `null` (nothing about time was
/// found; the UI keeps its own default). A date without a time is an
/// all-day event (`end` the next day, exclusive); a start without an end or
/// a length lasts [`DEFAULT_EVENT_MINUTES`]. Times are zoned in the
/// request's zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ParsedEventText {
    /// The text without the recognised parts, trimmed (may be empty).
    pub title: String,
    /// Start.
    pub start: Option<PimTime>,
    /// End.
    pub end: Option<PimTime>,
    /// The place ("at …"), if any.
    pub location: Option<String>,
    /// The repetition, if any.
    pub recurrence: Option<Recurrence>,
    /// The recognised parts, in text order, not overlapping.
    pub spans: Vec<ParsedSpan>,
}

impl ParsedEventText {
    /// Nothing recognised: the whole (trimmed) text is the title.
    pub fn title_only(text: &str) -> Self {
        Self {
            title: text.trim().to_owned(),
            start: None,
            end: None,
            location: None,
            recurrence: None,
            spans: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn palette_values_are_distinct_colours_in_order() {
        let palette = calendar_palette();
        assert_eq!(palette.len(), CalendarColor::ALL.len());
        assert_eq!(palette[0].value, "#2563eb");
        let mut values: Vec<_> = palette.iter().map(|c| c.value.clone()).collect();
        assert!(values.iter().all(|v| super::super::is_color(v)));
        values.sort();
        values.dedup();
        assert_eq!(values.len(), CalendarColor::ALL.len());
        assert_eq!(
            serde_json::to_value(&palette[8]).unwrap(),
            json!({"id": "slate", "value": "#64748b"})
        );
        assert_eq!(
            CalendarColor::of_value(" #2563EB "),
            Some(CalendarColor::Blue)
        );
        assert_eq!(CalendarColor::of_value("#000000"), None);
    }

    #[test]
    fn next_colour_is_the_first_unused_then_the_least_used() {
        assert_eq!(next_calendar_color([]), CalendarColor::Blue);
        assert_eq!(next_calendar_color(["#2563eb"]), CalendarColor::Cyan);
        // Unknown values and case do not matter.
        assert_eq!(
            next_calendar_color(["#2563EB", "tomato", "#0891b2"]),
            CalendarColor::Green
        );
        let all: Vec<&str> = CalendarColor::ALL.iter().map(|c| c.value()).collect();
        assert_eq!(
            next_calendar_color(all.iter().copied()),
            CalendarColor::Blue
        );
        let mut twice = all.clone();
        twice.extend(["#2563eb", "#0891b2"]);
        assert_eq!(next_calendar_color(twice), CalendarColor::Green);
    }

    #[test]
    fn preferences_default_and_wire_shape() {
        let prefs: CalendarPreferences = serde_json::from_value(json!({})).unwrap();
        assert_eq!(prefs, CalendarPreferences::default());
        assert_eq!(prefs.issues(), []);
        let value = serde_json::to_value(&prefs).unwrap();
        assert_eq!(
            value["work_hours"],
            json!({"start_minute": 480, "end_minute": 1080})
        );
        assert_eq!(
            value["work_days"],
            json!(["mon", "tue", "wed", "thu", "fri"])
        );
        assert_eq!(value["week_start"], json!(null));
        assert_eq!(value["show_week_numbers"], json!(false));
        assert_eq!(value["show_declined"], json!(true));
        // Older settings without a field keep the default for it.
        let partial: CalendarPreferences =
            serde_json::from_value(json!({"show_week_numbers": true, "week_start": "mon"}))
                .unwrap();
        assert!(partial.show_week_numbers);
        assert_eq!(partial.week_start, Some(Weekday::Mon));
        assert_eq!(partial.work_hours, WorkHours::default());
    }

    #[test]
    fn preferences_issues() {
        let mut prefs = CalendarPreferences {
            work_hours: WorkHours {
                start_minute: 600,
                end_minute: 600,
            },
            ..CalendarPreferences::default()
        };
        assert_eq!(prefs.issues(), [PimIssue::InvalidWorkHours]);
        prefs.work_hours = WorkHours {
            start_minute: 0,
            end_minute: MINUTES_PER_DAY,
        };
        assert_eq!(prefs.issues(), []);
        prefs.work_hours.end_minute = MINUTES_PER_DAY + 1;
        assert_eq!(prefs.issues(), [PimIssue::InvalidWorkHours]);
        let prefs = CalendarPreferences {
            work_days: vec![Weekday::Mon, Weekday::Mon],
            time_zone: Some("not a zone".into()),
            second_time_zone: Some("Europe/Berlin".into()),
            ..CalendarPreferences::default()
        };
        assert_eq!(
            prefs.issues(),
            [PimIssue::TooMany, PimIssue::InvalidTimeZone]
        );
        let none = CalendarPreferences {
            work_days: Vec::new(),
            ..CalendarPreferences::default()
        };
        assert_eq!(none.issues(), []);
    }

    #[test]
    fn free_time_hours_wire_shape_and_issues() {
        assert_eq!(
            serde_json::to_value(FreeTimeHours::WorkingHours).unwrap(),
            json!({"kind": "working-hours"})
        );
        let custom = FreeTimeHours::Custom {
            work_hours: WorkHours {
                start_minute: 540,
                end_minute: 1020,
            },
            work_days: vec![Weekday::Mon, Weekday::Tue],
        };
        assert_eq!(
            serde_json::to_value(&custom).unwrap(),
            json!({
                "kind": "custom",
                "work_hours": {"start_minute": 540, "end_minute": 1020},
                "work_days": ["mon", "tue"]
            })
        );
        assert_eq!(custom.issues(), []);
        let prefs = CalendarPreferences::default();
        assert_eq!(FreeTimeHours::Any.resolve(&prefs), None);
        assert_eq!(
            FreeTimeHours::WorkingHours.resolve(&prefs),
            Some((prefs.work_hours, prefs.work_days.clone()))
        );
        assert_eq!(
            custom.resolve(&prefs).map(|(h, _)| h.start_minute),
            Some(540)
        );
        let bad = FreeTimeHours::Custom {
            work_hours: WorkHours {
                start_minute: 600,
                end_minute: 540,
            },
            work_days: vec![Weekday::Mon, Weekday::Mon],
        };
        assert_eq!(
            bad.issues(),
            [PimIssue::InvalidWorkHours, PimIssue::TooMany]
        );
        // A request without `hours` searches around the clock.
        let request: crate::Request = serde_json::from_value(json!({
            "type": "FindFreeTime",
            "from": "2026-11-23T00:00:00Z", "to": "2026-11-30T00:00:00Z",
            "duration_minutes": 30, "tz": "UTC"
        }))
        .unwrap();
        assert!(matches!(
            request,
            crate::Request::FindFreeTime {
                hours: FreeTimeHours::Any,
                ..
            }
        ));
    }

    #[test]
    fn requests_leave_optional_fields_out() {
        let parse: crate::Request = serde_json::from_value(json!({
            "type": "ParseEventText", "text": "Lunch tomorrow", "tz": "UTC"
        }))
        .unwrap();
        assert!(matches!(
            parse,
            crate::Request::ParseEventText {
                now: None,
                locale: None,
                ..
            }
        ));
        let due: crate::Request = serde_json::from_value(json!({
            "type": "ListTasksDue",
            "from": "2026-11-23T00:00:00Z", "to": "2026-11-30T00:00:00Z", "tz": "UTC"
        }))
        .unwrap();
        assert!(matches!(
            due,
            crate::Request::ListTasksDue {
                calendar_ids: None,
                include_completed: false,
                ..
            }
        ));
        assert_eq!(
            serde_json::to_value(crate::Request::GetCalendarPreferences).unwrap(),
            json!({"type": "GetCalendarPreferences"})
        );
    }

    #[test]
    fn parsed_text_wire_shape() {
        let parsed = ParsedEventText {
            title: "Lunch with Bob".into(),
            start: Some(PimTime::zoned("2026-11-26T12:30:00", "Europe/Berlin")),
            end: Some(PimTime::zoned("2026-11-26T13:30:00", "Europe/Berlin")),
            location: Some("Corner café".into()),
            recurrence: None,
            spans: vec![ParsedSpan {
                kind: ParsedPart::AllDay,
                start: 15,
                end: 22,
            }],
        };
        let value = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            value["spans"][0],
            json!({"kind": "all-day", "start": 15, "end": 22})
        );
        assert_eq!(value["start"]["tz"], "Europe/Berlin");
        assert_eq!(
            serde_json::from_value::<ParsedEventText>(value).unwrap(),
            parsed
        );
        let plain = ParsedEventText::title_only("  Call mum ");
        assert_eq!(plain.title, "Call mum");
        assert!(plain.start.is_none() && plain.spans.is_empty());
    }
}
