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
    use crate::widgets::testing::parses;

    #[test]
    fn customization_widgets_parse() {
        assert!(parses(&[
            "smart-folders editable=#false",
            "config-errors hide-when-valid=#true"
        ]));
        assert!(!parses(&["smart-folders editable=\"no\""]));
    }
}
