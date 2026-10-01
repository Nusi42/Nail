//! Widgets of M3b WP84 (invitations and scheduling; contract ADR 0065).
//! Rendered by `ui/src/features/scheduling/`.
//!
//! Neither takes `row` children; the options are listed in
//! `userDocs/en/invitations-and-gmail-labels.md#layout-options`.

use super::OptionType::{Bool, OneOf, Str, UInt};
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // The invitation card of the open message (method, organizer,
    // attendees, answer buttons, conflicts).
    widget(
        "invitation-card",
        &[
            ("attendees", OneOf(&["collapsed", "expanded"])),
            ("conflicts", Bool),
        ],
        &[],
        false,
    ),
    // The attendee picker's free/busy timeline.
    widget(
        "freebusy-timeline",
        &[
            ("hour-start", UInt),
            ("hour-end", UInt),
            ("duration", UInt),
            ("time-zone", Str),
        ],
        &[],
        false,
    ),
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
            invitation-card attendees=\"expanded\" conflicts=#true
            freebusy-timeline hour-start=8 hour-end=18 duration=30 time-zone=\"Europe/Berlin\"
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
