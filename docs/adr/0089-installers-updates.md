# ADR 0089 (WP112): per-machine MSI, MAPI option, signing, update manifests, winget, installer CI

Status: accepted · 2026-10-02 · M4b WP112 (number to be assigned by the integrator) · amended by ADR 0093
(the NSIS MAPI option registers a protected copy in Program Files through one
elevated cmd.exe instead of a `.reg` import naming the per-user DLL; the
release update key reaches only `tauri bundle`; key handling moved to
`docs/release.md`)

## Context

Plan section 9, WP112: a per-machine MSI with `AUTOSTART`, `ADDTOPATH`,
`DISABLEUPDATES`, `MAPI` for silent installs; the per-user NSIS installer
stays the default and asks for elevation only for the MAPI component; an
offline WebView2 variant; ARM64 builds; signing steps that run only with a
configured certificate (EV later); Tauri updater artifacts and signed
manifests per channel; a winget manifest generator; CI that builds all
installers and runs install → launch → update → uninstall on
`windows-latest` against a loopback update server. ADR 0087 fixed the
registry facts (`nail_api::desktop::registry`), the MAPI registration
(`nail_api::mapi::registry`, DLLs at `mapi\<arch>\NailMapi.dll`) and the
manifest path (`<channel>/latest.json`). Tool facts were checked against
the installed Tauri CLI 2.12.0 (its embedded WiX template and CLI help),
`tauri-utils` 2.10 config types and `tauri-plugin-updater` 2.13.1.

## Decisions

### MSI: Tauri's WiX template + one fragment

