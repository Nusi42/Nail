# ADR 0123: Q1 contract — quality baseline: strict linters and formatters for every file, zero warnings

Status: accepted · 2026-10-04 · wave Q1 (contract)

## Context

The maintainer required linters and formatters for every kind of
file, with 0 warnings and 0 errors, without switching off lints that are
usually good (memory `zero-warnings-policy`, plan section 10 "Quality
baseline (Q1)"). Before Q1 the repository had rustfmt, clippy (`all` plus a
few extra lints), ESLint (`strict`, not type-aware) for `ui/` only, tsc, and
Prettier for `ui/` and `e2e/`. TOML, Markdown, workflows, spelling, the Node
scripts, `packages/*` and `examples/*` were not linted.

This contract sets the tooling up centrally and measures the findings. Four
packages then fix them in parallel, each in its own directories:

| Package | Owns |
| --- | --- |
| q-rust-a | `crates/nail-core`, `nail-mcp`, `nail-plugins` (incl. `src/prelude.js`), `nail-server`, `nail-cli`, `nail-mapi`, `nail-bench`, `nail-remote-auth`, `nail-proxy`, `app/src-tauri` |
| q-rust-b | `crates/nail-api`, `nail-store`, `nail-config`, `nail-search`, `nail-vault`, `nail-rules`, `nail-sieve`, `nail-triage`, `nail-compose` |
| q-rust-c | every other crate (`nail-proto-*`, `nail-portability`, `nail-ical`, `nail-exchange`, `nail-pgp`, `nail-smime`, `nail-pkcs11`, `nail-testkit`, `nail-testkit-net`, `nail-dav`, `nail-itip`, `nail-mime`, `nail-discovery`, `nail-auth`) |
| q-ts-ui | `ui/`, `locales/` |
| q-ts-rest | `e2e/`, `packages/`, `examples/`, `scripts/` (incl. the `scripts/interop` crate), `docs/`, `userDocs/`, `.github/`, `.claude/memory`, root config files except `Cargo.toml` and `package.json` |

No new crates, UI features or catalogs are needed for this wave, so there
are no stub slots; the shared configuration files (`Cargo.toml`
`[workspace.lints]`, `clippy.toml`, `eslint.config.base.js`, `package.json`,
`typos.toml`, `.markdownlint-cli2.jsonc`, `.editorconfig`, `taplo.toml`)
belong to this contract. A package that needs a change there (a word in
`typos.toml`, a `doc-valid-idents` entry) makes it and says so in its ADR;
the integrator merges. Lint levels are never lowered.

## Decisions

### The rule

0 warnings, 0 errors. Never relax a lint level or `-D warnings`, never
switch an ESLint rule off, never commit with `--no-verify`. An unavoidable
exception is written at the narrowest scope with its reason:

- Rust: `#[expect(lint, reason = "…")]`. `clippy::allow_attributes` and
  `clippy::allow_attributes_without_reason` are **deny**, so `#[allow]` is
  rejected and an `expect` that no longer fires warns (unfulfilled
  expectation).
- TypeScript / JavaScript: `// eslint-disable-next-line rule -- reason`.
  eslint-comments (`require-description`, `no-unlimited-disable`,
  `disable-enable-pair`) and `reportUnusedDisableDirectives: "error"`
  enforce it.

The rule is in `CLAUDE.md` ("Non-negotiable rules").

### Rust (`Cargo.toml` `[workspace.lints]`, `clippy.toml`)

Kept: `unsafe_code = "deny"`, `rust_2018_idioms`, `unused_qualifications`,
`clippy::all`, `dbg_macro`, `todo`, `unimplemented`, `large_futures`,
`needless_pass_by_ref_mut`. Added:

- **rustc:** `unused_lifetimes`, `let_underscore_drop`, `trivial_casts`,
  `trivial_numeric_casts`, `unused_import_braces`, `unreachable_pub` (331
  findings, all machine-applicable: `pub` → `pub(crate)`; feasible) and
  `missing_debug_implementations` (37; reasonable: a manual `Debug` that
  hides secrets is the fix where a field cannot derive it).
- **clippy:** the whole `pedantic` group (warn), `unwrap_used` and
  `expect_used` (warn; `allow-unwrap-in-tests` / `allow-expect-in-tests`
  in `clippy.toml` keep tests free), `allow_attributes` and
  `allow_attributes_without_reason` (deny).
