# Developer setup

How to build, test and run Nail on a development machine. Windows 11 (x64)
is the primary platform. User-facing documentation lives in
[`userDocs/`](../userDocs/); this page is for people working on Nail.

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | stable (1.99; `rust-version` 1.97), `x86_64-pc-windows-msvc` | `rustup component add rustfmt clippy` |
| Visual Studio Build Tools | 2022, "Desktop development with C++" | MSVC linker; SQLite and zstd are compiled from source |
| Node.js / npm | 26 / 11 | npm workspaces (`ui`, `e2e`, `packages/*`); **not** pnpm (ADR 0001) |
| WebView2 runtime | evergreen | Preinstalled on Windows 11; needed by the desktop app only |
| Playwright Chromium | matches `@playwright/test` | `npx -w e2e playwright install chromium`, once per machine |
| Lint tools (Q1, ADR 0123) | taplo-cli 0.10, cargo-machete 0.9, cargo-deny; typos 1.50, actionlint 1.6.25 | `cargo install taplo-cli cargo-machete cargo-deny --locked`; `pip install -r requirements-dev.txt` (Python 3; the scripts directory need not be on PATH, `npm run check` finds it). Everything else comes with `npm ci` |

Docker is not needed. Integration tests use the in-process mock servers from
`crates/nail-testkit` (IMAP) and `crates/nail-testkit-net` (SMTP, POP3,
OAuth2, HTTP, DNS; M1a).

### Work folders (ADR 0132)

`<master>` is the **master repository**. It
holds `main`, receives pushes and fast-forward merges, and nothing is built,
tested, run, installed or committed there (releases included). All work
happens in clones outside OneDrive, with `origin` pointing to the master:

| Folder | Branch | Use |
| --- | --- | --- |
| `<work root>\nail-work-1` | `work-1` | the lead: contracts, integration, reviews, fixes, verification, releases |
| `nail-work-2`, `nail-work-3`, … | `work-2`, `work-3`, … | one per parallel work package |

```sh
node scripts/work-folders.mjs create 6        # from a work folder: clone the master into nail-work-6 (branch work-6), npm ci
git fetch origin && git reset --hard origin/main  # a package folder starts from main (clean tree first)
git push --force-with-lease origin work-3       # a package is done (pre-push runs npm run check); the lead merges origin/work-3 into work-1
git push origin work-1 && git -C <master> merge --ff-only work-1   # after each green lead stage: main = work-1
node scripts/work-folders.mjs guard-master    # once: Cargo in the master refuses every build
```

`main` is always green: it only moves to a `work-1` commit whose full suite
passed. Hooks in every work folder: pre-commit = the fast per-file checks,
pre-push = `npm run check` (`.githooks/`), and Claude Code's post-edit hook
runs the per-file checks on every file an agent edits
(`.claude/settings.json` → `scripts/post-edit.mjs`, which finds the work
folder from the edited file and runs that folder's checks; findings go back
to the agent; files inside OneDrive are skipped).

Guards: `npm run dev` / `check` / `test` / `visual` / `tauri` /
`build:desktop`, the perf runner, the installer build, `npm ci`'s `prepare`
and the Git hooks refuse inside any OneDrive folder
(`scripts/lib/work-folder.mjs`). In the master, the gitignored `.cargo/config.local.toml`
(included by the committed `.cargo/config.toml`)
names a refusing `rustc-wrapper`. `create` also writes the folder's
`.claude/settings.local.json`, so Claude Code's memory stays in the checkout
(`.claude/memory`). The owner's Claude Code session runs in the master
folder and is only the launch point: it drives the work
folders by absolute path, and every edit, build, test and commit happens
there.

Cargo builds into each work folder's `target/`. Give parallel cargo
processes in one folder their own `CARGO_TARGET_DIR` (under `.nail-build/`).
Machine-specific cargo settings go into the gitignored
`.cargo/config.local.toml`, which the committed `.cargo/config.toml`
includes.

Build and test output (Playwright, visual, perf, the desktop test build,
installers) goes to the checkout's gitignored `.nail-build/`
(`NAIL_BUILD_DIR` overrides the base); the development update key stays in
`%LOCALAPPDATA%\nail-dev\keys`, outside every work folder.

## Install and build

```sh
npm ci                       # JS dependencies for ui/, e2e/ and packages/*
npm run build                # ui/dist (the Tauri shell embeds it; nail serve serves it)
cargo build                  # all crates except the desktop shell
cargo build -p nail-app      # the desktop shell (needs ui/dist)
```

`cargo build` / `cargo test` without `--workspace` skip `app/src-tauri`
(`default-members`). Use `--workspace` after `npm run build` to include it.

## Test

| What | Command |
| --- | --- |
| Rust unit and integration tests | `cargo test --workspace` |
| Every formatter check and linter, 0 warnings (below) | `npm run check` |
| Rust lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| rustdoc lints | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` |
| Rust formatting | `cargo fmt --all --check` |
| Socket-bind check (ADR 0057), then UI unit tests (Vitest) incl. the lint-rule self-test, then the `test` scripts of `packages/*` | `npm test` |
| UI unit tests with their console output, also when piped | `cd ui && npx vitest run --silent=false --reporter=default` (no `stderr \|` block expected) |
| Socket-bind check only | `npm run check:sockets` |
| ESLint (type-aware, whole repository, `--max-warnings 0`), tsc (every tsconfig), Prettier (whole repository) | `npm run lint` |
| Markdown, TOML, EditorConfig | `npm run lint:md`, `npm run lint:toml`, `npm run lint:editorconfig` |
| Playwright end to end (builds UI, `nail` and the desktop app first) | `npm run e2e` |
| Playwright only (already built) | `npm run e2e:only` |
| The desktop app for the WebView2 smoke test | `npm run build:desktop` |
| Installer and release scripts (node:test) | `node --test "scripts/test/*.test.mjs"` |
| The Simple MAPI DLL as x86 (32-bit Office) under WOW64 | `cargo test -p nail-mapi --target i686-pc-windows-msvc` (once: `rustup target add i686-pc-windows-msvc`) |
| Licences, bans, sources, advisories | `cargo deny check` (advisories need the RustSec database from GitHub) |
| The performance benchmark (M4c, budgets in `e2e/perf/budgets.json`) | `cargo run -p nail-bench --release -- --corpus small` (`standard` = 100k messages; `--tolerant` = CI tolerance 1.5, `--no-check`, `--in-process`, `--work-dir DIR`) |
| Merge benchmark reports and check the budgets | `nail-bench check --report A --report B [--tolerant] [--out F]` |
| The whole performance job (Rust run → Playwright scroll / first render → check) | `node e2e/perf/run-perf.mjs [--corpus standard\|small] [--tolerant]` after `cargo build --release -p nail-cli -p nail-bench` and `npm run build`; reports in `.nail-build/perf/<corpus>` |
| Only the UI perf scenario | `npx playwright test -c e2e/perf/playwright.perf.config.ts` (env `NAIL_PERF_CORPUS`, `NAIL_PERF_WORK`, `NAIL_PERF_TOLERANT`, `NAIL_PERF_FRESH`, `NAIL_BIN`, `NAIL_BENCH_BIN`) |

### Linters and formatters (Q1, ADR 0123)

The rule: **0 warnings, 0 errors.** Never lower a lint level, never switch an
ESLint rule off, never relax `-D warnings`, never commit with `--no-verify`.
An unavoidable exception is written at the narrowest scope with its reason:
`#[expect(lint, reason = "…")]` in Rust (`#[allow]` is rejected by clippy) or
`// eslint-disable-next-line rule -- reason` in TypeScript / JavaScript.
Deliberate non-English text or misspellings (German test strings, a broken
config in an error example) end their line with
`// spellchecker:disable-line`, or sit between `spellchecker:off` and
`spellchecker:on` comments (`//`, `#` or `<!-- -->`); see `typos.toml`.

Habits of the type-aware TypeScript / JavaScript rules (ADR 0127):

- Node scripts are typed with JSDoc. Parse JSON with `parseJson` /
  `readJson` (`scripts/lib/json.mjs`, they answer `unknown`) and narrow or
  cast the result; `JSON.parse` answers `any`, which the `no-unsafe-*`
  rules reject wherever it flows, and a JSDoc cast is invisible to ESLint.
- No non-null assertions: Playwright code uses `defined(value, "what")`
  (`e2e/support/defined.ts`), which fails with a clear message; elsewhere
  use `?? fallback`, `?.` or a check.
- Numbers in template literals go through `String(n)`.
- An `async` function needs an `await`; a test double that answers a
  promise without awaiting anything returns `Promise.resolve(…)` (or
  `asPromise(() => …)` in the plug-in test host, which keeps the rejection
  semantics of `async`).
- UI code (ADR 0128): tests use `must(value)` (`ui/src/test`) instead of
  `!`; text length is `charCount` / `chars` (counts like the Rust core) and
  "empty means missing" is `nonEmpty` / `hasText` (`ui/src/text`), not `||`.
- Feature barrels (`ui/src/features/*/index.ts`) never re-export a mock
  module (`mock.ts`, `mockSeed.ts`, `*Mock.ts`): that pulls the whole mock
  core into the app bundle (ADR 0129). Tests import mocks from the mock
  module itself; rules the UI needs live in a `model.ts`.

Habits of the Rust lints (ADRs 0124–0126, 0129):

- Every integration-test crate root (`tests/*.rs`, `tests/*/main.rs`)
  starts with `#![cfg(test)]`, so `clippy.toml`'s `allow-unwrap-in-tests` /
  `allow-expect-in-tests` also cover its helper functions. A test crate that
  uses only part of `tests/common` declares
  `#[expect(dead_code, reason = "…")] mod common;`.
- No `#[allow(…)]` anywhere (`clippy::allow_attributes` is deny); an
  expectation that no longer fires warns, so remove it with the cause.
- `let _ = value;` → `drop(value);` (same drop point) or, for a
  best-effort cleanup, log the error at debug level.
- Narrowing casts use `try_from` with a fallback or an error; float
  comparisons in tests that are exact by construction carry
  `#[expect(clippy::float_cmp, reason = "…")]` on the test.
- Types that hold secrets get a manual `Debug` that hides them.

Builds and platforms (ADR 0130):

- `vite build` (`npm run build`) fails on any warning: `ui/build/strictBuild.ts`
  records what Vite and Rolldown warn about and fails the build after
  listing it. Fix the cause (for example a barrel that pulls the mock core
  back in), and never raise `chunkSizeWarningLimit`.
