//! Widgets of M3b WP84 (Gmail labels and categories; contract ADR 0065).
//! Rendered by `ui/src/features/gmail/`.
//!
//! Neither takes `row` children; the options are listed in
//! `userDocs/en/invitations-and-gmail-labels.md#layout-options`.

use super::OptionType::{Bool, Str};
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // The labels of the selected Gmail account (label-based views,
    // management).
    widget(
        "label-list",
        &[("show-hidden", Bool), ("show-counts", Bool)],
        &[],
        false,
    ),
    // The inbox categories as a filter (Primary, Social, …).
    // `categories` = space-separated names from primary / social /
    // promotions / updates / forums.
    widget(
        "category-tabs",
        &[
            ("categories", Str),
            ("show-all", Bool),
            ("show-counts", Bool),
        ],
        &[],
        false,
    ),
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
            label-list show-hidden=#false show-counts=#true
            category-tabs categories=\"primary social\" show-all=#true show-counts=#false
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
