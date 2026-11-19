---
name: owner-installer-per-wave
description: "After every merged wave, build the per-user NSIS installer in nail-work-1 and copy it to the releases folder (NAIL_RELEASES_DIR, ADR 0218) for the owner's manual tests"
metadata:
  type: feedback
---

After every wave that is green and merged, build the Windows installer in `nail-work-1`
(`node scripts/build-installer.mjs --bundles nsis`, `CARGO_BUILD_JOBS=4`), run
the smoke test on it (`scripts/installer-payload-smoke.ps1`, ADR 0186 — the full `installer-smoke.ps1`
refuses beside the owner's installation; never uninstall or touch the owner's install or its HKCU keys), and copy the setup to `<releases folder>\<version>_<date>_<commit>\` (`NAIL_RELEASES_DIR`, set in the master's
`.env.local`; ADR 0218). Releases need no copy by hand: `release:prepare` copies them to `<releases folder>\X.Y.Z\`. Tell the owner the path in the wave report. Unsigned
until the EV certificate exists: SmartScreen warns ("More info" → "Run anyway").

**Why:** the owner tests the real app only through a proper installer; building in the master does not work
by design (ADR 0132).
**How to apply:** part of each wave's wrap-up, before the build-output cleanup ([[next-steps]]). When work
continues in `nail-work-1`, build from a clean checkout of the commit in another work folder (the build compiles
whatever is on disk). Keep the last few installers, delete older ones.

Related: [[owner-phone-reporting]], [[work-folders]]