- rustc warnings are enforced through clippy `-D warnings`. Code under
  `cfg(not(windows))` is linted by CI's `quality-linux` job (ubuntu), not on
  Windows. A crate that gains such a branch belongs in that job's `-p` list
  (`scripts/test/ci-workflow.test.mjs` checks it).
- Clap help text is the doc comment, printed verbatim. Do not put backticks
  in it; name environment variables in an `about` / `help` attribute.

The Vite configs that import a symlinked workspace package set
`resolve.preserveSymlinks: true` (`ui/vite.config.ts`,
`packages/create-nail-plugin/vitest.config.ts`). It dates from checkouts
inside OneDrive, where `node_modules` is a cloud-file reparse point that
Vite cannot canonicalize ("Failed to resolve import", or Node loading a
package's `.ts` sources natively). Work folders are outside OneDrive
(ADR 0132), but the setting stays harmless and keeps other checkouts
working.

| Files | Tool | Config | Fix |
| --- | --- | --- | --- |
| Rust | rustfmt; clippy (`all` + `pedantic` + restriction picks, `-D warnings`); rustdoc lints | `rustfmt.toml`, `[workspace.lints]` in `Cargo.toml`, `clippy.toml` | `cargo fmt --all`, `cargo clippy --fix` |
| TS / JS (ui, e2e, packages, examples, scripts, `crates/nail-plugins/src/prelude.js`) | ESLint (typescript-eslint `strictTypeChecked` + `stylisticTypeChecked`, type-aware; React, Hooks, jsx-a11y and the Nail rules in `ui/`), tsc | `eslint.config.base.js`, `eslint.config.js`, `ui/eslint.config.js`, the root `tsconfig.json` (loose Node scripts) | `npx eslint --fix <files>` |
| TS, JS, JSON, YAML, CSS, HTML | Prettier | `.prettierrc.json`, `.prettierignore` | `npm run format` |
| TOML | taplo | `taplo.toml` | `taplo fmt` |
| Markdown | markdownlint-cli2 (line length off for prose) | `.markdownlint-cli2.jsonc` | `npx markdownlint-cli2 --fix "**/*.md"` |
| Everything | typos | `typos.toml` (add a legitimate word with its reason) | `typos --write-changes` |
| Workflows | actionlint | — | — |
| Every text file | EditorConfig check (`scripts/check-editorconfig.mjs`) | `.editorconfig` | your editor |
| `Cargo.toml` dependencies | cargo machete; cargo deny | `deny.toml` | remove the dependency |

`npm run check` (`scripts/check.mjs`) runs all of them and prints a summary;
`node scripts/check.mjs --list` shows the steps, `--only a,b` / `--skip a,b`
select some, `--fail-fast` stops at the first failure. Cargo runs with
`-j $CARGO_BUILD_JOBS` (default 4). `cargo deny` checks advisories only with
`--advisories` or in CI (the RustSec database comes from the network).

The **pre-commit hook** (`.githooks/pre-commit`, enabled by `npm install` /
`npm ci` through the `prepare` script, which sets `core.hooksPath`) runs
rustfmt, Prettier, ESLint, taplo, markdownlint, typos and the EditorConfig
check on the staged files only, in parallel (measured at Q1: about 5 s for a
commit with TypeScript files, most of it type-aware ESLint starting up;
0.2 s for Rust / Markdown / TOML only). It reads the
working-tree copy of each staged file, so stage the whole file you fixed.

### Console output in UI tests

Vitest hides console output when stdout is not a terminal (piped output, CI
logs, coding agents), so warnings printed by a test were easy to miss.
`ui/src/test/setup.ts` turns them into failures: a test that logs
`console.error` or `console.warn` fails — React's "An update to X inside a
test was not wrapped in act(...)", `nail: missing message "…"`,
`nail: formatting "…"`, a failed action, anything else. A test that expects
such output declares it with `expectConsole(/pattern/)` (must appear) or
`allowConsole(/pattern/)` (may appear) from `ui/src/test/console.ts`; lines
logged after a test's checks fail the next test (or the file, after the last
one). Prefer fixing the cause:

- await the state the UI settles into (`findBy*`, `waitFor`, the harness's
  `settleQueries(env)` for pending queries);
- wrap direct store updates, dispatched DOM events and timed waits in `act`;
- call `cleanup()` before resetting a module-level store in `afterEach`
  (Testing Library unmounts only after the file's own `afterEach` hooks);
- mock what the test triggers (e.g. install the plug-in in the mock core);
- give fixture ids catalog entries (test catalogs, not the shipped files);
- run axe through `ui/src/test/axe.ts` (inside `act`, so the DOM does not
  change during the check).

`NAIL_CONSOLE_STACK=1 npx vitest run <file>` adds the call stack to each
reported line, which shows the state update behind an act(...) warning.

### Fast and stable UI tests (P1, ADR 0143)

- `ui/src/test/setup.ts` installs `ui/src/test/fastStyles.ts`: jsdom's
  `getComputedStyle` skips theme rules whose subject needs another
  `data-part`. `fastStyles.test.tsx` compares computed styles with and
  without the filter; if it fails after a jsdom upgrade, adapt the filter.
- Before unmounting, `setup.ts`'s `afterEach` waits (at most 3 s, never
  failing, skipped under fake timers) until every env made by `createEnv` is
  idle (`ui/src/test/liveEnvs.ts`), so late answers do not cause act(...)
  warnings.
- The axe helper (`ui/src/test/axe.ts`) reports violations only.
- Prefer `userEvent.setup({ delay: null })` in widget tests.
- Do not raise a test's timeout to hide load flakiness; find the cost (a CPU
  profile of the slow test) instead. Avoid `:has()` with a descendant search
  in skins: jsdom's style matching (and so every axe check) pays for it.

The catalog coverage test (`ui/src/i18n/coverage.test.ts`) also types every
template-string message id with the TypeScript checker: for
``t(`pim-view-${view}`)`` with a literal union type every value needs a
message; a plain-`string` value must be listed in `OPEN_TEMPLATES` with the
`runtimeIds()` that cover it. Every key context the UI sets
(`data-key-context`), the default layout binds or a plug-in manifest accepts
must be in `KEY_CONTEXTS` (`ui/src/features/customization/editorModel.ts`)
with a `customize-keys-context-*` label.

CI (`.github/workflows/ci.yml`) runs these jobs:

| Job | Runner | What |
| --- | --- | --- |
| `quality` | `windows-latest` | `npm run check` (every formatter and linter, 0 warnings; tools from crates.io and PyPI); blocking (ADR 0123) |
| `build-test` | `windows-latest` | build, `cargo test --workspace`, clippy, fmt, Vitest, lint, the Playwright web tests, the script tests |
| `desktop` | `windows-latest` | `npm run build:desktop`, then the WebView2 smoke tests (`desktop.spec.ts`, `desktop-m4b.spec.ts`, `desktop-m4c.spec.ts`, `desktop-m5c.spec.ts`) with `NAIL_E2E_DESKTOP=1` |
| `mapi` | `windows-latest` | `nail-mapi` tests as x86 under WOW64, release builds of `nail_mapi.dll` for x64 / x86 / ARM64, every export checked on the x64 and x86 DLLs (`nail-mapi-harness exports`) |
| `installer` | `windows-latest` | `node scripts/build-installer.mjs --offline-webview …` (NSIS, MSI, offline-WebView2 NSIS, portable zip, SBOMs, update signatures with a throwaway key), the next patch version as the update, the loopback update server, `installer-smoke.ps1` (install → start → update → start → uninstall), `msi-smoke.ps1` (elevated silent MSI install with properties → start: the app reports msi-managed updates → upgrade → uninstall), winget manifests; uploads everything |
| `installer-arm64` | `windows-latest` | the ARM64 installers, cross-built (not run: no ARM64 runner) |
| `deny` | `ubuntu-latest` | `cargo deny check` |
| `interop` | `ubuntu-latest` | clippy `-D warnings` on the interop crate, then the engines against real Dovecot and Stalwart (below) |
| `perf` | `windows-latest` | the 100k-message benchmark (`node e2e/perf/run-perf.mjs --corpus standard --tolerant` after release builds of `nail-cli` / `nail-bench` and the UI); uploads `report*.json` and the scroll trace; 60 minutes at most (M4c, ADR 0097) |

`.github/workflows/release.yml` (tags / manual) builds the signed x64 and
ARM64 packages with the release update key, writes the per-channel update
manifests and the winget manifests, and drafts a GitHub release (ADR 0089).
Repository settings it reads: secrets `NAIL_SIGN_PFX_BASE64` /
`NAIL_SIGN_PFX_PASSWORD` (optional; unsigned without), `NAIL_UPDATER_PRIVATE_KEY`
/ `NAIL_UPDATER_KEY_PASSWORD`; variables `NAIL_UPDATER_PUBKEY`,
`NAIL_UPDATE_BASE_URL`. Key handling, signing and the release steps:
[`docs/release.md`](release.md).

### Installer, portable zip and SBOMs (ADR 0040)

```sh
node scripts/build-installer.mjs [--skip-ui] [--arch x64|arm64] [--bundles nsis,msi] [--offline-webview]
    [--version V] [--update-base URL [--insecure-loopback-updates]] [--dev-updater-key] [--no-mapi] [--no-portable]
                                                  # NSIS + MSI, portable zip, NailMapi.dll (own arch + x86), SBOMs, SHA256SUMS → <build dir>/dist
powershell -File scripts/installer-smoke.ps1      # silent per-user install to a temp dir, start, [update,] uninstall
powershell -File scripts/msi-smoke.ps1            # elevated: silent MSI install with properties, start, upgrade, uninstall (CI)
node scripts/update-manifest.mjs --version V --channel stable --artifacts DIR --download-base URL --out DIR   # <channel>/latest.json, signatures checked first
node scripts/update-server.mjs --root DIR [--port N]   # loopback update server (127.0.0.1 only)
node scripts/update-check.mjs …                   # checks an endpoint and a download like the app does
node scripts/updater-key.mjs                      # the development update key (%LOCALAPPDATA%\nail-dev\keys; only packaging/updater/dev.pub is committed)
node scripts/winget-manifest.mjs --version V --artifacts DIR --installer-url-template T --out DIR
node scripts/sbom.mjs                             # CycloneDX SBOMs of the Rust and npm dependencies
```

Signing runs only when `NAIL_SIGN_THUMBPRINT` is set (`scripts/lib/sign.mjs`;
CI imports a `.pfx` from secrets with `scripts/import-signing-cert.ps1` and
checks the result with `scripts/verify-signatures.ps1`). The MSI is Tauri's
WiX template plus one fragment (`app/src-tauri/windows/wix/nail.wxs`, added
through `app/src-tauri/windows/bundle.json`, which also pins the upgrade
code). Installers, updates and winget: ADR 0089, `packaging/README.md`;
keys, signing and releasing: `docs/release.md`.

