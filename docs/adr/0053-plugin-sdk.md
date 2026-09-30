# ADR 0053: plug-in SDK, starter template, "Follow up" example, plug-in user docs

Status: accepted · 2026-09-30 · M2d WP63

## Context

Plan M2d WP63: the `@nail/plugin-api` TypeScript package (types generated
from the Rust contract, a JSX runtime producing component trees), a
starter template (`packages/create-nail-plugin`), the design's "Follow up"
example adapted to the read-only preview, and `userDocs/en/plugins/`. The
contract is ADR 0050; the host (`crates/nail-plugins`, WP61) was written in
parallel. This ADR records the SDK's decisions.

## Decisions

### The runtime API (`src/api.ts`) follows the host's prelude

The host installs a frozen global `nail` (WP61's `prelude.js`). The SDK
types exactly that object as `NailApi` and declares it as the global
`nail`: `plugin {id, version}`, `apiVersion`, `mail.{get, getBody, search,
thread}`, `ui.{registerCommand, registerPanel, showDialog(spec, {onAction}),
notify, updatePanel}`, `commands.register`, `storage.{get, set, delete,
keys, clear, usage}`, `http.fetch`, `settings.{get, all, onChange}`
(synchronous), `log.*`. Registrations return nothing (the host cannot undo
them). UI actions are `{componentId, values}`. `activate` receives `{plugin,
commands, panels, settings}`; `deactivate` runs synchronously.
`storage.usage()` resolves to `{keys, usedBytes, quotaBytes}` (camelCase,
as the host's runtime writes it). Error codes are those of `ErrorCode`
(`forbidden`, `invalid_request`, `too_large`, `rate_limited`, …).

Only the contract types (`src/generated/`, ts-rs, `npm run gen:api`) are
generated; `api.ts` is hand-written because the JavaScript surface lives in
the host's prelude, not in a Rust type. `index.ts` re-exports both.

### SDK conveniences on top of the host

- `nail` (export) wraps the global: dialog bodies, panel renders and
  `updatePanel` accept JSX / strings / lists (normalized with `toNodes`),
  and every property is looked up at call time, so code outside Nail fails
  with a clear `unsupported` error instead of `undefined`.
- `definePlugin(setup)` returns `{kind: "nail.plugin/1", apiVersion: "1",
  activate, deactivate?}`: it passes the host's context through, with panel
  providers normalized, and falls back to the global when the host passes
  no context.
- `isPluginApiError(e, code)` / `createPluginApiError` match the host's
  `NailError {code}`.
- `checkTree` mirrors the common rules of `validate_tree` (ids, depth,
  size, image types, table rows) for development; the host stays
  authoritative.

### JSX runtime (`@nail/plugin-api/jsx-runtime`)

The automatic runtime (`jsx`, `jsxs`, `jsxDEV`, `Fragment`;
`jsxImportSource: "@nail/plugin-api"`). Intrinsic elements are the node
types (`<text>`, `<contact-picker>`, `<stack>` …) with the node's fields as
props, typed from the generated `PluginUiNode` (`JSX.IntrinsicElements`).
`<text>` / `<markdown>` take their `value` from text children; `<stack>`
takes nested elements and turns strings into `text` nodes; other elements
refuse children; `null` / `undefined` / booleans / empty strings render
nothing (React semantics for `{cond && …}`); a `type` prop cannot change the
node type; unknown elements throw. There is no state or reconciliation:
the result is plain JSON.

### Sandbox globals

`@nail/plugin-api/globals` (a `.d.ts`) types `console`, `setTimeout`,
`clearTimeout` and `queueMicrotask`, the other globals of the prelude. The
template's tsconfig uses `lib: ["ES2023"]` and `types:
["@nail/plugin-api/globals"]` — no DOM or Node types, so code that uses
APIs the sandbox lacks does not type-check. A `tsconfig.sandbox.json`
checks that the SDK itself compiles under these settings.

### Testing (`@nail/plugin-api/testing`)

`createTestHost({manifest, permissions, messages, bodies, settings,
dialog, fetch, storageQuota})` implements `NailApi` in memory with the
host's rules: permission per call (`forbidden`), bodies never for
encrypted mail, `network:` host patterns, `https:` only, storage quota
(`too_large`), JSON copies, registrations only for commands / panels of the
manifest, tree checks on dialogs and panels. It records dialogs,
notifications, panel updates and log lines, and drives commands, panels,
panel buttons and setting changes. The example and template tests use it,
and plug-in authors can too.

### Tools (`@nail/plugin-api/tools`, bin `nail-plugin`)

Node-side, dependency-free (`node:zlib` deflate + `crc32`):

- `checkManifest` mirrors `PluginManifest::parse`'s main rules with the
  same issue kinds (ids, versions, `main`, permissions incl. the M4 names
  and `network:` patterns, commands, placements, keys, panels, settings,
  locales, counts).
- `packPlugin({dir, out})` writes `dist/<id>.nailplugin`: `manifest.json`
  first, then `main`, the `contributes.locales` files and `assets/**`,
  sorted; sources, maps and `node_modules` stay out. The zip writer is
  deterministic (fixed 1980-01-01 timestamps, given order), so the same
  build gives the same SHA-256 the install dialog shows. Checked against
  Python's `zipfile` and PowerShell's `Expand-Archive`; `readZip` verifies
  CRCs.

No zip library was added: the format needed is small and a deterministic
writer matters for the preview-then-install hash check (ADR 0050).

### Bundling: esbuild

Template and example bundle with esbuild (MIT; 0.28.2 as a dev dependency
of both packages): one ES module, `platform: "neutral"`, `target:
"es2023"`, `jsx: "automatic"`, everything inlined (Node built-ins fail to
resolve). esbuild's postinstall script is not run by this repository's npm
policy; the platform binary package (`@esbuild/win32-x64`) works without
it. The SDK declares esbuild as an optional peer dependency.

### Starter template (`packages/create-nail-plugin`)

`node packages/create-nail-plugin/index.mjs <dir> [--id] [--name] [--sdk]
[--force]` copies `template/` (manifest.json, src/index.tsx, tsconfig.json,
package.json, scripts/build.mjs, scripts/pack.mjs, locales/en.ftl,
README.md, `gitignore` → `.gitignore`). Placeholders `__PLUGIN_ID__`,
`__PLUGIN_NAME__`, `__SDK_SPEC__`, `__ESBUILD_SPEC__`,
`__TYPESCRIPT_SPEC__` are replaced as JSON string literals in code files
and raw in text files. The SDK spec defaults to a `file:` path to this
repository's `packages/plugin-api` (`@nail/plugin-api` is not published);
esbuild and TypeScript versions come from the SDK's package.json. The npm
script that packs is `package` (not `pack`, which would be confused with
`npm pack`).

### The "Follow up" example (`examples/follow-up`)

The design's example without tasks (write access and `tasks.*` are M4): a
command `follow-up` (reader toolbar after `mail.forward`, Shift+F in the
list) reads the message's headers (`mail.read`), opens a dialog with a due
date (`default-days` setting: `today` / `tomorrow` / a date) and a note,
stores `reminder:<messageId>` in plug-in storage, notifies, and redraws
its `due` panel (a table of reminders with "Remove past reminders").
Existing reminders can be edited or removed. Expected failures (nothing
selected, `forbidden` / `not_found`, `too_large`) become notifications
instead of stopping the plug-in. The panel has no placement (the user
adds `plugin-panel "follow-up.due"`), so installing only proposes the
toolbar item. Its scripts and tsconfig are byte-identical to the
template's (tested).

Runtime texts of the example are English literals in `src/strings.ts`:
they are the plug-in's data, and the preview API has no way to read the
plug-in's Fluent catalogs at run time (manifest texts use `%id%` +
`locales/en.ftl`).

`dist/` is gitignored. The built package is 5 KB; it is not committed.

### User docs (`userDocs/en/plugins/`)

README (overview, installing, layout, remote), permissions, getting
started, manifest reference, API reference, component reference. The
reference parts of `api-reference.md` and `components.md` are generated
between markers by `packages/plugin-api/scripts/gen-docs.mjs` (TypeScript
compiler API over the SDK's types; component descriptions and input values
from the doc comments of `nail_api::plugins::ui`); `npm run docs -w
packages/plugin-api` regenerates, and a test fails when they are stale.

### Tests

- plugin-api (Vitest, 49): JSX runtime (every node type, children rules,
  fragments, components, errors), `nail` wrapper and `definePlugin`, test
  host permissions / storage / registrations / settings, `checkTree`,
  zip round trip / determinism / CRC, `checkManifest` against the Rust
  rules, `packPlugin` and the CLI, docs freshness.
- create-nail-plugin (Vitest, 21): names and placeholders, scaffolding and
  its errors, the CLI; a scaffold in a temp dir (node_modules junctions to
  the repository's SDK, esbuild, TypeScript) type-checks, builds, packs and
  runs against the test host; the example type-checks, builds, packs into
  `follow-up.nailplugin` and behaves as described (save, edit, remove,
  cancel, settings, no selection, missing `mail.read`, full storage, panel).

Vite caches go to the temp directory, never into the repository.

## Open points

- The bundle is exercised in Node against the test host, not in QuickJS;
  the M2d Playwright scenarios (WP64) run the example in the real host.
- The hand-written types of the host-facing API (`api.ts`) are not
  checked against `prelude.js` automatically; a host test that evaluates
  the example bundle would catch drift.
