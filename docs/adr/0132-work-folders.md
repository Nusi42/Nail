# ADR 0132: Master repository in OneDrive, work folders outside it

Status: accepted · 2026-10-04 · maintainer requirement (between the M5d contract and its packages)

## Context

The repository lives in OneDrive. Until now a local `.cargo/config.toml`
moved Cargo's `target/` to `%LOCALAPPDATA%\nail-build\target`, and test
output went to `%LOCALAPPDATA%\nail-build`. `node_modules` and `ui/dist`
still sat in OneDrive. That caused the reparse-point workarounds for Vite
(ADR 0129), and all parallel agents shared one checkout and one npm tree.
The maintainer keeps their projects as a master repository in OneDrive with
clones in `<work root>\<project>.winN`, each on its own
branch `work-<N>`, and required the same here: the master folder is the
master and is never used for development.

## Decisions

- **Master:** `<master>` holds `main` and the
  shared history. It only receives pushes and fast-forward merges. Nothing
  is built, tested, run, installed or committed there, releases included
  (release builds run in a work folder on a `main` commit, later in CI).
- **Work folders** (outside OneDrive) are clones of the master, with
  `origin` pointing to it:
  - `<work root>\nail-work-1`, branch `work-1`: the lead.
    Contracts, integration, reviews, fixes and verification run here.
  - `nail-work-2` and up, branches `work-2`, …: one per parallel package. Each
    is reset to `main` (which holds the contract), commits its package and
    pushes its branch. The integrator merges the branches into `work-1`.
  - `main` moves only by `git merge --ff-only work-1` in the master, after
    each green lead stage (contract, integration, fixes), so `main` is
    always green. This is the only write ever made in the master.
- **Process:** master, `work-1` lead, `work-2+` builders from `main`,
  merges into `work-1`, fast-forward of `main`, a committed `.cargo/config.toml` that includes the ignored
  `config.local.toml`, a pre-push check, and a Claude Code post-edit hook
  (`.claude/settings.json`) that runs the per-file checks on every edited
  file and hands findings back to the agent (exit code 2, at most 20
  lines). Waves run with Nail's wave workflow.
- **The post-edit hook decides by the edited file.** The lead session
  runs in the master, so the session folder always lies in OneDrive. The
  hook therefore finds the checkout from the *edited file* (the nearest
  folder with `.git` and `scripts/lib/run-checks.mjs`), runs that work
  folder's own checks, and skips only files inside OneDrive. The Git hooks
  (`.githooks/`, set by `npm ci`) are active in every work folder.
- **Build output stays in each work folder:** Cargo's default `target/`,
  and the default build base is the checkout's gitignored `.nail-build/`
  (Playwright, visual, perf, the desktop test build, installers;
  `NAIL_BUILD_DIR` still overrides). Parallel work folders never share a
  build directory. The development update key stays in
  `%LOCALAPPDATA%\nail-dev\keys` (a development-secrets folder, moved
  there from the retired `%LOCALAPPDATA%\nail-build`; `NAIL_KEYS_DIR`
  overrides), so a `git clean` of a work folder
  cannot delete it.
- **Guards:**
  - `scripts/lib/work-folder.mjs` `assertWorkFolder` refuses inside any
    OneDrive folder. It is called by `npm run dev`, `check`, `test`,
    `visual`, `tauri` and `build:desktop`, by `npm ci` / `npm install`
    (`prepare`) and by the Git hooks (pre-commit: per-file checks;
    pre-push: `npm run check`), so nobody commits in the master.
  - `scripts/test/hooks.test.mjs` keeps the hooks, the Claude Code
    settings and the cargo include wired.
  - In the master, `node scripts/work-folders.mjs guard-master` writes a
    `.cargo/config.local.toml` (gitignored, included by the committed
    `.cargo/config.toml`) whose `rustc-wrapper` refuses every
    compilation (also `cargo metadata`).
  - CLAUDE.md and the wave workflow (`.claude/workflows/nail-wave.js`) name
    the work folders.
- **Tooling:** `node scripts/work-folders.mjs create <n>` (run from a work
  folder) clones the master into `nail-work-<N>`, takes or creates `work-<N>`,
  writes the folder's `.claude/settings.local.json` (memory in the
  checkout) and runs `npm ci`.
- **Project memory** (`.claude/memory/`) and the CodeGraph index
  (`.codegraph/`) live in `nail-work-1`. Memory changes reach the master like
  any other commit.

## Consequences

- OneDrive syncs only sources and Git objects. The master's old
  `node_modules`, `ui/dist` and `.codegraph` are removed;
  `%LOCALAPPDATA%\nail-build` is no longer used (except `keys/`).
- Each work folder needs its own `npm ci` and Cargo build (several GB each).
- Parallel packages no longer share `node_modules` or a dev server lock.
  Each commits through the pre-commit hook in its own folder.
- When the repository moves to GitHub, GitHub becomes `origin` for the work
  folders and the master becomes one more clone.

## Alternatives considered

- **Keep one checkout and move only build output:** that was the previous
  state; `node_modules` stayed in OneDrive and agents shared one tree.
- **Git worktrees of the master:** Git state would live in OneDrive and be
  written by every agent at once. Clones keep each folder independent.
