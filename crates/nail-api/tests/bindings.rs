//! Fails when the checked-in TypeScript bindings in `ui/src/api/generated/`
//! differ from what the current Rust types generate.
//!
//! To regenerate: `npm run gen:api`, or
//! `NAIL_UPDATE_BINDINGS=1 cargo test -p nail-api --test bindings`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn checked_in_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/api/generated")
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
    let fresh_dir = tempfile::tempdir().unwrap();
    let written = nail_api::export_typescript(fresh_dir.path()).unwrap();
    let fresh = read_tree(fresh_dir.path());
    assert_eq!(written, fresh.keys().cloned().collect::<Vec<_>>());

    let target = checked_in_dir();
    if std::env::var_os("NAIL_UPDATE_BINDINGS").is_some() {
        if target.exists() {
            std::fs::remove_dir_all(&target).unwrap();
        }
        std::fs::create_dir_all(&target).unwrap();
        nail_api::export_typescript(&target).unwrap();
        return;
    }

    let current = read_tree(&target);
    let missing: Vec<_> = fresh.keys().filter(|k| !current.contains_key(*k)).collect();
    let extra: Vec<_> = current.keys().filter(|k| !fresh.contains_key(*k)).collect();
    let changed: Vec<_> = fresh
        .iter()
        .filter(|(k, v)| current.get(*k).is_some_and(|c| c != *v))
        .map(|(k, _)| k)
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty() && changed.is_empty(),
        "TypeScript bindings in ui/src/api/generated are stale.\n\
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
