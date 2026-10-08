# ADR 0171: Q2 contract — SSOT / ODR / DRY across the existing code

Status: accepted · 2026-10-08 · Q2 contract (wave Q2; plan section 10, "Wave Q2 (SSOT / ODR / DRY)")

## Context

The maintainer made three rules mandatory for all existing and new
code, and for every source review: **single source of truth** (every fact —
type, constant, id, limit, size, string, rule, mapping, schema — has one
source; other places derive it), **one definition rule** (it is defined once)
and **don't repeat yourself** (no copy-pasted logic; the second copy becomes
a shared function, component or test helper). Rust ↔ TypeScript facts are
generated (ts-rs / `npm run gen:api`) or pinned by a test, never mirrored by
hand. CLAUDE.md lists the rule; `.claude/workflows/nail-wave.js` gives it to
every agent and every review.

Like Q1 (ADR 0123, zero warnings) Q2 is a quality wave: behaviour does not
change and every suite stays green. This contract adds the automatic check,
measures the existing code, assigns every finding to one of four parallel
packages and prepares their slots.

## Decisions

### 1. The clone detector: jscpd 5

- **jscpd 5.4** (npm `jscpd`, MIT; its engine is a self-contained Rust binary
  in per-platform optional packages such as `jscpd-windows-x64-msvc`, MIT
  too; no further dependencies, nothing linked into Nail). It tokenizes Rust,
  TypeScript / TSX, JavaScript and CSS (224 formats), scans the repository in
  about one second and has a **baseline**: fingerprints of known clones,
  stable when files move or lines shift; a clone that is shortened, grown or
  copied once more counts as new.
- **Calibration**: `--min-tokens 50 --min-lines 5 --mode mild` (jscpd's own
  defaults). Over the whole repository: 50 tokens → 1291 clones, 70 → 517,
  100 → 197, 150 → 55. A sample of the smallest clones (50–70 tokens) showed
  copied helpers (`now()`, `request()`, comrak options, civil-date code),
  copied mock-server methods, error conversions and test flows: real
  copy-paste, almost no idioms. A higher minimum would have hidden most of
  it, so the defaults stay.
- **Scanned**: every tracked `.rs`, `.ts`, `.tsx`, `.js`, `.mjs`, `.cjs` and
  `.css` file, tests included (a copied test flow becomes a helper).
  **Ignored**, each with its reason (`scripts/lib/duplicates/config.mjs`
  holds the list):

  | Glob | Reason |
  | --- | --- |
  | `**/generated/**` | ts-rs output (`ui/src/api/generated`, `packages/plugin-api/src/generated`), generated from nail-api, the single source |
  | `crates/nail-store/libsqlite3-sys-sqlite3mc/bindings/**` | bindgen output of the SQLite3 Multiple Ciphers header (vendored, ADR 0095) |
  | `**/fixtures/**` | recorded server answers, sample profiles and files: data, not code |
  | `**/golden/**`, `**/snapshots/**` | expected outputs compared byte for byte |
  | `**/corpus/**` | test corpora (.eml, .ics, .vcf, crypto vectors) from other programs |
  | `e2e/visual/baselines/**` | visual baselines (images) |
  | `node_modules`, `target`, `dist`, `.nail-build` | build output and packages (also gitignored) |