Nothing is written into the repository. The installer's NSIS template is a
copy of Tauri's (`app/src-tauri/windows/nsis/installer.nsi`); a test fails when
the Tauri CLI version changes, so re-copy it on upgrades.

### Interop tests (real servers, CI job `interop`)

`scripts/interop/` is its own Cargo workspace (copy the root `Cargo.lock`
into it first); see `scripts/interop/README.md`. It needs Docker, so it runs
in CI only; without `NAIL_INTEROP=1` the server tests return at once.
The servers are Dovecot and Stalwart (mail, ManageSieve) and, since M3a,
**Radicale** on port 5232 over TLS for CalDAV / CardDAV
(`NAIL_INTEROP_DAV=radicale=https://localhost:5232/`; `start-servers.sh`
starts `tomsquest/docker-radicale`, `NAIL_INTEROP_RADICALE=0` skips it;
ADR 0060). The DAV checks of `tests/dav.rs` also run locally without
Docker against the in-process mock (`MockDavServer` over TLS), and can be
pointed at a Radicale installed from PyPI (see the interop README).

**Owner-run checks (M3 human gate).** Microsoft and Google services cannot
run in CI; the automated suite covers them with recorded-style fixtures
and the mock servers (`MockGraphServer`, `MockEwsServer`,
`MockAutodiscoverServer`, `MockGmailServer`). Before the M3 gate the owner
runs, by hand:

- **Exchange Online** (a Microsoft 365 test tenant): add the account
  through Autodiscover (Graph, Microsoft sign-in with the app's client id),
  then the `exchange.spec.ts` checklist by hand — mail sync / flag / move /
  categories / send, a shared mailbox, automatic replies, a Nail rule
  placed as an inbox rule, the room finder (needs `Place.Read.All` admin
  consent), a meeting answered, a To Do task. Also confirm the behaviours
  listed as owner checks in ADR 0073 (nested folders in `mailFolders/delta`,
  `cancelledOccurrences`, `If-Match` change keys, MIME creates outside
  Drafts) and the EWS retirement date (ADR 0075).
- **Exchange Server 2019 / SE**: the same with EWS — Windows sign-in as the
  domain user (Kerberos), with explicit credentials and with NTLM
  (`cargo test -p nail-exchange --test sspi_live -- --ignored` with
  `NAIL_EWS_LIVE_URL`, optionally `NAIL_EWS_LIVE_PACKAGE=ntlm`), streaming
  notifications, the `CategoryList`, `ResolveNames` (ADR 0074).
- **Google**: a test account with the restricted Gmail scope on the Google
  OAuth client: the `gmail.spec.ts` checklist by hand, Google CalDAV /
  CardDAV with the calendar and contacts scopes.

### Dependency rules

No GPL / LGPL / AGPL dependency, no OpenSSL and no aws-lc (TLS uses
`ring`; ADRs 0029 and 0036). `deny.toml` enforces the licence allowlist,
bans `openssl-sys` / `aws-lc-*` and allows only crates.io: run
`cargo deny check` after adding a crate (CI runs it too). It records the
RUSTSEC-2023-0071 exception (`rsa` 0.9, ADR 0034) and one licence
exception: `sqlite3mc-src` carries `Unlicense` for its bundled miniz, which
Nail's build does not compile (ADR 0095).

### SQLite3 Multiple Ciphers (ADR 0095)

The bundled SQLite is SQLite3 Multiple Ciphers (full-file database
encryption). `rusqlite`'s `libsqlite3-sys` is replaced through the root
`[patch.crates-io]` by `crates/nail-store/libsqlite3-sys-sqlite3mc`, a
stand-in with the original crate's API and pre-generated bindings that
compiles the amalgamation from the `sqlite3mc-src` crate (no OpenSSL, no
bindgen). The first build compiles it (about 80 s in debug). To upgrade:
bump `sqlite3mc-src` together with `rusqlite`, copy the new
`libsqlite3-sys` version's `src/` and bindings into the stand-in (keep its
`build.rs`), set the stand-in's version to the new `libsqlite3-sys`
version, then run `cargo test -p nail-store -p nail-vault` and
`cargo deny check`.

### Sockets and the Windows firewall (ADR 0057)

Nail and its tests never open a socket on a non-loopback or unspecified
address, so Windows Defender Firewall never asks to "allow this app". The
one exception is the remote UI when the user enables it on a non-loopback
address. DNS lookups go through the OS resolver (`DnsQuery_W`), the mock
DNS server is asked from `127.0.0.1`, and every mock server and test
listener binds `127.0.0.1`. `npm test` runs `scripts/check-socket-binds.mjs`,
which fails on new binds to `0.0.0.0` / `[::]` (allowlist with reasons in
`scripts/lib/socket-binds.mjs`).

Builds before this rule left firewall rules for hashed test binaries
(`discover-<hash>.exe`, `engines-<hash>.exe`) and `nail.exe` under
`%LOCALAPPDATA%\nail-build`. To remove those inbound rules, run in an
**elevated** PowerShell as the same user (it only touches rules whose
program lies under that folder):

```powershell
Get-NetFirewallApplicationFilter | Where-Object { $_.Program -like "$env:LOCALAPPDATA\nail-build\*" } | Get-NetFirewallRule | Where-Object Direction -eq Inbound | Remove-NetFirewallRule
```

Leave out `| Remove-NetFirewallRule` first to see what would go.

### End-to-end tests (Playwright)

The suite in `e2e/` drives the real web UI served by the real core: the
prebuilt `nail serve --seed demo` binary with temporary data and config
directories. It builds nothing itself (ADR 0010). `npm run e2e:only` needs,
first:

```sh
npm run build -w ui                      # ui/dist, served by nail serve
cargo build -p nail-cli                  # the nail binary (debug profile)
npm run build:desktop                    # nail-app for the desktop smoke test
npx -w e2e playwright install chromium   # once per machine
```

How it works:

- `e2e/playwright.config.ts` starts one shared server
  (`nail serve --seed demo --port <free port> --data-dir <tmp> --config-dir <tmp> --ui-dir <repo>/ui/dist`)
  for the read-only scenarios. The runner picks the free port once and
  hands it to the workers in `NAIL_E2E_PORT`; set it yourself to pin one.
- Scenarios that edit `theme.kdl` / `layout.kdl`, use another locale or
  another seed start a private server on port 0 (`startNail()` in
  `e2e/support/nail.ts`), so they never disturb each other.
- The M1a scenarios start their own `--seed demo-multi` server each: two
  configured accounts on in-process mock servers (see "Run" below). They
  drive the mocks through the server's test controls with `control()` /
  `serverMessages()` from `e2e/support/nail.ts`.
- The binary is found through `cargo metadata` (the target directory lives
  outside the repository). `NAIL_BIN` selects another binary, `NAIL_LOG`
  (e.g. `info`) shows server logs. Every server gets `--ui-dir
  <repo>/ui/dist` explicitly: building `nail-app` copies Tauri's resources
  into `target/debug/ui`, which `nail serve` would otherwise prefer (a
  stale bundle after a UI rebuild).
