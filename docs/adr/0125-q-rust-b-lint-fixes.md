# ADR 0125: Q1 Rust group B — zero findings in the API, store, config, search, vault, rules, Sieve, triage and compose crates

Status: accepted · 2026-10-04 · wave Q1 (package q-rust-b)

## Context

ADR 0123 set up the quality baseline and measured the findings. This
package owns `crates/nail-api`, `nail-store`, `nail-config`, `nail-search`,
`nail-vault`, `nail-rules`, `nail-sieve`, `nail-triage` and `nail-compose`.
At the contract it had 593 clippy findings (714 with every test target), 5
rustdoc findings, 18 typos findings and 1 cargo machete finding. No lint
level was lowered and `Cargo.toml` / `clippy.toml` were not changed.

## Decisions

### Fixes, not exceptions

- `cargo clippy --fix` applied the machine-applicable suggestions
  (`unreachable_pub` → `pub(crate)`, method references, `allow` →
  `expect`, …). Everything else was fixed by hand.
- **No `unwrap` / `expect` in library code.** Invariants are now carried by
  types or control flow: `CompiledRule::new` keeps the condition tree that
  validation compiled (no second compile and `expect`); the thread merge
  takes the first thread separately and never builds an empty set; JWZ
  subject grouping uses `let … else`; query builders pop the single item
  instead of `next().expect()`; `derive_account_id` / `derive_rule_id` loop
  instead of `(2..).find().expect()`; the KDL editors use
  `ensure_children()`; ManageSieve SASL picks the mechanism from a list. The
  two KDL `format_mut()` calls right after `set_format()` keep an
  `expect` with a reason (`kdl` has no `get_or_insert` for formats).
- **Parameter struct:** `nail_search::search` takes a `PageRequest` (sort,
  offset, limit) instead of three loose arguments. `exec::TermPlan`
  replaces an `Option<Option<Box<dyn Query>>>`, `manager::Seen` replaces
  an `Option<Option<String>>`.
- **Casts:** `try_from` with a saturating fallback or a typed error
  (`encode_frame` returns `TooLarge` when the length does not fit a `u32`);
  the timer wheel uses a table of level shifts instead of `usize as u32`.
  Float to integer casts of clamped values keep an `expect` with the bound
  as the reason.
- **Ignored results** (`let _ = …` on a `Result`): best-effort cleanups
  (temporary files, old backups, lock files, socket shutdown,
  `TCP_NODELAY`, duplicate Fluent ids) now log at `debug` level instead of
  dropping the error silently.
- **Docs:** `# Panics` sections where a built-in default that cannot fail
  is resolved; intra-doc links to private items became plain text or links
  to the public re-export; ambiguous `format` / `print` links use `mod@`.

### Exceptions that stay (`#[expect(…, reason = "…")]`)

- `struct_excessive_bools` on wire / settings types whose booleans are
  independent (one checkbox each, IMAP flags, mirrored server properties,
  database columns). Turning them into enums would change the JSON wire
  format and the generated TypeScript for no gain.
- `needless_pass_by_value` on `SearchIndex::open_with` / `PimIndex::open_with`
  (public, called by `nail-core` with an owned `Arc` wrapper). Changing the
  signature needed `nail-core`, which another package owned in this wave;
  the integration (ADR 0129) changed both to `Option<&DirectoryWrapper>` and
  removed the two expectations.
- Integration-test helpers: `allow-unwrap-in-tests` covers `#[test]`
  functions and `#[cfg(test)]` code, but not the helper functions of an
  integration-test crate (`tests/*.rs`). This package first gave such files
  a file-level `#![expect(clippy::unwrap_used, …)]`; the integration
  (ADR 0129) replaced that with the repository-wide convention of the other
  packages: every integration-test crate root starts with `#![cfg(test)]`,
  which brings its helpers under `allow-unwrap-in-tests` with no exception
  attribute.
- Shared test fixtures (`tests/common/mod.rs`): a test crate that uses only
  part of them declares `#[expect(dead_code, …)] mod common;` instead of the
  old blanket `#![allow(dead_code)]` in the module, so a fixture that no
  test uses at all is reported.

### Not changed

- `crates/nail-store/libsqlite3-sys-sqlite3mc` is the vendored
  `libsqlite3-sys` 0.38.2 (ADR 0095). It is not a workspace member and has
  no `[lints] workspace = true`. Its `#[allow(dead_code, clippy::all)]` on
  the bindgen output was upstream's; the integration removed it (the
  bindings are all `pub`, so nothing fires, and clippy never lints a
  non-member), so no `#[allow` is left under `crates/`.

### Generated bindings

Doc comments of three API types changed (`LocalizedMessage`,
`VacationSettings`, `PluginActionTarget`); `npm run gen:api` regenerated
`ui/src/api/generated/` (comments only, no type changes).

## Consequences

- `cargo clippy -p <crate> --all-targets -- -D warnings` and
  `RUSTDOCFLAGS=-D warnings cargo doc -p <crate> --no-deps` pass for all
  nine crates; typos and cargo machete report nothing for them.
- `nail_search::search` has a new signature (only `run_request` and the
  crate's tests call it).
