# Release checklist — Nail 1.0

What has to happen between the release candidate `1.0.0-rc.1` (plan M4,
WP125) and the 1.0 release. The automated gate (every suite green, every
wave's Playwright scenarios, the installers, the performance budgets) is
the integrators' job and recorded in `docs/plan.md` section 9; the gates
below need people, accounts or keys, and are the maintainer's decisions.

Procedures for keys and signing: [`docs/release.md`](release.md). Real-account
items: [`docs/backlog.md`](backlog.md).

## Human gates

| # | Gate | Who | Done when | Notes |
| --- | --- | --- | --- | --- |
| 1 | **Security review sign-off** of the whole product | an independent reviewer | written sign-off, findings fixed or accepted | Scope: the core's request routing and remote access (`check_remote_request`, grants), the remote-UI login (passwords, TOTP, passkeys, pairing, sessions), the remote-content proxy (SSRF), plug-in sandbox and permissions, MCP writes and confirmations, crypto (OpenPGP, S/MIME, smartcards), encryption at rest (full-file database encryption, recovery key, `vault.json`), backups (Argon2id, STREAM), the diagnostics redaction, the installers (MSI policies, MAPI registration), the updater (signed manifests). ADRs 0029–0099 describe the decisions. |
| 2 | **EV code-signing certificate and a signed build** | owner | `scripts/verify-signatures.ps1` passes on the release artefacts | Buy the EV certificate (hardware token or cloud HSM); configure `NAIL_SIGN_THUMBPRINT` (or the PFX secrets for a standard certificate); run `release.yml`. Unsigned builds work but SmartScreen warns. |
| 3 | **Real-account tests** from `docs/backlog.md`, as far as wanted before 1.0 | owner + tester | each chosen item run once by hand and noted here | Google account (Gmail API, Google CalDAV / CardDAV), Outlook.com and a Microsoft 365 tenant (Graph: shared mailboxes, rooms, rules, out of office, large attachments over 4 MB, send as / on behalf), optionally grommunio (EWS) in the interop job and an Exchange Server lab (Windows sign-in). |
| 4 | **Smartcard test on real hardware** | tester | signing and decrypting with a real card (S/MIME and OpenPGP) noted here | The PKCS#11 code path is covered only by a soft token in the automated tests (ADR 0095): try a PIV card / YubiKey (OpenSC module) and an OpenPGP card. |
| 5 | **First green GitHub CI run** | owner | the `ci.yml` jobs (build-test, desktop, mapi, installer incl. the elevated MSI smoke test, installer-arm64, deny, interop, perf) green on GitHub's runners | Never run on GitHub yet: every job ran locally only. Fix runner differences (paths, timeouts, the perf tolerance) in CI. |
| 6 | **Release update key** | owner, on a trusted offline machine | `NAIL_UPDATER_PRIVATE_KEY` / `_PASSWORD` secrets and the `NAIL_UPDATER_PUBKEY` variable set; two offline backups of the key | `docs/release.md` "Update key handling". Losing the key means installed copies can never be updated again. |
| 7 | **External penetration test** | external tester | report, findings fixed | **Only if internet mode is to be unlocked** (`nail_api::remote::internet_mode_lock()`); not needed for 1.0 with internet mode locked. |
| 8 | **Encryption at rest default** | owner | decision recorded in an ADR | Ships **off** by default (plan section 7); the UI recommends turning it on. Decide whether 1.0 turns it on for new installations. |

## Release steps (after the gates)

1. Set the version (`1.0.0`) in the Cargo workspace, `app/src-tauri/tauri.conf.json`
   and the npm packages (`ui`, `e2e`, `packages/plugin-api`,
   `packages/create-nail-plugin`); `npm install --package-lock-only`.
2. Move the `[Unreleased]` / candidate entries of `CHANGELOG.md` under the
   version and date; write `packaging/release-notes/1.0.0.md` and the user
   page `userDocs/en/release-notes.md`.
3. Full local run (CLAUDE.md "Commands"), then tag `v1.0.0`; `release.yml`
   builds, signs and drafts the GitHub release with the update manifests.
4. Publish the manifests at the update base URL (`stable/latest.json`,
   `beta/latest.json`), the installers at the release URLs; submit the
   winget manifests (`node scripts/winget-manifest.mjs`).
5. Smoke test the published installers (`scripts/installer-smoke.ps1`,
   update from the release candidate).

## Status of the candidate

| Item | State |
| --- | --- |
| Automated gate (all suites, Playwright, installers locally, performance budgets) | see `docs/plan.md` section 9, "M4 automated gate" |
| Gates 1–8 | open |
