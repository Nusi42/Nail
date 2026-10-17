# Nail — project guide for Claude

Nail (working name) is an email + PIM client: Rust core, TypeScript/React UI, Tauri 2 desktop shell, a web-server mode that serves the same UI, and MCP access to all data.

- What to build: `docs/design.md` (exported from the living doc linked in its header).
- How and in which order: `docs/plan.md` (milestones M0–M5, work packages, gates).
- Decisions and deviations: `docs/adr/NNNN-title.md`.
- Where the code lives: `docs/architecture.md` (code map + notes by milestone); commands and test conventions:
  `docs/dev-setup.md`.
- Project memory: `.claude/memory/` (see "Memory" below).
- Where to work: the work folders, never the master (see "Work folders" below).

## Non-negotiable rules

- **Work folders (ADR 0132).** `<master>` is the master repository: it only receives
  pushes and `git merge --ff-only work-1`. Never edit, build, test, run, install or commit anything there (the npm entry
  points, `prepare`, the pre-commit hook and its Cargo rustc wrapper refuse). Work in `<work root>\nail-work-1`
  (branch `work-1`, the lead) or, for a parallel package, `nail-work-<N>` (branch `work-<N>`, reset to `origin/main`, push the
  branch when done; never push to `main`). New folder: `node scripts/work-folders.mjs create <n>`.
  Details: `docs/dev-setup.md` "Work folders". Hooks: pre-commit (per-file checks), pre-push (`npm run check`), and
  the Claude Code post-edit hook (per-file checks on each edited file; fix what it reports).
- **Tests from day 1.** Every change ships with unit tests; every user-visible behaviour with a Playwright scenario. Never report work as done while any suite is red or tests were skipped.
- **Docs split.** Internal docs → `docs/`. User-facing manuals and references → `userDocs/<lang>/` (`en` written, `de` empty for now).
- **Languages.** No literal user-visible strings in React or Rust. Strings live in Fluent catalogs `locales/en/*.ftl` (complete) and `locales/de/*.ftl` (same files, empty). Missing keys fall back to English per key.
- **Everything themeable.** React components never use inline styles or literal class names. They emit `data-part` attributes; styling comes only from the compiled theme (`theme.kdl`). Structure comes only from `layout.kdl`.
- **The core owns data.** The UI talks only to `NailClient`; business logic lives in Rust. Every write goes through the operation queue.
- **Windows first.** macOS, Linux and mobile come later.
- **No fixed development ports.** Take a free port or fall back to one, so Nail runs next to other projects (`scripts/dev.mjs`: Vite on `NAIL_DEV_PORT` / 5791 / a free port + Tauri `devUrl`; `nail serve` falls back from 4780; tests bind port 0).
- **No non-loopback sockets.** Never open sockets on non-loopback/unspecified addresses (TCP or UDP), except the user-enabled remote UI listener; use the OS DNS API; tests bind 127.0.0.1 (ADR 0057; `scripts/check-socket-binds.mjs` runs in `npm test`).
- **No private data in git (ADR 0192, maintainer rule).** Nothing of the maintainer's accounts or profile — addresses, domains,
  providers / server names, folder names, mailbox contents, account ids, the Windows user name — in code, tests, docs,
  ADRs, memory, workflow args or commit messages. Write paths as `%USERPROFILE%\…` (prose) / `~/…` (Git Bash); test
  data = `example.com` / `.net` / `.org` and the demo personas; pass the maintainer's bug reports to agents generically. The guard
  `scripts/check-private.mjs` (pre-commit staged content, commit-msg, pre-push, step `private` of `npm run check`)
  reads its patterns from `%USERPROFILE%\.nail-private\patterns.txt`, which lives outside every repository: never
  commit it or copy its values anywhere (absent file = passes, for CI).
- **Zero warnings (Q1, ADR 0123).** 0 warnings, 0 errors; never relax lint levels or `-D warnings` (no new `allow`
  in `[workspace.lints]`, no ESLint rule switched off, no `--no-verify`). An unavoidable exception needs
  `#[expect(lint, reason = "…")]` (Rust; `#[allow]` is denied) / `// eslint-disable-next-line rule -- reason` at the
  narrowest scope. `npm run check` runs every formatter and linter; the hooks check staged / edited files. The
  conventions that keep it at 0 (test crate roots, casts, spelling markers, `must()`, mock-free barrels, …):
  `docs/dev-setup.md` "Linters and formatters".
- **SSOT / ODR / DRY (maintainer rule, mandatory, existing and new code).** Every fact (type, constant, id, limit, size, string,
  rule, mapping, schema) has one source of truth and one definition; everything else derives it (generated code,
  imports, shared tokens, shared helpers). Rust ↔ TypeScript facts are generated (ts-rs, `npm run gen:api`) or pinned by
  a test, never mirrored by hand. No copy-pasted logic: the second copy is the moment to extract a shared function,
  component or test helper. Reviews report every violation as must-fix; the duplicate check (`npm run
  check:duplicates`, blocking) stays at 0; a reasoned non-violation goes into `scripts/duplicates/allow.json`. Where
  shared things live: `docs/dev-setup.md` "Linters and formatters" (after the duplicate check).
