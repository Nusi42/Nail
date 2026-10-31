# Releasing, signing and the update key

Internal procedures for Windows releases (plan M4b WP112; ADRs 0089, 0093,
0201, 0203). The tools and file formats are described in `packaging/README.md` and
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
  the old key that embeds the new public key. Its public half is
  `packaging/updater/release.pub`, which `release:prepare` builds into the
  app and checks every signature against.
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

Releases are GitHub releases of the public repository (ADR 0203). Two
commands in the lead's work folder, on a clean checkout of its branch:

1. Write the version's section of `userDocs/en/release-notes.md`
   (``## Nail X.Y (`X.Y.Z`)``; it becomes the GitHub release's text, its
   introduction the update dialog's text) and keep the changelog's
   `[Unreleased]` entries current. Commit both.
2. Prepare, with the release update key set for this command only (see
   "Who sees the private key"):

   ```text
   TAURI_SIGNING_PRIVATE_KEY_PATH=<private key> TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<password or empty>      npm run release:prepare -- --version X.Y.Z
   ```

   It refuses uncommitted files or an existing tag, sets the version
   (`Cargo.toml` `[workspace.package]` and every copy), moves the
   changelog's `[Unreleased]` entries under it, runs the local verification
   (`docs/dev-setup.md` "Verification (no hosted CI)" without ARM64, the
   installer smoke tests and the perf run, which stay by hand;
   `--skip-checks` for a re-run after a late failure), builds
   the x64 NSIS installer and the portable zip with update signatures and
   the update base `<repository>/releases/download`, writes
   `<channel>/latest.json` (every signature checked against
   `packaging/updater/release.pub`) and the release text, and commits
   `chore(release): release X.Y.Z`. Output: `.nail-build/release/X.Y.Z/`.
   `--dry-run` does all of it without committing (the version files are
   restored); with `--dev-updater-key` it needs no release key.
3. Install the built installer and try it.
4. `npm run release:publish` (`-- --yes` without the question): tags
   `vX.Y.Z`, pushes the branch and the tag to origin; with a master
   checkout there it fast-forwards the master, and the master pushes `main`
   and the tag to the public remote (`--remote`, default `github`; only the
   master publishes, ADR 0205); a plain clone pushes the branch as `main`
   and the tag itself. Then it creates the GitHub release
   with every built file (a pre-release for a `-beta` version), and
   replaces `latest.json` on the rolling release of the channel, `stable`
   or `beta` (a stable release also refreshes `beta` unless beta is newer).
   Needs the GitHub CLI, signed in once (`gh auth login`). A re-run goes on
   where a failure stopped.

Other builds by hand: signed builds need `NAIL_SIGN_THUMBPRINT` (then
`scripts/verify-signatures.ps1 -Folder <out>`); the MSI, the offline
WebView2 installer and ARM64 come from
`node scripts/build-installer.mjs --arch arm64 --bundles nsis,msi --offline-webview --out <dir>`
(cross-built; once: `rustup target add aarch64-pc-windows-msvc
i686-pc-windows-msvc`); winget manifests from
`node scripts/winget-manifest.mjs --version X.Y.Z --artifacts <dir> --installer-url-template "<repository>/releases/download/v{{version}}/{{file}}" --out .nail-build/winget`.
