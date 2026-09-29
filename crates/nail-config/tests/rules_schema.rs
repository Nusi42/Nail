//! JSON Schema snapshot of `rules.kdl` (`crates/nail-config/schema/rules.schema.json`).
//! Regenerate with `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema`.

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::Value;

#[test]
fn rules_schema_snapshot() {
    let schema = nail_config::rules::rules_schema();
    assert_eq!(schema.get("title"), Some(&Value::from("Nail rules.kdl")));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("rules.schema.json");
    let actual = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("NAIL_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with NAIL_UPDATE_SNAPSHOTS=1)", path.display()));
    let expected: Value = serde_json::from_str(&expected).unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(
        actual, expected,
        "rules.schema.json is stale; regenerate with NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema"
    );
}

/// Every node and property name the reader accepts is in the schema.
#[test]
fn the_schema_names_every_node_and_property() {
    let text = serde_json::to_string(&nail_config::rules::rules_schema()).unwrap();
    for name in [
        // Rule, triggers, scope.
        "rule",
        "id",
        "enabled",
        "trigger",
        "scope",
        "when",
        "then",
        "every",
        "days",
        "on-send",
        "older-than",
        "account",
        // Conditions.
        "all",
        "any",
        "not",
        "header",
        "exists",
        "from",
        "to",
        "cc",
        "recipients",
        "reply-to",
        "sender",
        "subject",
        "body",
        "query",
        "attachment",
        "name",
        "type",
        "size",
        "larger",
        "smaller",
        "date",
        "before",
        "after",
        "age",
        "newer-than",
        "flag",
        "set",
        "tag",
        "folder",
        "role",
        "dkim",
        "spf",
        "dmarc",
        "list-id",
        "equals",
        "contains",
        "starts-with",
        "ends-with",
        "regex",
        "case-sensitive",
        // Actions.
        "move",
        "copy",
        "delete",
        "permanent",
        "archive",
        "untag",
        "mark-read",
        "forward",
        "identity",
        "redirect",
        "reply",
        "notify",
        "stop",
    ] {
        assert!(
            text.contains(&format!("\"{name}\"")),
            "schema lacks {name:?}"
        );
    }
}
