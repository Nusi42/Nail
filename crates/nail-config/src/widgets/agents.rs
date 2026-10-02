//! Widgets of M4a WP104 (the confirmation center for writes by AI agents
//! and plug-ins; contract ADR 0079). Rendered by `ui/src/features/agents/`.
//!
//! Slot created by WP100 as future widgets (any options accepted); WP104
//! gives them typed options. Plug-in banners / badges, composer actions
//! and attachment previews are mounted inside existing widgets (reader,
//! composer), so they need no widget of their own.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Pending confirmations with approve / deny / always allow, and the
    // history (a settings tab, a side panel or its own window).
    future_widget("confirmation-center", false),
    // A small indicator (status bar, toolbar) with the number of pending
    // confirmations; opens the center.
    future_widget("confirmation-indicator", false),
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
            confirmation-indicator
            confirmation-center show-history=#true
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
