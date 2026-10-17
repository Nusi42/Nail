# 0198 — GitHub remote, public commit identity, license, version 0.8.0

Status: accepted (2026-10-17)

## Context

The owner wants `<public remote>` (private for now) as the
repository's remote. Until now `main` lived only in the master
(ADR 0132). Before the first push the whole history was checked: the files
and messages were clean (ADR 0192), but every commit carried the maintainer's
real name and address as author and committer, and a few documents named
another project of the maintainer. Requirement: neither the real addresses,
the real name nor other projects in the repository or its history. The
code had no license yet (`LicenseRef-Proprietary`, `UNLICENSED`).

## Decision

- **Remote.** `nail-work-1` has the remote `github`. After the lead's
  fast-forward of `main` (ADR 0132), `git push github work-1:main` publishes
  it; the pre-push hook runs the private-data guard and `npm run check`
  first. The master stays the local hub, and package branches are not
  pushed to GitHub.
- **Public identity.** Commits are made as
  `Nusi42 <167581605+Nusi42@users.noreply.github.com>` (GitHub's noreply
  address, linked to the account), set in the local config of the master
  and of every work folder; `scripts/work-folders.mjs create` copies it
  from the master (`identityConfig`).
- **History rewritten.** The author and committer of every
  commit became that identity, the real name in the design document's
  byline was removed, and the other project's name was replaced. File
  trees are unchanged otherwise; every commit id changed again (as after
  the ADR 0192 rewrite), so older notes may name ids that no longer exist.
- **The guard checks identities.** `scripts/check-private.mjs --message`
  (commit-msg hook) also checks the author and committer that git is about
  to record (`git var GIT_AUTHOR_IDENT` / `GIT_COMMITTER_IDENT`), and
  `--push` checks each pushed commit's author and committer
  (`PUSH_LOG_FORMAT`). The private patterns (outside the repository) allow
  the GitHub account's public forms only.
- **License: AGPL-3.0-or-later, dual licensing.** Nail is licensed under
  the GNU Affero General Public License v3.0 or later (`LICENSE`; Cargo
  `license = "AGPL-3.0-or-later"`, root `package.json`): anyone may use and
  change it, also in private closed builds; whoever distributes a changed
  Nail, or offers it to others over a network (`nail serve`, the remote UI),
  must make its source available under the same license. The maintainer
  also offers Nail under other terms (for example a closed commercial
  edition): contributions come with a contributor license agreement that
  allows relicensing (`CONTRIBUTING.md`), and no GPL-family dependency may
  be added (`deny.toml`). The plug-in kit (`packages/plugin-api`,
  `packages/create-nail-plugin`) and the example plug-ins are MIT-licensed,
  so plug-ins can use any license. Every dependency is permissive or
  MPL-2.0, which fits both.

- **Version 0.8.0.** Decision: the product is a 0.8, not a 1.0
  candidate: the version is `0.8.0` (Cargo workspace, `tauri.conf.json`, the
  npm packages, `packaging/release-notes/0.8.0.md`, the user release notes).
  The internal `1.0.0-rc.1` was never published; 1.0 follows the release
  checklist.

## Consequences

- The release checklist's "first GitHub CI run" can happen now: the push
  runs `.github/workflows/ci.yml` on GitHub's runners.
- Installations of the internal `1.0.0-rc.1` see 0.8.0 as older: the
  updater does not offer it, and the installer asks before installing an
  older version over it.
- A new machine needs the identity set before its first commit; the
  commit-msg hook refuses a commit whose identity matches a private
  pattern.
