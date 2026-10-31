---
name: rebase-not-merge
description: "Maintainer decision (2026-10-31): combine branches by rebasing, never with merge commits; the pre-push hook refuses them (ADR 0208)"
metadata:
  type: feedback
---

Rule: package branches are rebased onto the lead's branch and fast-forwarded; never a merge commit, on any branch
that reaches `main`.

**Why:** the maintainer said "rebase is much more correct". The public `main` has a ruleset that requires linear
history; a merge commit only passes on the maintainer's bypass with a violation notice (first seen 2026-10-31).

**How to apply:** `git switch -C integrate origin/<branch> && git rebase <lead branch> && git switch <lead branch> &&
git merge --ff-only integrate`, resolving conflicts per commit. The `linear` step of the pre-push hook
(`scripts/check-linear.mjs`) refuses merge commits on pushes to and from the master. Old merge commits on `main` stay.

Related: [[work-folders]], [[commit-messages]], [[next-steps]]
