# Releasing, signing and the update key

Internal procedures for Windows releases (plan M4b WP112; ADRs 0089, 0093).
The tools and file formats are described in `packaging/README.md` and
`scripts/README.md`; this page is about the keys and the steps.

## Update key handling

The update key is a minisign key pair (Tauri's `tauri signer`), separate
from the Authenticode certificate.

- **Never commit a private key.** Only public halves are in the repository.
- **Development key**: `node scripts/updater-key.mjs --generate` writes the
  private key to `%LOCALAPPDATA%\nail-dev\keys\nail-updater-dev.key`
  (empty password) and its public half to `packaging/updater/dev.pub`;
  `node scripts/updater-key.mjs` checks the pair. Builds with
  `--dev-updater-key` embed `dev.pub` and sign with the local key. Such
  builds are for testing only: anyone with the dev key could sign updates
  for them.
- **CI smoke tests** generate a throwaway key per run (ci.yml), so no secret
  is needed for pull requests.
- **Release key**: generate it once on a trusted, offline machine
  (`npx tauri signer generate -w nail-updater.key`, with a strong password),
  store the private key and password **only** as the repository secrets
  `NAIL_UPDATER_PRIVATE_KEY` (file content) and `NAIL_UPDATER_KEY_PASSWORD`,
  and the public half as the repository variable `NAIL_UPDATER_PUBKEY`.
  Keep an offline backup (two copies, e.g. password manager + encrypted USB
  stick): **losing it means installed copies can never be updated again**,
  and a leak means an attacker who also controls the update host can ship
  "updates". Rotating the key needs one release signed with the old key
  that embeds the new public key.
- Locally a build picks up `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) and
  `NAIL_UPDATER_PUBKEY` / `NAIL_UPDATER_PUBKEY_FILE`; one half without the
  other is an error.

## Authenticode signing

Only when configured; otherwise builds are unsigned (SmartScreen warns) and
everything else works the same. The EV certificate comes later.

- `NAIL_SIGN_THUMBPRINT` (SHA-1 of a code-signing certificate in the
  Windows certificate store), optional `NAIL_SIGN_TIMESTAMP_URL` (RFC 3161;
  default DigiCert's), `NAIL_SIGN_DESCRIPTION`, `NAIL_SIGNTOOL`.
- The bundler signs Nail.exe, the uninstaller, the NSIS installer and the
  MSI; `build-installer.mjs` signs `bin\nail.exe`, the MAPI DLLs and the
  portable Nail.exe with signtool (SHA-256, timestamped).
- CI: the secrets `NAIL_SIGN_PFX_BASE64` / `NAIL_SIGN_PFX_PASSWORD` (a .pfx)
  are imported by `scripts/import-signing-cert.ps1`; then
  `scripts/verify-signatures.ps1` checks every file. An EV certificate on a
  hardware token or in a cloud HSM is used through `NAIL_SIGN_THUMBPRINT`
  (and the vendor's signtool / CSP) on a runner that can reach it.

### Who sees the private key in CI

`release.yml` hands `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` to the
**"Build installers" step only** (step-level `env`), never to the job:
job-level variables reach every step, including `npm ci`, whose
dependencies run install scripts. Inside that step `build-installer.mjs`
passes the key on only to `tauri bundle` (`scripts/lib/build.mjs`
`withoutUpdaterSecrets`): the UI build, cargo (and every crate's build
script) and `tauri build --no-bundle` run without it. A script test
(`scripts/test/build.test.mjs`) checks both. Keep it that way when adding
steps: anything that runs third-party code must not see the key.

### Test-only overrides are never part of a release

Two cargo features exist only for the e2e desktop build
(`e2e/scripts/build-desktop.mjs`): `nail-app/test-overrides`
(`NAIL_REGISTRY_ROOT`, `NAIL_UPDATE_ENDPOINT` / `NAIL_UPDATE_PUBKEY`,
`NAIL_MAPI_PIPE`) and `nail-mapi/test-overrides` (`NAIL_MAPI_PIPE`,
`NAIL_MAPI_APP`). In a shipped build these variables would let a normal
user replace the administrator's update policy, the embedded update key,
or make the MAPI DLL start any program inside other (elevated) programs.
`build-installer.mjs` and the `mapi` CI job never enable them
(`scripts/test/ci-workflow.test.mjs`).

## Releasing

1. Bump the version (`tauri.conf.json`, workspace `Cargo.toml`), add
   `packaging/release-notes/<version>.md`, tag `v<version>`.
2. `release.yml` builds x64 + ARM64 (NSIS, MSI, offline NSIS, portable,
   SBOMs, `.sig`), signs if configured, writes the update and winget
   manifests, creates a draft GitHub release.
3. Check the draft, publish it, then upload `updates/<channel>/latest.json`
   to the update host and open the winget pull request.

Before the first public release: the release update key and the update
host (maintainer), `packaging/release-notes/<version>.md`, and one green run of
the `installer`, `installer-arm64`, `mapi` and `desktop` CI jobs.
