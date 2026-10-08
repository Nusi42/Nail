//! Widgets of M3a WP74 (calendars, contacts and tasks; contract ADR 0058,
//! UI ADR 0062). Rendered by `ui/src/features/pim/`.
//!
//! None of them takes `row` children; options are the views' settings
//! (`userDocs/en/calendar-contacts-tasks.md#layout-options`).

use super::OptionType::{Bool, OneOf, Str, UInt};
use super::{WidgetSpec, widget};

/// The calendar views (`view` / `views` of the `calendar` widget).
pub(crate) const VIEWS: &[&str] = &[
    "day",
    "multi-day",
    "week",
    "work-week",
    "month",
    "year",
    "agenda",
    "free-time",
];

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // A calendar view (day, multi-day, week, work week, month, year,
    // agenda, free-time timeline).
    widget(
        "calendar",
        &[
            ("view", OneOf(VIEWS)),
            ("views", Str),
            ("days", UInt),
            ("calendars", Str),
            ("time-zone", Str),
            ("secondary-zone", Str),
            ("hour-start", UInt),
            ("hour-end", UInt),
            ("scroll-to", UInt),
            ("max-events", UInt),
            ("duration", UInt),
            ("work-start", UInt),
            ("work-end", UInt),
            ("header", Bool),
            // M5b: the view switch in the header.
            ("view-switch", OneOf(&["segmented", "menu"])),
        ],
        &[],
        false,
    ),
    // Today's (or the next days') agenda.
    widget(
        "agenda",
        &[("days", UInt), ("time-zone", Str), ("calendars", Str)],
        &[],
        false,
    ),
    // A small month for navigation.
    widget(
        "mini-month",
        &[("time-zone", Str), ("calendars", Str)],
        &[],
        false,
    ),
    // A task list.
    widget(
        "task-list",
        &[
            ("lists", Str),
            ("show-completed", Bool),
            ("sort", OneOf(&["due", "priority", "title", "manual"])),
            ("quick-add", Bool),
            // M5b review: the Tasks view's header (title, "New task").
            ("header", Bool),
        ],
        &[],
        false,
    ),
    // A contact list.
    widget(
        "contact-list",
        // `header` (M5b review): the Contacts view's header (title, search,
        // "New contact", "More").
        &[
            ("address-books", Str),
            ("page-size", UInt),
            ("header", Bool),
        ],
        &[],
        false,
    ),
    // A contact's card.
    widget("contact-card", &[], &[], false),
    // Calendars and task lists by source (show / hide, colour, new).
    // M5b: per-calendar actions in a "…" menu (default) or as buttons;
    // headers per source (account / subscriptions).
    widget(
        "calendar-list",
        &[
            ("actions", OneOf(&["menu", "buttons"])),
            ("show-sources", Bool),
        ],
        &[],
        false,
    ),
    // M5b: the settings page's "Calendar & contacts" section (WP164).
    widget("pim-settings", &[], &[], false),
];

#[cfg(test)]
mod tests {
    use crate::widgets::testing::parses;

    #[test]
    fn pim_widgets_parse() {
        assert!(parses(&[
            "mini-month time-zone=\"Europe/Berlin\"",
            "calendar-list",
            "calendar view=\"week\" views=\"day week month\" scroll-to=8 header=#true",
            "agenda days=3",
            "task-list sort=\"due\" show-completed=#false header=#true",
            "contact-list page-size=50 header=#true",
            "contact-card",
            "pim-settings",
        ]));
    }

    #[test]
    fn pim_widget_options_are_checked() {
        for bad in [
            "calendar view=\"fortnight\"",
            "task-list sort=\"random\"",
            "agenda colour=\"red\"",
            "calendar scroll-to=\"eight\"",
        ] {
            assert!(!parses(&[bad]), "{bad} should be refused");
        }
    }
}
