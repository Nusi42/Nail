# packaging/

Distribution of the Windows app (plan M4b WP112, ADRs 0087, 0089):
signing, update manifests, winget. Build output never goes into the
repository (it lives in OneDrive): everything writes to
`%LOCALAPPDATA%\nail-build\` or `NAIL_BUILD_DIR`, like `scripts/`.

| Path | What |
| --- | --- |
| `updater/dev.pub` | public half of the **development** update key (the private half lives outside the repository) |
| `winget/nail.winget.json` | package facts for the winget manifests (identifier, publisher, licence, descriptions) |
| `release-notes/<version>.md` | optional release notes (Markdown) put into the update manifests |

The tools are in `scripts/` (tests in `scripts/test/`):

| Script | What |
| --- | --- |
| `build-installer.mjs` | NSIS + MSI (+ offline WebView2 variant), x64 / ARM64, signing, update signatures |
| `update-manifest.mjs` | `<channel>/latest.json` from a release's installers and `.sig` files (checked first) |
| `winget-manifest.mjs` | the three winget manifest files from the NSIS installers |
| `update-server.mjs` | a loopback (127.0.0.1) static server for manifests and installers (smoke tests) |
| `update-check.mjs` | what the app's updater does with a manifest: pick, download, verify |
| `updater-key.mjs` | create / check the development update key |
| `import-signing-cert.ps1`, `verify-signatures.ps1` | CI: Authenticode certificate from secrets, signature check |

## Update channels and manifests

The app reads `<base>/<channel>/latest.json` (`nail_api::desktop::
UpdateChannel::manifest_path`), channels `stable` and `beta`. The base URL
is built into Nail.exe (`build-installer.mjs --update-base <https URL>` or
`NAIL_UPDATE_BASE_URL`; the shell swaps `stable/latest.json` of the
configured endpoint for the chosen channel). A build without a base URL
reports updates as *not configured*.

Format: Tauri's static updater JSON — `version`, `notes` (Markdown, shown as
text), `pub_date`, and `platforms` with `windows-x86_64-nsis`,
`windows-x86_64-msi`, `windows-aarch64-nsis`, … plus `windows-<arch>` =
the NSIS installer. The updater picks `windows-<arch>-<installer kind>`
first, so an MSI install only ever gets an MSI.

A **stable** release writes `stable/latest.json` and also `beta/latest.json`
(unless the current beta is newer: pass it with `--existing-beta`); a
pre-release version (`1.2.0-beta.1`) can only go to `beta`.

```sh
node scripts/update-manifest.mjs --version 0.2.0 --channel stable \
  --artifacts <x64 dist> --artifacts <arm64 dist> \
  --download-base https://github.com/<owner>/<repo>/releases/download/v0.2.0 \
  --notes packaging/release-notes/0.2.0.md --out <dir>
# then publish <dir>/stable and <dir>/beta at the base URL
```

"Signed manifests": each installer listed carries a minisign signature
(`.sig`, written by the bundler) that Nail.exe checks against the update
key built into it, **including the version the installer was signed for**
(`requireSignedVersion`), so a tampered manifest can neither point at a
foreign file nor pair a new version number with an old installer. The
manifest generator verifies every signature (and version) before it writes
anything.

## Update key handling

The update key is a minisign key pair (Tauri's `tauri signer`), separate
from the Authenticode certificate.

- **Never commit a private key.** Only public halves are in the repository.
- **Development key**: `node scripts/updater-key.mjs --generate` writes the
  private key to `%LOCALAPPDATA%\nail-build\keys\nail-updater-dev.key`
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

## winget

```sh
node scripts/winget-manifest.mjs --version 0.2.0 --artifacts <x64 dist> --artifacts <arm64 dist> \
  --installer-url-template "https://github.com/<owner>/<repo>/releases/download/v{{version}}/{{file}}" \
  --out <dir>
winget validate <dir>/manifests/n/Nail/Nail/0.2.0     # where winget is installed
```

The package wraps the per-user `setup.exe` (`nullsoft`, scope user, silent
`/S`, ProductCode `Nail`). Submitting is a pull request to
`microsoft/winget-pkgs` (manual, owner task); the installers must be public
at the URLs first.

## Releasing (summary)

1. Bump the version (`tauri.conf.json`, workspace `Cargo.toml`), add
   `packaging/release-notes/<version>.md`, tag `v<version>`.
2. `release.yml` builds x64 + ARM64 (NSIS, MSI, offline NSIS, portable,
   SBOMs, `.sig`), signs if configured, writes the update and winget
   manifests, creates a draft GitHub release.
3. Check the draft, publish it, then upload `updates/<channel>/latest.json`
   to the update host and open the winget pull request.
