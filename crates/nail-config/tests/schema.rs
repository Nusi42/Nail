//! JSON Schema snapshots of `theme.kdl` and `layout.kdl`.
//!
//! The checked-in files `crates/nail-config/schema/*.schema.json` are what
//! editors and `nail config schema` use. Regenerate them after changing the
//! file models with `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema`.
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;

use pretty_assertions::assert_eq;
use serde_json::Value;

/// Top-level property names, sorted.
fn properties(schema: &schemars::Schema) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| {
            let mut keys: Vec<String> = p.keys().cloned().collect();
            keys.sort();
            keys
        })
        .unwrap_or_default()
}

#[test]
fn theme_schema_snapshot() {
    let schema = nail_config::theme_schema();
    assert_eq!(schema.get("title"), Some(&Value::from("Nail theme.kdl")));
    assert_eq!(
        properties(&schema),
        [
            "appearance",
            "extends",
            "icons",
            "message",
            "mode",
            "name",
            "parts",
            "raw-css",
            "sounds",
            "tokens"
        ]
    );
    common::schema_snapshot("theme.schema.json", &schema, "schema");
}

#[test]
fn layout_schema_snapshot() {
    let schema = nail_config::layout_schema();
    assert_eq!(schema.get("title"), Some(&Value::from("Nail layout.kdl")));
    assert_eq!(
        properties(&schema),
        [
            "actions",
            "extends",
            "keys",
            "menus",
            "name",
            "presets",
            "rail",
            "settings-page",
            "toolbar-settings",
            "toolbars",
            "views",
            "window-frames",
            "windows"
        ]
    );
    let text = serde_json::to_string(&schema).unwrap();
    for needle in [
        "\"split\"",
        "\"stack\"",
        "\"tabs\"",
        "\"widget\"",
        "\"submenu\"",
        "\"header\"",
        "\"more\"",
        "\"rank\"",
        "\"min-size\"",
        "\"multiple\"",
    ] {
        assert!(text.contains(needle), "layout schema lacks {needle}");
    }
    common::schema_snapshot("layout.schema.json", &schema, "schema");
}

#[test]
fn accounts_schema_snapshot() {
    let schema = nail_config::accounts_schema();
    assert_eq!(schema.get("title"), Some(&Value::from("Nail accounts.kdl")));
    assert_eq!(
        properties(&schema),
        [
            "account",
            "caldav",
            "calendar-subscription",
            "carddav",
            "oauth-provider",
            "undo-send-delay",
            "virtual-account"
        ]
    );
    common::schema_snapshot("accounts.schema.json", &schema, "schema");
}
