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
    #[test]
    fn agents_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            confirmation-indicator hide-when-empty=#false
            confirmation-center show-history=#true
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }

    #[test]
    fn unknown_options_are_refused() {
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            confirmation-center show-counts=#true
        }
    }
}
";
        assert!(crate::parse_layout(text, "layout.kdl").is_err());
    }
}