- The desktop smoke test (`e2e/tests/desktop.spec.ts`) starts the real
  `nail-app.exe --seed demo` with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<free port>`
  set only in that child's environment, and attaches with
  `chromium.connectOverCDP` (`e2e/support/desktop.ts`). The app is built by
  `npm run build:desktop` (`e2e/scripts/build-desktop.mjs`: `nail-app` with
  `tauri/custom-protocol`, so it serves the embedded `ui/dist`) into its own
  target directory, `.nail-build/desktop-target`, so the
  feature does not force rebuilds of the shared target directory. The same
  build enables the test-only cargo features `nail-app/test-overrides` and
  `nail-mapi/test-overrides`: only then do the app and the DLL honour
  `NAIL_REGISTRY_ROOT`, `NAIL_UPDATE_ENDPOINT` / `NAIL_UPDATE_PUBKEY`,
  `NAIL_MAPI_PIPE`, `NAIL_MAPI_APP` and `NAIL_TEST_BACKGROUND_WINDOWS` (a
  plain `cargo build` or an installer build ignores them; ADR 0093).
  `startDesktop` sets `NAIL_TEST_BACKGROUND_WINDOWS=1` (ADR 0146): the test
  apps' windows are cloaked, never activated and off the taskbar, so they
  never take your mouse or keyboard while the suites run; `connectDesktop`
  and the specs check it (`expectWindowsInBackground`, test control
  `windows`). Run with `NAIL_TEST_BACKGROUND_WINDOWS=0` to watch them.
  `NAIL_APP_BIN` selects another binary. A seeded app keeps its data,
  config and WebView2 profile in a temporary directory. The test always
  runs locally on Windows; in CI it is skipped unless `NAIL_E2E_DESKTOP=1`,
  which the CI workflow sets after `npm run build:desktop` (ADR 0028).
- The global setup (`e2e/support/global-setup.ts`) builds the example
  plug-ins (`follow-up`, `attachment-check`, `csv-preview`) once (esbuild +
  `@nail/plugin-api/tools`' `packPlugin`, `buildExample` in
  `e2e/support/plugins.ts`) into `.nail-build/e2e/plugins/`
  and hands the package paths to the workers (`NAIL_E2E_EXAMPLE_<NAME>`,
  read with `examplePackage(name)`).
- The browser window is 1280 × 1000 (M5b: the message list under the list
  header shows every row of the demo inboxes). Shared helpers for the M5b
  shell are in `e2e/support/views.ts`: `showView` / `viewRegion` (a view is
  a region "`<View>` view" in the view host), `openSettings` (a section of the
  settings page) and `openSettingsTab` (the section of a former settings
  tab), `runFromPalette` (actions without a button), `toolbarAction` /
  `readerAction` (a bar's button, else its More menu), `accountSections` /
  `accountSectionNamed` / `sectionInbox` (the folder pane's account
  sections). The app reopens the last view after a reload or restart (UI
  state, shared with remote sessions): switch back to Mail before reloading
  when a test expects the mail list.
- M5c: every message has its own composer window (`composer:<key>`, titled
  by its subject): find it with `composerWindow(page)` (the newest) /
  `composerWindows(page)`, and discard one with `discardComposer(page, w)`
  (it answers the "Discard?" confirmation a saved or changed message asks).
  Windows are non-modal and several can be visible: address a window by
  `[data-part="window"][data-window="<id>"]` rather than a bare
  `getByRole("dialog")`, and use `getByLabel(…, { exact: true })` inside a
  window when the label is part of its title bar's name ("Title bar of
  …"). Remembered geometry lives in the server's config dir
  (`window-state.json`), so scenarios that move windows start their own
  server (`startNail`).
- Traces, screenshots and the CI HTML report go to the checkout's
  gitignored `.nail-build/e2e/` (`NAIL_BUILD_DIR` overrides the base), not
  into tracked files.

| File | Covers |
| --- | --- |
| `e2e/tests/mail.spec.ts` | Default three-pane layout from `data-part` attributes (no inline styles or classes), seeded folders and messages, sandboxed reader iframe with remote images blocked, hostile HTML neutralised, `j`/`k` navigation, German locale falling back to English |
| `e2e/tests/config.spec.ts` | `theme.kdl` hot reload of a CSS custom property (and keep-last-valid on errors), `layout.kdl` moving and removing panes, `--locale de` fallback |
| `e2e/tests/sync.spec.ts` | `--seed demo-imap`: an IMAP account synced from the mock server, `F5` (`sync.now`) round trip |
| `e2e/tests/accounts.spec.ts` | `--seed demo-multi`: unified inbox with two accounts and their status, adding an account through discovery (autoconfig mock), unverified discovery answers shown with a warning (`elsewhere.example` SRV mock), a POP3 account receiving mail, a virtual account merging folders and deduplicating a mailing-list message |
| `e2e/tests/mailops.spec.ts` | `--seed demo-multi`: flag / archive / delete reach the mock IMAP server, a server-side flag change arrives through IDLE, writes made offline replay after reconnecting |
| `e2e/tests/search-threads.spec.ts` | `--seed demo-multi`: the query language (fields, quotes, OR / NOT, error position), a conversation across folders in the card feed |
| `e2e/tests/p2.spec.ts` | `--seed demo-multi` with the demo conversations (`nail_testkit::DEMO_CONVERSATIONS`): conversation rows (participants, "me", count, "Sender: preview", Compact), the card feed (oldest first, folds, Expand / Collapse all, own replies from Sent, keys), Design B through a user `layout.kdl`, the responsive standard; review screenshots in `<build dir>/screens/p2/` |
| `e2e/tests/p2-list.spec.ts` | Conversation rows and expandable rows at every standard size in Frost, Graphite Compact, Nocturne and High contrast (`<build dir>/screens/p1/p2-list-*`) |
| `e2e/tests/p2-quickwin.spec.ts` | A strip-era user layout keeps the strip (newest three, "N earlier messages"); the one-time offer turns it into cards |
| `e2e/tests/send.spec.ts` | `--seed demo-multi`: a plain-text mail cancelled by undo send, then sent to the mock SMTP server, with the Sent copy on the IMAP server; a Sent copy queued while IMAP is offline and filed once it is back |
| `e2e/tests/a11y.spec.ts` | axe-core scan of the main window, the reader and dark mode, and (`--seed demo-multi`) of the composer in each mode, the outbox, filter rules, account setup, the snoozed list, the snooze picker, a folder's context menu and sync dialog, every section of the settings page and the merge dialog; M5b: every built-in theme (Frost, Sage, Graphite, Nocturne, High contrast) over the mail view, the reader's More menu, the calendar view and the settings page, colour contrast included; any violation, whatever its impact, fails |
| `e2e/tests/composer.spec.ts` | `--seed demo-multi`: HTML and Markdown mails (bold text, Markdown source part, attachment, core-rendered preview) with their MIME parts as the mock SMTP server received them; reply quoting in HTML (blockquote) and Markdown (`>`); drafts autosaved to the server's Drafts folder, continued from the outbox after a reload, removed after sending |
| `e2e/tests/rules.spec.ts` | `--seed demo-multi`: an arrival rule (entered in the text view) moves and flags new mail on the IMAP server and its run is undone as one unit; the test-rule preview with "why it matches"; a rule created from a message and written to `rules.kdl` |
| `e2e/tests/mcp.spec.ts` | an MCP client created in the settings is denied until "Read mail" is granted, then reads `nail://accounts` and finds messages with `search_messages` over Streamable HTTP (`e2e/support/mcp.ts`); the activity log shows both |
| `e2e/tests/customization.spec.ts` | the inspector (part path, theme rules, layout node with file positions), dragging a divider writes `layout.kdl`, the merge dialog applies a shared theme and reverts it (backup), the error list shows every error of two broken files and clears one when it is fixed |
| `e2e/tests/security.spec.ts` | `--seed demo-multi` (both accounts have an OpenPGP key and an S/MIME certificate of the test CA): a signed + encrypted OpenPGP mail from Work to Home with `Subject: ...` on the mock SMTP server, read in Home with a verified badge; a tampered signature shows as invalid; an S/MIME sign + encrypt round trip with a valid chain; Autocrypt turns encryption on after one message (`autocrypt-alice.eml`); the sender badge from a trusted `Authentication-Results` (`authserv-id "mx.nail.example"`) and a look-alike link warning; encryption at rest on and off with mail and search intact; an MCP client reads an encrypted message only with the "Encrypted messages" grant |
| `e2e/tests/desktop.spec.ts` | the desktop app over WebView2 CDP: demo mail and reading through Tauri IPC, search, Shift+F10 context menu, the rich composer's core-rendered Markdown preview, the running MCP server; a second app started with a `mailto:` link opens the composer with its fields |
| `e2e/tests/desktop-m4b.spec.ts` | M4b on the desktop app: the first-run screen, About Nail (F1), tray / startup and default-apps settings; `nail-mapi-harness send` through `nail_mapi.dll` → the composer opens with the file attached and the program's note; the tray's New message (seeded-only test control) and start hidden (`--autostart`); the updater on a loopback update server (throwaway key, `nail-fake-installer` as the package) → Restart to install runs it and Nail quits; MSI facts with `DisableUpdates=1` staged below `NAIL_REGISTRY_ROOT` hide the updater; `nail plugin dev` through the desktop endpoint (token) |
| `e2e/tests/remote.spec.ts` | the remote UI (turned on over the local WebSocket: TLS with a self-signed certificate, host name `localhost`) in a second browser context: pairing with a code from the desktop's dialog (fingerprint, used code refused, Trusted Types block an injected script), passkey registration and login with a CDP virtual authenticator, password + TOTP (a used code refused), password guessing slowed down (backoff, audit), session revoke on the desktop logs the browser out, internet mode locked in the settings and in the core, the local UI's CSP blocking an inline script (`e2e/support/remote.ts`: `rpc`, `enableRemoteUi`, `totp`, `trustingBrowser`) |
| `e2e/tests/remote-content.spec.ts` | `--seed demo-multi`: remote images blocked, then loaded through the proxy after "Always load from" the sender; a private target is refused (403), a forged token is a 404, a loopback literal is never signed; "Load once" loads without adding an allow-list entry |
| `e2e/tests/plugins.spec.ts` | plug-ins (private `--seed demo` each): the full Follow-up example installed through Settings → Plug-ins with its six permissions (write marked), placed (toolbar item after Forward and Shift+F in `layout.kdl`), run from the reader toolbar (dialog, the task it creates in the task window, notification, log line; axe on dialog and toast); a plug-in without `mail.read.body` refused a body (toast, log line); a runaway plug-in stopped and flagged while another message opens; `nail plugin dev` loading a folder, its console and a hot reload |
| `e2e/tests/a11y-windows.spec.ts` | axe on every layout window (the views' windows in the view host), settings page section and feature dialog (incl. the plug-in install dialog) in default, high-contrast and dark mode against `--seed demo-multi`; checks its own coverage against the layout |
| `e2e/tests/pim.spec.ts` | `--seed demo-multi` (M3a): a weekly event edited "this and following" (two series); an event created in the CalDAV calendar reaches the mock CalDAV server and another client's event (`dav-put`) appears after "Sync"; the ICS subscription's holiday is read-only and a second feed is added from the sources dialog; a reminder fires at the controlled clock time (`clock-set`) and is dismissed; a contact created in the CardDAV book reaches the server and is suggested in the composer; duplicate contacts (local + CardDAV) merged into one linked person; a task with a subtask completed; an `.ics` given with `--launch` previewed and imported; an MCP client lists events only after a grant |
| `e2e/tests/scheduling.spec.ts` | `--seed demo-multi` (M3b): an iMIP invitation (delivered with `imap-deliver`) is added on arrival and accepted from the reader's card, the `REPLY` reaches the mock SMTP server and the event is in the calendar; an organizer's update moves the event and the cancellation is removed with "Remove from calendar"; a meeting created in the event editor is sent ("Send invitations"), Grace's `COUNTER` is accepted from the card and the updated `REQUEST` goes out; the attendee picker shows `bob`'s busy blocks from the mock CalDAV server's outbox |
| `e2e/tests/gmail.spec.ts` | `--seed demo-multi` (M3b): the Gmail account switched on in the settings syncs (four inbox conversations, chat skipped), category tabs filter the inbox, a label applied with "Labels…" reaches the mock Gmail server, archive removes `INBOX`, mail is sent through the API (`gmail-sent`), a message delivered on the server (`gmail-deliver`) arrives by history polling |
| `e2e/tests/exchange.spec.ts` | `--seed demo-multi` (M3c): an Exchange Online account added through Autodiscover (mock Autodiscover → Graph, the redirect confirmed, the mock Microsoft sign-in) syncs; an on-premises account added through Autodiscover (EWS, password) syncs and gets a server-side message by streaming notifications (`ews-deliver`); the "Contoso" Graph account switched on: Focused / Other tabs, flag, move into a subfolder, a new category and a category set on a message reach the mock Graph tenant (`graph-messages`, `graph-settings`), mail sent through Graph (`graph-sent`); a shared mailbox opened (its folder tree) and automatic replies set through Settings → Vacation reply and read back in the Exchange editor; a rule created from a message placed as an inbox rule; a To Do task created, a room booked through the room finder and an invitation answered through Graph's meeting API (`graph-pim`, `graph-meetings`, no mail sent) |
| `e2e/tests/agents.spec.ts` | `--seed demo-multi` (M4a): an MCP client created and granted in the settings writes a draft without confirmation; its send waits until approved in the confirmation center ("always allow" then lets the next send run at once; axe on the center); a denied Trash and a permanent delete (always asks, no always-allow) change nothing; the `summarize_thread` prompt; a rate limit saved in the settings trips and is audited as denied |
| `e2e/tests/extensions.spec.ts` | `--seed demo-multi` (M4a): the before-send example vetoes ("Keep editing", "Send anyway"; axe on the veto dialog); an arrival hook tags new mail (keyword on the IMAP server) and an open hook's banner button reaches the plug-in; the CSV preview renders a table; an hourly job runs after `clock-advance` and "Run now"; a runaway arrival hook is stopped while mail keeps arriving; a composer action inserts text attributed to its plug-in |
| `e2e/tests/m2c.spec.ts` | `--seed demo-multi` (M2c): a rule placed "Server" with its Sieve script and the vacation reply on the mock ManageSieve server; "Not junk" / "Mark as junk" train the filter and the next similar message is filed in Junk; a snoozed message leaves the list and returns when the core's clock passes the wake time; a new-mail notification (recorded Web Notification) and quiet hours suppressing the next; the server search finds a message of the headers-only folder and opening it fetches the body; the right-click menu (and Shift+F10); a key recorded in Settings → Customize → Keys is written to `layout.kdl` and works |
| `e2e/tests/m5b.spec.ts` | `--seed demo` / `demo-multi` (M5b): the rail and Ctrl+1 … 4 / `g m` switch views with the focus moving into the view, rail arrow keys; the Mail badge matches the All inboxes count; the last view survives a reload (`ui-state.json` `shell.view`); the reader bar's labelled / icon actions, More from the keyboard and actions moving into More when narrow; filter chips; settings search → the setting marked, the remembered section; the calendar view switch and a calendar's "…" menu (recolour); dates in English with a German browser; a pre-rail `layout.kdl` keeps working and is offered the new default through the merge dialog; a remote browser in dark mode next to a light desktop; views from the command palette |
| `e2e/tests/m5b-contract.spec.ts` | the contract's fallback with a pre-rail `layout.kdl`: view shortcuts and the palette open the views as windows |
| `e2e/tests/m5c.spec.ts` | `--seed demo` / `demo-multi` (M5c): the window manager — move by the title bar, resize from each edge and corner down to the minimum size, snap left / right (preview, coming loose), maximize by double-click and button, keyboard move / resize (arrows, Alt+F7 / Alt+F8 with Enter / Escape), geometry remembered across a reload (`window-state.json`) and Reset window positions, two composers side by side (one sent, the other kept), a confirmation blocking only its composer and "Delete permanently?" blocking only the main window, Escape |
| `e2e/tests/desktop-m5c.spec.ts` | the desktop app (M5c): a composer pops out into its own OS window (`nail_test_shell("pop-outs")`), is sent there and the OS window closes; a popped-out composer returns to the main window with what was typed |
| `e2e/tests/m5a.spec.ts` | `--seed demo-multi` (M5a): the five themes picked in Settings → Appearance and written to `theme.kdl` (Follow Windows off), the accent override and "Use theme defaults", Compact density across themes (tag line hidden, smaller rows, `data-nail-density`), Follow Windows driven by `system-appearance` (Graphite, High contrast, another pair), a user theme added to `themes/` while running and a broken one listed, an attachment card's Save downloading the file |
| `e2e/tests/m5d.spec.ts` | `--seed demo` (M5d): Customize layout from the rail's View menu (swap Folders and Message, confirm `layout.kdl`, Undo to no file, hide / show), a Mini month added and the Reader below / Focus presets applied, Reset to default, the Window layout card; the reader toolbar customized (Forward into More with `M`, Undo, Reply all dragged into More, Reset to default); accent and corner radius written to `theme.kdl` and undone; New theme "Ocean Café" → `themes/ocean-cafe.kdl`, a token edited in the theme editor (applied to the page), Undo, Duplicate as new theme; `layout.kdl` changed on disk → Undo refuses, "Undo anyway" backs the hand edit up |
| `e2e/tests/responsive-helper.spec.ts` | self-test of the P1 responsive helper `e2e/support/responsive.ts` (ADR 0139) on static pages: page overflow / page scroll, clipped text (not an ellipsis or a line clamp), overlaps within a window (not across windows, not scrolled away), off-screen controls, label alignment, controls left at the browser's default width (`undersized-control`, ADR 0160: free room beside them, short of their column's edge; `data-width` controls skipped), scopes and ignores, the 8-size sweep with its screenshots, `resizeWindow` down to a min-size |

