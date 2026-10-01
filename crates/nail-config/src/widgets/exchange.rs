//! Widgets of M3c WP94 (Exchange accounts; contract ADR 0072). Rendered by
//! `ui/src/features/exchange/`.
//!
//! Slot created by WP90 as future widgets (any options accepted); WP94
//! gives them typed options. Shared mailboxes appear in the existing
//! folder tree, the room finder in the event editor and the
//! out-of-office editor as a dialog, so they need no widget of their own.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Focused / Other tabs over the message list of an Exchange inbox.
    future_widget("focused-tabs", false),
    // A banner while automatic replies are on, with a way to switch them
    // off or edit them.
    future_widget("automatic-replies-banner", false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn exchange_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            automatic-replies-banner
            focused-tabs show-counts=#true
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