- **rustdoc:** `private_intra_doc_links`, `unescaped_backticks` on top of the
  default rustdoc lints; checked by `cargo doc --workspace --no-deps` with
  `RUSTDOCFLAGS=-D warnings`.

The only pedantic lints switched off (pure style here, 3 of 103):

| Lint | Findings | Why off |
| --- | --- | --- |
| `must_use_candidate` | 2089 | It asks for `#[must_use]` on every getter and constructor. Ignoring such values is harmless; `#[must_use]` is added by hand where dropping a value is a bug (and `return_self_not_must_use` stays on). |
| `missing_errors_doc` | 1393 | Internal, unpublished crates. Every error is a typed enum (`thiserror`) whose variants are documented where they are defined; a `# Errors` section per function would repeat that. `missing_panics_doc` stays on: hidden panics are real findings. |
| `too_many_lines` | 238 | A line count, not a defect: protocol state machines, request dispatchers and table-driven tests are long by nature, and splitting them only to satisfy a counter adds indirection. |

Not added, decided:

- `module_name_repetitions` is in clippy's `restriction` group since 1.86
  (not part of pedantic any more); not enabled.
- `missing_docs` / `clippy::missing_docs_in_private_items`: the crates are
  internal; rustdoc lints check the docs that exist.

`clippy.toml` `doc-valid-idents` lists proper names `doc_markdown` would
otherwise want in backticks (OAuth2, OpenPGP, CalDAV, CardDAV, ManageSieve,
WebView2, SQLite, …; `..` keeps clippy's defaults). That cut
`doc_markdown` from 595 to 51 findings without hiding real ones.

The `scripts/interop` crate is its own workspace; its `Cargo.toml` copies
the lint tables (keep them in sync) and the CI `interop` job runs clippy on
it with `-D warnings`.

### TypeScript / JavaScript (`eslint.config.base.js`)

One shared flat config for every TS / JS file:

- `@eslint/js` recommended, typescript-eslint **`strictTypeChecked` +
  `stylisticTypeChecked`**, type-aware through `parserOptions.projectService`,
  `consistent-type-imports`, eslint-comments, eslint-config-prettier last.
- `no-floating-promises` knows `node:test`'s `test` / `describe` (the
  `allowForKnownSafeCalls` option typescript-eslint documents for it).
- `react()`: eslint-plugin-react (recommended + jsx-runtime),
  **eslint-plugin-react-hooks** (recommended), **eslint-plugin-jsx-a11y**
  (recommended) and the Nail rules (`react/jsx-no-literals`, no inline
  styles / class names / literal user-visible attributes, `react/no-danger`).
- `eslint.config.js` (root) covers everything outside `ui/`;
  `ui/eslint.config.js` adds `react()`. ESLint 10 uses the config nearest to
  each file, so `npx eslint .` at the root lints the whole repository.
- Every file has type information: TypeScript walks up from a file's
  nearest `tsconfig.json` to one that includes it. The new root
  `tsconfig.json` (`allowJs`, `checkJs`, `strict`) collects the loose Node
  scripts (`scripts/**`, the `.mjs` files of `e2e/`, `examples/*/scripts`,
  `packages/*/scripts`, the template scripts), the root configs,
  `packages/plugin-api/tools/index.mjs` (shadowed by its `.d.mts` in the
  package project) and `crates/nail-plugins/src/prelude.js` (a classic
  script run by QuickJS: `sourceType: "script"`, ES globals only).
  `packages/create-nail-plugin/templates/tsconfig.json` types the template
  sources; `ui/tsconfig.node.json` got `allowJs` for `eslint.config.js`.
- Ignored (no product code): build output, generated code
  (`ui/src/api/generated`, `packages/plugin-api/src/generated`), test
  fixtures that only look like JS (`crates/*/tests/fixtures`, a Thunderbird
  `prefs.js`) and `.claude/` (workflow-harness scripts run by the Claude
  Code workflow runtime with its own globals; also in `.prettierignore`).
- `ui/src/lint-rules.test.ts` lints its fixtures through the project
  service's default project (`allowDefaultProject`, typed by
  `tsconfig.app.json`) and now also proves jsx-a11y, a type-aware rule and
  the described-disable rule fire.

`npm run lint` = ESLint over the whole repository with `--max-warnings 0`,
`lint:types` (every workspace's `typecheck`, the root `tsconfig.json`, the
examples; `packages/create-nail-plugin` also checks `template/` and
`templates/`) and Prettier over the whole repository. The workspace `lint`
scripts run their own ESLint + tsc + Prettier for standalone use.

