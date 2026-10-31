# 0205 — Only the master publishes

Status: accepted (2026-10-31); revises ADRs 0132 and 0199 (where `main` is
published from) and ADR 0203 (the pushes of `release:publish`)

## Context

With a master checkout (ADRs 0132, 0199), `main` was published from the
lead's work folder: `git push <public remote> <lead branch>:main`, after its
pre-push hook ran `npm run check`, while the master refused every push of
its own. So the public remote was configured in a work folder, any work
folder could push any branch to a network remote, and what reached the
public `main` was decided in a checkout that is reset and rebuilt all the
time. The pre-push hook ran the same checks for every remote: there was no
place where "where may this checkout push" was decided.

## Decision

- **The master is the only checkout that pushes to the public remote:**
  `git -C <master> push github main`. It pushes `main` to `main` and release
  tags (`refs/tags/v<version>`, ADR 0203), nothing else, and never to a
  local path.
- **Work folders push only to the master** (a local path). A push from a
  work folder to a network remote (`https://…`, `ssh://…`, `git@host:…`) is
  refused, with a message that names the master's command.
- **One function decides:** `pushPlan` in `scripts/lib/push-plan.mjs` takes
  the checkout's role (master; work folder = its `origin` is a local master
  checkout; plain clone) and the remote's URL (local when it is a path:
  `C:\…`, `/…`, `../x`, `\\host\share`, `file://…`; network otherwise) and
  answers the steps to run or a refusal. `.githooks/pre-push` only runs
  `scripts/pre-push.mjs`, which runs the plan; it runs on load (not behind
  an "is this the main module" test), so a hook can never silently pass.
- **What each push runs:**
  - work folder → master: the private-data guard on the pushed commits,
    then `npm run check`, as before;
  - master → public remote: only checks that build nothing: the guard on
    the pushed commits (messages, identities, added lines) and on every
    tracked file, and the dependency licences (`cargo deny check
    licenses`). `main` only moves to a commit that passed the full check in
    a work folder, so the full check is not repeated;
  - plain clone (no master, e.g. a contributor's fork): the guard and
    `npm run check` for every remote, as before;
  - every other combination is refused.
- **The master's guards let the master's checks run.** Its rustc wrapper
  answers cargo's two questions that compile nothing (`rustc -vV` and the
  target information, needed by `cargo metadata` and so by `cargo deny`)
  and still refuses every compilation. `work-folders.mjs guard-master` also
  sets the master's `core.hooksPath`: without it git would not run the
  pre-push hook there at all.
- **The hooks find their scripts next to themselves**
  (`"$(dirname "$0")/../scripts/…"`) and check the checkout git runs them
  in, so the tests run the real hooks through git on throw-away
  repositories.
- **Releases follow:** `release:publish` pushes the branch and the tag to
  origin; with a master it fast-forwards the master and has the master push
  `main` and the tag (`publishPushes`); a plain clone pushes them itself.
- The public remote moves from the lead's work folder to the master.

## Alternatives

- **Publish from the lead's work folder** (the previous flow): the full
  check runs right before the push, but the remote and the credential live
  in a folder that is reset and rebuilt, any work folder can push anywhere,
  and the decision what may go public is spread over the folders. Rejected.
- **Repeat `npm run check` in the master:** the master never builds
  (ADR 0199), and `main` only moves to commits that passed it. Rejected.
- **Refuse every network push outside the master,** plain clones included:
  would stop contributors from pushing to their forks. Rejected; without a
  master the old behaviour stays.

## Consequences

- Publishing is two steps from the lead: fast-forward the master, then
  `git -C <master> push github main`. The maintainer's paths and the remote
  stay in the untracked `CLAUDE.local.md`.
- The master needs `cargo-deny` on PATH and the crate sources in Cargo's
  cache; its pre-push hook fails, and says how to install it, otherwise.
- An existing master gets the new wrapper and its `core.hooksPath` by
  running `node scripts/work-folders.mjs guard-master` again from a work
  folder.
