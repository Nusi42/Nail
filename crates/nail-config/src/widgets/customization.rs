//! Widgets of M1b WP24 (customization). Only that package edits this table (ADR 0022).

use super::{OptionType, WidgetSpec, widget};

/// Widgets of this feature (React components in
/// `ui/src/features/customization/index.ts`).
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Every error of every configuration file, with "open file at line".
    widget(
        "config-errors",
        &[("hide-when-valid", OptionType::Bool)],
        &[],
        false,
    ),
    // Saved searches (smart folders); `editable=#false` hides save / remove.
    widget(
        "smart-folders",
        &[("editable", OptionType::Bool)],
        &[],
        false,
    ),
    // M2c WP54: the Customize tab's editors (toolbar order, key bindings,
    // message-list columns), written back through the layout patches.
    widget("toolbar-editor", &[], &[], false),
    widget("key-binding-editor", &[], &[], false),
    widget("column-editor", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn customization_widgets_are_registered() {
        let text = "layout \"x\" extends=\"default\" {\n    window \"main\" {\n        stack {\n            smart-folders editable=#false\n            config-errors hide-when-valid=#true\n        }\n    }\n}\n";
        crate::parse_layout(text, "layout.kdl").unwrap();
        let bad = text.replace("editable=#false", "editable=\"no\"");
        assert!(crate::parse_layout(&bad, "layout.kdl").is_err());
    }
}
