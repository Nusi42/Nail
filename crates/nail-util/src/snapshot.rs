//! Snapshot and golden files of tests: "write the file while an update
//! variable is set, else compare with it", defined once (wave Q2, SSOT /
//! ODR / DRY). Every snapshot of the workspace (nail-api's fixtures and
//! TypeScript vectors, nail-config's golden files and schemas, nail-compose's
//! golden messages) goes through [`stored`].

use std::path::Path;

/// Set to rewrite JSON snapshots, fixtures and vectors
/// (`NAIL_UPDATE_SNAPSHOTS=1 cargo test …`).
pub const UPDATE_SNAPSHOTS: &str = "NAIL_UPDATE_SNAPSHOTS";

/// Set to rewrite golden files (`NAIL_UPDATE_GOLDEN=1 cargo test …`).
pub const UPDATE_GOLDEN: &str = "NAIL_UPDATE_GOLDEN";

/// Set to rewrite the generated TypeScript bindings
/// (`NAIL_UPDATE_BINDINGS=1 cargo test -p nail-api --test bindings`).
pub const UPDATE_BINDINGS: &str = "NAIL_UPDATE_BINDINGS";

/// Whether the update variable `env` is set.
#[must_use]
pub fn updating(env: &str) -> bool {
    std::env::var_os(env).is_some()
}

/// The stored text of the snapshot at `path`, with CRLF line ends read as
/// LF (Git may check files out either way). While `env` is set, `actual` is
/// written first (creating the directory), so the comparison that follows
/// passes and the file is current.
///
/// # Panics
///
/// When the file cannot be written, or does not exist (the message names
/// the variable that creates it). Only tests call this.
#[must_use]
pub fn stored(path: &Path, actual: &str, env: &str) -> String {
    if updating(env) {
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            panic!("{}: {e}", dir.display());
        }
        if let Err(e) = std::fs::write(path, actual) {
            panic!("{}: {e}", path.display());
        }
    }
    match std::fs::read_to_string(path) {
        Ok(text) => text.replace("\r\n", "\n"),
        Err(e) => panic!("{}: {e} ({})", path.display(), hint(env)),
    }
}

/// Compares `actual` (CRLF read as LF) with the snapshot at `path`
/// ([`stored`]).
///
/// # Panics
///
/// When the snapshot differs: the message names the file and how to
/// regenerate it.
pub fn check_text(path: &Path, actual: &str, env: &str) {
    let expected = stored(path, actual, env);
    let actual = actual.replace("\r\n", "\n");
    assert!(
        expected == actual,
        "{} is stale ({}).\n--- stored\n{expected}\n--- actual\n{actual}",
        path.display(),
        hint(env)
    );
}

/// How to regenerate a snapshot.
#[must_use]
pub fn hint(env: &str) -> String {
    format!("run the test with {env}=1 to regenerate it")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_while_updating_and_compares_otherwise() {
        let dir = std::env::temp_dir().join(format!("nail-util-snapshot-{}", std::process::id()));
        let path = dir.join("nested/a.txt");
        // A variable nobody sets: compare mode.
        let unset = "NAIL_UTIL_SNAPSHOT_TEST_UNSET";
        let missing = std::panic::catch_unwind(|| stored(&path, "x", unset));
        assert!(missing.is_err(), "a missing snapshot fails");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "line\r\nnext\n").unwrap();
        assert_eq!(stored(&path, "ignored", unset), "line\nnext\n");
        check_text(&path, "line\r\nnext\n", unset);
        let stale = std::panic::catch_unwind(|| check_text(&path, "other", unset));
        assert!(stale.is_err(), "a different text fails");
        // `PATH` is always set: update mode writes the text first.
        assert_eq!(stored(&path, "new\n", "PATH"), "new\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hints_name_the_variable() {
        assert!(hint(UPDATE_GOLDEN).contains("NAIL_UPDATE_GOLDEN=1"));
        assert!(updating("PATH") && !updating("NAIL_UTIL_SNAPSHOT_TEST_UNSET"));
    }
}