- **No template copy.** Tauri's `main.wxs` is already per-machine
  (`InstallScope="perMachine"`, `ProgramFiles64Folder`), so Nail adds a
  fragment (`app/src-tauri/windows/wix/nail.wxs`) through
  `bundle.windows.wix.fragmentPaths` + `featureRefs` (`NailIntegration`).
  Less to re-copy on CLI upgrades than the NSIS template. Static bundle
  settings live in `app/src-tauri/windows/bundle.json` (WP112's slot), which
  `build-installer.mjs` merges over `tauri.conf.json`; the **upgrade code is
  fixed** (`4B724EE5-4370-4C05-BE94-9D606D52D37B`, test-pinned).
- **Properties**: `AUTOSTART` (HKLM Run `Nail` = `"<dir>\Nail.exe"
  --autostart`), `ADDTOPATH` (WiX `Environment`, machine PATH, `<dir>\bin`
  only), `DISABLEUPDATES` (default **1**; `DisableUpdates` DWORD under
  `HKLM\Software\Nail\Install` — the contract's install fact, not the
  policy key, which stays for Group Policy), `MAPI` (both registry views).
  All `Secure`. **Remembered**: each is read back from the facts key in
  `AppSearch`, but a command-line value is saved before and restored after
  (`SetProperty` in both sequences), so `msiexec /i new.msi /passive` (the
  Tauri updater, if IT allowed it) keeps every choice while IT can still
  change one explicitly. Normalized private properties (`NAIL_*`) turn
  `1` / `#1` into the DWORDs.
- **Registration** machine-wide (ProgIDs, Open with, Capabilities,
  RegisteredApplications) mirrors the NSIS hooks; texts in
  `wix/languages/en-US.wxi` (preprocessor variables; a custom `.wxl` would
  have to repeat Tauri's own strings). Tests keep the two installers'
  types, ProgIDs and texts identical.
- `Version` is the MSI ProductVersion (`1.2.0-beta.3` → `1.2.0.3`,
  `scripts/lib/build.mjs msiVersion`); MSI has no pre-release field.

### NSIS: install facts and the MAPI option

- `hooks.nsh` writes `HKCU\Software\Nail\Install` (`Installer=nsis`,
  `Scope=per-user`, `InstallDir`, `Version`, `Mapi`) on every install and
  update; uninstall removes it. No `DisableUpdates` (per-user installs
  update themselves unless a policy says otherwise).
- **MAPI option** (options page checkbox, `/MAPI=0|1`, default off,
  remembered): Windows reads Simple MAPI providers from
  `HKLM\Software\Clients\Mail`, so this one step needs admin rights. The
  32-bit, non-elevated installer writes a UTF-16 `.reg` file and runs
  **one** `ExecShellWait runas reg.exe import … /reg:64` (the 32-bit view
  addressed as `WOW6432Node` in the same file) → one UAC prompt; the result
  is read back from the registry (declined → `Mapi=0`, a detail line, Nail
  installed anyway). **Superseded by ADR 0093:** the elevated step is one
  `cmd.exe` that copies the DLLs to `%ProgramFiles%\Nail MAPI` and registers
  that copy (a registration naming the per-user DLL let unelevated code run
  in elevated programs and other users' sessions). Chosen over an elevated `Nail.exe --setup-mapi-*`
  helper because it stays inside the installer slot and needs no app flag.
  Updates keep the registration; uninstall removes it only when `DLLPath`
  points into this installation (the MSI may own it). Known limits: the
  DLL lives in the user's profile, so other users of the machine cannot use
  a per-user registration; silent `/MAPI=1` still prompts — unattended
  deployments use the MSI.
- Neither installer sets the default mail client (`Clients\Mail`'s default
  value): the user chooses (WP113 / WP114 default-apps UI).
- The x64 packages carry `mapi\x64` + `mapi\x86`, ARM64 `mapi\arm64` +
  `mapi\x86`. x64 programs on ARM64 read the native view and would load
  the ARM64 DLL — not supported for now (would need an x64 / ARM64EC DLL).

### Build (`scripts/build-installer.mjs`)

- Order: UI → CLI and **MAPI DLLs** with a static C runtime (the DLL is
  loaded into other programs) → `tauri build --no-bundle --target <triple>`
  → copy Nail.exe for the portable zip (the bundler later patches the exe
  with its installer kind, which the updater uses) → signtool for files the
  bundler does not sign → `tauri bundle --bundles nsis,msi` → offline NSIS
  (`webviewInstallMode: offlineInstaller`, renamed
  `-offline-setup.exe`, no update signature: updates use the normal
  installer) → zip → SBOMs → `SHA256SUMS`.
- `--arch arm64` cross-builds (`aarch64-pc-windows-msvc`).

### Signing

Environment only (`scripts/lib/sign.mjs`): `NAIL_SIGN_THUMBPRINT` (+
timestamp URL, description, signtool path). With it, the bundler signs
Nail.exe / installers / uninstaller (`certificateThumbprint`, SHA-256,
RFC 3161), and signtool signs the CLI, the DLLs and the portable exe;
without it the build says so and stays unsigned. CI imports a `.pfx` from
secrets when present (`import-signing-cert.ps1`) and then checks every file
(`verify-signatures.ps1`). An EV token / cloud HSM later uses the same
thumbprint path.

### Updates

- Build-time `plugins.updater` (overlay): the update key's public half,
  one endpoint `<base>/stable/latest.json` (the shell derives the beta URL;
  no base → no endpoint → "not configured"), `installMode: passive`,
  **`requireSignedVersion: true`** (Tauri CLI 2.12 puts the version into the
  signature's trusted comment). `http` only for a loopback base with
  `--insecure-loopback-updates` (smoke builds; refused for other hosts).
- `createUpdaterArtifacts: true` only with a key: `TAURI_SIGNING_PRIVATE_KEY`
  (+ password) **and** the public half (`NAIL_UPDATER_PUBKEY[_FILE]`);
  one without the other is an error, neither builds without updates.
- **Manifests** (`scripts/update-manifest.mjs`): Tauri static format,
  `windows-<arch>-nsis|msi` plus `windows-<arch>` = NSIS (an MSI install
  finds its `-msi` entry first). Every signature is verified (own minisign
  check in Node: Ed25519 + BLAKE2b-512, key id, trusted comment, version)
  before writing. Stable releases also refresh `beta` unless the beta is
  newer; pre-releases go to beta only. Notes are Markdown text, ≤ 64 KiB.
- **Key handling**: the development key's private half lives in
  `%LOCALAPPDATA%\nail-build\keys` (`scripts/updater-key.mjs`), only
  `packaging/updater/dev.pub` is committed; CI smoke runs use a throwaway
  key; the release key exists only as secrets (+ offline backup).
  Details in `docs/release.md`.

### winget

`scripts/winget-manifest.mjs` writes the 1.10.0 multi-file manifest
(version / installer / defaultLocale) for the NSIS installers: `nullsoft`,
scope user, `/S` / `/P`, ProductCode `Nail`, protocols and extensions as
registered. Facts in `packaging/winget/nail.winget.json`; the installer URL
template is a required argument (no guessed host).

### CI

- `ci.yml` `installer`: throwaway update key → build A (NSIS, MSI, offline
  NSIS, loopback update URL) → build B (next patch version) → manifests for
  B on `scripts/update-server.mjs` (127.0.0.1:47811) → **NSIS smoke**
  (install A, facts, launch, update check + verified download from the
  loopback server, `B-setup.exe /S /UPDATE`, facts / options / data kept,
  launch B, uninstall) → **MSI smoke** (silent install with `ADDTOPATH=1
  MAPI=1`, facts incl. `DisableUpdates=1`, both MAPI views, launch, upgrade
  to B with only `AUTOSTART=1` → remembered values kept, uninstall) →
  winget manifests → upload. `installer-arm64`: cross-built, uploaded.
- `release.yml` (tags `v*` / manual): x64 + ARM64 builds with release
  signing and update keys, manifests, winget, draft GitHub release.
  Publishing manifests at the update host and the winget PR stay manual.
- The update step of the smoke test applies the update the way the plug-in
  does (manifest → target → verified download → installer with `/UPDATE`);
  the app's own updater client is WP113's and its desktop scenario WP115's.

## Consequences

- WP113 must: read `HKLM` facts for MSI installs, treat an `HKLM` Run
  value as "start with Windows: on, managed"; derive channel URLs from
  `plugins.updater.endpoints[0]`; register the updater plug-in only when
  `plugins.updater` exists; keep `requireSignedVersion` (apiRequests).
- Not verified locally: MSI install / upgrade (needs elevation), ARM64 and
  offline builds (download + MSVC ARM64 tools), signing with a real
  certificate, the release workflow. The CI jobs run them.
- Upgrading the Tauri CLI: re-check the WiX template's ids (`INSTALLDIR`,
  `ProductVersion`) the fragment relies on.
