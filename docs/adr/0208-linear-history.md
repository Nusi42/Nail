# 0208 — Branches are rebased, never merged

Status: accepted (2026-10-31)

## Context

The lead combined the package branches with merge commits ("Merge the fix
for links in HTML mail"). The public repository's ruleset on `main`
requires linear history; a merge commit gets through only on the
maintainer's bypass, with a rule-violation notice on the push. Merge commits
also make the public history harder to read: each package appears twice
(its commits and the merge), and `git bisect` walks into side branches.

## Decision

- **Package branches are rebased onto the lead's branch** and then
  fast-forwarded into it (`git rebase <lead branch>` on a copy of the
  package branch, then `git merge --ff-only`). Conflicts are resolved per
  commit during the rebase. The lead's branch, the master's `main` and the
  public `main` stay a straight line.
- **The pre-push hook refuses merge commits** in what a push adds: on a
  work folder's push to the master and on the master's push to the public
  remote (`linear` step of `pushPlan`, `scripts/check-linear.mjs`; same
  commit selection as the private-data guard: commits on no
  remote-tracking branch and not under the remote's old value). A plain
  clone (a contributor's fork) is not checked: GitHub squash-merges its
  pull requests anyway.
- Merge commits already on `main` stay; history is not rewritten.

## Alternatives

- **Merge commits** (the old flow): keep each package together, but break
  the ruleset and clutter the history. Rejected.
- **Squash each package into one commit:** linear, but loses the package's
  separate commits and their messages. Rejected for integration; GitHub
  pull requests still squash.

## Consequences

- The integrator rebases instead of merging; the `nail-wave` workflow says
  so.
- A rebased package has new commit ids; its branch in the work folder is
  reset to `origin/main` for the next package anyway.
