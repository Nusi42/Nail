//! Widgets of M3a WP74 (calendars, contacts and tasks; contract ADR 0058).
//! Rendered by `ui/src/features/pim/`.
//!
//! Slot created by WP70 with the PIM widget kinds design.md names, still
//! as future widgets (any options accepted); WP74 gives them typed options
//! (calendar views, mini-month, agenda, task list, contact list / card)
//! and adds the editors, dialogs and settings widgets it needs.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // A calendar view (day, multi-day, week, work week, month, year,
    // agenda, free-time timeline).
    future_widget("calendar", false),
    // Today's (or the next days') agenda.
    future_widget("agenda", true),
    // A small month for navigation.
    future_widget("mini-month", false),
    // A task list.
    future_widget("task-list", true),
    // A contact list.
    future_widget("contact-list", true),
    // A contact's card.
    future_widget("contact-card", false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn pim_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }

    #[test]
    fn pim_widgets_parse_in_a_layout() {
        let text = "layout \"x\" extends=\"default\" {
    window \"calendar\" {
        stack {
            mini-month
            calendar
            task-list
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
