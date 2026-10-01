//! Widgets of M3b WP84 (invitations and scheduling; contract ADR 0065).
//! Rendered by `ui/src/features/scheduling/`.
//!
//! Slot created by WP80 with the kinds the plan names, still as future
//! widgets (any options accepted); WP84 gives them typed options and adds
//! the dialogs and settings widgets it needs.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // The invitation card of the open message (method, organizer,
    // attendees, answer buttons, conflicts).
    future_widget("invitation-card", false),
    // The attendee picker's free/busy timeline.
    future_widget("freebusy-timeline", true),
];

#[cfg(test)]
mod tests {
    #[test]
    fn scheduling_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            invitation-card
            freebusy-timeline
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
