# 0199 — The maintainer's setup out of the repository

Status: accepted (2026-10-17)

## Context

Requirement: before the repository goes public, it must be free of the
maintainer's personal setup: anyone who clones it should be able to work with their own
folders and remotes. The work-folder flow (ADR 0132) was written around the
maintainer's machine. `CLAUDE.md`, `docs/`, the ADRs, the memory and the
`nail-wave` workflow named the master's and the work folders' paths, the
GitHub remote and the installer folder. And `scripts/lib/work-folder.mjs`
detected the master by a path segment "OneDrive": anyone who cloned Nail
into their own OneDrive could not run an npm script or commit.

## Decision

- **The master is configured, not detected.** A checkout is the master only
  when `NAIL_CHECKOUT_ROLE=master` is set in the environment or in its
  gitignored `.env.local` (`KEY=value` lines, read with Node's `parseEnv`;
  the environment wins). `node scripts/work-folders.mjs guard-master` (run
  from a work folder) writes it into `origin`'s `.env.local`, together with
  the refusing rustc wrapper. Without that setting, a clone is a work folder
  and nothing is refused. `assertWorkFolder`, the post-edit hook and
  `work-folders.mjs create` use the role; `isInOneDrive` is gone.
- **Names are settings.** `NAIL_WORK_FOLDER_PREFIX` (default `nail-work-`) and
  `NAIL_WORK_BRANCH_PREFIX` (default `work-`) name the work folders and their
  branches; `create` reads them from the master's settings.
- **The `nail-wave` workflow takes its places as arguments:** `master`,
  `masterSh`, `workRoot`, `workRootSh`, `trailer` (required), and
  `folderPrefix`, `branchPrefix`. The Workflow runtime has no Node APIs to
  read `.env.local`.
- **The maintainer's own facts live in an untracked `CLAUDE.local.md`**
  (gitignored), which Claude Code loads next to `CLAUDE.md`: where the master
  and the work folders are, the public remote and how `main` is published,
  the installer folder, the CodeGraph index. `CLAUDE.md` and `docs/` describe
  the optional flow with placeholders (`<master>`, `<work root>`).
- **Existing text was neutralized:** personal paths in docs, ADRs and memory
  became `<master>`, `<work root>`, `<installer folder>` and `<projects>`;
  the memory stays in git (the maintainer's decision), scrubbed. Technical notes about
  checkouts inside cloud-synced folders such as OneDrive stay: they help
  anyone.
- **History.** Before the first push, the same placeholders are applied to
  the whole history (as in ADR 0198), and the reachable history is scanned.

## Consequences

- A plain clone builds and commits with no setup; the master flow is opt-in.
- The maintainer's master needs its `.env.local` (`guard-master`) before its
  next `main` arrives, or the master would stop refusing builds.
- Wave arguments must name the folders; old `args/*.json` files lack them
  and get them from `CLAUDE.local.md` when a wave is rerun.
