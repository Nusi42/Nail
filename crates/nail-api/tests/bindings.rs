//! Fails when the checked-in TypeScript bindings in `ui/src/api/generated/`
//! (the whole API) or `packages/plugin-api/src/generated/` (what plug-in
//! code sees, M2d) differ from what the current Rust types generate.
//!
//! To regenerate: `npm run gen:api`, or
//! `NAIL_UPDATE_BINDINGS=1 cargo test -p nail-api --test bindings`.
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Export = fn(&Path) -> Result<Vec<String>, ts_rs::ExportError>;

fn checked_in_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/api/generated")
}

fn plugin_api_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/plugin-api/src/generated")
}

/// Reads every file below `dir` into `relative path -> contents` with line
/// endings normalized, so a CRLF checkout does not count as stale.
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap();
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let text = std::fs::read_to_string(&path)
                    .unwrap()
                    .replace("\r\n", "\n");
                out.insert(rel, text);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn typescript_bindings_are_up_to_date() {
    check_up_to_date(
        nail_api::export_typescript,
        &checked_in_dir(),
        "ui/src/api/generated",
    );
}

#[test]
fn plugin_api_bindings_are_up_to_date() {
    check_up_to_date(
        nail_api::export_plugin_typescript,
        &plugin_api_dir(),
        "packages/plugin-api/src/generated",
    );
}

fn check_up_to_date(export: Export, target: &Path, name: &str) {
    let fresh_dir = tempfile::tempdir().unwrap();
    let written = export(fresh_dir.path()).unwrap();
    let fresh = read_tree(fresh_dir.path());
    assert_eq!(written, fresh.keys().cloned().collect::<Vec<_>>());

    if std::env::var_os("NAIL_UPDATE_BINDINGS").is_some() {
        if target.exists() {
            std::fs::remove_dir_all(target).unwrap();
        }
        std::fs::create_dir_all(target).unwrap();
        export(target).unwrap();
        return;
    }

    let current = read_tree(target);
    let missing: Vec<_> = fresh.keys().filter(|k| !current.contains_key(*k)).collect();
    let extra: Vec<_> = current.keys().filter(|k| !fresh.contains_key(*k)).collect();
    let changed: Vec<_> = fresh
        .iter()
        .filter(|(k, v)| current.get(*k).is_some_and(|c| c != *v))
        .map(|(k, _)| k)
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty() && changed.is_empty(),
        "TypeScript bindings in {name} are stale.\n\
         missing: {missing:?}\nextra: {extra:?}\nchanged: {changed:?}\n\
         Regenerate with `npm run gen:api` \
         (or NAIL_UPDATE_BINDINGS=1 cargo test -p nail-api --test bindings)."
    );
}

#[test]
fn bindings_contain_the_protocol_roots() {
    let dir = tempfile::tempdir().unwrap();
    let written = nail_api::export_typescript(dir.path()).unwrap();
    for name in [
        "Request.ts",
        "Response.ts",
        "Event.ts",
        "ServerMessage.ts",
        "RpcRequest.ts",
        "index.ts",
    ] {
        assert!(
            written.iter().any(|f| f == name),
            "{name} not generated: {written:?}"
        );
    }
    let request = std::fs::read_to_string(dir.path().join("Request.ts")).unwrap();
    assert!(request.contains("\"type\": \"ListAccounts\""), "{request}");
    let rpc = std::fs::read_to_string(dir.path().join("RpcRequest.ts")).unwrap();
    assert!(rpc.contains("id: number"), "u64 must map to number: {rpc}");
}

#[test]
fn plugin_bindings_hold_the_plugin_surface_only() {
    let dir = tempfile::tempdir().unwrap();
    let written = nail_api::export_plugin_typescript(dir.path()).unwrap();
    for name in [
        "PluginManifest.ts",
        "PluginUiNode.ts",
        "PluginDialogSpec.ts",
        "PluginDialogResult.ts",
        "PluginPermission.ts",
        "PluginMessage.ts",
        "index.ts",
    ] {
        assert!(written.iter().any(|f| f == name), "{name}: {written:?}");
    }
    for protocol in ["Request.ts", "Response.ts", "Event.ts", "PluginInfo.ts"] {
        assert!(!written.iter().any(|f| f == protocol), "{protocol} leaked");
    }
    let permission = std::fs::read_to_string(dir.path().join("PluginPermission.ts")).unwrap();
    assert!(permission.contains("`network:${string}`"), "{permission}");
    let node = std::fs::read_to_string(dir.path().join("PluginUiNode.ts")).unwrap();
    assert!(node.contains("\"type\": \"contact-picker\""), "{node}");
    assert!(node.contains("maxLength?: number"), "{node}");
}

/// A doc comment containing `*/` (a cron step like `*/15`, a MIME pattern
/// `type/*/`) would end the generated `TSDoc` comment early and break the
/// TypeScript build (M4a).
#[test]
fn doc_comments_never_close_the_tsdoc_comment_early() {
    for dir in [checked_in_dir(), plugin_api_dir()] {
        for (file, text) in read_tree(&dir) {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.contains("*/") {
                    assert!(
                        trimmed == "*/"
                            || trimmed.ends_with(" */")
                                && !trimmed[..trimmed.len() - 2].contains("*/"),
                        "{file}: {line}"
                    );
                }
            }
        }
    }
}
