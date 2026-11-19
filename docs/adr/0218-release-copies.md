# 0218 — Copies of built releases in a releases folder

Status: accepted (2026-11-19)

## Context

`release:prepare` builds into the lead work folder's `.nail-build/release/X.Y.Z/`,
which `work-folders.mjs clean` and new builds remove. The maintainer kept the
installers to try by copying them by hand to a folder outside the repository.
That step was only written in the maintainer's notes, and the folder was hard
to find again.

## Decision

- A new setting, `NAIL_RELEASES_DIR`, names a folder where `release:prepare`
  copies each committed release (the whole `X.Y.Z` folder: installer, update
  signature, portable zip, `latest.json`, checksums, SBOMs, notes) to
  `<NAIL_RELEASES_DIR>/X.Y.Z/`, replacing an older copy of that version. A dry
  run copies nothing.
- It is read like the work-folder settings: the environment, then the
  checkout's `.env.local`, then (in a work folder) its master's `.env.local`;
  a relative path is relative to the checkout whose file names it. Unset
  means no copy, so other clones behave as before.
- `/releases/` is gitignored, so `NAIL_RELEASES_DIR=releases` in the master's
  `.env.local` keeps the copies next to the master checkout. Copying files
  there is not a build: the master still never builds, tests or commits.

## Consequences

- The installers to try outlive the build output of the work folders.
- A master inside a synced folder (OneDrive) syncs the copies too, about
  175 MB per release; old versions are deleted by hand.
