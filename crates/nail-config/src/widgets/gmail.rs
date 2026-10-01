//! Widgets of M3b WP84 (Gmail labels and categories; contract ADR 0065).
//! Rendered by `ui/src/features/gmail/`.
//!
//! Slot created by WP80 as future widgets (any options accepted); WP84
//! gives them typed options.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // The labels of the selected Gmail account (label-based views,
    // management).
    future_widget("label-list", true),
    // The inbox categories as a filter (Primary, Social, …).
    future_widget("category-tabs", false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn gmail_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            label-list
            category-tabs
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