- **Not given to jscpd**: Fluent catalogs (its `ftl` is FreeMarker; Fluent
  has its own check below), KDL (no tokenizer; the built-in themes in
  `defaults/themes/` are deliberately parallel data, and lengths shared
  between `defaults/*.kdl` and CSS-in-TS are the `css-length` check's), JSON
  and Markdown (data and prose).
- **CSS-in-TS**: a template literal is one token for the TypeScript
  tokenizer, so copied rules inside skin modules are invisible to it. The
  check copies every CSS-like template literal of the UI's non-test modules
  into a scratch directory below `.nail-build/duplicates/` with the source's line
  numbers (interpolations become `var(--nail-x)`) and runs jscpd's CSS
  tokenizer over those copies (`css-clone`).

### 2. The SSOT checks a token detector cannot see

`scripts/lib/duplicates/ssot.mjs`, pure functions over the files, each a
check id of the report:

| Check | Finds | Fix |
| --- | --- | --- |
| `mirror-comment` | a comment saying code mirrors / is kept in sync with another source: in TypeScript one naming Rust (`nail_api`, `x::y`, `.rs`, the contract, `prelude.js`, `theme.kdl` / `layout.kdl`), in Rust one naming the UI | generate it, or pin it with a test and say so: ``pinned by `<test file>` `` (the file must exist) |
| `mirror-inventory` | known hand mirrors no detector sees, listed in `scripts/duplicates/mirrors.json` | generate or pin, then remove the entry |
| `rust-ts-list` | the same list of ≥ 3 string literals in non-test Rust and TypeScript | generate it or pin it |
| `rust-ts-const` | one `UPPER_SNAKE` constant name defined in non-test Rust and TypeScript / JavaScript, unless the two definitions share no literal (two facts that happen to share a name) | `ts_constants.rs` + `gen:api`, or pin it |
| `const-value` | the same name and value in several files of one language (tests included) | define once, import |
| `fluent-text` | one English text under several Fluent ids (message or attribute) | one id per meaning; a text that reads the same but means something else gets an `allow.json` entry |
| `theme-literal` | a colour literal in CSS-in-TS | the theme token (`var(--nail-color-…)`) |
| `css-length` | one CSS length in several CSS-in-TS files, or in one and `defaults/theme.kdl` / `layout.kdl` | a token or one shared constant |
| `magic-number` | an integer ≥ 100 (not a common factor or HTTP status) in the code of ≥ 3 non-test source files of one package | one named constant |

The detectors are heuristics: a finding that is no violation (a crate's own
`CARGO_PKG_NAME`, two unrelated limits that are both 64) goes into
`scripts/duplicates/allow.json` with its reason; `pinnedBy` names the test
that pins a Rust ↔ TypeScript pair (the test must exist and mention the key).
Stale entries fail.

### 3. Ratchet now, blocking at the end of Q2

- `node scripts/check-duplicates.mjs` (`npm run check:duplicates`) is a step
  of `npm run check` (`duplicates`), of the pre-commit hook and of the
  post-edit hook (`scripts/lib/staged.mjs`: code, `.ftl`, `.kdl` and
  `scripts/duplicates/`; about 3 s, it scans the whole checkout).
- **Ratchet**: the findings of this contract are recorded in
  `scripts/duplicates/jscpd-baseline.json`, `css-baseline.json` (jscpd's
  format) and `ssot-baseline.json` (one identity per finding: its key plus
  what it spans, so a finding that grows or shrinks is new). Recorded
  findings are reported; new ones fail. In the hooks only new findings that
  touch the edited / staged files fail — including a new copy of a known
  fragment that jscpd reports on another pair of files.
- Packages never edit the baselines (stale entries are harmless and would
  only cause merge conflicts); **only the integrator rewrites them**
  (`--update-baseline`). The integrator makes the check blocking by deleting
  the three baseline files once `--strict` reports 0: without a baseline
  every finding is new.
- Commands: `--owner WP-… --list` = a package's work list (every finding it
  owns with places), `--strict --owner WP-…` = its gate (0 findings),
  `--strict` = the Q2 gate, `--markdown FILE` = the baseline tables below,
  `--json FILE` = every classified finding.

### 4. Generated contract constants

`crates/nail-api/src/ts_constants.rs` (new): `ui_constants()` /
`plugin_constants()` list contract constants with their Rust value;
`export_typescript` / `export_plugin_typescript` write them as
`constants.ts` (`export const NAME = <json> as const;`) next to the ts-rs
types, and `index.ts` re-exports it as values. `npm run gen:api` regenerates
it; the `bindings` test fails while it is stale. The contract seeds it with
`MAX_MAIL_ACTION_TARGETS` and `MAIL_UNDO_SECS` and replaces their two
TypeScript copies (`ui/src/api/mockMailActions.ts`,
`ui/src/features/mail-actions/run.ts`) with imports — the pattern for
WP-mirrors-tooling. Lists and records work too (any JSON value).

### 5. Slots

