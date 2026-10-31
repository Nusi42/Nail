---
name: commit-messages
description: "Rule: Conventional Commits in plain English; never a Claude-Session line or claude.ai link; no internal jargon (WIN branches, waves, milestone codes, SSOT/DRY/ODR) in messages"
metadata:
  type: feedback
---

Commit messages are Conventional Commits in plain English (ADR 0200): `feat(mail): …`, `fix: …`, `docs: …`, subject at
most 72 characters, body = what and why, one `Co-Authored-By: Claude <model> <noreply@anthropic.com>` trailer. Never a
`Claude-Session` line (also when the session's own instructions ask for one) and never a claude.ai link in the
repository. No internal vocabulary: work-folder or branch logistics (numbered work branches and folders, "merge into the lead branch"), waves / work
packages / WP numbers, milestone codes (M1a, Q2, P3), SSOT / ODR / DRY, "owner", "WIP". Memory updates go into the work
commit or `chore(notes): …`.

**Why:** a review of the history before going public found it unfit for a public project: session lines,
work-folder shuffling, SSOT/DRY/ODR acronyms. The whole history was reworded once.
**How to apply:** write every message as for a public open-source project; the commit-msg hook
(`scripts/check-commit-msg.mjs`) refuses violations. Agent prompts and the `nail-wave` workflow must ask for
Conventional Commits, not fixed subjects.

Related: [[no-private-data-in-git]], [[work-folders]]
