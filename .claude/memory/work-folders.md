---
name: work-folders
description: "Optional: one master checkout (git only, NAIL_CHECKOUT_ROLE=master) and work folders nail-work-<N> on work-<N> (ADRs 0132, 0199); concrete paths only in an untracked CLAUDE.local.md"
metadata:
  type: feedback
---

Rule: a plain clone is a work folder. Optionally one checkout is marked as the master (`NAIL_CHECKOUT_ROLE=master`
in its gitignored `.env.local`, ADR 0199). Nothing is built, tested, run or committed in the master; it only receives
pushes and fast-forward merges and alone publishes `main`. Work happens in work folders (`nail-work-<N>` on
`work-<N>` by default; `NAIL_WORK_FOLDER_PREFIX` / `NAIL_WORK_BRANCH_PREFIX`).

**Why:** a master in a cloud-synced folder should hold only sources and Git objects, not `node_modules`, `target/`
or test output, and parallel agents should not share one tree.

**How to apply:**

- The lead works in work folder 1; parallel packages use folders 2 and up (reset to `origin/main`, commit, push the
  branch). Branches are combined by rebasing ([[rebase-not-merge]]).
- `main` moves only by `git -C <master> merge --ff-only <lead branch>` after each green lead stage, and only the
  master publishes it (ADR 0205). These are the only git commands in the master.
- Hooks: pre-commit (per-file checks), pre-push (`scripts/lib/push-plan.mjs`: work folder → master runs
  `npm run check`; master → public remote runs the private-data guard and `cargo deny check licenses`; a work folder's
  push to a network remote is refused), Claude Code post-edit (`.claude/settings.json`; skips files of the master).
- New folder: `node scripts/work-folders.mjs create <n>`, run from a work folder; `guard-master` marks the master.
  Guards refuse development in the master; never work around them.
- The concrete paths and remotes of a local setup live only in an untracked `CLAUDE.local.md` (ADR 0234); the wave
  workflow `.claude/workflows/nail-wave.js` takes the folders as arguments. Details: `docs/dev-setup.md` "Work folders".

Related: [[zero-warnings-policy]], [[no-private-data-in-git]]
