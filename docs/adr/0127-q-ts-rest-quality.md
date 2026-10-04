# ADR 0127: Q1 rest — zero findings in e2e, packages, examples, scripts, docs and workflows

Status: accepted · 2026-10-04 · wave Q1 (package q-ts-rest)

## Context

ADR 0123 set up the quality baseline and measured the findings. This
package owns `e2e/`, `packages/`, `examples/`, `scripts/` (with the
`scripts/interop` crate), `docs/`, `userDocs/`, `.github/`, `.claude/memory`
and the root configuration files except `Cargo.toml` and `package.json`.
At the contract it had 2059 ESLint findings, 507 tsc errors (root
`tsconfig.json`), 124 markdownlint findings, 23 typos findings and 44 clippy
findings in `scripts/interop`. No lint level was lowered and no rule was
switched off.

## Decisions

### TypeScript / JavaScript

- **Node scripts are typed with JSDoc** (`@param`, `@returns`, `@typedef`)
  instead of being converted to TypeScript: they run with plain `node`,
  without a build step, in CI jobs that have no `npm ci` for a TS loader.
- **JSON** — `JSON.parse` answers `any`, and a JSDoc cast
  (`/** @type {T} */ (x)`) is invisible to typescript-eslint (it sees the
  `any` expression inside the parentheses). `scripts/lib/json.mjs` has
  `parseJson` / `readJson`, which answer `unknown`; callers cast that (tsc
  sees the cast, ESLint sees `unknown`) or narrow it. Code that reads
  untrusted JSON (the plug-in manifest check, the update check, the visual
  manifest) narrows field by field instead of casting.
- **`packages/plugin-api/tools/manifest.mjs`** works on `unknown` with three
  small helpers (`isRecord`, `fields`, `items`) instead of `any`. Behaviour
  is unchanged for every JSON shape the tests cover; a few malformed shapes
  that used to throw a `TypeError` or be iterated character by character
  (a string where a list belongs) are now reported as issues
  (`contributes.locales` that is not an object, `contexts` that is not a
  list), and a hook kind is looked up with `Object.hasOwn` (before, `"toString"`
  passed the `in` check). Character limits count Unicode scalar values with
  `Array.from(s).length`, as Nail's Rust parser does (`chars().count()`);
  the spread operator did the same but trips `no-misused-spread`.
- **No non-null assertions.** `e2e/support/defined.ts` exports
  `defined(value, what)`, which answers the value or throws
  "`what` is missing" — a clearer failure than the `TypeError` an assertion
  leads to. Pixel arithmetic and other bounded index reads use `?? 0`.
- **Template literals** take numbers through `String(n)` (identical
  output; `restrict-template-expressions` allows no numbers in the strict
  preset). Nullable values get an explicit fallback (`?? ""`), which made
  a few messages say "(missing)" / "(none)" instead of "undefined".
- **`async` without `await`**: the plug-in test host (`testing.ts`) answers
  the asynchronous host API synchronously. Its methods now return
  `asPromise(() => …)` (`new Promise((resolve) => resolve(fn()))`), which
  keeps what `async` gave them: a throw becomes a rejected promise. Test
  doubles elsewhere return `Promise.resolve(…)`.
- **Empty stubs** (`() => {}` in tests) hold a `/* no-op */` comment, the
  documented way to mark an intentionally empty function.
- **`jsx-runtime.ts`**: the compile-time check that every node type has JSX
  props is now part of `JSX.IntrinsicElements` (a missing type makes every
  element fail to type-check) instead of an unused constant; `jsx` /
  `jsxDEV` keep their public signatures (key, source info) as typed
  constants whose implementations ignore those arguments.
- **`gen-docs.mjs` `cleanDoc`** keeps list items apart ("•") when it joins
  a doc comment onto one line and turns example e-mail addresses into code,
  so the generated user docs pass markdownlint (MD037, MD034). The
  regenerated `userDocs/en/plugins/api-reference.md` changed in three lines.

### The exceptions (each at the narrowest scope, with its reason)