**Responsive checks (P1, ADR 0139).** `e2e/support/responsive.ts` holds the standard sizes (`STANDARD_SIZES`: 1600 / 1280 / 1024 / 800 × 1000 / 700) and `expectResponsive(page, name, { prepare, scope, ignore, skip, aligned })`, which resizes the viewport to each size, runs `prepare`, fails on page overflow, page scroll, clipped text, text running out of a box that does not clip it (`overflowing`, ADR 0147), off-screen controls, overlaps (a text leaf counts with its sideways spill), misaligned label columns and text-like controls left at the browser's intrinsic width although their row has room (`undersized-control`, ADR 0160 — a control small on purpose carries `data-width="short" | "medium" | "content"`, which the skin sizes and the check skips) among visible `[data-part]` elements, and saves `<build dir>/screens/p1/<name>-<w>x<h>.png` for the screenshot review. `layoutIssues(page)` checks the current state; `resizeWindow(page, frame, size)` drags a window-manager window's south-east handle (`{ width: 1, height: 1 }` = its min-size).

### Visual regression (M5a, ADR 0106)

Screenshots of 24 screens (the main window with the rail and a message,
the reader's More menu, the composer, the settings page, the calendar views,
Contacts and Tasks, a context menu, an HTML message, attachment cards before
and after saving, the Move dialog, a modal confirmation, and — M5d —
customize-layout mode, the toolbar customizer, Settings → Appearance and the
theme editor, and — P2 — a conversation in the card feed and the
expandable conversation list; `e2e/visual/screens.ts`) for every built-in theme
(Frost, Sage, Graphite, Nocturne, High contrast) × density (Comfortable,
Compact), compared with reviewed baselines. Its own Playwright config
(`e2e/visual/playwright.visual.config.ts`), not part of `npm run e2e`.
The screens of a worker share one `nail serve`: a screen that changes data
undoes it in its `close` hook after the capture (P3: the modal confirmation
moves the sample to Trash, because Delete permanently asks only there, and
undoes the move).

```text
npm run build -w ui && cargo build -p nail-cli   # prerequisites (or NAIL_BIN=<nail.exe>)
npm run visual             # compare with e2e/visual/baselines/<platform>/, then build the review page
npm run visual:update      # also write baselines of new / changed screens
npm run visual:review      # rebuild the review page from the last run
npm run visual:selftest    # end-to-end self-test of the harness with temporary baselines
```

- The review page is `<build dir>/visual/review/index.html`
  (`.nail-build/visual/review/index.html` in the checkout): baseline,
  actual and difference side by side, filterable by status, skipped variants
  with their reasons. Open the file; nothing is served.
- Baselines are per platform because fonts differ; `win32` is the
  reference. Accepting a change = `npm run visual:update`, review every
  "new" / "changed" screen on the page, commit `e2e/visual/baselines/`.
- Each worker runs its own `nail serve --seed demo-multi` on a free port;
  every screen starts from a fresh page with a fixed clock, UTC, en-US,
  reduced motion, 1280 × 1000.
- Variables: `NAIL_VISUAL_THEMES`, `NAIL_VISUAL_DENSITIES`,
  `NAIL_VISUAL_SCREENS` (comma-separated filters), `NAIL_VISUAL_WORKERS`
  (default 2), `NAIL_VISUAL_MAX_DIFF_RATIO` (default 0.002 of the pixels),
  `NAIL_VISUAL_THRESHOLD` (default 0.15 per pixel), `NAIL_VISUAL_BASELINES`,
  `NAIL_VISUAL_RESULTS`. Details: `e2e/visual/README.md`.
- CI job `visual` (non-blocking for now) uploads the review page as the
  artifact `visual-review`.

### Generated files and snapshots

Some checked-in files are generated. A test fails when one is stale;
regenerate it with the matching command:

