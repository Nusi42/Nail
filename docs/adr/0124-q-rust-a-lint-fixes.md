# ADR 0124: Q1 lint fixes in the core, MCP, plug-in, server, CLI, MAPI, bench, remote-auth, proxy and shell crates

- Status: accepted (2026-10-04)
- Context: wave Q1, package q-rust-a (ADR 0123). Owned: `crates/nail-core`, `nail-mcp`, `nail-plugins`
  (incl. `src/prelude.js`), `nail-server`, `nail-cli`, `nail-mapi`, `nail-bench`, `nail-remote-auth`,
  `nail-proxy`, `app/src-tauri`. At the contract they had 1485 clippy, 15 rustdoc, 305 ESLint, 147 tsc,
  12 typos and 3 cargo-machete findings.

## Decisions

### Fixes, not exceptions

- Machine-applicable suggestions were applied first (`cargo clippy --fix`: `pub` to `pub(crate)`,
  `Duration::from_hours`, method references, `clone_from`, …). The rest was fixed by hand.
- `let _ = expr;` on values with a destructor (`let_underscore_drop`, 240 findings) became `drop(expr);`.
  The value is dropped at the end of the statement in both forms, so nothing changes. `let _ = x;` used
  only to silence an unused parameter in a trait default method became an `_x` parameter.
- `trivial_casts` (`Arc<T> as Arc<dyn Trait>`): the cast is gone wherever the target type coerces it.
  `Arc::clone(&x)` where an `Arc<dyn Trait>` is expected became `x.clone()`, because `Arc::clone` infers
  its type parameter from the expected type. Closures that build a trait object state their return type
  (`.map(|h| -> Arc<dyn DesktopHost> { h })`).
- `unwrap` / `expect` in production code became error handling. `Option::take` plus `Option::insert`
  replaces "load if missing, then `expect("loaded above")`" (snooze and reminder schedulers, the backup
  mail target). `WindowsSystemAppearance::new` returns `None` when Windows cannot create its event, and
  `platform_source` then has no OS source. `Service::from_parts` returns `Result`. Exchange `status`
  answers `invalid_request` instead of panicking. `nail-app`'s `build.rs` returns `Result` from `main`.
- Parameter structs: `Service::send_imip` takes `ImipMail` (7 parameters before), the job context of
  the plug-in host is a `JobContext` struct instead of a 4-tuple, and the iMIP test helper takes `Imip`.
- Integer/float conversions: `TryFrom` with an explicit fallback wherever a value can be out of range.
  `nail-bench` keeps its statistics and synthetic-corpus conversions in `nail_bench::num` (`f64_of`,
  `usize_of`, `i64_of`), with the only `#[expect(clippy::cast_*)]` of the crate and unit tests.
  `nail-mapi` uses `windows::size_u32` / `count_u32` for Win32 size and count fields.
  rquickjs converts `u64` itself, so the blob ids of the plug-in runtime are `u64`, not `f64`.
- Values passed by value but not consumed became references (`Denied::new(&permission)`,
  `PluginHost::deliver_event(&[MessageId])`, …). `untrusted()` now moves its data into the envelope
  instead of `json!` serializing a borrowed copy. `From<PortabilityError> for CoreError` replaces the
  `api` helper.
- `async fn`s with no `.await` (`exchange::engine_of`, `setup::engine_for`, `Service::transport`,
  PIM index helpers, …) are plain functions now.
- Dead code was removed (`not_ready`, `dav_source_ids`, `invalidate_calendar_instances`, `is_writable`).
- Types without `Debug` got a derived `Debug`, or a manual one with `finish_non_exhaustive` where a
  field cannot derive it or holds a secret.
- Unused dependencies (cargo machete) were removed: `nail-bench` → `nail-search`, `nail-remote-auth` →
  `hex`, `nail-app` → `serde`. Cargo.lock follows when the integrator builds.

### Tests

- Every `tests/*.rs` crate root starts with `#![cfg(test)]`. The line is always true there. Without it,
  clippy's `allow-unwrap-in-tests` / `allow-expect-in-tests` (`clippy.toml`) does not see the helper
  functions of integration tests (it checks `#[test]` and `#[cfg(test)]` items only). q-rust-c reached
  the same pattern (ADR 0126).
- Shared fixtures in `tests/common/mod.rs` (`nail-mcp`, `nail-plugins`, `nail-remote-auth`) lost their
  crate-wide `#![allow(dead_code)]`. Every test crate that uses only part of them declares
  `#[expect(dead_code, reason = "…")] mod common;`. The compiler checks the expectation per test crate.
  q-rust-b uses the same pattern (ADR 0125).

