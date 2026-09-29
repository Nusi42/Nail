//! Widgets of M1b WP23 (MCP, ADR 0025).

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature.
///
/// `mcp-clients`: MCP client settings (tokens, grants, revoke, audit log);
/// `mcp-context-indicator`: the on-screen indicator shown while an agent
/// reads the UI context (`ui/src/features/mcp/`).
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget("mcp-clients", &[], &[], false),
    widget("mcp-context-indicator", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn mcp_widgets_are_registered() {
        let text = "layout \"x\" extends=\"default\" {\n    window \"main\" {\n        stack {\n            mcp-clients\n            mcp-context-indicator\n        }\n    }\n}\n";
        crate::parse_layout(text, "layout.kdl").unwrap();
        let bad = text.replace("mcp-clients", "mcp-clients bogus=#true");
        assert!(crate::parse_layout(&bad, "layout.kdl").is_err());
    }
}
