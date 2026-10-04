# ADR 0130: Q1 review fixes — Linux lint job, strict Vite build, fewer suppressions

Status: accepted · 2026-10-04 · wave Q1 (review fixes)

## Context

The two Q1 reviews confirmed three medium findings and five low ones
against ADRs 0123–0129. Each was checked against the code before fixing.

## Decisions

### Non-Windows `cfg` branches are linted on Linux

plan.md and ADR 0126 said "CI's Linux job" checks the `cfg(not(windows))`
branches. That was wrong. The only Linux Rust step linted the interop crate,
which is its own workspace. For it the engine crates are path dependencies,
so clippy and `-D warnings` do not apply to them. `nail-exchange` is not even
in its dependency graph.

New blocking CI job `quality-linux` (ubuntu-latest):
`cargo clippy --locked -p nail-core -p nail-discovery -p nail-exchange
-p nail-smime -p nail-server -p nail-config -p nail-bench --all-targets --
-D warnings`. It uses the same workspace lints as the Windows `quality` job.
`nail-app` (WebKitGTK on Linux) and `nail-mapi` (a Windows DLL) are
Windows-only and stay out. `scripts/test/ci-workflow.test.mjs` scans
`crates/*/src` for `cfg(not(windows))` / `cfg(unix)` / `cfg(not(target_os…))`.
It fails when such a crate is missing from the job.

This job could not be run on the development machine: no Linux target and no
Linux C compiler for `ring` / SQLite3 Multiple Ciphers. Its first CI run is
its verification. A finding there is fixed in code, like any other.

### `vite build` fails on warnings

Nothing enforced "the build prints no warnings". Vite and Rolldown warnings
only scroll by, and the exit code stays 0. `ui/build/strictBuild.ts` gives
`vite build` a custom logger and a plugin:

- The logger prints as usual and records every `warn` / `warnOnce`. Rolldown's
  logs reach it through Vite's `onLog` handler, and Vite's own notices (the
  chunk-size warning) go through the logger directly.
- After the bundle is written, the plugin's `closeBundle` fails the build
  and lists every recorded warning, so one run shows them all.

The dev server and Vitest keep Vite's normal logger (`command === "build"`
only). `ui/build/strictBuild.test.ts` runs real builds of tiny projects in
the temp directory: a clean one passes, and a chunk over the limit or a
direct `eval` fails it. Rolldown's `checks.bundlerTimings` stays off
(ADR 0129: it depends on machine load).

Cargo builds: rustc warnings are enforced by clippy, which runs every
rustc lint with `-D warnings` on the workspace, all targets (`quality`, and
`quality-linux` for the non-Windows branches). `cargo build -p nail-cli` and
`build:desktop` compile the same code without new lints, so they get no
`RUSTFLAGS=-D warnings` of their own. That flag would change the cache key and
rebuild every dependency in those jobs.

### Suppressions replaced by code

- `nail-proto-ews` mock: 22 operation handlers that never fail return
  `String`, and the dispatcher wraps them (`Ok(get_folder(…))`). All 22
  `#[expect(clippy::unnecessary_wraps)]`, which gave a false reason, are gone.
  `get_streaming_events` keeps `OpResult`. Unit tests drive `handle`:
  envelope and status 200, an error response message, and a fault for
  an unknown operation.
- `nail-config` widget tables: the five `#[expect(unused_imports)]` imports
  and the dead `future_widget` helper are deleted. A wave that needs an
  untyped slot adds the import or helper back. The test
  `every_widget_has_typed_options` keeps tables from carrying placeholders
  between waves.

### Smaller fixes

- **`nail --help`:** clap prints doc comments verbatim, so the backticks from
  the `doc_markdown` fix showed in the help text (`$`NAIL_MCP_TOKEN``). Help
  lines that name an environment variable are now `about` / `help`
  attributes, not doc comments. The other help lines lost their backticks.
  The test `help_text_has_no_markdown_backticks` renders every
  subcommand's long help.
- **WebSocket frames:** `handleMessage` handles `response` and `event`
  explicitly and warns about any other kind (version skew) and about
  non-object JSON, instead of emitting `undefined` as an event.
- **Prettier:** `.prettierignore` no longer skips all of `crates/`. It skips
  only `crates/**/fixtures/`, `crates/**/corpus/`, `crates/*/schema/` and
  `crates/*/tests/`, so the hand-written `nail-plugins/src/prelude.js` is
  formatted (it had drifted).
- **mcp.test.tsx:** the grant form's checkboxes are still found by label (by
  role takes 10.6 s instead of 2.5 s in jsdom, close to the 20 s timeout
  under load). The test now also asserts `toHaveRole("checkbox")` on each one.

## Consequences

- Warnings that only show on Linux, or only in the Vite build, now turn CI
  red.
- `npm run check` does not run `vite build`. CI's `npm run build` steps (and
  `npm run e2e`) enforce it.
