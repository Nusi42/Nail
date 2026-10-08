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
    use crate::widgets::testing::parses;

    #[test]
    fn scheduling_widgets_parse() {
        assert!(parses(&[
            "invitation-card attendees=\"expanded\" conflicts=#true",
            "freebusy-timeline hour-start=8 hour-end=18 duration=30 time-zone=\"Europe/Berlin\"",
        ]));
    }
}
