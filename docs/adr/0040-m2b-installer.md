# ADR 0040: Windows installer, portable build, supply chain and CI

Status: accepted (M2b WP44)
Date: 2026-09-29

## Context

Plan section 7, WP44, and the design section "Windows installer &
distribution" ask for a per-user NSIS installer from the Tauri bundler
(WebView2 bootstrapper, `mailto:` / `webcal:` / `webcals:` and `.eml` /
`.ics` / `.vcf` via RegisteredApplications and Capabilities, optional start
with Windows, optional `nail` CLI on `PATH`, uninstall that keeps data), a
portable zip, `cargo-deny` with a licence allowlist and advisories, a
CycloneDX SBOM per build, and CI jobs for the installer, the desktop smoke
test and real-server interop. The design notes that its tooling details
were written from memory; they were checked against Tauri CLI 2.12.0
(`tauri-utils` 2.10 config types and the NSIS template embedded in the CLI).

## Decisions

### Installer: Tauri NSIS with a copied template and hooks

- `bundle.windows.nsis.installMode = "currentUser"`: no admin prompt.
  **Deviation:** the design's "per-machine on request" is not offered by
  the NSIS build. Tauri's `both` mode requests `Highest` execution level,
  which shows a UAC prompt to every administrator. Per-machine installs are
  the MSI's job (M4).