### Other files

| Kind | Tool | Config | Install |
| --- | --- | --- | --- |
| TOML | taplo 0.10 (`fmt --check`, `check --no-schema`) | `taplo.toml`: 4-space indent, 100 columns, key order kept. All TOML was formatted once in its own commit ("Q1: taplo config and TOML formatting"). | `cargo install taplo-cli --locked` (the npm `@taplo/cli` 0.7 WebAssembly build finds no files on Windows) |
| Markdown | markdownlint-cli2 0.23 | `.markdownlint-cli2.jsonc`: defaults, every `*.md` in the repo. `MD013` (line length) off: prose is one paragraph per line or wrapped by hand, the rendered output does not depend on it, and reflowing ~250 documents would bury real changes. `MD041` takes a front-matter `name` as the title (memory files). | npm |
| Spelling | typos 1.50 | `typos.toml`: excludes lock files, binary / byte-exact test data, generated code, German text; legitimate tokens (`HELO`, `UNAUTHENTICATE`, `FPR`, `EDE`, `X-GM-THRID`, …) listed with their reason | PyPI `typos` (binary wheel) |
| GitHub workflows | actionlint 1.6.25 | defaults | PyPI `actionlint-py==1.6.25.2`: the newest release with a Windows binary wheel; later releases are source-only and download the binary from GitHub during install, which the "registries only" rule excludes. Upgrade when a wheel exists. Without shellcheck on PATH, `run:` scripts are not shell-checked. |
| Every text file | `scripts/check-editorconfig.mjs` (on the npm `editorconfig` parser) | `.editorconfig`: UTF-8, LF, final newline, no trailing whitespace, spaces (2; 4 for Rust, TOML, KDL, Python, PowerShell, NSIS, WiX); byte-exact test data (`*.eml`, `*.ics`, keys, corpora, fixtures, goldens, visual baselines) unset | npm (own script, 0 findings) |
| Rust dependencies | cargo machete 0.9 | defaults | `cargo install cargo-machete --locked` |
| Licences, bans, sources, advisories | cargo deny | `deny.toml` (unchanged) | `cargo install cargo-deny --locked` |

The npm `editorconfig-checker` downloads its binary from GitHub, and the
PyPI build's newest wheel is from 2021 (104 MB), so the EditorConfig check
is a small script with unit tests instead.

### One entry point: `npm run check`

`scripts/check.mjs` (steps in `scripts/lib/check-steps.mjs`) runs, in this
order: rustfmt (workspace + `scripts/interop`), taplo fmt / check, Prettier,
EditorConfig, typos, markdownlint, actionlint, ESLint (`--max-warnings 0`),
tsc (`npm run lint:types`), clippy (`--workspace --all-targets -- -D
warnings`), rustdoc (`RUSTDOCFLAGS=-D warnings`), cargo machete, cargo deny.
It runs every step (unless `--fail-fast`), prints a summary and exits 1 when
a step failed or its tool is missing (with the install command). Cargo runs
with `-j $CARGO_BUILD_JOBS` (default 4, shared machine). `cargo deny`
checks advisories only with `--advisories` or in CI, because the RustSec
database comes from the network; locally it checks licences, bans and
sources. npm tools run through node (`<bin:pkg>`), so no shell is involved;
the PyPI tools are found on PATH or in Python's script directories.
`--only` / `--skip` / `--list` select steps.

CI: a new blocking job `quality` (windows-latest) installs taplo-cli,
cargo-machete and cargo-deny from crates.io and the PyPI tools from
`requirements-dev.txt`, builds the UI (the Tauri crate embeds it) and runs
`npm run check`. It is red until the parallel packages land.
`scripts/test/ci-workflow.test.mjs` keeps the job blocking.

### Pre-commit hook

`.githooks/pre-commit` → `scripts/pre-commit.mjs`; `npm install` / `npm ci`
set `core.hooksPath` through the `prepare` script
(`scripts/install-hooks.mjs`, a no-op outside a git checkout). It checks
only the staged files (added, copied, modified, renamed), in parallel:
rustfmt, Prettier (`--ignore-unknown`), ESLint (`--max-warnings 0`), taplo
fmt / check, markdownlint, typos (`--force-exclude`) and EditorConfig.
It reads the working-tree copy of each staged file, so stage whole files.
A missing tool fails the hook with its install command. Never bypass it.

