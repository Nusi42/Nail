# 0203 — Releases on GitHub, prepared and published by script

Status: accepted (2026-10-25)

## Context

ADR 0089 built the pieces of a Windows release (installers, update
signatures, per-channel update manifests, winget manifests) and ADR 0201
moved every build to the maintainer's machine, but the release itself was a
list of hand steps in `docs/release.md`: the version typed into six files,
release notes in three places (`CHANGELOG.md`, `userDocs/en/release-notes.md`
and `packaging/release-notes/<version>.md`), the GitHub release created in
the web UI, and no decision where installed copies find their updates. The
repository is public on GitHub.

## Decision

- **Two commands.** `npm run release:prepare -- --version X.Y.Z` (on a
  clean checkout of the lead branch) sets the version, moves the changelog's
  `[Unreleased]` entries under it, runs the local verification, builds
  the installers, writes the update manifest and the release text, commits
  `chore(release): release X.Y.Z` and leaves a release record in the build
  folder. After the installer has been tried, `npm run release:publish` tags
  `vX.Y.Z`, pushes the branch and the tag (origin, then the public remote's
  `main`), creates the GitHub release with the GitHub CLI and publishes the
  update manifest. Each publish step checks first, so a re-run continues
  where a failure stopped. `--dry-run` builds without committing. Both are
  Node scripts with tests, like the rest of the tooling.
- **One version.** `[workspace.package] version` in `Cargo.toml` is the
  source; `tauri.conf.json`, the npm workspaces' `package.json` files and
  their `package-lock.json` entries are copies that `release:prepare` sets
  and a script test pins. The version does not move to a `-dev` suffix
  after a release: an MSI needs a plain number, and a pre-release version
  would put development builds on the beta channel.
- **One public repository.** `[workspace.package] repository` names it; the
  release URLs derive from it.
- **GitHub Releases only.** Each version is a release `vX.Y.Z` with its
  files. In-app updates read `<base>/<channel>/latest.json`; with the base
  `<repository>/releases/download`, the manifest of each channel is the
  asset `latest.json` of a rolling release tagged with the channel's name
  (`stable`, `beta`), marked as a pre-release so a numbered release stays
  "Latest". The app needs no change, and no second host. The manifest's
  signatures, not the host, decide what the app installs.
- **The first releases**: x64 only, the NSIS installer and the portable zip
  (the MSI and ARM64 builds stay available in `build-installer.mjs`), not
  code-signed (SmartScreen warns; the release notes say how to go on).
  Signing is added by configuring a certificate; the scripts do not change.
- **One source for the user-facing notes.** The version's section of
  `userDocs/en/release-notes.md` is the GitHub release's text (its links
  pointing into the tagged sources); its introduction, as plain text with a
  link to the release, is the update dialog's text. The hand-written
  `packaging/release-notes/` goes. `CHANGELOG.md` stays the record for
  developers.
- **The update key** is set for the one `release:prepare` command; only
  the installer build sees it, and only the bundler inside that
  (`docs/release.md` "Who sees the private key"). Its public half is
  committed as `packaging/updater/release.pub`; where the private half is
  kept is the maintainer's business, not the repository's.

## Consequences

- A release is two commands and a manual test of the installer in between.
- The version can no longer drift between the Rust, Tauri and npm copies.
- An installed copy updates from GitHub; anyone who can replace release
  assets still cannot ship an update without the update key.
- The rolling `stable` and `beta` tags exist in the public repository.
- winget manifests are not part of the scripts yet (`winget-manifest.mjs`
  stays a separate step).
