//! Nail's MCP server (M1b WP23; stub after WP20).
//!
//! Expected shape (ADR 0022): an [`rmcp`] server over Streamable HTTP on
//! `127.0.0.1` with per-client bearer tokens (only hashes are stored), a
//! stdio bridge for `nail mcp --client <name>`, the design's read-only
//! resources and tools, resource subscriptions (new mail, UI context),
//! grants checked on every call and an audit entry per call.

#![warn(missing_docs)]

#[cfg(test)]
mod tests {
    #[test]
    fn rmcp_server_types_are_available() {
        let info = rmcp::model::ServerConfig::default();
        let value = serde_json::to_value(&info).unwrap();
        assert!(value.get("protocolVersion").is_some(), "{value}");
    }
}
