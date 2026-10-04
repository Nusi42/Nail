# ADR 0128: Q1 UI — zero findings in the renderer

Status: accepted · 2026-10-04 · wave Q1 (package q-ts-ui; written down by the integration from the package's report)

## Context

ADR 0123 set up the quality baseline. The q-ts-ui package owns `ui/` and
`locales/`. At the contract `ui/` had 3554 ESLint findings (type-aware
`strictTypeChecked` + `stylisticTypeChecked`, React Hooks, jsx-a11y), one
markdownlint and seven typos findings, and the Vite build printed two
`INEFFECTIVE_DYNAMIC_IMPORT` warnings. No lint level was lowered and no rule
was switched off.

## Decisions

### Vite on a OneDrive checkout

Vite's native resolver canonicalizes the real path of every package it
resolves. Inside a OneDrive folder `node_modules` is a cloud-file reparse
point (tag `0x9000e01a`), the canonicalization fails and no bare import
resolves ("Failed to resolve import"). `ui/vite.config.ts` sets
`resolve.preserveSymlinks: true`; the renderer imports no symlinked
workspace package, so nothing else changes. The integration applied the same
setting to `packages/create-nail-plugin/vitest.config.ts`, where the
symlinked `@nail/plugin-api` was otherwise externalized and loaded by Node
natively (ADR 0129).

### Fixes

- **Generated code:** flat-config ignore patterns are relative to the config
  file, so the shared `ui/src/api/generated/` entry never matched from
  `ui/eslint.config.js`. The integration moved the fix into the shared list
  (`**/src/api/generated/`, `**/plugin-api/src/generated/`).
- **All 11 `react-hooks/exhaustive-deps` disables are gone:** `useEffectEvent`
  (MergeDialog, ContextMenu, ImportDialog, PimMenu, ViewHost), lazy
  `useState` instead of once-only `useMemo` (ContactEditor, TaskEditor;
  EventEditor got `initialForm()`), a CodeMirror compartment in
  MarkdownEditor (a new label now updates without recreating the editor; new
  test), `optionId` moved out of Palette, TimeGrid declares its real
  dependencies.
- **`unbound-method`:** 348 method signatures became function properties;
  the day-drag hook returns arrow functions.
- **Non-null assertions:** fixed one by one in source (tuples,
  destructuring, explicit checks); tests use the new `must()` helper.
- **Small typed helpers** with unit tests: `ui/src/text` (`charCount` /
  `chars` count text like the Rust core, `nonEmpty` / `hasText` treat empty
  text as missing instead of `||`), `ui/src/test` (`must`, `stubProperties`
  for `window.matchMedia` / `URL.createObjectURL`, typed versions of Vitest's
  asymmetric matchers). New UI code uses them.
- **Test setup** provides no-op `scrollIntoView`, pointer capture and
  `elementFromPoint` (jsdom lacks them), so components lost their `?.`
  guards.
- **Deprecated APIs:** `FormEvent` → `SubmitEvent`, `fetchQuery` → `query`.
  Floating promises, async click handlers, template typing, unnecessary
  conditions and `act` usage fixed.
- **Build:** `select.ts` loads the security demo data through the mock
  module it already loads; `BootTheme` imports the default theme statically
  (the first screen is styled at once; its test was updated).
- **Login probe:** the UI treats `204 No Content` on `POST /api/auth` as "no
  login" (`NO_LOGIN_STATUS`) and still accepts `404` from older servers; the
  integration made `nail serve` without remote login answer 204.
- **Slow test:** the MCP "grants scopes" test uses label queries (about 3 s
  instead of 11.6 s; it timed out at 20 s under full-suite load).

### Remaining exceptions (33 comments, each with `-- reason`)

- 2 × `no-control-regex`: the regexes exist to reject control characters.
- 8 × `jsx-a11y/no-autofocus`: a dialog or login step deliberately moves
  focus. Component props named `autoFocus` became `focusOnOpen` /
  `focusOnMount`.
- 22 comments covering 25 jsx-a11y exceptions where the keyboard is handled
  by the containing widget (roving focus, `aria-activedescendant`) or Escape
  bubbles up from controls inside.
- The existing `react-hooks/incompatible-library` comment in
  `MessageList.tsx`.

Three `section` elements lost a redundant `role="region"`.

## Consequences

- `npm run lint -w ui` (ESLint `--max-warnings 0`, `tsc -b`, Prettier) has 0
  findings; the full Vitest suite is green without `stderr |` blocks.
- The chunk-size warning of the build was left to the integration, which
  took the mock core out of the app bundle and split the initial code into
  parallel chunks (ADR 0129).