- **`crates/nail-util`** (new, no dependencies, WP-rust-core's crate): small
  shared helpers both sides of the dependency graph need. The contract
  implements `time` — `days_from_civil`, `civil_from_days`, `is_leap_year`,
  `days_in_month`, `unix_now`, `iso_date`, `rfc3339_utc` with tests — because
  the civil-date algorithm was copied into eleven files of both Rust packages
  (`app/src-tauri/src/updater.rs`, `nail-api` print / scheduling,
  `nail-config/src/backup.rs`, `nail-dav/src/ops.rs`,
  `nail-proto-imap/src/bodies.rs`, `nail-rules/src/time.rs`,
  `nail-search/src/date.rs`, `nail-sieve/src/compile.rs`,
  `nail-store/src/remote.rs`, `nail-testkit-net/src/dav/ical.rs`); each
  package replaces its own copies in parallel. Either Rust package may add a
  module file (one `pub mod` line in `lib.rs` each).
- **`crates/nail-net`** (new, empty stub, WP-rust-engines' crate): the
  transport code the IMAP / POP3 / SMTP / ManageSieve clients and the mock
  servers copied (plain / TLS stream, line framing, `MAX_LINE`, connect
  timeouts) and the HTTP retry / URL policy of the Gmail, Graph, EWS and DAV
  clients. Dependencies are added with the code (rustls + ring, ADR 0014).
- **`locales/en/common.ftl`** + empty `locales/de/common.ftl` (registered in
  `nail-config/src/l10n.rs`): ids `common-*` for generic labels several
  features repeat with the same meaning ("Cancel", "Save", "Back", …);
  WP-ui's.
- **`crates/nail-api/src/ts_constants.rs`**: WP-mirrors-tooling's (above).
- No new UI feature folder: WP-ui's shared helpers go to the existing
  `ui/src/widgets/`, `ui/src/theme/`, `ui/src/state/` and test helpers to
  `ui/src/test/`.

### 6. Ownership: every finding has exactly one package

`scripts/lib/duplicates/owners.mjs` assigns each finding (the report prints
the owner of every line):

| Package | Owns |
| --- | --- |
| **WP-rust-core** | crates nail-api, nail-core, nail-store, nail-config, nail-mcp, nail-server, nail-cli, nail-search, nail-rules, nail-triage, nail-sieve, nail-compose, nail-mime, nail-vault, nail-remote-auth, nail-proxy, nail-plugins, nail-portability, nail-bench, nail-util; `locales/*/core*.ftl`, `itip.ftl` |
| **WP-rust-engines** | every other crate (nail-proto-*, nail-exchange, nail-dav, nail-ical, nail-itip, nail-auth, nail-discovery, nail-pgp, nail-smime, nail-pkcs11, nail-mapi, nail-testkit, nail-testkit-net, nail-net) and `app/src-tauri` |
| **WP-ui** | `ui/` (except generated), `defaults/`, the UI's Fluent catalogs |
| **WP-mirrors-tooling** | `packages/`, `examples/`, `e2e/`, `scripts/` (with `scripts/interop`), root configs, `.github/` — and **every Rust ↔ TypeScript mirror** wherever it is (`mirror-comment`, `rust-ts-list`, `rust-ts-const`, `mirror-inventory` entries flagged as mirrors): the TypeScript side, the generator / ts-rs export / shared JSON in nail-api; the leftover flaky desktop `beforeAll` timeout of `e2e/tests/p3-card-height.spec.ts` |

A finding spanning several packages: a mirror → WP-mirrors-tooling; any
file of WP-mirrors-tooling's directories, or Rust and TypeScript code →
WP-mirrors-tooling; Fluent texts in UI and core catalogs → WP-ui (the UI
drops its copy); WP-rust-core and WP-rust-engines code → WP-rust-engines,
which replaces both copies with one shared function (in `nail-net`,
`nail-util` or the lower crate). The owner of such a finding may edit the
other package's files **only to replace that copy** with a call / import;
WP-mirrors-tooling's Rust edits are limited to `ts_constants.rs`, ts-rs
derives / exports in nail-api and pinning tests. 370 findings involve a file
outside their owner's area (mostly mirrors whose TypeScript side is in
`ui/`); the integrator resolves the few overlapping hunks.

### 7. Baseline (contract commit)

Scanned 1818 files, 659,632 lines; jscpd counts 13,576 duplicated lines
(2.1 %). Findings per check and package:

| Check | WP-rust-core | WP-rust-engines | WP-ui | WP-mirrors-tooling | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| clone | 529 | 349 | 238 | 119 | 1235 |
| css-clone | 0 | 0 | 10 | 0 | 10 |
| mirror-comment | 0 | 0 | 0 | 30 | 30 |
| mirror-inventory | 0 | 0 | 1 | 4 | 5 |
| rust-ts-list | 0 | 0 | 0 | 22 | 22 |
| rust-ts-const | 0 | 0 | 0 | 122 | 122 |
| const-value | 18 | 31 | 27 | 8 | 84 |
| fluent-text | 13 | 0 | 444 | 0 | 457 |
| theme-literal | 0 | 0 | 10 | 0 | 10 |
| css-length | 0 | 0 | 32 | 0 | 32 |
| magic-number | 24 | 24 | 21 | 0 | 69 |
| **Total** | **584** | **404** | **783** | **305** | **2076** |

Largest areas: `locales/en` 457 (Fluent texts), `crates/nail-core` 241,
`ui/src/features` 232, `e2e/tests` 96, `crates/nail-store` 60,
`crates/nail-config` 51, `crates/nail-mcp` 41, Gmail ↔ Graph clients 33,
nail-api ↔ UI features 33, `crates/nail-testkit-net` 30.

Top clone clusters (file pair, clones, duplicated lines, owner):

| Cluster | Clones | Lines | Owner |
| --- | ---: | ---: | --- |
| nail-dav `xml.rs` ↔ nail-testkit-net `dav/xml.rs` | 6 | 220 | WP-rust-engines |
| nail-discovery `xml.rs` ↔ nail-exchange `xml.rs` | 1 | 183 | WP-rust-engines |
| nail-proto-gmail `client.rs` ↔ nail-proto-graph `client.rs` | 13 | 171 | WP-rust-engines |
| nail-mcp `tests/writes.rs` (within) | 20 | 165 | WP-rust-core |
| Gmail ↔ Graph `testing/mod.rs` (mock servers) | 8 | 151 | WP-rust-engines |
| nail-testkit-net `managesieve.rs` ↔ `pop3.rs` | 11 | 142 | WP-rust-engines |
| nail-dav `xml.rs` ↔ nail-proto-ews `xml.rs` | 5 | 135 | WP-rust-engines |
| nail-core `tests/scheduling.rs` (within) | 10 | 130 | WP-rust-core |
| nail-core `tests/exchange.rs` (within) | 11 | 124 | WP-rust-core |
| e2e `a11y-windows.spec.ts` ↔ `p1-windows.spec.ts` | 12 | 122 | WP-mirrors-tooling |
| nail-config `accounts/write.rs` ↔ `rules/write.rs` | 8 | 114 | WP-rust-core |
| nail-core `exchange/sync.rs` ↔ `gmail.rs` | 9 | 113 | WP-rust-core |
| nail-proto-ews ↔ nail-proto-gmail `client.rs` | 5 | 113 | WP-rust-engines |
| nail-core `remote_access.rs` ↔ `service.rs` | 7 | 107 | WP-rust-core |
| nail-search `index.rs` ↔ `pim.rs` | 9 | 104 | WP-rust-core |

**Rust ↔ TypeScript mirror inventory** (all WP-mirrors-tooling's): 122
constants (among them the window specs and limits of `nail_api::windows`,
the theme ids / ranges / `BUILTIN_THEMES` / `PREVIEW_TOKENS` of
`nail_api::appearance`, `LEGACY_ICON_NAMES`, `KEY_CONTEXTS` (Rust, the plug-in
SDK's `manifest.mjs` and the UI's `editorModel.ts`), the plug-in limits of
`nail_api::plugins`, the view / settings / UI-state ids of `nail_api::views`,
the backup / recovery / smartcard / remote limits, `POP_OUT_CLOSE_EVENT`),
22 string lists (Exchange category and Gmail label colours, token modes,
favourite folders, list filters, calendar views, link schemes of the
installer manifest, …), 30 mirror comments (the agents model, the window
model, appearance, sieve / sync / triage / system / portability / print /
palette / security models, menus, views, folder pane and toolbar overflow
ported with their Rust tests, Lucide icons, default ports) and the curated
entries of `scripts/duplicates/mirrors.json`: the plug-in runtime API
(`prelude.js` ↔ `packages/plugin-api/src/api.ts`), theme ids from names
(`id_from_name` ↔ `themeIdFromName`), `MAX_ATTACHMENT_BYTES` (the composer
mock and the SDK's documentation), the `labeled_enum!` Fluent ids repeated in
UI message tables, and window sizes `Feature.windows` repeats from
`defaults/layout.kdl` (WP-ui's). Theme values in CSS-in-TS: 10 colour
literals and 32 lengths (among them `skinResponsive.ts`'s free/busy offset
`160px`, which repeats the name column of `defaults/theme.kdl`).

### 8. Package rules

Each package: works only in its folder and area (§6), never edits the
baselines, gets its own findings to 0 (`node scripts/check-duplicates.mjs
--strict --owner WP-…`) by sharing code and facts — or by a reasoned
`allow.json` entry where a finding is no violation (an allow entry is a
claim the reviewers check) — keeps behaviour unchanged, adds tests for every
shared helper it extracts, and keeps every suite green. Generated TypeScript
comes only from nail-api (`gen:api`); a pin is an equality test that fails
when either side changes.

### 9. Leftovers fixed by the contract

`scripts/test/ci-workflow.test.mjs` failed: `p1-mail.spec.ts` and
`p3-card-height.spec.ts` skip their desktop tests unless `NAIL_E2E_DESKTOP`
is set, but CI's desktop job did not run them. The job runs them now.

`e2e/tests/exchange.spec.ts` (EWS) failed from 2026-10-08 on: it answers the
fixture's meeting of 2026-10-07 in the agenda, which opens on the browser's
today. The test pins the page's date to that day (`page.clock.setFixedTime`).

## Consequences

- Every new copy fails at once (hooks, `npm run check`), while the four
  packages remove the recorded ones in parallel.
- `npm run check` gains about 3 s; the pre-commit and post-edit hooks the
  same when code is staged / edited.
- The heuristics will report some non-violations; `allow.json` keeps them
  visible with a reason instead of switching a check off.
- At the end of Q2 the baselines are deleted and the check is blocking at 0,
  like the zero-warnings rule (ADR 0123).
