# ADR 0004: Toolchain and version choices for the M0 scaffold

Status: accepted · 2026-09-29 · M0 WP0

## Decisions

### Rust workspace

- **Edition 2024, resolver 3** (the default for edition 2024; it is feature
  resolver 2 plus MSRV-aware dependency resolution). `rust-version = "1.97"`.
- **`default-members` exclude `app/src-tauri`.** `tauri::generate_context!()`
  embeds `ui/dist`, which exists only after `npm run build`. Plain
  `cargo build` / `cargo test` cover the core crates; `--workspace` (as in
  CI, after the UI build) includes the desktop shell.
- **Lints** in `[workspace.lints]`: `unsafe_code = "deny"`, clippy `all` plus a
  few extras; CI runs clippy with `-D warnings`. `nail-api` additionally
  warns on missing docs, because it is the documented contract.
- **Static VC runtime only in release builds** (added in M0 WP5).
  `tauri-build` links the VC runtime statically by default by putting an
  empty `msvcrt.lib` on the native link search path. In
  `cargo test --workspace` that path leaks into the doctests of every other
  crate, which then fail to link. `app/src-tauri/build.rs` therefore enables
  `static_vc_runtime` only for the `release` profile (the distributed
  binary), and debug builds and tests use the dynamic runtime.
- **Build output outside the repo**: a gitignored `.cargo/config.toml` sets
  `target-dir` outside the master folder.

### UI toolchain

| Package | Version | Note |
| --- | --- | --- |
| React | 19.3 | |
| Vite / `@vitejs/plugin-react` | 8.3 / 6.1 | |
| Vitest + jsdom + Testing Library | 5.0 / 30 / 16 | |
| TypeScript | **6.0** (not 7.0) | `typescript-eslint` 8.71 supports `>=4.8.4 <6.1.0`; move to 7.x when it does |
| ESLint | **10.11** | with `@eslint/js` 10 and `typescript-eslint` 8.71 |
| `eslint-plugin-react` | 7.37.5 | needed for `react/jsx-no-literals`; its peer range stops at ESLint 9 |
| Playwright | 1.63 (Chromium only for now) | |

**ESLint 10 with an override.** ESLint 9 is out of support, but
`eslint-plugin-react`'s last release declares `eslint: ^9.7` as its highest
peer. The rules we use (`jsx-no-literals`, `no-danger`, `forbid-dom-props`,
the recommended set) work on ESLint 10; `ui/src/lint-rules.test.ts` proves
that the project rules fire. The root `package.json` therefore has
`"overrides": { "eslint-plugin-react": { "eslint": "$eslint" } }`. Remove the
override when the plugin widens its peer range, or replace the plugin if it
breaks.

**Project lint rules** (`ui/eslint.config.js`):

- `react/jsx-no-literals` (`noStrings`, props ignored) — no text in JSX.
- `no-restricted-syntax` — no `style` prop; no string or template literals in
  `className`; no literal text in `aria-label`, `title`, `alt`,
  `placeholder`, `label` and similar attributes.
- `react/no-danger` — no `dangerouslySetInnerHTML` (mail HTML renders only in
  the sandboxed iframe).
- Test files may contain literal fixture text; the styling rules still apply.

### Formatting

`rustfmt` (edition 2024) and Prettier (width 100). `.gitattributes` forces
LF line endings so formatting checks behave the same on Windows.
