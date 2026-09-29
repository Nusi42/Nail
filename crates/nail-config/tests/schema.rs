//! JSON Schema snapshots of `theme.kdl` and `layout.kdl`.
//!
//! The checked-in files `crates/nail-config/schema/*.schema.json` are what
//! editors and `nail config schema` use. Regenerate them after changing the
//! file models with `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema`.

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::Value;

fn check_snapshot(name: &str, schema: &schemars::Schema) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(name);
    let actual = serde_json::to_string_pretty(schema).unwrap() + "\n";
    if std::env::var_os("NAIL_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with NAIL_UPDATE_SNAPSHOTS=1)", path.display()));
    // Compare as JSON values so key order (serde_json's `preserve_order`
    // feature may be switched on by other crates) does not matter.
    let expected: Value = serde_json::from_str(&expected).unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(
        actual, expected,
        "{name} is stale; regenerate with NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema"
    );
}

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
            "extends", "icons", "message", "mode", "name", "parts", "raw-css", "sounds", "tokens"
        ]
    );
    check_snapshot("theme.schema.json", &schema);
}

#[test]
fn layout_schema_snapshot() {
    let schema = nail_config::layout_schema();
    assert_eq!(schema.get("title"), Some(&Value::from("Nail layout.kdl")));
    assert_eq!(
        properties(&schema),
        [
            "actions", "extends", "keys", "menus", "name", "toolbars", "windows"
        ]
    );
    let text = serde_json::to_string(&schema).unwrap();
    for needle in [
        "\"split\"",
        "\"stack\"",
        "\"tabs\"",
        "\"widget\"",
        "\"submenu\"",
    ] {
        assert!(text.contains(needle), "layout schema lacks {needle}");
    }
    check_snapshot("layout.schema.json", &schema);
}
