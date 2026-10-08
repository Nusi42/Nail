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
    use crate::widgets::testing::parses;

    #[test]
    fn editor_widgets_parse() {
        assert!(parses(&[
            "layout-editor-settings",
            "toolbar-editor-settings",
            "theme-editor id=\"editor\"",
        ]));
        assert!(!parses(&["theme-editor theme=\"frost\""]));
    }
}
