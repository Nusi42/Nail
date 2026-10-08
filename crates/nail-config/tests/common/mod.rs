//! Helpers shared by the integration tests of nail-config: golden files,
//! schema snapshots, layout patches, the watcher and the user docs' KDL
//! examples.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nail_api::accounts::AccountsConfig;
use nail_api::{LayoutNodeRef, LayoutPatch};
use nail_config::{
    AccountsDocument, ConfigManager, ConfigUpdate, ConfigWatcher, apply_layout_patches,
    parse_accounts, parse_layout_all,
};
use pretty_assertions::assert_eq;
use serde_json::Value;

/// Compares `actual` with `tests/golden/<folder>/<name>.kdl`; with
/// `NAIL_UPDATE_GOLDEN=1` it rewrites the file instead.
pub(crate) fn golden(folder: &str, name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(folder)
        .join(format!("{name}.kdl"));
    if std::env::var_os("NAIL_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with NAIL_UPDATE_GOLDEN=1)", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(expected, actual, "golden file {}", path.display());
}

/// Compares a JSON Schema with `schema/<name>` (as JSON values, so key
/// order does not matter); with `NAIL_UPDATE_SNAPSHOTS=1` it rewrites the
/// file instead. `test` names the test target in the hint.
pub(crate) fn schema_snapshot(name: &str, schema: &schemars::Schema, test: &str) {
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
        "{name} is stale; regenerate with NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test {test}"
    );
}

/// A layout node of `window` at `breakpoint`, by child indexes.
pub(crate) fn at(window: &str, breakpoint: u32, path: &[u32]) -> LayoutNodeRef {
    LayoutNodeRef {
        window: window.into(),
        breakpoint,
        path: path.to_vec(),
    }
}

/// Applies `patches` to a layout text (`None` = no file yet); the result
/// must be a valid layout.
pub(crate) fn apply(text: Option<&str>, patches: &[LayoutPatch]) -> String {
    let out = apply_layout_patches(text, patches, "layout.kdl")
        .unwrap_or_else(|e| panic!("patch failed: {e}"));
    parse_layout_all(&out, "layout.kdl").unwrap_or_else(|e| panic!("invalid result: {e:?}"));
    out
}

/// Removes every `source` (file locations) from resolved JSON, so layouts
/// read from different texts compare by meaning.
pub(crate) fn strip_sources(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("source");
            map.values_mut().for_each(strip_sources);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_sources),
        _ => {}
    }
}

/// Writes a file into the config directory (`name` may name a
/// subdirectory, which is created).
pub(crate) fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Watches the manager's directory; every update arrives on the receiver.
pub(crate) fn watch(manager: &ConfigManager) -> (ConfigWatcher, mpsc::Receiver<ConfigUpdate>) {
    let (tx, rx) = mpsc::channel();
    let watcher = manager
        .watch(Duration::from_millis(50), move |update| {
            tx.send(update).ok();
        })
        .unwrap();
    (watcher, rx)
}

/// Waits for an update matching `pred`, skipping others.
pub(crate) fn wait_for(
    rx: &mpsc::Receiver<ConfigUpdate>,
    pred: impl Fn(&ConfigUpdate) -> bool,
) -> ConfigUpdate {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(update) if pred(&update) => return update,
            Ok(_) => {}
            Err(e) => panic!("no matching config update: {e}"),
        }
    }
}

/// The text of a user-facing document (`userDocs/en/<file>`).
pub(crate) fn user_doc(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../userDocs/en")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The ```` ```kdl ```` code blocks of a Markdown text, in order.
pub(crate) fn kdl_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match (&mut current, line.trim_start()) {
            (None, "```kdl") => current = Some(String::new()),
            (Some(block), "```") => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            (Some(block), _) => {
                block.push_str(line);
                block.push('\n');
            }
            (None, _) => {}
        }
    }
    assert!(current.is_none(), "unclosed code block");
    blocks
}

/// Writes `config` into a new `accounts.kdl`: the text contains `line`
/// and reads back as `config`.
pub(crate) fn assert_renders(config: &AccountsConfig, line: &str) {
    let mut d = AccountsDocument::empty("accounts.kdl");
    d.set_config(config.clone()).unwrap();
    assert!(d.text().contains(line), "{}", d.text());
    assert_eq!(&parse_accounts(d.text(), "accounts.kdl").unwrap(), config);
}
