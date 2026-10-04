# ADR 0129: Q1 integration — zero warnings across the repository

Status: accepted · 2026-10-04 · wave Q1 (integration)

## Context

ADR 0123 set up the quality baseline (every file kind has a formatter or
linter, 0 warnings, 0 errors, one entry point `npm run check`, a pre-commit
hook, a blocking CI job `quality`). Five packages fixed the findings of
their directories in parallel; their decisions are ADRs 0124 (q-rust-a),
0125 (q-rust-b), 0126 (q-rust-c), 0127 (q-ts-rest) and 0128 (q-ts-ui; the
package reported but wrote no ADR, so the integration wrote it from the
report). The packages verified in snapshots or per crate, because the other
packages' crates were mid-edit; this integration checked the real checkout
as a whole and resolved what the packages asked for.

## Decisions

### What the packages asked for

- **Spelling (`typos.toml`):** `elemente` / `archiv` (German default folder
  names `nail-proto-imap` recognizes), `crates/nail-ical/tests/corpus/`
  excluded (a recorded iCalendar / vCard corpus, like `tests/fixtures/`),
  and an `extend-ignore-re` for Mork column definitions in Thunderbird
  address books instead of allowing their two-letter column ids as words
  everywhere. The last findings in `ui/` were fixed in code (a renamed
  variable, a regex alternative written as `ments?`) or marked
  (`spellchecker:off` around German relative dates in a test); ADR 0124's
  list of deliberate test misspellings sits in a `spellchecker:off` block.
- **markdownlint:** `packaging/release-notes/*.md` become the update
  manifest's in-app notes verbatim (`update-manifest.mjs --notes`), so they
  start with a sentence, not an H1. A directory-level
  `packaging/release-notes/.markdownlint-cli2.jsonc` turns off only MD041
  there, with that reason; the defaults still apply to those files. The
  `<View> view` placeholder in `ui/src/features/README.md` became code.
- **`POST /api/auth` without remote login:** `nail serve` (loopback, no
  login) answers `204 No Content` (`nail_api::remote::NO_LOGIN_STATUS`)
  instead of 404, so the browser no longer logs an error at start; the UI
  already treats 204 as "no login" (ADR 0128). New router test; other
  unknown API routes still answer 404.
- **ESLint ignores of generated code** moved into the shared list as
  `**/src/api/generated/` and `**/plugin-api/src/generated/`: flat-config
  ignore patterns are relative to the config file, so the root-relative
  entries never matched from `ui/eslint.config.js`. The workaround there is
  gone.
- **`SearchIndex::open_with` / `PimIndex::open_with`** take
  `Option<&DirectoryWrapper>`; the two `needless_pass_by_value`
  expectations are gone (`nail-core` passes `wrapper.as_ref()`).
- **One convention for integration tests:** q-rust-b had given 34 test
  files a file-level `#![expect(clippy::unwrap_used, …)]`; the others
  started every integration-test crate root with `#![cfg(test)]`, which
  brings the helper functions under `clippy.toml`'s
  `allow-unwrap-in-tests` / `allow-expect-in-tests` with no exception
  attribute. The integration converted every `tests/*.rs` to `#![cfg(test)]`
  and removed the now unneeded unwrap / expect expectations in test files.
- **No `#[allow` under `crates/` or `app/`:** the last one was upstream's
  `#[allow(dead_code, clippy::all)]` on the bindgen output of the vendored
  `libsqlite3-sys` (ADR 0095). Nothing there fires (the bindings are all
  `pub`; clippy never lints a non-member), so the attribute was removed
  rather than turned into an expectation that would warn as unfulfilled.
- **The vendored crate is formatted in the repository style** (rustfmt
  edition 2024, as the pre-commit hook checks every staged `.rs` file):
  `cargo fmt --all` never covered it because it is not a workspace member,
  so the first commit through the hook found its import order and two long
  `compile_error!` lines. Formatting only; no code changed.
- **`create-nail-plugin` tests under Node 26:** the workspace symlink
  `node_modules/@nail/plugin-api` could not be canonicalized on the OneDrive
  checkout, so Vite treated the package as external and Node loaded its
  TypeScript sources natively (failing on extensionless imports).
  `resolve.preserveSymlinks: true` in its `vitest.config.ts` (as in
  `ui/vite.config.ts`) fixes it; all 56 tests run again and the root
  `npm test` is green.
