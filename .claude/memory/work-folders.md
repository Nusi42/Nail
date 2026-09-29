---
name: work-folders
description: "The owner keeps a master checkout (git only, NAIL_CHECKOUT_ROLE=master) and develops in work folders nail-work-<N> on work-<N> (owner rule, ADRs 0132, 0199); concrete paths only in CLAUDE.local.md"
metadata:
  node_type: memory
  type: feedback
  modified: 2026-10-17T10:26:45.124Z
---

Rule: the master checkout `<master>` is the master directory; work folders live in `<work root>`.
The master must not be used for debugging or development, at most for release builds. We chose not at
all: releases are also built in a work folder on a `main` commit. Since ADR 0199 the master is marked by
`NAIL_CHECKOUT_ROLE=master` in its gitignored `.env.local`, not detected by its path, and the concrete paths, the
public remote and the installer folder live only in the untracked `CLAUDE.local.md` (requirement: everyone who clones the
repository must be able to set it up on their own).

**Why:** a synced master should hold only sources and Git objects, not `node_modules`, `target/` or test output.
Parallel agents should not share one tree. Personal paths do not belong in a public repository.

**How to apply:**

- The maintainer's session runs in the master folder (requirement: sessions start there, never in a
  different folder). It is only the launch point: drive `nail-work-1` (branch `work-1`, the lead) and the others by absolute path.
  Parallel packages use `nail-work-2` and up (`work-<N>`, reset to `origin/main`, commit, push). Integrate by merging into
  `work-1`.
- `main` moves only by `git -C <master> merge --ff-only work-1` after each green lead stage. It is the only write in the
  master.
- Hooks: pre-commit (per-file checks), pre-push (`npm run check`), Claude Code post-edit (`.claude/settings.json`,
  per-file checks on each edit; finds the work folder from the edited file, skips files of the master).
- New folder: `node scripts/work-folders.mjs create <n>`, run from a work folder; `guard-master` marks the master.
- Guards refuse development in the master: npm entry points, `prepare`, the pre-commit hook, and the master's rustc
  wrapper. Never work around them.
- Build output goes to each folder's `target/` and `.nail-build/`. The dev update key stays in
  `%LOCALAPPDATA%\nail-dev\keys`.
- The wave workflow `.claude/workflows/nail-wave.js` takes the folders as arguments (from `CLAUDE.local.md`).

Related: [[next-steps]], [[zero-warnings-policy]], [[no-private-data-in-git]]
