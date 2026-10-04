//! Widgets of M2d (plug-ins, ADR 0050): plug-in panels placed in the layout
//! as `plugin-panel "<plugin>.<panel>"`. Rendered by
//! `ui/src/features/plugins/`.
//!
//! Slot created by WP60 with the panel widget; the plug-in manager
//! widget came with the integration (WP64).

use super::{OptionType, WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // A plug-in panel. The panel is the node's argument
    // (`plugin-panel "follow-up.due"`), stored as the `panel` option;
    // `panel="follow-up.due"` is the same.
    widget(
        "plugin-panel",
        &[("panel", OptionType::Str)],
        &["panel"],
        false,
    ),
    // The plug-in manager (install, enable / disable, grants, settings,
    // logs, placements; ADR 0052). Also the dialog `plugin-manager`.
    widget("plugin-manager", &[], &[], false),
];

/// Prefixes every widget kind of this feature starts with.
#[cfg(test)]
const PREFIXES: &[&str] = &["plugin-"];

#[cfg(test)]
mod tests {
    #[test]
    fn plugin_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(
                super::PREFIXES.iter().any(|p| spec.kind.starts_with(p)),
                "{}",
                spec.kind
            );
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }

    fn layout(panel: &str) -> String {
        format!(
            "layout \"x\" extends=\"default\" {{
    window \"y\" {{
        split \"horizontal\" {{
            message-list
            {panel}
        }}
    }}
}}
"
        )
    }

    fn panel_options(text: &str) -> serde_json::Map<String, serde_json::Value> {
        fn walk(
            node: &nail_api::LayoutNode,
            found: &mut Option<serde_json::Map<String, serde_json::Value>>,
        ) {
            if let nail_api::LayoutNode::Widget { kind, options, .. } = node
                && kind == "plugin-panel"
            {
                *found = Some(options.clone());
            }
            for child in node.children() {
                walk(child, found);
            }
        }
        let file = crate::parse_layout(text, "layout.kdl").unwrap();
        let mut found = None;
        for breakpoints in file.windows.values() {
            for breakpoint in breakpoints {
                walk(&breakpoint.root, &mut found);
            }
        }
        found.expect("a plugin-panel widget")
    }

    #[test]
    fn plugin_panels_take_their_panel_as_argument_or_option() {
        for text in [
            layout("plugin-panel \"follow-up.due\""),
            layout("plugin-panel panel=\"follow-up.due\""),
        ] {
            assert_eq!(panel_options(&text)["panel"], "follow-up.due");
        }
    }

    #[test]
    fn invalid_plugin_panels_are_refused() {
        for bad in [
            "plugin-panel",
            "plugin-panel \"follow-up\"",
            "plugin-panel \"Follow.Up\"",
            "plugin-panel \"a.b\" \"c.d\"",
            "plugin-panel \"a.b\" panel=\"c.d\"",
            "plugin-panel panel=\"nope\"",
        ] {
            assert!(
                crate::parse_layout(&layout(bad), "layout.kdl").is_err(),
                "{bad}"
            );
        }
    }
}
