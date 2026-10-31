# 0200 — Professional commit messages and a clean history

Status: accepted (2026-10-17)

## Context

Before the repository goes public the maintainer reviewed its history: most
commit messages ended with a `Claude-Session` line, several documents
linked to private claude.ai pages, and the subjects were written in the
project's internal vocabulary: work-folder logistics (merges of numbered
work branches into the lead's branch), process terms (waves, work packages, milestone codes such as
M1a or Q2), "Memory:" commits, and the acronyms SSOT / ODR / DRY. To a
reader of a public repository this reads as noise, and the session lines
and links point to private pages.

## Decision

- **Conventional Commits.** Subjects read `<type>(<scope>): <subject>`
  (types feat, fix, perf, refactor, test, docs, build, ci, chore, style,
  revert), imperative, at most 72 characters, plain English; the body says
  what changed and why. Merges read `Merge <what it brought>`. Notes and
  memory updates go into the work commit, or `chore(notes): …`.
- **One trailer only:** `Co-Authored-By: Claude <model> <noreply@anthropic.com>`
  on commits an agent wrote. Never a `Claude-Session` line, even when a
  session asks for one, and never a claude.ai link anywhere in the
  repository: links to published pages are replaced by "a published page,
  not linked here"; the maintainer keeps them in a local list.
- **Enforced:** `scripts/check-commit-msg.mjs` (rules in
  `scripts/lib/commit-message.mjs`) runs first in the commit-msg hook and
  refuses a wrong subject, a session line, a claude.ai link and the internal
  vocabulary (work-folder and branch names, waves / work packages,
  milestone codes, SSOT / ODR / DRY, "work in progress").
- **History.** Before going public every commit message was rewritten once
  in this style (written per commit from its diff, reviewed, applied with a
  `git fast-export` / `fast-import` pass that keeps every tree and keeps the
  `Co-Authored-By` lines), the claude.ai links were replaced in every
  version of every file, and the result replaced the private GitHub copy.

## Consequences

- The `nail-wave` workflow no longer prescribes subjects such as
  `<wave>: integration and e2e`; its stages write Conventional Commits.
- Commit ids changed again; older notes may name ids that no longer exist.