- **Nail's copy of the Tauri template** (`app/src-tauri/installer/installer.nsi`,
  based on `@tauri-apps/cli` 2.12.0, changes marked `; NAIL:`), because the
  stock template cannot be hooked where Nail needs it:
  1. the per-user default directory is `%LOCALAPPDATA%\Programs\Nail`
     (stock: `%LOCALAPPDATA%\Nail`, which is Nail's data directory);
  2. an options page before installing (see below);
  3. the uninstall checkbox reads "Also delete my mail data, settings and
     stored passwords";
  4. Nail's strings are included after the languages are loaded.
  `app/src-tauri/tests/installer.rs` fails when `package-lock.json` pins
  another Tauri CLI than the one the copy is based on, so an upgrade forces
  a re-copy (procedure in `installer/README.md`).
- **Registration in `installer/hooks.nsh`, not Tauri's `fileAssociations` /
  deep-link plugin.** Tauri's `APP_ASSOCIATE` writes the extension's default
  value and its deep links overwrite `HKCU\Software\Classes\mailto`, i.e.
  they take the user's choice over. Windows wants apps to register and let
  the user choose. The hooks write, per user: ProgIDs `Nail.Url.mailto`,
  `Nail.Url.webcal` (for `webcal` and `webcals`), `Nail.eml`, `Nail.ics`,
  `Nail.vcf` (`"Nail.exe" "%1"`), `OpenWithProgids` entries for the three
  extensions, `HKCU\Software\Nail\Capabilities` (`FileAssociations`,
  `URLAssociations`) and `HKCU\Software\RegisteredApplications\Nail`, then
  `SHChangeNotify(SHCNE_ASSOCCHANGED)`. Uninstall removes exactly these.
  The first-run "open Default apps" button can use
  `ms-settings:defaultapps?registeredAppUser=Nail` (integrator).
- `mainBinaryName = "Nail"`: the installed app is `Nail.exe`; the CLI is
  `bin\nail.exe`. Only `bin\` goes on `PATH` (not the uninstaller or the
  app), and the two never collide on the case-insensitive file system.
- **WebView2:** `embedBootstrapper` (silent), installed only when missing.
  The offline variant (full runtime) is not built yet.
- **Options page** (instead of a checkbox on the finish page, a small
  deviation): "Start Nail when I sign in to Windows" (HKCU `Run` value
  `Nail` = `"Nail.exe" --autostart`; the app starts minimized, as there is
  no tray yet) and "Add the nail command-line tool to PATH". Both default
  to off, or to the previous installation's choice. The same code path
  serves silent installs: `/AUTOSTART=0|1`, `/ADDTOPATH=0|1`. Updates
  (`/UPDATE`) keep the user's choices.
- **PATH and credentials are changed by `Nail.exe`, not by NSIS**
  (`--setup-path-add`, `--setup-path-remove`, `--setup-purge-credentials`;
  `app/src-tauri/src/setup_helper.rs`). NSIS strings are 1024 characters,
  so NSIS would truncate a long user `PATH`; the helper keeps the value's
  type (`REG_EXPAND_SZ`) and all other entries, and broadcasts
  `WM_SETTINGCHANGE`. The credential purge deletes generic credentials
  whose target is `<name>.nail`: how `keyring` stores
  `keychain:nail/<name>` references (`SecretRef::SERVICE`). References to
  other services that a user typed into `accounts.kdl` are not touched.
- **Uninstall keeps** `%APPDATA%\Nail`, `%LOCALAPPDATA%\Nail`, Tauri's
  `%APPDATA%` / `%LOCALAPPDATA%\app.nail.desktop` (WebView2 profile) and the
  credentials unless the checkbox (silent: `/DELETEDATA`) is set. It always
  removes the registration, the `PATH` entry, the `Run` value and the
  remembered install directory (`HKCU\Software\Nail\Nail`; Tauri keeps it,
  so a later install would default to a directory that is gone).
- Unsigned: signing, MSI, winget, the updater and ARM64 builds are M4.
  The Simple MAPI provider (design) is not part of WP44.

### Launch arguments

Windows starts `Nail.exe "<url or path>"`. `app/src-tauri/src/launch.rs`
parses the argument into `LaunchRequest` (`compose` with a decoded RFC 6068
`mailto:` — `to` / `cc` / `bcc` / `subject` / `body` / `in-reply-to` only,
`attach=` and other fields ignored; `subscribeCalendar` with the `https:`
URL; `openMessageFile`, `importCalendarFile`, `importContactsFile`). The
shell keeps the requests of its start in `PendingLaunch` and hands them to
the UI once through the new command `nail_launch_requests`; `LAUNCH_CHANNEL`
(`nail://launch`) is reserved for requests that arrive while running. What
each request does, and single-instance forwarding (a second start currently
opens a second window and core on the same profile), are for the integrator
(WP45).

### Portable zip

`Nail/Nail.exe`, `Nail/bin/nail.exe` and an empty `Nail/portable` marker.
With the marker next to the executable, the app keeps `data\`, `config\`
and `webview\` beside it (`app/src-tauri/src/portable.rs`; seeded demo runs
still use a temporary directory). It relies on the installed WebView2
runtime (part of Windows 11). The zip is written with Windows' `tar.exe`
(PowerShell 5's `Compress-Archive` writes `\` separators).

### Supply chain

- `deny.toml` at the repository root: licence allowlist MIT, Apache-2.0
  (also with LLVM exception), BSD-2/3-Clause, ISC, Zlib, Unicode-3.0 /
  DFS-2016, CC0-1.0, CDLA-Permissive-2.0, bzip2-1.0.6 and MPL-2.0; GPL,
  LGPL and AGPL are rejected by not being listed (an `A OR B` expression
  passes when one side is allowed, e.g. `r-efi`'s `MIT OR Apache-2.0 OR
  LGPL-2.1-or-later`). Nail's own crates are skipped (`publish = false`).
  `openssl`, `openssl-sys`, `aws-lc-rs`, `aws-lc-sys` are banned; only
  crates.io is an allowed source. Advisories: RUSTSEC-2023-0071 (`rsa` 0.9,
  Marvin timing side channel) is ignored with the rationale of ADR 0034:
  no fixed release in rPGP's RustCrypto generation, decryption is blinded.
- **SBOM:** `scripts/sbom.mjs` writes CycloneDX 1.5 JSON per build: the
  Rust crates reachable from `nail-app` and `nail-cli` through normal
  dependencies (from `cargo metadata` for the release target, SHA-256 from
  `Cargo.lock`, licences, purls, dependency graph) and the UI's npm runtime
  packages (from `package-lock.json`, SHA-512 integrity). Not
  `cargo cyclonedx`, which writes its files next to every `Cargo.toml`
  (into the OneDrive repository), and not `npm sbom`, which refuses this
  workspace because the root package has no version.

### Build and CI

- `scripts/build-installer.mjs`: UI → `nail` CLI in release with a static C
  runtime (`-C target-feature=+crt-static`, own target subdirectory; the
  app gets the same from `tauri-build`) → `tauri build --bundles nsis` with
  an overlay config that adds the CLI as `bin/nail.exe` → portable zip →
  SBOMs → `SHA256SUMS`. Output in `<build dir>/dist`, never in the repo.
- `scripts/installer-smoke.ps1`: silent install into a temp directory,
  checks files / registration / `PATH`, runs `nail --version`, starts
  `Nail.exe mailto:…` (portable marker added first, so nothing lands in the
  real profile) until the core's MCP endpoint appears, uninstalls, checks
  that everything but the data is gone. Refuses to run when Nail is
  installed for the user.
- `.github/workflows/ci.yml`: `build-test` (as before, plus the script
  tests; Playwright web only), `desktop` (the WebView2 smoke test with
  `NAIL_E2E_DESKTOP=1`), `installer` (build, smoke, upload of installer,
  zip, SBOMs and checksums), `deny` (`cargo-deny-action`), `interop`
  (Ubuntu, Dovecot 2.3 and Stalwart in Docker).
- **Interop tests** live in `scripts/interop/`, a crate with its own
  `[workspace]` (so Linux CI does not build the Tauri shell) and path
  dependencies on the engine crates; CI copies the root `Cargo.lock` into
  it. They run with `NAIL_INTEROP=1` and return at once otherwise. The
  servers are started with `docker run` after the checkout rather than as
  `services:`, because they need the repository's config files and the
  certificates generated in the job (test CA, trusted as the only root).
  Covered: IMAP login over TLS and STARTTLS, wrong passwords, folders,
  APPEND, UID SEARCH by Message-ID, FETCH, STORE, MOVE, EXPUNGE; POP3 over
  TLS and STLS reading what IMAP stored; SMTP submission (587 and 465) with
  delivery checked over IMAP (Stalwart). **ManageSieve** is not covered: the
  client arrives with M2c (WP51); port 4190 is already exposed.

## Consequences

- `app/src-tauri` gains `launch.rs`, `portable.rs`, `setup_helper.rs`, the
  `--autostart` / `--setup-*` flags and positional targets, a second Tauri
  command, and Windows-only dependencies `windows` (workspace, plus the
  `Win32_Security_Credentials` and `Win32_UI_WindowsAndMessaging`
  features) and `windows-registry` 0.6 (MIT / Apache-2.0, already in the
  lock file through Tauri).
- A Tauri CLI upgrade needs the template re-copied (test enforced).
- Not verified locally: the `deny` advisories check (needs the RustSec
  database from GitHub) and the interop job (no Docker on the development
  machine). The Stalwart configuration in particular is written from the
  documented format and must be checked on the first CI run; the image tag
  is a workflow variable.
- Follow-ups for WP45 / later: map `LaunchRequest` onto `nail_api` requests
  and have the UI call `nail_launch_requests`; single-instance forwarding
  (`tauri-plugin-single-instance`, MIT / Apache-2.0) that pushes into
  `PendingLaunch` and emits `nail://launch`; the first-run Default-apps
  button; portable mode for `nail` CLI (`bin\..\portable`); tray and a
  hidden autostart; offline WebView2 variant; German installer strings;
  signing, MSI, winget, updater, ARM64 (M4).
