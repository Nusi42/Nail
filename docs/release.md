# Releasing, signing and the update key

Internal procedures for Windows releases (plan M4b WP112; ADRs 0089, 0093,
0201). The tools and file formats are described in `packaging/README.md` and
`scripts/README.md`; this page is about the keys and the steps. Nail has no
hosted CI (ADR 0201): a release is built, signed and published locally.

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
  for them. The installer smoke tests use it, so they need no secret.
- **Release key**: generate it once on a trusted, offline machine
  (`npx tauri signer generate -w nail-updater.key`, with a strong password)
  and keep the private key and its password outside every repository and
  work folder. Keep an offline backup (two copies, e.g. password manager +
  encrypted USB stick): **losing it means installed copies can never be
  updated again**, and a leak means an attacker who also controls the update
  host can ship "updates". Rotating the key needs one release signed with
  the old key that embeds the new public key.
- A build picks up `TAURI_SIGNING_PRIVATE_KEY` (key text, or
  `TAURI_SIGNING_PRIVATE_KEY_PATH`) + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
  and the public half in `NAIL_UPDATER_PUBKEY` / `NAIL_UPDATER_PUBKEY_FILE`;
  one half without the other is an error.

### Who sees the private key

Set `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` **only for the
`build-installer.mjs` command** (in a shell started for it, closed
afterwards), never in your profile or a shell that also runs `npm ci`:
dependencies run install scripts. Inside that command `build-installer.mjs`
passes the key on only to `tauri bundle` (`scripts/lib/build.mjs`
`withoutUpdaterSecrets`): the UI build, cargo (and every crate's build
script) and `tauri build --no-bundle` run without it. A script test
(`scripts/test/build.test.mjs`) checks this. Keep it that way when adding
steps: anything that runs third-party code must not see the key.

## Authenticode signing

Only when configured; otherwise builds are unsigned (SmartScreen warns) and
everything else works the same. The EV certificate comes later.

- `NAIL_SIGN_THUMBPRINT` (SHA-1 of a code-signing certificate in the
  Windows certificate store), optional `NAIL_SIGN_TIMESTAMP_URL` (RFC 3161;
  default DigiCert's), `NAIL_SIGN_DESCRIPTION`, `NAIL_SIGNTOOL`.
- The bundler signs Nail.exe, the uninstaller, the NSIS installer and the
  MSI; `build-installer.mjs` signs `bin\nail.exe`, the MAPI DLLs and the
  portable Nail.exe with signtool (SHA-256, timestamped).
- A standard certificate as a `.pfx`: import it into
  `Cert:\CurrentUser\My` once (`scripts/import-signing-cert.ps1` with
  `NAIL_SIGN_PFX_BASE64` / `NAIL_SIGN_PFX_PASSWORD`, or `Import-PfxCertificate`)
  and set `NAIL_SIGN_THUMBPRINT` to the thumbprint it prints. An EV
  certificate on a hardware token or in a cloud HSM is used through
  `NAIL_SIGN_THUMBPRINT` (and the vendor's signtool / CSP) on the machine
  that can reach it.
- After the build, `scripts/verify-signatures.ps1 -Folder <dist>` checks that
  every file is signed and timestamped.

### Test-only overrides are never part of a release

Two cargo features exist only for the e2e desktop build
(`e2e/scripts/build-desktop.mjs`): `nail-app/test-overrides`
(`NAIL_REGISTRY_ROOT`, `NAIL_UPDATE_ENDPOINT` / `NAIL_UPDATE_PUBKEY`,
`NAIL_MAPI_PIPE`) and `nail-mapi/test-overrides` (`NAIL_MAPI_PIPE`,
`NAIL_MAPI_APP`). In a shipped build these variables would let a normal
user replace the administrator's update policy, the embedded update key,
or make the MAPI DLL start any program inside other (elevated) programs.
`build-installer.mjs` never enables them.

## Releasing

Work in a clean work folder reset to the release commit (the build compiles
whatever is on disk).

1. Bump the version (`tauri.conf.json`, workspace `Cargo.toml`), add
   `packaging/release-notes/<version>.md`, tag `v<version>`;
   `node scripts/release-version.mjs --tag v<version>` checks that the tag
   matches the app version, `--channel` prints `stable` or `beta`.
2. Run the full local verification (`docs/dev-setup.md` "Verification (no
   hosted CI)") on that checkout.
3. Build x64 and ARM64 (NSIS, MSI, offline NSIS, portable, SBOMs, `.sig`),
   signed if configured, with the release update key given only to this
   command (see "Who sees the private key"), `NAIL_UPDATER_PUBKEY` and
   `NAIL_UPDATE_BASE_URL` set:
   `node scripts/build-installer.mjs --arch x64 --offline-webview --out .nail-build/release/x64`,
   the same with `--arch arm64` and `--out .nail-build/release/arm64`
   (cross-built; once:
   `rustup target add aarch64-pc-windows-msvc i686-pc-windows-msvc`).
4. `scripts/verify-signatures.ps1 -Folder .nail-build/release/x64` (and
   `arm64`) when signing is configured.
5. Write the update manifests (every `.sig` is checked against the public
   key first):
   `node scripts/update-manifest.mjs --version <version> --channel <channel> --artifacts .nail-build/release/x64 --artifacts .nail-build/release/arm64 --download-base https://github.com/<owner>/<repo>/releases/download/v<version> --notes packaging/release-notes/<version>.md --out .nail-build/updates`,
   and the winget manifests:
   `node scripts/winget-manifest.mjs --version <version> --artifacts .nail-build/release/x64 --artifacts .nail-build/release/arm64 --installer-url-template "https://github.com/<owner>/<repo>/releases/download/v{{version}}/{{file}}" --out .nail-build/winget`.
6. Push the tag and create the GitHub release by hand (web UI): title
   `Nail <version>`, the release notes, every file of
   `.nail-build/release/x64` and `.nail-build/release/arm64` attached, marked
   as a pre-release for the `beta` channel.
7. Upload `updates/<channel>/latest.json` to the update host and open the
   winget pull request.

Before the first public release: the release update key and the update
host (maintainer), `packaging/release-notes/<version>.md`, and one green
local verification run including the installer and MSI smoke tests, the
MAPI tests and the desktop smoke tests.
