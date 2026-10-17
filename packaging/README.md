# packaging/

Distribution of the Windows app (plan M4b WP112, ADRs 0087, 0089):
signing, update manifests, winget. Build output never goes into the
repository: everything writes to the checkout's gitignored `.nail-build/`
or `NAIL_BUILD_DIR`, like `scripts/`.

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
| `import-signing-cert.ps1`, `verify-signatures.ps1` | import a `.pfx` code-signing certificate (prints its thumbprint for `NAIL_SIGN_THUMBPRINT`), signature check |

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

## Update key, signing and releasing

The procedures (update key handling and rotation, Authenticode signing,
the release steps) are internal documentation: [`docs/release.md`](../docs/release.md).

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