- **Record decisions.** New library choices or deviations from the design get an ADR.
- **Keep the docs current.** When commands, layout or conventions change, update this file, `docs/architecture.md`
  and `docs/dev-setup.md` in the same commit. This file holds only rules and pointers; details go into `docs/`.

## Tooling facts

- Rust stable (1.99; `rust-version` stays 1.97), MSVC, Node 26 + npm 11. **npm workspaces, not pnpm** (pnpm is not installed). No docker locally: integration tests use in-process mock servers from `crates/nail-testkit`.
- Work folders live outside OneDrive (ADR 0132): Cargo builds into the folder's `target/`, test and build output goes to its
  gitignored `.nail-build/` (`NAIL_BUILD_DIR` overrides; the development update key stays in `%LOCALAPPDATA%\nail-dev\keys`).
  When running several cargo processes in one folder, give each its own `CARGO_TARGET_DIR` under `.nail-build/`.
- Config files are KDL v2.
- SQLite is SQLite3 Multiple Ciphers (M4c, ADR 0095) through the root `[patch.crates-io]` stand-in
  `crates/nail-store/libsqlite3-sys-sqlite3mc`; the first build compiles its amalgamation (about 80 s in debug).

## Commands

The essentials; every command (benchmarks, installers, snapshots, plug-ins, perf, visual) is in `docs/dev-setup.md`.

```text
npm ci                                     # npm workspaces: ui, e2e, packages/*
npm run build                              # ui/dist — required before any `cargo … --workspace` (Tauri embeds it)
cargo test --workspace -j 4                # incl. the Tauri shell (plain `cargo test` skips app/src-tauri)
npm test                                   # socket-bind check, Vitest (ui), packages/* tests, script tests
npm run check                              # every formatter and linter, 0 warnings (node scripts/check.mjs --only a,b / --list)
npm run e2e                                # builds ui + nail-cli + desktop app, then Playwright (e2e:only = already built)
npm run visual                             # visual regression (visual:update writes baselines, then review)
npm run readme:media                       # re-record the README's GIFs and stills in docs/media (review before commit)
npm run gen:api                            # regenerate the TypeScript contract (a test fails when stale)
cargo run -p nail-cli -- serve --seed demo-multi   # web mode with mock accounts and test controls (also demo, demo-imap)
npm run dev / npm run tauri dev            # Vite on a free port / desktop app with hot reload
```

## If you are the lead (the main session)

The owner starts Claude Code in the master folder `<master>`. That folder
is only the launch point: drive the work folders by absolute path (`git -C`, `cd` in each command), and edit, build,
test and commit only there; in the master run nothing but `git merge --ff-only work-1`. A new session picks up the work
like this:

1. Read `.claude/memory/next-steps.md` (the current wave and what comes next) and the wave's status line in
   `docs/plan.md`.
2. `git fetch` in `nail-work-1`, then check the package branches: `git log --oneline main..origin/work-<N>`. Agents belong
   to the session that started them: a package with commits on its branch and no running agent is finished or
   half-finished — check it in its folder (`npm run check`, its tests), then merge it or let an agent finish it.
3. Run waves with the `nail-wave` workflow (`.claude/workflows/README.md`): packages in `nail-work-2` and up, reset to
   `main`; reviewers with a fresh context; merge into `work-1`, full suite, push `work-1`,
   `git -C <master> merge --ff-only work-1` (Git Bash), then `git push github work-1:main` from
   `nail-work-1` (GitHub, ADR 0198).
4. CodeGraph's index lives in `nail-work-1/.codegraph` (the master has none): pass
   `projectPath: <work root>\nail-work-1` to `codegraph_explore`, or run `codegraph explore` there.
5. Every agent prompt names the folder, the branch and the work package; agents report the commands they ran and
   their results.
6. The owner reviews proposals and milestone results as HTML pages (artifacts); keep replies short.
7. The owner may stop a session after any step and resume with "Continue where we left off: follow 'If you are the
   lead' in CLAUDE.md and .claude/memory/next-steps.md". So after every finished step, and whenever background work
   starts, update `.claude/memory/next-steps.md` (state, running work with folder / branch, what waits for the owner,
   next steps), commit it on `work-1` and fast-forward `main` before reporting.

## Git

- `main` lives in the master and moves only by fast-forward to `work-1` after a green lead stage. Work folders commit on
  their own `work-<N>` branch. Commit only when asked, or as part of an agreed workflow stage.
- GitHub (`<public remote>`, remote `github` in `nail-work-1`) gets `main` after each fast-forward. Every
  commit uses the public identity of the folder's local config (GitHub noreply, ADR 0198), never a real name or
  address; the commit-msg and pre-push hooks check it.
- License: AGPL-3.0-or-later with a contributor license agreement (dual licensing, `CONTRIBUTING.md`); the plug-in kit
  and the example plug-ins MIT (ADR 0198). Never add a GPL / LGPL / AGPL dependency: it would block the other license.
- End commit messages with the Co-Authored-By / Claude-Session lines required by the session.

## Memory

Project memory lives in the repo at `.claude/memory/` (one fact per file, index in `.claude/memory/MEMORY.md`). Don't store project memory anywhere else. Update or delete entries that turn out to be wrong.