| File(s) | Regenerate with |
| --- | --- |
| `ui/src/api/generated/` (TypeScript types from `nail-api`) | `npm run gen:api` |
| `packages/plugin-api/src/generated/` (plug-in-facing TypeScript types from `nail-api`, M2d) | `npm run gen:api` |
| Reference sections of `userDocs/en/plugins/api-reference.md` and `components.md` (from the SDK types and `ui.rs` doc comments) | `npm run docs -w packages/plugin-api` |
| `crates/nail-config/schema/*.schema.json` | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema` |
| `crates/nail-mime/tests/snapshots/` (insta) | `INSTA_UPDATE=always cargo test -p nail-mime --test corpus`, or `cargo insta review` |
| `crates/nail-api/fixtures/default-{theme,layout}.json` (after changing `defaults/*.kdl`) | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults`, then `npx prettier --write crates/nail-api/fixtures` |
| `crates/nail-config/schema/rules.schema.json` | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema` |
| `crates/nail-config/tests/golden/` (layout write-back, merge) | `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-config --test write_back` (or `--test merge`) |
| `crates/nail-compose/tests/golden/*.eml` (built MIME, CRLF, kept byte for byte by `.gitattributes`) | `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-compose --test mime` |
| `crates/nail-testkit/corpus/crypto/pgp/` (OpenPGP interop vectors made by GnuPG 2.4; Git for Windows ships it as `/usr/bin/gpg`) | `bash crates/nail-testkit/corpus/crypto/pgp/generate.sh` (random session keys: every file changes, the tests must still pass) |
| `crates/nail-testkit/corpus/crypto/smime/` (S/MIME test CA, certificates incl. the demo-multi `ada-work` / `ada-home`, PKCS#12 files password `nail-test`, OpenSSL-made messages) | `bash crates/nail-testkit/corpus/crypto/smime/generate.sh` (needs OpenSSL 3.4+, Git for Windows ships `/mingw64/bin/openssl`; OpenSSL is a fixture tool only) |

## Run

### Web mode (`nail serve`)

```sh
npm run build
cargo run -p nail-cli -- serve --seed demo --open
```

This serves the UI and API on `http://127.0.0.1:4780/`, or on a free port
when 4780 is taken (another Nail or another project): the URL printed on
stdout is always the real one, and `nail plugin dev` finds it through
`<data-dir>/ui-endpoint.json`. An explicit `--port N` that is taken is an
error. Useful options:

| Option | Meaning |
| --- | --- |
| `--seed demo` | Load the demo mail corpus as a local account (no network). Every seeded run keeps its secrets in `<data-dir>/demo-keychain.json` (a plain demo keychain file, never the OS keychain; a restart with the same `--data-dir` finds them — M4c, the recovery-key scenario), trusts only the corpus' S/MIME test root and looks up no keys online |
| `--seed demo-imap` | Serve the demo corpus from an in-process mock IMAP server and sync it like a real account |
| `--seed demo-multi` | Two configured accounts on in-process mock servers: "Work" (IMAP with IDLE, the demo corpus, a Junk folder, a folder `Newsletters` synced headers only, and a mock ManageSieve server whose `sieve` node starts switched off) and "Home" (POP3), SMTP for both, and `bob@new.example` (password `new-secret`) for trying account setup with discovery (`bob@elsewhere.example` shows an unverified SRV answer). Writes its `accounts.kdl` into the config dir (refusing one that holds real accounts), keeps passwords in memory (never the OS keychain) and exposes the test controls (below). Both accounts get an OpenPGP key (generated) and an S/MIME certificate (`ada-work.p12` / `ada-home.p12` of the corpus), `prefer-encrypt="mutual"` and `authserv-id "mx.nail.example"`. M3a: a mock CalDAV / CardDAV server (sources `work-calendars` / `work-contacts`, user `ada`, password `dav-secret` in the memory keychain) with sample events ("Team sync" weekly, "Quarterly review" tomorrow) and contacts (Grace Hopper, Charles Babbage), and the ICS subscription `holidays` on the mock HTTP server. M3b: the mock CalDAV server's user address is `ada@work.example`, a second principal `bob` (`bob@example.org`, busy tomorrow 10:00 and 14:00 UTC) answers free/busy through the schedule outbox, "Project kickoff" (in two days) is a meeting the work identity organizes with bob invited; a Gmail API account "Gmail" (`ada@gmail.example`, mock Gmail server with the demo mailbox, signed in through a mock OAuth2 server, polls every 10 s) is configured but **switched off** (`enabled=#false`) so the other suites keep their counts — Settings → Accounts → Gmail → "Synchronize this account" turns it on |
| `--port N` | Use exactly port `N` (fails when it is taken); `--port 0` = a free port. Without `--port`: 4780, else a free port. The URL is the only line printed on stdout |
| `--data-dir DIR` | Store location (default `%LOCALAPPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--config-dir DIR` | `theme.kdl` / `layout.kdl` location (default `%APPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--ui-dir DIR` | UI bundle (default `$NAIL_UI_DIR`, `ui/` next to the binary, or the repository's `ui/dist`) |
| `--locale de` | UI language override |
| `--no-watch` | Do not watch the config files |
| `--mcp-port PORT` | Port of the MCP server on 127.0.0.1 (default: the MCP settings' fixed port, else a free one; written to `<data-dir>/mcp-endpoint.json`) |
| `--no-mcp` | Do not start the MCP server |
| `--launch FILE_OR_LINK` | Open a file or link as the desktop app does when Windows starts it (`.ics` / `.vcf` → import dialog, `.eml` → reader, `mailto:` → composer, `webcal:` → new subscription); repeatable (M3a) |
| `-v`, `-vv` | More logging; `NAIL_LOG=nail_core=debug` sets the filter precisely |

A seeded run never uses your real profile: without `--data-dir` /
`--config-dir` it runs in a temporary directory that is removed on exit
(ADR 0021). To keep the demo data between runs, pass directories:

```sh
cargo run -p nail-cli -- serve --seed demo --data-dir "$TEMP/nail-data" --config-dir "$TEMP/nail-config"
```

The server binds to 127.0.0.1 only and has no authentication (ADR 0009):
it is the desktop's own UI. The **remote UI** is separate (ADR 0041): off
by default, turned on in Settings → Remote access (`UpdateRemoteUiSettings`),
it listens on its own port with TLS (a self-signed certificate kept in the
store, its key in the keychain) and requires a login (pairing code,
passkey, password + code). `nail serve` and the desktop app both run it
when enabled. Seeded runs never fetch remote content from the internet:
their proxy resolves no names (demo-multi: only its mock image host).
A `nail.exe` in the portable zip (`bin\nail.exe` next to the `portable`
marker's folder) defaults to the zip's `data` and `config` folders. `nail serve` never opens an editor on the machine (the browser
may be elsewhere): "open file at line" (`OpenConfigFile`) answers
`unsupported` there (`ServiceOptions::open_editor`, ADR 0028). The desktop
app opens one unless it is seeded.

#### MCP server and `nail mcp`

`nail serve` and the desktop app start the read-only MCP server (ADR 0025)
on `127.0.0.1` with the core as its data provider
(`Service::start_mcp_server`); `GetMcpServerInfo` reports its URL. Agents
with a stdio transport use the bridge:

```sh
NAIL_MCP_TOKEN=<token> nail mcp --client <name> [--url URL] [--data-dir DIR]
```

It finds the server through `<data-dir>/mcp-endpoint.json` (or `--url`),
checks that the token belongs to `--client`, and forwards stdio to the
loopback HTTP endpoint. Tokens come from Settings → MCP clients. When no
Nail runs on the data directory, `nail mcp` starts one without a window
(`--config-dir`, `--mcp-port`; `--no-start` refuses instead) for as long as
the client is connected. One process per data directory: `nail serve`,
the desktop app and a headless `nail mcp` take `nail.lock`
(`DataDirLock`); the desktop app shows who holds it and exits.

#### Plug-ins and `nail plugin dev`

Plug-ins run inside the core (ADR 0055): installed packages are kept in
`<data-dir>/plugins/<id>.nailplugin` and verified by hash at every start.
To develop one:

```sh
node packages/create-nail-plugin/index.mjs <dir> [--template command|hook|composer|attachment] [--id ID] [--name NAME] [--sdk PATH]
npm run package --prefix <copy>             # build + pack a copy of examples/<name> made outside the repo (the scripts write dist/ next to the manifest; see examples/README.md)
nail plugin dev <folder> [--url URL | --data-dir DIR]
```

`nail serve` writes `<data-dir>/ui-endpoint.json` with its URL;
`nail plugin dev` finds it there (or takes `--url`), loads the folder with
every declared permission over the local WebSocket (`LoadDevPlugin`),
reloads it on changes and prints its log (host lines, its own lines, API
calls) until Ctrl+C. In the desktop app use "Load a development folder…" in
Settings → Plug-ins, or (M4b, ADR 0087) switch on the local endpoint in the
desktop settings: the app then writes the same file with a token, which
`nail plugin dev` sends as `Authorization: Bearer` (that endpoint loads
development folders and streams their logs, nothing else).

#### Test controls (`--seed demo-multi` only)

With `--seed demo-multi` (and only then) the server answers
`POST /api/test/<command>` with a JSON body, to drive the mock servers
from tests or by hand (`curl -X POST http://127.0.0.1:4780/api/test/smtp-messages`):

| Command | Arguments | Effect |
| --- | --- | --- |
| `imap-deliver` | `account?`, `mailbox?`, `raw`, `flags?` | New message on the IMAP server (`account` `work` or `new`, mailbox `INBOX` by default) |
| `imap-flags` | `account?`, `mailbox?`, `subject`, `flags` | Replaces the flags of the first message whose subject contains `subject` |
| `imap-messages` | `account?`, `mailbox?` | Lists `{uid, subject, flags}` |
| `imap-mailboxes` | `account?` | Lists `{name, count}` |
| `imap-offline` | `account?`, `offline` | Simulates an outage (connections refused and dropped) |
| `pop3-deliver` | `raw`, `uid?` | New message in the POP3 maildrop |
| `pop3-state` | – | `{uids, deleted}` of the POP3 server |
| `smtp-messages` | – | What the SMTP server received: `{from, to, data}` |
| `remote-content` | – | `{images, private, requests}`: base URLs of the made-up image hosts (`images.nail.example` → the mock HTTP server, `private.nail.example` → a private address the proxy refuses) and the paths the image host was asked for |
| `sieve-scripts` | – | `{active, uploads, scripts: [{name, active, content}]}` of the mock ManageSieve server (M2c) |
| `dav-objects` | `collection?` | `[{name, etag, data}]` of the mock CalDAV / CardDAV server: `personal` (default), `tasks`, `holidays` (read-only) or `contacts` (M3a) |
| `dav-put` | `collection?`, `name`, `data` | Stores an iCalendar / vCard object as another client would; `{etag}` |
| `dav-delete` | `collection?`, `name` | Deletes an object as another client would |
| `dav-user-put` | `user?`, `name`, `data` | Stores an event in the `personal` calendar of another principal (`bob` by default; free/busy, M3b) |
| `gmail-deliver` | `raw`, `labels?` | New message on the mock Gmail server (labels `INBOX`, `UNREAD` by default); `{id}` (M3b) |
| `gmail-message` | `id?` or `subject?` | `{id, labels}` of a Gmail message (by id or a subject it contains) |
| `gmail-modify` | `id?` / `subject?`, `add?`, `remove?` | Changes a message's labels on the server (history polling picks it up) |
| `gmail-labels` | – | `[{id, name}]` of the mock Gmail account |
| `gmail-sent` | – | Raw texts of what Nail sent through `messages.send` |
| `graph-deliver` | `raw`, `folder?` | New message in the mock Graph tenant of the `exchange` account (`inbox` by default); `{id}` (M3c) |
| `graph-messages` | `mailbox?` | `[{id, folder, subject, read, flag, categories, inference}]` of the user's (or a shared) mailbox |
| `graph-folders` | `mailbox?` | `[{id, name, well_known}]` |
| `graph-sent` | – | Raw texts of what Nail sent through `sendMail` |
| `graph-settings` | – | `{categories, automatic_replies, rules}` (Graph JSON) |
| `graph-pim` | – | `{events, tasks, contacts}` (Graph JSON; tasks with their list and checklist) |
| `graph-meetings` | – | `[{event_id, action}]`: meeting answers / cancellations received |
| `ews-deliver` | `raw`, `folder?` | New message on the mock EWS server (distinguished folder name, `inbox` by default); `{id}` (M3c) |
| `ews-messages` | `folder?` | `[{id, subject, read, categories, start, start_time_zone}]` of an EWS folder (distinguished name such as `inbox` / `calendar`, or a display name such as `Archive`) |
| `ews-settings` | – | `{oof: {state, internal, external}, rules: [{name, enabled}]}` of the mock EWS server |
| `ews-meetings` | – | `[{kind, subject, comment}]`: meeting requests, answers and cancellations the mock EWS server sent |
| `ews-sent` | – | Raw texts of what Nail sent through EWS |
| `feeds` | – | `{holidays, town}`: URLs of the demo ICS feeds (`holidays` is configured, `town` is for adding a subscription) |
| `clock` | – | `{now}`: the core's triage clock (snooze, quiet hours, notifications, reminders), Unix seconds |
| `clock-set` | `at` | Sets that clock to `at` (Unix seconds); it keeps running |
| `clock-advance` | `seconds` | Moves that clock forward (or back, negative) |
| `system-appearance` | `scheme` (`light` / `dark`), `high_contrast?` | The OS light / dark and contrast state Follow Windows sees (M5a; seeded runs start light, whatever the machine does) |

For UI work with hot module reloading, run `npm run dev`. It prints the
dev server's URL (`http://127.0.0.1:5791/` when that port is free). Add
`?mock=1` to use the in-memory mock core. Add `?lang=de` or `?mode=dark` to
override the locale or the colour mode.

### Development ports

Nail uses no fixed development ports, so it runs next to other Vite / Tauri
projects on the same machine (plan section 10):

- `npm run dev` and `npm run tauri dev` go through `scripts/dev.mjs`
  (helpers in `scripts/lib/dev-port.mjs`, tests in
  `scripts/test/dev-port.test.mjs`). It picks the port — `NAIL_DEV_PORT`
  exactly (an error when taken), else 5791 when free (deliberately not
  Vite's 5173), else any free port — and starts Vite on exactly that port
  (`strictPort`, host `127.0.0.1`).
- For `npm run tauri dev` (`scripts/tauri.mjs` sends `dev` there; every
  other Tauri command goes straight to the Tauri CLI) it then runs
  `tauri dev --config <temp file>` with `build.devUrl` set to that port
  and `beforeDevCommand` removed (`null` in the JSON merge patch the CLI
  applies), so Tauri starts no second dev server. Arguments after `dev`
  pass through: `npm run tauri dev -- --release`.
  `tauri.conf.json` names `http://127.0.0.1:5791` and no `beforeDevCommand`;
  a bare `npx tauri dev` therefore expects `npm run dev` on 5791.
- `ui/vite.config.ts` reads `NAIL_DEV_PORT` too; `npx vite` in `ui/` without
  it starts at 5791 and moves on when that is taken.
- `nail serve` without `--port` falls back to a free port when 4780 is
  taken (above); the MCP server already uses a free port unless one is
  configured.

### Data and config directories

```text
<data-dir>/nail.sqlite3        metadata (SQLite, WAL; plus -wal/-shm files)
<data-dir>/blobs/ab/<62 hex>   raw messages, zstd-compressed, addressed by SHA-256
<config-dir>/theme.kdl         optional; see userDocs/en/theme-reference.md
<config-dir>/layout.kdl        optional; see userDocs/en/layout-reference.md
<config-dir>/accounts.kdl      accounts, identities, virtual accounts (M1a); see userDocs/en/accounts-reference.md
<config-dir>/rules.kdl         filter rules (M1b); see userDocs/en/rules.md
<config-dir>/backups/          earlier theme.kdl / layout.kdl versions from the merge dialog (newest 20)
<data-dir>/mcp-endpoint.json   URL of the running MCP server (no secret), for `nail mcp`
```

`nail config schema theme`, `nail config schema layout` and
`nail config schema accounts` print the JSON Schemas of the config files.
Secrets are never in these files: `accounts.kdl` only holds keychain
references (ADR 0013).

### Desktop app

```sh
npm run tauri dev
```

This starts Vite on a free port (see "Development ports") and the desktop
app with the matching `devUrl`. The app loads the UI from the dev server and
uses the default directories. To run the built UI without a dev
server, with demo data in temporary directories:

```sh
npm run build
cargo run -p nail-app --features tauri/custom-protocol -- --seed demo --data-dir "$TEMP/nail-app-data" --config-dir "$TEMP/nail-app-config"
```

The app accepts `--data-dir`, `--config-dir`, `--seed demo` and `--locale`,
plus what Windows passes after `--` (a `mailto:` link, a `.eml` / `.ics` /
`.vcf` file; ADR 0041): the UI takes these launch requests from the core.
A second start of an unseeded app hands its targets to the running one.
With `--seed demo` and no directories it runs in a temporary directory
(data, config and the WebView2 profile), like `nail serve --seed`. It
starts the MCP server like `nail serve`.

M4b (ADRs 0090, 0092): the app shows a tray icon (Open Nail, New message,
Check mail, Quit Nail), closes to the tray, starts hidden with
`--autostart` (the Run key's argument), serves the Simple MAPI pipe and,
when switched on, the `nail plugin dev` endpoint. A seeded run never touches
the real registry, Run key or pipe. Test variables of the shell:

| Variable | Effect |
| --- | --- |
| `NAIL_REGISTRY_ROOT=Software\NailTests\<name>` | Both hives below that `HKCU` key (`<root>\HKCU\…`, `<root>\HKLM\…`): stage installation facts (`Software\Nail\Install`) and policies (`Software\Policies\Nail`) without touching HKLM; seeded runs otherwise use an in-memory registry |
| `NAIL_UPDATE_ENDPOINT=http://127.0.0.1:<port>/{{channel}}/latest.json` + `NAIL_UPDATE_PUBKEY=<minisign public key>` | A loopback update source (other hosts are refused); enables the updater in development builds; plain `http` needs a debug build (or the release overlay's `dangerousInsecureTransportProtocol`); policies still win |
| `NAIL_MAPI_PIPE=\\.\pipe\<name>` | The Simple MAPI pipe name (seeded runs serve a pipe only with it); the DLL and `nail-mapi-harness` read it too, and `NAIL_MAPI_APP` (only together with it) names the app the DLL may start |
| `NAIL_TEST_BACKGROUND_WINDOWS=1` | Test runs (ADR 0146; set by `startDesktop`): every window (main, pop-outs, print) is DWM-cloaked (shown as far as Windows and the shell know, never drawn, no mouse input), not activatable (`WS_EX_NOACTIVATE`, shown with `SW_SHOWNOACTIVATE`, never `set_focus`) and off the taskbar; no OS notifications, file dialogs (a pick is cancelled), Settings app or start-refusal message box |

Seeded runs also register the test control `nail_test_shell` (a tray menu
item id, `window-visible`, `pop-outs`, `close-pop-outs` or `windows` — each
OS window's visible / cloaked / activatable / foreground facts), which the desktop smoke tests call through
`window.__TAURI_INTERNALS__.invoke`; a real profile never has it.

To inspect the WebView2 window with Chrome DevTools or Playwright, start it
with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
and connect to `http://127.0.0.1:9333`, for example with Playwright's
`chromium.connectOverCDP`.

## Layout of the code

See [`CLAUDE.md`](../CLAUDE.md) for the crate map, [`plan.md`](plan.md) for
milestones and [`adr/`](adr/) for decisions. The main ADRs for the M0
architecture are:

- **0002:** the protocol
- **0003:** the config models
- **0006:** KDL parsing
- **0007:** the store
- **0008:** the renderer
- **0009:** the core, server, CLI and desktop shell
- **0010:** the e2e harness
- **0011:** build output locations (`ui/dist` stays in the repository)

For M1a:

- **0012:** the M1a contract (protocol additions, engine traits, crate shapes)
- **0013:** `accounts.kdl` (format, validation, format-preserving writes)
- **0014:** libraries for SMTP, POP3, OAuth2, keychain, discovery and search
- **0015:** store v2, threading, drafts / outbox, sync-side merge
- **0016:** search index and query language
- **0017:** SMTP, POP3, OAuth2, keychain, discovery, mock network servers
- **0018:** the IMAP engine (own client on `imap-proto`, IDLE, QRESYNC, writes)
- **0019:** the M1a user interface (windows, forms, optimistic writes, mock core)
- **0020:** core integration (engines, replay, outbox, virtual accounts, demo seed)
- **0021:** M1a review fixes (UIDVALIDITY-safe replay, Sent copies via the queue, `ChooseIdentity`)

For M1b:

- **0022:** the M1b contract (rich drafts, rules, MCP, UI context, config write-back and merge,
  saved searches, `RetryOp`), store v4, the `nail-compose` / `nail-rules` / `nail-mcp` stubs
  and the per-package slots (`ui/src/features/README.md`)
- **0023:** the composer (`nail-compose`: MIME, Markdown, CSS inlining, quoting; TipTap / CodeMirror UI)
- **0024:** the rules engine (`rules.kdl` reader and format-preserving writer, planned actions, rules UI)
- **0025:** the read-only MCP server (Streamable HTTP on loopback, tokens, grants, audit, `nail mcp` bridge)
- **0026:** customization tooling (every config error, `layout.kdl` write-back, merge dialog, inspector)
- **0027:** M1b integration (composer, rules, MCP and customization in the core; server Drafts
  copy, `ListDrafts`; default layout / theme; desktop smoke test over WebView2 CDP)
- **0028:** M1b review fixes (rule mail marked against loops, redirects through the outbox,
  escaped reply templates, autosave upload races, restricted MCP search / context, partial undo)

For M2a:

- **0029:** the M2a contract (message security, keys and certificates, recipient key status,
  sign / encrypt options, auth results, reader warnings, encryption at rest, security settings),
  identity `crypto` in `accounts.kdl`, store v5, the `nail-pgp` / `nail-smime` / `nail-vault`
  stubs, the crypto test-vector folders and the `security` UI slot
- **0030:** OpenPGP and Autocrypt (`nail-pgp` on rPGP: keyring, PGP/MIME, inline read, header
  protection, Autocrypt Level 1, WKD / keys.openpgp.org)
- **0031:** S/MIME (`nail-smime` on RustCrypto `cms`: sign / encrypt / verify / decrypt, chain
  validation, PKCS#12, the Windows certificate store)
- **0032:** data protection (`nail-vault`: sealed columns, blobs and search index, the data key,
  the migration; `Authentication-Results`; link and attachment safety)
- **0033:** the security UI (key manager, identity settings, composer toggles, badges,
  warnings, encryption-at-rest settings, Autocrypt setup)
- **0034:** M2a integration (decrypt / verify on read, sign / encrypt on send, Autocrypt in and
  out, the vault switch with the swappable search index, MCP `encrypted` rule, demo keys, e2e)
- **0035:** M2a review fixes

For M2b:

- **0036:** the M2b contract (remote UI, remote content, store v6, `webauthn_rp`, slots)
- **0037:** remote-UI authentication (`nail-remote-auth`: passkeys, Argon2id + TOTP, pairing,
  sessions, rate limits, authorization, axum login route and middleware)
- **0038:** transport and browser hardening (`nail-server` TLS, certificates, ACME, headers,
  sandbox origin; `nail-proxy`)
- **0039:** the remote-UI and privacy UI
- **0040:** installer, portable build, cargo-deny, SBOMs, CI jobs
- **0041:** M2b integration (remote-UI host, `handle_remote`, remote content through the proxy,
  Trusted Types, sandbox rendering, launch requests, e2e)

For M2c:

- **0043:** the M2c contract (localized core messages, Sieve / vacation, junk / snooze /
  notifications, folder sync policies and on-demand bodies, context menus, layout reset
  patches, MCP port setting, `RuleFromMessage`, store v7, the `nail-sieve` / `nail-triage`
  stubs, slots). Fluent texts built in Rust live in `locales/<lang>/core*.ftl`;
  `cargo test -p nail-api --test catalog` checks that every id the contract names exists.
- **0044:** `nail-sieve` — the Sieve compiler (placement + localized reasons), the
  parse / decompile round-trip harness, the ManageSieve client, upload / remove of the
  managed script; `nail-testkit-net`'s `MockSieveServer` + `validate_script`
- **0045:** `nail-triage` — tokenizer, junk classifier, snooze scheduler, notification policy
- **0046:** engine polish — localized Rust messages (`nail_config::l10n`), body sync policies,
  IMAP search keys, Markdown bodies, fixed MCP port, headless `nail mcp`, `DataDirLock`
- **0047:** UI polish — context menus, Customize editors, triage / Sieve / sync UI, lazy
  editors (vendor / catalogs chunks in `ui/vite.config.ts`), axe in Vitest
  (`ui/src/a11y.test.tsx`), `defaults/themes/` example themes
- **0048:** M2c integration (Sieve upload task, junk / snooze / notifications in the core,
  headers-only sync + on-demand bodies + server search, replay connection, localized
  messages end to end, desktop MCP port + lock, M2b follow-ups, the adjustable clock)

For M4c:

- **0094:** the M4c contract (`nail_api::portability`: import sources mbox / EML folder /
  Maildir / Thunderbird profile / ICS / vCard, export targets, jobs with progress events and
  per-item reports, "Local Folders"; `nail_api::backup`: items, the encrypted `.nailbackup`
  format, verify, dry-run restore plans; `nail_api::recovery`: the vault recovery key;
  `nail_api::smartcard`: PKCS#11 settings, tokens, keys, PIN prompts; `nail_api::palette`:
  recent actions + `ui/src/actions/palette.ts`; `nail_api::print`: print jobs, the print
  view, PDF through the desktop host; `nail_api::diagnostics`: the bundle with an exact
  manifest; `PickPath`; send-as / on-behalf identities, upload progress, server thread ids;
  store v15; new crates `nail-portability`, `nail-pkcs11`, `nail-bench`; slots
  `ui/src/features/{portability,palette,print}`). Texts built in Rust:
  `locales/<lang>/core-portability.ftl`, `core-vault.ftl`; UI: `portability.ftl`,
  `palette.ftl`, `print.ftl`.

For M4b:

- **0087:** the M4b contract (`nail_api::desktop`: installation facts from the installers'
  registry values, the updater's settings / states with the policy winning (MSI-managed, group
  policy, portable, development), tray / startup settings, default apps, first run, the desktop
  `nail plugin dev` endpoint, the `DesktopHost` trait; `nail_api::mapi`: Simple MAPI messages,
  result codes, the named pipe and its frames; `Service::submit_mapi` → a draft the composer
  opens, never sent for a program; slots `crates/nail-mapi`, `app/src-tauri/windows/` (was
  `installer/`), `packaging/`, `ui/src/features/system/`). Texts built in Rust:
  `locales/<lang>/core-desktop.ftl`; UI: `system.ftl`.
- **0088 (WP111):** `nail-mapi` — `NailMapi.dll` (13 Simple MAPI exports checked against
  windows-sys, VirtualQuery-checked reads, private copies of the files in `%TEMP%\nail-mapi`,
  the per-user pipe with the same-user server check, starting Nail.exe, registration values;
  x64 / x86 / ARM64) and `nail-mapi-harness`.
- **0089 (WP112):** the per-machine MSI (WiX fragment, `AUTOSTART` / `ADDTOPATH` /
  `DISABLEUPDATES` / `MAPI`), the NSIS MAPI option, signing when configured, update manifests
  per channel (own minisign check), winget manifests, the installer CI jobs and `release.yml`.
- **0090 (WP113):** the desktop shell — `DesktopHost`, tray, close to tray / start hidden, the
  Run key, installation facts and policies from the registry, the updater over
  tauri-plugin-updater (install on restart and on quit), default apps, the Simple MAPI pipe
  server, the `nail plugin dev` endpoint (127.0.0.1, bearer token, no Origin).
- **0091 (WP114):** the desktop UI (`ui/src/features/system/`: updates and About, tray and
  startup, default apps, first run, the composer note for drafts from other programs).
- **0092:** M4b integration (wiring, the pipe server waiting for the client before it
  disconnects, update status refetched after commands, the machine-wide Run key, the composer /
  launch mounts, layout / theme, typed widget options, the attachment limit in `nail_api`, the
  desktop smoke scenarios and their test hooks, the `mapi` CI job, the MSI smoke checking
  msi-managed updates).

For M4a:

- **0079:** the M4a contract (writes by agents and plug-ins: `nail_api::agents` action kinds, policies
  (destructive kinds always ask; "always allow" per MCP client / plug-in for the rest) and
  confirmations; the MCP tool table, write-tool arguments (`--features json-schema` derives their JSON
  Schemas), prompts and rate limits; plug-in write / hook permissions, hooks, composer / attachment /
  link / MCP extensions, background jobs (`CronSchedule`), `nail.l10n`, binary fetch bodies; store v14;
  slots). Texts built in Rust: `locales/<lang>/core-agents.ftl`, `core-mcp.ftl`; UI: `agents.ftl`.

For M3c:

- **0072:** the M3c contract (Exchange accounts as an `exchange` node in `accounts.kdl` with
  `engine="graph"` (Exchange Online) or `"ews"` (on-premises), `auth "windows"` (SSPI Negotiate /
  Kerberos / NTLM); shared / delegate mailboxes, categories ↔ tags with colours, Focused Inbox,
  inbox rules, automatic replies, rooms, To Do; the engine seams `nail_api::exchange::engine` and
  PIM item shapes `nail_api::exchange::pim`; store v12 (a rebuild step with foreign keys off);
  the `nail-proto-graph` / `nail-proto-ews` stubs (mock Graph / EWS servers:
  `--features testing`) and `nail-exchange` (SSPI, Autodiscover, mapping; mock Autodiscover:
  `--features testing`); slots). Texts built in Rust: `locales/<lang>/core-exchange.ftl`,
  `core-graph.ftl`, `core-ews.ftl`; UI: `exchange.ftl`.
- **0073 (WP91):** `nail-proto-graph` — the Microsoft Graph engine and the mock Graph server
  (appended messages are drafts, category renames by replacement, To Do ids `<list>/<task>`,
  4 MB request limit).
- **0074 (WP92):** `nail-proto-ews` — the EWS engine and the mock EWS server (Exchange2016 with
  a 2013 SP1 fallback, connection-bound Negotiate, streaming notifications, Windows zone ids).
- **0075 (WP93):** `nail-exchange` — Autodiscover with engine choice and the credential rule,
  SSPI sign-in, the Exchange ↔ Nail mapping, EWS retirement notes.
- **0076 (WP94):** the Exchange UI (`ui/src/features/exchange/`).
- **0077:** M3c integration (the Exchange engine task in `nail-core/src/exchange/`: mail per
  folder, replay, Exchange queue, settings and rule placement, the `exchange:<account>` PIM
  source with meetings through the engines' meeting APIs, free/busy and rooms, `focus:`,
  Autodiscover in `DiscoverAccount`, per-protocol OAuth scopes; demo-multi Graph / EWS accounts
  and mock Autodiscover; `exchange.spec.ts`).

For M3b:

- **0065:** the M3b contract (invitations and scheduling: iTIP methods, the invitation card on
  `MessageBody.invitation`, answers / counters / organizer updates and cancellations, free/busy;
  Gmail accounts as a `gmail` node in `accounts.kdl` with labels ↔ folders / tags, thread ids,
  categories, label management; `MailApiEngine` / `AdaptivePoll` / `FreeBusyProvider`; store v11;
  the `nail-itip` / `nail-proto-gmail` stubs (mock Gmail server: `--features testing`); slots).
  Texts built in Rust: `locales/<lang>/core-scheduling.ftl`, `core-gmail.ftl`.
- **0066:** `nail-itip` — iTIP / iMIP (cards, applying, building, iMIP mail, conflicts,
  RFC 6638 delivery); texts in `locales/<lang>/itip.ftl`
- **0067:** `nail-proto-gmail` — the Gmail API engine and the mock Gmail server
- **0068:** CalDAV scheduling and free/busy in `nail-dav`; the scheduling mock DAV server
- **0069:** the invitations / scheduling / Gmail UI (`ui/src/features/{scheduling,gmail}/`)
- **0070:** M3b integration (`nail-core/src/scheduling.rs`, `gmail.rs`: cards, arrival
  processing, iMIP through the outbox as prepared mail, server scheduling, free/busy, the
  Gmail engine loop / replay / label queue / sending, Gmail discovery, `category:`;
  demo-multi Gmail account + DAV principals)

For M3a:

- **0058:** the M3a contract (calendars, events with recurrence / overrides / edit scopes,
  contacts, groups, merge links, tasks, reminders, `.ics` / `.vcf` import, read-only MCP
  payloads; PIM sources as `calendar-subscription` / `caldav` / `carddav` nodes in
  `accounts.kdl`; store v10; the `nail-ical` (calcard) / `nail-dav` stubs; slots). Texts built
  in Rust: `locales/<lang>/core-pim.ftl`, `core-ical.ftl`, `core-dav.ftl`.
- **0059:** `nail-ical` — own lossless content-line layer and patching writers, own
  recurrence engine (`recur.rs`, `expand.rs`, edit scopes in `edit.rs`), zones (IANA /
  Windows / custom `VTIMEZONE`), alarms, feeds, import previews; the calendar corpus
  (`crates/nail-ical/tests/corpus`, byte-exact: nested `.gitattributes -text`)
- **0060:** `nail-dav` — CalDAV / CardDAV client (discovery, sync-collection with ctag /
  ETag fallback, conditional writes, server wins), `MockDavServer` in `nail-testkit-net`,
  Radicale in `scripts/interop`
- **0061:** PIM store APIs (items + PIM queue, instance cache, contacts / duplicates /
  merge / Collected / autocomplete ranking, reminders), `PimIndex`, the reminder scheduler
- **0062:** the PIM UI (`ui/src/features/pim/`)
- **0063:** M3a integration (`nail-core/src/pim/`: requests, source engines, reminders,
  search; MCP PIM resources / tools; plug-in `calendar.read`; `nail serve --launch`;
  default layout windows; demo-multi DAV / ICS mocks)
