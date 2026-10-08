//! Test vectors for the TypeScript ports of contract rules (wave Q2, SSOT /
//! ODR / DRY, ADR 0171). The UI implements a few pure rules again for
//! immediate feedback and its mock core (window ids, views, menus, theme
//! choice, recovery keys, …); their single source is the Rust code. A Rust
//! test fills a [`Vectors`] by running the Rust functions over a list of
//! inputs and [`Vectors::check`]s the file `fixtures/ts-vectors/<area>.json`
//! of this crate; the UI's tests run their port over the same file
//! (`ui/src/test/vectors.ts`), so either side changing alone fails.
//!
//! Regenerate with `NAIL_UPDATE_SNAPSHOTS=1` (`cargo test -p nail-api --test
//! ts_vectors`, `cargo test -p nail-config ts_vectors`, `cargo test -p
//! nail-core ts_vectors`); the tests fail while a file is stale.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

/// The directory of the vector files (`crates/nail-api/fixtures/ts-vectors`).
#[must_use]
pub fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/ts-vectors")
}

/// One area's cases: `{ "fn": <TypeScript name>, "args": [...], "out": ... }`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Vectors {
    cases: Vec<Value>,
}

impl Vectors {
    /// Records that `function(args…)` answers `out`.
    ///
    /// # Panics
    ///
    /// When `out` does not serialize to JSON (a test bug).
    pub fn case(&mut self, function: &str, args: Value, out: impl Serialize) -> &mut Self {
        let out = serde_json::to_value(out).expect("vector answers serialize to JSON");
        self.cases
            .push(json!({ "fn": function, "args": args, "out": out }));
        self
    }

    /// The file's text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text =
            serde_json::to_string_pretty(&json!({ "cases": self.cases })).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Writes (with `NAIL_UPDATE_SNAPSHOTS` set) or checks
    /// `<vectors_dir>/<area>.json`.
    ///
    /// # Panics
    ///
    /// When the file is stale (a failing test).
    pub fn check(&self, area: &str) {
        let path = vectors_dir().join(format!("{area}.json"));
        let text = self.render();
        if std::env::var_os("NAIL_UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(vectors_dir()).expect("vectors directory");
            std::fs::write(&path, &text).expect("vector file written");
        }
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            actual == text,
            "{} is stale: run the test with NAIL_UPDATE_SNAPSHOTS=1",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_cases() {
        let mut v = Vectors::default();
        v.case("isX", json!(["a"]), true);
        assert_eq!(
            v.render(),
            "{\n  \"cases\": [\n    {\n      \"args\": [\n        \"a\"\n      ],\n      \"fn\": \"isX\",\n      \"out\": true\n    }\n  ]\n}\n"
        );
    }
}
