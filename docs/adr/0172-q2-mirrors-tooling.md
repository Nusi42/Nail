# ADR 0172: Q2 WP-mirrors-tooling — one source for every Rust ↔ TypeScript fact, DRY tooling and e2e

Status: accepted · 2026-10-08 · Q2 WP-mirrors-tooling (contract ADR 0171; the
integrator renumbers)

## Context

ADR 0171 made SSOT, ODR and DRY mandatory and gave this package every
Rust ↔ TypeScript mirror (the TypeScript side, the generator in nail-api),
`packages/`, `examples/`, `e2e/`, `scripts/` and the root configs, plus the
flaky desktop `beforeAll` of `e2e/tests/p3-card-height.spec.ts`. The
detector (`node scripts/check-duplicates.mjs --owner WP-mirrors-tooling`)
counted 305 findings.

| Check | Before | After |
| --- | ---: | ---: |
| clone | 119 | 0 |
| mirror-comment | 30 | 0 |
| mirror-inventory | 4 | 0 (one left is WP-ui's `window-sizes`) |
| rust-ts-list | 22 | 0 |
| rust-ts-const | 122 | 0 |
| const-value | 8 | 0 |
| **Total** | **305** | **0** |

## Decisions

### Generated constants (`crates/nail-api/src/ts_constants.rs`)

- `npm run gen:api` writes `constants.ts` (`export const X = <json> as const
  [satisfies T]`) and the same values as `constants.json` into
  `ui/src/api/generated/` and `packages/plugin-api/src/generated/`
  (`ui_constants()` includes `plugin_constants()`). Node scripts import the
  JSON (`with { type: "json" }`); `write_index` lists both files, so the
  staleness test covers them.
- Sections: plug-in limits and writes (permissions, hook / job limits, MCP
  scopes, action kinds and their ask / allow rules), mail, appearance, shell,
  windows, remote, portability, security (recovery key alphabet and
  grouping, PIN limits), desktop, providers (Exchange categories, Gmail label
  palette — moved from nail-proto-gmail into `nail_api::gmail`), enums, agents,
  triage, sieve and all 49 `labeled_enum!` types.
- `labeled_enum!` implements a crate-private `Labeled` trait (`NAME`,
  `VALUES`, `label_id`), so every labeled enum yields `<NAME>_VALUES` and
  `<NAME>_MESSAGE_IDS` without a hand list. Other enums go through an
  `every!` macro: an exhaustive variant list checked by a `match`, so a new
  variant fails to compile until it is listed.

### Test vectors for the TypeScript ports

The UI keeps a few pure rules for immediate feedback and its mock core
(window ids, views, folder pane, overflow, menus, theme ids, recovery keys,
smartcards, remote access presets, mail actions, agents, Exchange
categories, print ranges, desktop states, triage / sieve issues, sync
policies, portability progress, plug-in extension ids). Their single source
is the Rust function: `nail_api::ts_vectors::Vectors` records `{fn, args,
out}` cases from the Rust code into
`crates/nail-api/fixtures/ts-vectors/<area>.json` (tests in
`crates/nail-api/tests/ts_vectors.rs`, `crates/nail-config/src/ts_vectors.rs`,
`crates/nail-core/src/sync_policy.rs`; regenerate with
`NAIL_UPDATE_SNAPSHOTS=1`); `ui/src/test/vectors.ts` `describeVectors` runs
the TypeScript ports over the same file. Either side changing alone fails.

The vectors found five ports that answered differently from Rust; the
TypeScript now matches: category names with control characters, print date
ranges (Feb 30 is refused), portability progress fractions, plug-in
extension ids (validated like panel ids) and theme names.

### Pinned where generation does not fit

- `src/prelude.test.ts` (plugin-api) pins the members of the host's
  `prelude.js` against the hand-typed `NailApi` with the TypeScript compiler
  API.
- `scripts/test/rust-ts-pins.test.mjs` compares ordered string lists of
  nail-config / nail-compose with their TypeScript counterparts (these crates
  are WP-rust-core's; their lists may move into nail-api later).
- `scripts/test/interop-lints.test.mjs` pins `scripts/interop`'s `[lints]`
  against the root `[workspace.lints]`.
- `scripts/duplicates/allow.json` entries name their `pinnedBy` test.

### Detector: a shrunk known finding stays known

Removing definitions from a file shrank WP-ui's known magic-number findings,
which then counted as new. `knownInBaseline` treats a finding whose members
are a subset of a recorded finding with the same key as known (tested in
`scripts/test/duplicates.test.mjs`).

### Tooling

- `scripts/lib/args.mjs` `forEachArg` is the one argument reader of the
  Node scripts; `scripts/lib/program.mjs` (`repoRoot`, `isMain`, `runMain`:
  answer → exit code, error → message and 2) the one program entry;
  `scripts/lib/cargo.mjs` (`cargoTargetDir`, `workspaceBinary`) finds the
  workspace's binaries for the e2e, perf and visual harnesses. The post-edit
  hook keeps a plain `isMain` (a broken hook must not block edits).
- The SDK builds plug-ins itself: `nail-plugin build [dir] [--watch]`
  (`tools/build.mjs`); the template and the examples lost their copied
  `scripts/`. `tools/program.mjs` is the SDK's own entry helper (a published
  package cannot depend on the repository's scripts). `tools/contract.mjs`
  reads the generated contract.

### End-to-end tests

- Servers: `nailPerTest` / `nailPerFile` / `nailFixture` / `nailOnDemand`
  (`e2e/support/nail.ts`, one server slot behind them) replace every
  hand-written `beforeEach` / `afterEach` pair; `e2e/support/serve.mjs`
  (`spawnServe`, `coreRequest`) starts `nail serve` and sends core requests
  for the harness, the perf scenarios and the visual theme probe.
- Shared steps: `views.ts` (`openMail`, `messageRow(s)`, `messageList`,
  `refreshList`, `moveSelectionTo`, `selectInTrash`, `closeWindow`,
  `nextFrames`, `applyLabel`, `saveVacationReply`, `DEMO_MULTI_CONVERSATIONS`),
  `compose.ts` (`newMessage`, `sendMessage`, SMTP / API sent mail),
  `mcp.ts` (`createMcpClient`, `grantMcpAccess`), `plugins.ts`
  (`installPlugin`, `pluginDevConsole`), `calendar.ts`, `demo.ts` (demo
  thread facts), `axe.ts`, `remote.ts` (`pairedRemoteMail`,
  `openInDarkTheme`), `responsive.ts` (`expectNoIssuesIn`, `showListTab`).
- `windowTour.ts`: the a11y window sweep and the P1 responsive sweep are one
  `WindowTour` (visit / close) with shared steps; each keeps only what it
  checks per window.
- Desktop: `desktopTests()`, `desktopPerFile()`, `desktopPerTest()`,
  `launchDesktop()` and `configWithIdentity()` in `support/desktop.ts`.

### The flaky desktop `beforeAll` (`p3-card-height.spec.ts`)

Starting the app could need more than the 180 s test timeout: CDP port (up
to 90 s), page (60 s), `__TAURI_INTERNALS__` (60 s) and the first-run screen
(60 s) on a busy machine. `desktopPerFile` now starts the app in `beforeAll`
with its own timeout (`DESKTOP_START_TIMEOUT`, 10 min) and `launchDesktop`
retries a failed start once with a new app (the first error is logged). The
same start serves `desktop.spec.ts`, `p1-mail.spec.ts` and, per test,
`desktop-m4b/m4c/m5c.spec.ts`.

## Consequences

- A new Rust ↔ TypeScript fact goes into `ts_constants.rs` (or a vector
  file); `npm run gen:api` and the vector tests keep both sides equal.
- New e2e specs start servers and desktop apps only through the support
  helpers; the detector reports a re-grown copy.
- Edits outside this package (made because the mirror's Rust side had to
  export a value): nail-api visibility (`recovery`, `remote`, `windows`,
  `views`, `gmail::LABEL_PALETTE`, `labels.rs` macro), nail-proto-gmail
  re-exports the palette, `app/src-tauri` uses `POP_OUT_CLOSE_EVENT`,
  nail-config `theme_edit::id_from_name` is `pub(crate)` with its vectors,
  nail-core `sync_policy` writes `sync.json`.
