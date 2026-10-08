//! Widgets of M1b WP23 (MCP, ADR 0025).

use super::{WidgetSpec, widget};

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
    use crate::widgets::testing::parses;

    #[test]
    fn mcp_widgets_parse() {
        assert!(parses(&["mcp-clients", "mcp-context-indicator"]));
        assert!(!parses(&["mcp-clients bogus=#true"]));
    }
}
