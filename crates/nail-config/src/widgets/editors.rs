//! Widgets of M5d (the in-app editors; contract ADR 0131), registered by the
//! contract so the parallel packages never edit this table:
//!
//! * `layout-editor-settings` — WP181: the "Layout & toolbars" settings card
//!   (Customize layout, the presets, Reset to default). Rendered by
//!   `ui/src/features/layout-editor/`.
//! * `toolbar-editor-settings` — WP182: the toolbars with Customize / Reset
//!   per toolbar. Rendered by `ui/src/features/toolbar-editor/`.
//! * `theme-editor` — WP183: the full theme editor (token groups, part
//!   overrides, live preview), for a layout window or settings section that
//!   shows it. Rendered by `ui/src/features/appearance/`.
//!
//! None takes options or `row` children yet; a package that needs one asks
//! the integration (WP185) for it.

use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget("layout-editor-settings", &[], &[], false),
    widget("toolbar-editor-settings", &[], &[], false),
    widget("theme-editor", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn editor_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::widgets::find(spec.kind).is_some(), "{}", spec.kind);
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            layout-editor-settings
            toolbar-editor-settings
            theme-editor id=\"editor\"
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
        let bad = "layout \"x\" extends=\"default\" {\n    window \"main\" {\n        theme-editor theme=\"frost\"\n    }\n}\n";
        assert!(crate::parse_layout(bad, "layout.kdl").is_err());
    }
}
