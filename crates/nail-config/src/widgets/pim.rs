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
        ],
        &[],
        false,
    ),
    // A contact list.
    widget(
        "contact-list",
        &[("address-books", Str), ("page-size", UInt)],
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
    #[test]
    fn pim_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
            assert!(spec.options.is_some(), "{} has typed options", spec.kind);
        }
    }

    #[test]
    fn pim_widgets_parse_in_a_layout() {
        let text = "layout \"x\" extends=\"default\" {
    window \"calendar-test\" {
        stack {
            mini-month time-zone=\"Europe/Berlin\"
            calendar-list
            calendar view=\"week\" views=\"day week month\" scroll-to=8 header=#true
            agenda days=3
            task-list sort=\"due\" show-completed=#false
            contact-list page-size=50
            contact-card
            pim-settings
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }

    #[test]
    fn pim_widget_options_are_checked() {
        for bad in [
            "calendar view=\"fortnight\"",
            "task-list sort=\"random\"",
            "agenda colour=\"red\"",
            "calendar scroll-to=\"eight\"",
        ] {
            let text = format!(
                "layout \"x\" extends=\"default\" {{\n    window \"w\" {{\n        {bad}\n    }}\n}}\n"
            );
            assert!(
                crate::parse_layout(&text, "layout.kdl").is_err(),
                "{bad} should be refused"
            );
        }
    }
}
