//! Widgets of M4a WP104 (the confirmation center for writes by AI agents
//! and plug-ins; contract ADR 0079, UI ADR 0083). Rendered by
//! `ui/src/features/agents/`.
//!
//! Neither takes `row` children; the options are listed in
//! `userDocs/en/confirmations.md#layout-options`. Plug-in banners / badges,
//! composer actions and attachment previews are mounted inside existing
//! widgets (reader, composer), so they need no widget of their own.

use super::OptionType::Bool;
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Pending confirmations with approve / deny / always allow, and the
    // history (a settings tab, a side panel or its own window).
    // `show-history` (default #true).
    widget("confirmation-center", &[("show-history", Bool)], &[], false),
    // A small indicator (status bar, toolbar) with the number of pending
    // confirmations; opens the center. `hide-when-empty` (default #true).
    widget(
        "confirmation-indicator",
        &[("hide-when-empty", Bool)],
        &[],
        false,
    ),
];

#[cfg(test)]
mod tests {
    use crate::widgets::testing::parses;

    #[test]
    fn agents_widgets_parse() {
        assert!(parses(&[
            "confirmation-indicator hide-when-empty=#false",
            "confirmation-center show-history=#true",
        ]));
        assert!(!parses(&["confirmation-center show-counts=#true"]));
    }
}