| Where | Rule | Why |
| --- | --- | --- |
| `packages/plugin-api/src/api.ts` `NailSettingsApi.get<T>` | `no-unnecessary-type-parameters` | Public plug-in API: callers name the setting's type (`get<string>(key)`); removing the parameter breaks plug-ins. |
| `api.ts` `PluginArrivalHook`, `PluginBeforeSendHook`, `PluginOpenHook`, `PluginMcpToolHandler` | `no-invalid-void-type` (disable / enable pair per type) | A handler may be a plain function without `return`; its inferred return type is `void`, which `undefined` would not accept. Changing it breaks plug-ins. |
| `jsx-runtime.ts` `namespace JSX` | `no-namespace` (description added) | TypeScript looks the JSX types up in a namespace named `JSX`. |
| Playwright fixtures and a skipped test without fixtures (`e2e/tests/*`, `e2e/visual/*`) | `no-empty-pattern` | Playwright requires an object destructuring pattern as the first argument; `({}, use)` is its documented form for a fixture without dependencies. |

### Spelling (typos)

Words were fixed where they were misspelled (the spelling "unparsable"
spelling, protocol commands with a plural "s" glued on rephrased, a
two-letter variable renamed to `pNumeric`). `typos.toml` gets one word
(`flate`: the flate2 crate and zip's `deflate-flate2-*` features) and an
`extend-ignore-re` for deliberate non-English text or misspellings, marked
at the narrowest scope: a line ending in `spellchecker:disable-line`, or a
block between `spellchecker:off` and `spellchecker:on` comments (the
patterns typos documents). They mark German test strings (the attachment
check example, the Fluent test catalog, the German relative dates the
locale test must not show) and two deliberately broken config examples.

### Markdown

`--fix` handled blank lines around lists, headings and fences; the rest was
fixed by hand: code fences got a language, placeholders such as
`<plug-in>` or `<View>` became code (inline HTML otherwise), bare URLs got
angle brackets, a table cell's `|` was split into two code spans, and code
spans with a meaningful trailing space (`` `From ` ``, `` `-- ` ``) became
quoted text ("From ", "-- "), since a code span cannot end in a space.
`--fix` turns a prose line starting with `+` (meaning "plus") into a list
item; those lines were rewritten with the `+` at the end of the previous
line. A stale link in `userDocs/en/getting-started.md` (to a section of the
M0 preview) now points at "What Nail can do".

### Rust (`scripts/interop`)

The library's `from_env` panics with a message naming the variable
instead of `unwrap()` (it is documented to panic on a misconfigured CI
job). The integration test crates `tests/dav.rs` and `tests/servers.rs`
declare `#![cfg(test)]`: clippy's `allow-unwrap-in-tests` covers only
`#[test]` functions and `#[cfg(test)]` items, not the helper functions of an
integration test crate, which is test code throughout.

### Socket allowlist

`crates/nail-proxy/src/policy.rs` now writes the blocked range "this
network" as `Ipv4Addr::UNSPECIFIED` (clippy, package q-rust-a); the entry
in `scripts/lib/socket-binds.mjs` follows (a destination range, not a
bind).

## Consequences

- ESLint (`--max-warnings 0`), tsc (root `tsconfig.json`, every workspace
  `typecheck`, the examples) and Prettier report nothing for `e2e/`,
  `packages/`, `examples/`, `scripts/` and the root files; markdownlint
  nothing for `docs/`, `userDocs/`, `.claude/memory` and this package's
  READMEs; typos nothing in these areas; actionlint and the EditorConfig
  check nothing.
- New Node scripts follow the same pattern (JSDoc, `readJson`, `defined`);
  `docs/dev-setup.md` lists the habits.
- `packages/create-nail-plugin`'s Vitest suite cannot import
  `@nail/plugin-api` in this environment (Node 26 loads the linked package's
  `.ts` sources natively and fails on the extensionless relative imports);
  the files at HEAD fail the same way, so this is not caused by the lint
  changes. It needs a separate fix (see the package report).
