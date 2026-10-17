# 0201 — No hosted CI

Status: accepted (2026-10-17)

## Context

The repository had two GitHub Actions workflows: `ci.yml` (quality, build
and tests, desktop, MAPI, installers, ARM64, cargo-deny, interop, perf,
visual) and `release.yml` (signed release builds, manifests, a draft
release). On a private repository Actions cost minutes, and the maintainer
does not want hosted CI. The local hooks and runs already gate every merge:
the pre-commit hook checks staged files, the pre-push hook runs
`npm run check`, and the integrators run the full suite before `main` moves.

## Decision

- **No hosted CI.** The workflows (`.github/workflows/ci.yml`,
  `release.yml`), the `actionlint` check and the tests of the workflows
  (`scripts/test/ci-workflow.test.mjs`) are removed; Actions are disabled
  for the repository.
- **Verification** is the pre-push hook (`npm run check`) plus the full
  local run listed in `docs/dev-setup.md` "Verification (no hosted CI)":
  `cargo test --workspace`, `npm test`, the script tests, `npm run e2e`,
  `npm run visual`, the desktop smoke tests (`NAIL_E2E_DESKTOP=1`), the
  MAPI tests, the installer build with `installer-smoke.ps1` and
  `msi-smoke.ps1`, the ARM64 installer build, `cargo deny check` and the
  perf run. It is green on a clean checkout before a merge into `main` and
  before a release.
- **Releases** are built, signed and published locally per
  `docs/release.md`: `scripts/build-installer.mjs` with the release update
  key given only to that command, `scripts/verify-signatures.ps1`,
  `scripts/update-manifest.mjs` and `scripts/winget-manifest.mjs`, then a
  GitHub release created by hand.
- This supersedes the CI parts of ADRs 0040, 0089, 0092, 0123 and 0130.

## Consequences

- `cfg(not(windows))` branches and the ARM64 builds are checked only when
  someone runs the checks on such a machine (or cross-builds ARM64 locally).
- The interop tests need Docker locally (`NAIL_INTEROP=1`); without it they
  return at once.
- Item 5 of the release checklist becomes the full local verification run.
- The release update key never leaves the maintainer's machines; nothing
  has to be stored as a repository secret.
