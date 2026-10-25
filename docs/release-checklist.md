# Release checklist — Nail 1.0

What has to happen between the current version `0.8.0` (the plan M4 candidate,
once numbered `1.0.0-rc.1`; WP125) and the 1.0 release. The automated gate (every suite green, every
wave's Playwright scenarios, the installers, the performance budgets) is
the integrators' job and recorded in `docs/plan.md` section 9; the gates
below need people, accounts or keys, and are the maintainer's decisions.

Procedures for keys and signing: [`docs/release.md`](release.md). Real-account
items: [`docs/backlog.md`](backlog.md).

## Human gates

| # | Gate | Who | Done when | Notes |
| --- | --- | --- | --- | --- |
| 1 | **Security review sign-off** of the whole product | an independent reviewer | written sign-off, findings fixed or accepted | Scope: the core's request routing and remote access (`check_remote_request`, grants), the remote-UI login (passwords, TOTP, passkeys, pairing, sessions), the remote-content proxy (SSRF), plug-in sandbox and permissions, MCP writes and confirmations, crypto (OpenPGP, S/MIME, smartcards), encryption at rest (full-file database encryption, recovery key, `vault.json`), backups (Argon2id, STREAM), the diagnostics redaction, the installers (MSI policies, MAPI registration), the updater (signed manifests). ADRs 0029–0101 describe the decisions. |
| 2 | **Code signing and a signed build** | owner | `scripts/verify-signatures.ps1` passes on the release artefacts | A standard certificate, Azure Trusted Signing or SignPath's open-source plan (an EV certificate no longer skips SmartScreen's reputation check); configure `NAIL_SIGN_THUMBPRINT` (or import a `.pfx`); build locally (`docs/release.md` "Releasing"). The preview releases ship unsigned (ADR 0203): SmartScreen warns. |
| 3 | **Real-account tests** from `docs/backlog.md`, as far as wanted before 1.0 | owner + tester | each chosen item run once by hand and noted here | Google account (Gmail API, Google CalDAV / CardDAV), Outlook.com and a Microsoft 365 tenant (Graph: shared mailboxes, rooms, rules, out of office, large attachments over 4 MB, send as / on behalf), optionally grommunio (EWS) in the interop run and an Exchange Server lab (Windows sign-in). |
| 4 | **Smartcard test on real hardware** | tester | signing and decrypting with a real card (S/MIME and OpenPGP) noted here | The PKCS#11 code path is covered only by a soft token in the automated tests (ADR 0095): try a PIV card / YubiKey (OpenSC module) and an OpenPGP card. |
| 5 | **Full local verification run** | owner | on a clean checkout of the release commit, all green: `npm run check`, `cargo test --workspace`, `npm test`, `npm run e2e`, `npm run visual`, the desktop smoke tests (`NAIL_E2E_DESKTOP=1`), the MAPI tests, the installer build with `installer-smoke.ps1` and the elevated `msi-smoke.ps1`, the ARM64 installer build, `cargo deny check`, the perf run, and the interop run where Docker is available | There is no hosted CI (ADR 0201); the list is `docs/dev-setup.md` "Verification (no hosted CI)". |
| 6 | **Release update key** | owner, on a trusted offline machine | the key pair generated, the private key and password kept outside every repository, the public half ready for `NAIL_UPDATER_PUBKEY`; two offline backups of the key | `docs/release.md` "Update key handling". Losing the key means installed copies can never be updated again. |
| 7 | **External penetration test** | external tester | report, findings fixed | **Only if internet mode is to be unlocked** (`nail_api::remote::internet_mode_lock()`); not needed for 1.0 with internet mode locked. |
| 8 | **Encryption at rest default** | owner | decision recorded in an ADR | Ships **off** by default (plan section 7); the UI recommends turning it on. Decide whether 1.0 turns it on for new installations. |

## Release steps (after the gates)

1. Write the `1.0.0` section of the user page `userDocs/en/release-notes.md`
   and keep the `[Unreleased]` entries of `CHANGELOG.md` current.
2. `npm run release:prepare -- --version 1.0.0` with the release update key
   (gate 6): version, changelog, most of the local verification (gate 5;
   the installer smoke tests, ARM64 and perf by hand), installers, update manifest, release commit (`docs/release.md`
   "Releasing"). Sign first if gate 2 is done.
3. Try the installer, then `npm run release:publish`: tag, pushes, the
   GitHub release, the `stable` manifest. Submit the winget manifests.
4. Smoke test the published installers (`scripts/installer-smoke.ps1`,
   update from the previous release).

## Status of the candidate

| Item | State |
| --- | --- |
| Automated gate (all suites, Playwright, installers locally, performance budgets) | met after the post-M4c fixes (ADR 0101: initial sync 93–108 s of 300, incremental 734–757 ms of 3,000); see `docs/plan.md` section 9, "M4 automated gate" |
| Gates 1–8 | open |
