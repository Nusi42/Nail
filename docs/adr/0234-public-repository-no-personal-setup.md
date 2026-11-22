# 0234 — Public repository: no personal development setup in tracked files

Status: accepted

## Context

Nail is developed in a public repository. Over time the tracked files had
collected details of one developer's own setup: where checkouts and keys
live, the names of local folders, branches and WSL distributions, other
projects on the same machine, how status reports reach the maintainer, and
when or why such personal choices were made. None of this helps anyone who
clones the project, and it is private. ADR 0192 already keeps account data
out of git; this ADR covers the development setup.

## Decision

1. **Rule.** Everything tracked (code, docs, ADRs, guides, status notes,
   memory notes, workflow files, commit messages) describes only what anyone
   who clones the project needs to build, test, use or release it. How or
   where a developer works is never committed: their folders and sync
   services (or paths into them), where keys or secrets are kept, removable
   drives, their computers, WSL distributions or other projects on them,
   their tools' local setup, and the dates or reasons of such personal
   choices. A personal decision that changes the project is recorded as a
   general project rule without personal detail.
2. **Untracked local files.** Personal setup goes into an untracked
   `CLAUDE.local.md` (paths, remotes) and the gitignored `.claude/local/`
   folder. The lead's running status (`.claude/local/next-steps.md`) lives
   there and is never committed; project facts others need (feature status,
   open flaky tests, follow-ups) go into the `docs/plan.md` status lines or
   tracked memory notes, without personal detail.
3. **Private patterns without a default location.** The private-data guard
   (ADR 0192) reads its patterns only from the file `NAIL_PRIVATE_PATTERNS`
   names (the environment, the checkout's gitignored `.env.local` or its
   master's). Unset, it passes and prints a one-line notice. The code and
   the docs name no location.
4. **Agents and reviews.** `CLAUDE.md` states the rule; the `nail-wave`
   workflow's shared preamble tells every builder and reviewer, and reviewers
   report violations as must-fix.
5. **Fix forward.** The existing mentions were rewritten or removed in a
   regular commit; the history is not rewritten.

## Consequences

- A new clone works without anyone's personal setup; whoever develops keeps
  their own in the untracked files.
- The lead's status no longer appears in commits; resuming a session reads
  the local file when it exists and the `docs/plan.md` status lines.
- Older commits still contain the removed mentions.
