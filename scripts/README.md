# scripts/

Build and release helpers (plan M2b WP44, M4b WP112; ADRs 0040,
0089). None of them writes build output into the repository (it lives
in OneDrive): output goes to `%LOCALAPPDATA%\nail-build\` or
`NAIL_BUILD_DIR`.

| Script | What |
| --- | --- |
| `build-installer.mjs` | NSIS installer, MSI, offline-WebView2 NSIS, portable zip, update signatures, SBOMs, checksums → `<build dir>/dist` (Windows; x64 or ARM64) |
| `installer-smoke.ps1` | install → launch → update (loopback server) → uninstall of the NSIS installer in a temp directory, per user |
| `msi-smoke.ps1` | silent MSI install with properties → launch → upgrade → uninstall (elevated; CI) |
| `update-manifest.mjs` | per-channel update manifests (`<channel>/latest.json`) from a release, signatures checked |
| `update-server.mjs` | loopback (127.0.0.1) static server for manifests and installers |
| `update-check.mjs` | the app updater's check + verified download, for the smoke tests |
| `winget-manifest.mjs` | winget manifests from the NSIS installers |
| `updater-key.mjs` | create / check the development update key (private half outside the repository) |
| `release-version.mjs` | the app version, the next patch version, tag check, channel (CI) |
| `import-signing-cert.ps1` | CI: Authenticode certificate from secrets → `NAIL_SIGN_THUMBPRINT` (skipped without) |
| `verify-signatures.ps1` | CI: every exe / msi signed and timestamped |
| `sbom.mjs` | CycloneDX SBOMs only (Rust crates from `cargo metadata`, UI packages from `package-lock.json`) |
| `check-socket-binds.mjs` | fails on sockets bound to unspecified / non-loopback addresses outside the allowlist in `lib/socket-binds.mjs` (ADR 0057; first step of `npm test`) |
| `lib/*.mjs` | the scripts' pure helpers; tests in `test/` |
| `interop/` | engine tests against real Dovecot / Stalwart servers (own crate; see its README) |

```sh
node scripts/build-installer.mjs                          # full build (UI, CLI, MAPI DLLs, app, NSIS + MSI)
node scripts/build-installer.mjs --skip-ui --bundles nsis # reuse ui/dist, NSIS only
node scripts/build-installer.mjs --arch arm64 --offline-webview
node scripts/build-installer.mjs --dev-updater-key --update-base https://updates.example/nail
powershell -File scripts/installer-smoke.ps1              # newest installer in <build dir>/dist
node scripts/sbom.mjs --out <dir>
node --test "scripts/test/*.test.mjs"                     # tests of the helpers
cargo deny check                                          # deny.toml at the repository root
```

`build-installer.mjs` options (`lib/build.mjs` `parseArgs`): `--skip-ui`,
`--no-cli` (without `bin\nail.exe`), `--no-mapi` (without the MAPI DLLs),
`--no-portable`, `--arch x64|arm64` (default: this machine), `--bundles
nsis,msi` (default both), `--offline-webview` (also
`Nail_<v>_<arch>-offline-setup.exe` with the full WebView2 runtime),
`--version V` (build another version, e.g. the smoke test's update),
`--update-base URL` (or `NAIL_UPDATE_BASE_URL`), `--insecure-loopback-updates`
(http on 127.0.0.1 only, smoke tests), `--dev-updater-key`, `--target-dir DIR`
(default `<build dir>/installer-target`), `--out DIR` (default
`<build dir>/dist`). The Tauri bundler downloads NSIS, WiX 3.14 and the
WebView2 installers to `%LOCALAPPDATA%\tauri` on first use. The x86
`NailMapi.dll` needs the `i686-pc-windows-msvc` Rust target, ARM64 builds
`aarch64-pc-windows-msvc` (plus the MSVC ARM64 tools and clang); a missing
DLL target is a warning locally and an error in CI.

Signing (Authenticode: `NAIL_SIGN_THUMBPRINT`, …) and update keys
(`TAURI_SIGNING_PRIVATE_KEY`, `NAIL_UPDATER_PUBKEY`, …) are environment
only and optional: docs/release.md. The update key reaches only
`tauri bundle`; installer builds never enable the test-only
`test-overrides` features.

`installer-smoke.ps1` refuses to run when Nail is installed for the current
user. It writes only per-user registry keys, removes them again, and keeps
the app's data inside its temporary directory (portable marker). With
`-UpdateEndpoint` / `-UpdatePubkey` it also updates the installation from a
loopback update server. `msi-smoke.ps1` needs an elevated shell and refuses
to run when Nail is installed per machine.
