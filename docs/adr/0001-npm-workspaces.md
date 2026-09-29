# ADR 0001: npm workspaces instead of pnpm

Status: accepted · 2026-09-29 · M0 WP0

## Context

`design.md` sketches the repository as a "Cargo + pnpm workspace". The
development machine (Windows 11, Node 26.7, npm 11.19) has no pnpm, and the
JavaScript side of M0 is small: the React renderer (`ui/`) and the Playwright
suite (`e2e/`). Later packages (`@nail/plugin-api`, a plug-in starter
template) will join them.

## Decision

Use **npm workspaces**. The root `package.json` is private and declares the
workspaces `ui` and `e2e`; one `package-lock.json` at the root pins the whole
tree. Root scripts delegate to the workspaces:

| Script | Runs |
| --- | --- |
| `npm run build` | `vite build` in `ui` (after `tsc -b`) |
| `npm test` | Vitest in `ui` |
| `npm run lint` | `lint` in every workspace (ESLint + `tsc` + Prettier in `ui`; `tsc` + Prettier in `e2e`) |
| `npm run e2e` | Playwright in `e2e` |
| `npm run gen:api` | regenerates `ui/src/api/generated` from `nail-api` |
| `npm run tauri` | the Tauri CLI (`@tauri-apps/cli`, a root dev dependency) |

CI uses `npm ci`.

## Consequences

- No extra tool to install; works with the Node toolchain that is already
  there and on GitHub's Windows runners.
- npm hoists dependencies to the root `node_modules`; packages must still
  declare everything they import (ESLint/TypeScript catch misses).
- Overrides live in the root `package.json` only (see ADR 0004).
- Switching to pnpm later is mechanical (workspace file + lockfile).