### The exceptions (`#[expect(lint, reason = "…")]`, every one)

- `unsafe_code`: each module or function with FFI keeps its exception with a reason (Win32, COM,
  WebView2, rquickjs byte access, MessageBoxW, Credential Manager, GetProcessMemoryInfo, the MAPI DLL
  tree and its DLL test).
- `clippy::match_same_arms` on `check_remote_request` and `remote_event` (`nail-core/src/remote_access.rs`):
  these exhaustive, security-relevant matches group their arms by why a request or event is allowed,
  each group with its own comment. Merging arms that only happen to share `Ok(())` would hide that.
- `clippy::struct_excessive_bools` on `ServiceOptions`: independent on/off switches of an options
  struct, not a state machine.
- `clippy::expect_used` in `Csrf::mac` (`nail-remote-auth`): HMAC-SHA256 accepts keys of any length.
- `clippy::unnecessary_wraps` on Windows' `purge_credentials` (same signature as the failing non-Windows
  stub), `clippy::needless_pass_by_value` on the `nail_test_shell` Tauri command (Tauri passes arguments
  by value), `clippy::cast_*` on `PopOutScreen::px` (a rounded, non-negative size; `as` saturates).
- Test-only: `deprecated` on two MCP subscription tests (rmcp 3.5 deprecates resource subscriptions),
  `clippy::cast_ptr_alignment` on the guard-page / misaligned-pointer tests of `nail-mapi`,
  `clippy::unused_self` on rmcp's `#[tool]` echo method and on the env guard of the DLL test.

Two `#[expect(deprecated)]` on rmcp `subscribe` / `unsubscribe` implementations were unfulfilled and
are gone. The `non_snake_case` allow of `nail-mapi`'s exports was not needed (`#[no_mangle]` exports are
exempt).

### LNK4199 in the `nail-app` test build

`build.rs` delay-loads comctl32 for every link of the package (`cargo:rustc-link-arg`). The only
executable without any comctl32 import was the unit-test executable of the `nail-app` binary. `main.rs`
only calls `nail_app_lib::run` and has no tests. The `[[bin]]` target now has `test = false`, so that
empty executable is no longer linked. The linker reports nothing for `cargo build -p nail-app` and
`cargo test -p nail-app --no-run`. No `/IGNORE` flag is used.

### `prelude.js` (QuickJS classic script)

- It is fully typed with JSDoc. Typedefs are prefixed `Prelude…`, because a classic script's typedefs
  are global in the root `tsconfig.json`. Values from plug-in code are `unknown`, and the
  `typeof … === "function"` checks are type guards (`isCallable`). `JSON.parse` / `stringify` and
  `ArrayBuffer.isView` are captured bound before plug-in code runs, as before.
- Promise-returning calls build the promise with `new Promise(executor)`. A throw in the executor rejects
  with the thrown value unchanged, exactly as the former `try { … } catch (e) { return Promise.reject(e) }`.
- Behaviour kept: panels are stored as the plug-in's own object (`render` / `onAction` keep their
  `this`). `limitOf` keeps `options && options.limit`.
- Small differences, all on invalid input:
  - A value that cannot be serialized to JSON now throws a `TypeError` before the host call. Before,
    `undefined` reached the host.
  - `fetch` sends `method: ""` as given; before, an empty method became `GET`. The host's own default
    (`GET` for a missing or non-string method) still applies.
  - A dispatch of an unknown kind throws a `TypeError` instead of "not a function".
- The one ESLint exception: `no-unused-expressions` on the script's completion value, which the host
  takes. `no-base-to-string` on `String(body)` for non-byte fetch bodies, which mirrors `fetch`.

### Spelling

<!-- spellchecker:off -->

Deliberate misspellings in test data (`lsit`, `raed`), the `pn-` id prefix and the surname `Wirth` carry
`// spellchecker:disable-line`. The German test strings of `nail-plugins/src/l10n.rs` sit in a
`spellchecker:off` / `on` block. `b"abd"` test bytes became `b"abx"`.

<!-- spellchecker:on -->

## Consequences

- The ten crates have 0 clippy findings (`--all-targets`, pedantic, per crate and together) and 0
  rustdoc findings. `prelude.js` has 0 ESLint and 0 tsc findings, and typos reports nothing in these
  directories.
- New integration test files start with `#![cfg(test)]`, and new shared fixtures follow the
  `mod common;` pattern above.
