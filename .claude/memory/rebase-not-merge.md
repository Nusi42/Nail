---
name: rebase-not-merge
description: "Rule: combine branches by rebasing, never with merge commits; main requires linear history and the pre-push hook refuses merge commits (ADR 0208)"
metadata:
  type: feedback
---

Rule: branches are combined by rebasing; `main` requires linear history (ADR 0208). Package branches are rebased
onto the lead's branch and fast-forwarded; never a merge commit, on any branch that reaches `main`.

**Why:** a linear history is easier to read, bisect and review.

**How to apply:** `git switch -C integrate origin/<branch> && git rebase <lead branch> && git switch <lead branch> &&
git merge --ff-only integrate`, resolving conflicts per commit. The `linear` step of the pre-push hook
(`scripts/check-linear.mjs`) refuses merge commits on pushes to and from the master. Old merge commits on `main` stay.

Related: [[work-folders]], [[commit-messages]]