### Licences of the new tools

All new npm dev dependencies are MIT: typescript-eslint, `@eslint/js`,
globals, eslint-plugin-react, eslint-plugin-react-hooks,
eslint-plugin-jsx-a11y (+ `@types/eslint-plugin-jsx-a11y`),
`@eslint-community/eslint-plugin-eslint-comments`, eslint-config-prettier,
markdownlint-cli2, editorconfig; `typescript` is Apache-2.0. The tools
installed outside npm (taplo-cli MIT, cargo-machete MIT, cargo-deny
MIT / Apache-2.0, typos MIT / Apache-2.0, actionlint MIT) are not linked
into Nail. `npm audit` reports a ReDoS advisory in `braces` (via
markdownlint-cli2 → globby → micromatch) with no fixed release; it only
sees our own glob patterns, at development time.

## Findings at the contract (the parallel packages' work)

The full report (per crate, per lint, per area, per rule) is
`%LOCALAPPDATA%\nail-build\q1\findings.md`, next to the raw outputs
(`clippy.json`, `doc.json`, `eslint.json`, `tsc-root.txt`,
`markdownlint.txt`, `typos.txt`, `machete.txt`). Clippy was measured with
the two deny-level lints overridden to warn so every crate compiled.

| Package | clippy | rustdoc | ESLint | tsc | markdownlint | typos | machete |
| --- | --- | --- | --- | --- | --- | --- | --- |
| q-rust-a | 1485 | 15 | 305 (`prelude.js`) | 147 (`prelude.js`) | 0 | 12 | 3 |
| q-rust-b | 593 | 5 | 0 | 0 | 0 | 18 | 1 |
| q-rust-c | 974 | 3 | 0 | 0 | 3 | 19 | 8 |
| q-ts-ui | 0 | 0 | 3554 | 0 | 1 | 7 | 0 |
| q-ts-rest | 44 (interop) | 0 | 2059 | 507 | 124 | 23 | 0 |
| **Total** | **3096** | **23** | **5918** | **654** | **128** | **79** | **12** |

rustfmt, taplo (after the formatting commit), Prettier, EditorConfig,
actionlint and cargo deny (licences, bans, sources) report nothing.

Largest clippy lints: `unwrap_used` 755 (nail-core alone 246), `unreachable_pub`
331, `let_underscore_drop` 263, `expect_used` 108, `format_push_string` 106,
`redundant_closure_for_method_calls` 101, `cast_possible_truncation` 97,
`borrow_as_ptr` 96 (mostly nail-mapi / nail-exchange FFI), `trivial_casts` 92,
`single_match_else` 90, `duration_suboptimal_units` 89, `allow_attributes*`
94. Largest crates: nail-core 614, nail-config 178, nail-plugins 177,
nail-mcp 169, nail-proto-ews 141, nail-store 140, nail-mapi 122, nail-app 115.
The rustdoc-only findings are 15 `private_intra_doc_links`, 6
`broken_intra_doc_links` and 2 `invalid_html_tags` (`cargo doc` also repeats
the rustc lints of the library targets).

ESLint: `ui` 3554 (mostly autofixable: `no-confusing-void-expression` 1553,
`array-type` 445, `consistent-type-definitions` 346,
`non-nullable-type-assertion-style` 233; then `unbound-method` 316,
`restrict-template-expressions`), `scripts` 1355 and `packages/plugin-api`
354 (untyped JavaScript: `no-unsafe-*`; JSDoc types fix them and the tsc
errors together), `e2e` 287 (`no-non-null-assertion` 77),
`crates/nail-plugins/src/prelude.js` 305. Most Markdown findings are
missing blank lines around headings and lists (`--fix` handles them) and
emphasis used as a heading.

## Consequences

- Every kind of file in the repository has a formatter or linter, and one
  command checks them all; CI blocks on it.
- Until the packages land, `npm run check`, `npm run lint` and CI's
  `quality` / `build-test` lint steps are red; `cargo build`, `cargo test`,
  `npm test`, `npm run e2e:only` and `cargo clippy` without `-D warnings`
  still work. Files this contract touched (configs, scripts, the docs it
  edited) are already clean, so their commits pass the hook.
- `cargo clippy -- -D warnings` now covers pedantic; new code is written
  clean from the start.
- Type-aware ESLint over the whole repository takes about 3 minutes on the
  reference machine; the hook lints only staged files.