- **`scripts/interop/Cargo.lock`** refreshed from the root lock, so
  `--locked` works locally again (CI copies the root lock in anyway).
- Not changed: the `m4c-not-ready` message stays; the UI mock still answers
  with it. `nail_testkit::NO_FLAGS` was already used everywhere.

### New finding at integration

Workspace clippy with `-D warnings` found 10 `float_cmp` findings in
`nail-bench` unit tests that the package's snapshot run had not shown. The
compared values are exact by construction (the nearest-rank percentile
returns one of the samples; budgets are scaled by exact factors; merging
copies measurements), so each of the three test functions carries
`#[expect(clippy::float_cmp, reason = "…")]`.

### Build output without warnings

`npm run build` printed "Some chunks are larger than 500 kB" (main chunk
1.97 MB) and, under load, rolldown's `PLUGIN_TIMINGS` notice.

- **The mock core is out of the app bundle.** It reached the main chunk
  only because feature barrels (`features/*/index.ts`) re-exported mock
  helpers (`seedAgentsDemo`, `setUpdateState`, `installDemoExtensions`, the
  theme files, …), which pulled `api/mockMail.ts` and every feature mock in
  statically. The barrels no longer re-export mock modules (tests import
  them from the mock module itself); the pure appearance rules the settings
  page needs moved from `appearance/mock.ts` to `appearance/model.ts`. The
  mock now loads only through `api/select.ts`'s dynamic import (`?mock=1`,
  tests): about 580 kB less code for every real start.
- **The initial code is split into parallel chunks**
  (`build.rolldownOptions.output.codeSplitting.groups`): `vendor` (React,
  TanStack, zustand, Fluent; existing), `vendor-ui` (React Aria, Lucide,
  `@internationalized`, …), `catalogs` (existing), `default-theme` (the
  440 kB reference theme BootTheme imports statically) and `app`: the
  renderer's own `$initial` modules, capped at ~400 kB per chunk
  (`maxSize`). `output.strictExecutionOrder: true` keeps module evaluation
  in import order across those chunks: without it the first e2e run failed
  everywhere (module-level code in one chunk read a value of a module in
  another chunk that had not run yet). Lazy chunks (editors, palette, mock) stay lazy because only
  `$initial` modules are captured. The UI is served from disk or loopback,
  so more requests cost nothing, and the vendor / theme chunks stay cached
  across app updates. The limit (`chunkSizeWarningLimit`) was not raised.
- **Rolldown's plug-in timing check is off** (`checks.bundlerTimings:
  false`). It reports when JavaScript plug-in hooks ran for a noticeable
  share of the build time; for the same input it fires on a busy machine
  and not on an idle one (here Vite's own asset loader reading the ~100
  Fluent catalogs). It says nothing about the code and cannot be a
  deterministic zero-warnings gate. Justification: a build
  that is green or not depending on what else runs on the machine would
  make "0 warnings" meaningless; every other rolldown check stays on.

`cargo build` (workspace, `nail-cli`, the desktop app with
`custom-protocol` + `test-overrides`) prints no warnings either; the
release `tauri build` runs only in CI's release workflow (it needs the
NSIS / WiX downloads), which uses the same crates and lints.

### ADR numbering

The packages wrote `0124-*`; they are now 0124 q-rust-a, 0125 q-rust-b,
0126 q-rust-c, 0127 q-ts-rest, 0128 q-ts-ui (new), 0129 this integration.

## Consequences

- `npm run check` passes every step with 0 warnings; the CI job `quality`
  runs it and is blocking (no `continue-on-error`;
  `scripts/test/ci-workflow.test.mjs` keeps it so).
- No `#[allow(` remains in `crates/`, `app/` or `scripts/interop`; every
  `eslint-disable` has a `-- reason`; there is no `@ts-ignore` /
  `@ts-expect-error`.
- `npm run check` takes about 3.5 minutes on the reference machine with a
  warm cargo cache (ESLint 143 s, tsc 20 s, rustdoc 12 s, clippy 6 s).
- The pre-commit hook works and is fast: about 5 s for a commit with
  TypeScript files (type-aware ESLint start-up), 0.2 s for Rust, Markdown
  and TOML only (measured with 6 and 2 staged files).
- New code is written clean: the pre-commit hook checks staged files, and
  CLAUDE.md lists the conventions the packages converged on.
