---
name: public-repo-no-personal-setup
description: "Rule: the repository is public; tracked files describe only what anyone who clones Nail needs, never a developer's personal setup (ADR 0234)"
metadata:
  type: feedback
---

Rule: everything committed (code, docs, ADRs, guides, status notes, memory notes, workflow files, commit messages)
describes only what anyone who clones the project needs to build, test, use or release it. How or where a developer
works is never committed: their folders and sync services (or paths into them), where keys or secrets are kept,
removable drives, their computers, WSL distributions or other projects on them, their tools' local setup, and the
dates or reasons of such personal choices. When a personal decision changes the project, record it as a general
project rule without personal detail.

**Why:** the repository is public; personal setup is private and of no use to anyone who clones it.
**How to apply:** write generic rules only. Personal setup goes into untracked local files: `CLAUDE.local.md`
(paths, remotes) and the gitignored `.claude/local/` (the lead's running status `next-steps.md`, personal notes).
Tell every agent and builder the rule (the `nail-wave` preamble does); reviews report violations as must-fix.

Related: [[no-private-data-in-git]], [[commit-messages]], [[work-folders]]
