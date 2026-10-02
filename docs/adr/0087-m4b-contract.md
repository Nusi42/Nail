# ADR 0087: M4b contract — installation facts, updater, tray and startup, default apps, Simple MAPI, `nail plugin dev` on the desktop, first run, slots

Status: accepted · 2026-10-02 · M4b WP110

## Context

Wave M4b (plan section 9) makes Nail a proper Windows app: the Simple MAPI
provider (WP111, `crates/nail-mapi`), installers, signing and the update
pipeline (WP112, `app/src-tauri/windows/`, `packaging/`, `scripts/`,
`.github/workflows/`), the desktop shell (WP113, the rest of
`app/src-tauri`) and the UI (WP114, `ui/src/features/system/`), built in
parallel and integrated by WP115. This ADR fixes what they build against
and the slots that keep them apart.

Constraints (plan "Scope decisions for M4", design "Windows installer &
distribution" and its scope decisions): two installers — per-user NSIS
(default, built-in silent updater) and per-machine MSI for IT (in-app
updater disabled by policy); the Tauri updater with signed manifests, its
own key, stable and beta channels; signing only when a certificate is
configured; Windows never lets an app make itself the default (first run
offers Settings → Default apps); "Start with Windows" minimized to the
tray via the per-user Run key; Simple MAPI as a small Rust DLL (x64, x86,
ARM64) forwarding to the running app; every outgoing action needs a
confirmation the user sees; no GPL / LGPL / AGPL, no OpenSSL / aws-lc; no
non-loopback sockets; strings in Fluent catalogs; the core owns data.

## Decisions

### Desktop facts and settings (`nail_api::desktop`)

- **Installation facts** (`InstallationInfo`): version, `kind`
  (`nsis` / `msi` / `portable` / `development`; winget wraps NSIS, so it is
  `nsis`; MSI-managed = `kind == msi`), `scope` (`per-user` /
  `per-machine`, `null` when not installed), `architecture` (`x64` / `x86`
  / `arm64`), install dir, `updates_disabled` (`UpdateDisabledReason`:
  `msi-managed`, `policy`, `portable`, `development`, `not-configured`),
  `forced_channel`, `mapi_installed`, WebView2 version. The shell reads them
  from registry values the installers write (`desktop::registry`:
  `HKCU|HKLM\Software\Nail\Install` → `Installer`, `Scope`, `InstallDir`,
  `Version`, `DisableUpdates` (the MSI's `DISABLEUPDATES`, default 1),
  `Mapi`; policy `HKLM|HKCU\Software\Policies\Nail` → `DisableUpdates`,
  `UpdateChannel`; Run key value `Nail` = `"<Nail.exe>" --autostart`).
- **Updates**: `UpdateSettings` (channel `stable` / `beta`, `auto_check`
  (on start and every 24 h), `auto_download`; stored by the core);
  `UpdateState` (tagged `state`: `idle`, `checking`, `up-to-date`,
  `available {update}`, `downloading {update, downloaded_bytes,
  total_bytes}`, `ready-to-install {update}` = "restart to install",
  `installing`, `failed {failure, detail, update}`, `disabled {reason}`);
  `UpdateInfo` (version, channel, published at, release notes as Markdown —
  untrusted text, never HTML — size); `UpdateFailure` (`network`,
  `manifest`, `signature`, `download`, `install`). `UpdateStatus::compose`
  makes **the policy win**: a disabled updater is `disabled` whatever the
  host reports, a forced channel replaces the stored one. Manifests live at
  `<base>/<channel>/latest.json` (`UpdateChannel::manifest_path`).
- **Tray and startup** (`DesktopSettings`): `close_to_tray` (default on),
  `start_hidden` (when started with `--autostart`; default on),
  `start_with_windows` (the Run key — the host's value wins on reads),
  `plugin_dev_endpoint` (default off).
- **Default apps** (`DefaultAppsStatus`): registered under
  `RegisteredApplications`, one `DefaultAppAssociation` per target
  (`mailto`, `webcal`, `webcals`, `eml`, `ics`, `vcf`) with `registered`
  and `state` (`default` / `other` / `unknown` where Windows does not tell),
  and the Simple MAPI client (`registered`, `state`).
  `default_apps_settings_uri(scope)` = `ms-settings:defaultapps?
  registeredAppUser=Nail` (per user) / `registeredAppMachine=Nail` (per
  machine).
- **First run** (`FirstRunState`: completed, when, which version). Stored by
  the core and served in every mode.
- **`nail plugin dev` on the desktop**: with `plugin_dev_endpoint` the shell
  runs a loopback WebSocket endpoint and writes `ui-endpoint.json`
  (`UiEndpointFile {url, pid, token}`, `UI_ENDPOINT_FILE`). It serves only
  `LoadDevPlugin` (`dev_endpoint_allows`) and forwards only
  `PluginLogLine` / `PluginsChanged` (`dev_endpoint_forwards`); the
  WebSocket upgrade must carry `Authorization: Bearer <token>`. `nail plugin
  dev` sends the token when the file has one (done in this contract);
  `nail serve`'s file has none.
- **The shell's side**: trait `DesktopHost` (Rust only; `attach(notifier)`,
  `installation`, `update_state`, `apply_update_settings`,
  `check_for_updates` / `download_update` / `install_update`,
  `start_with_windows`, `apply_desktop_settings`, `dev_endpoint_url`,
  `default_apps`, `open_default_apps_settings`). Methods are quick; long work
  runs in the host's tasks and reports through the `DesktopNotifier`
  (`DesktopChange::Update | Settings | DefaultApps`), which the core turns
  into events. Passed as `ServiceOptions::desktop`.
- **Requests** (12; desktop only — `DesktopOnly` in remote sessions,
  `unsupported` + `desktop-unavailable` without a host, except the first
  run): `GetInstallation`, `GetUpdateStatus`, `SetUpdateSettings`
  (`forbidden` + `update-channel-locked` against a forced channel),
  `CheckForUpdates` / `DownloadUpdate` / `InstallUpdate` (`forbidden` +
  `update-disabled-*` when disabled, `conflict` unless the state allows it:
  `UpdateState::can_check / can_download / can_install`; `InstallUpdate`
  answers `Done` before Nail quits), `GetDesktopSettings`,
  `SetDesktopSettings`, `GetDefaultApps` (with the Settings URI),
  `OpenDefaultAppsSettings`, `GetFirstRun`, `CompleteFirstRun`. **Events**
  (3): `UpdateStatusChanged {status}`, `DesktopSettingsChanged`,
  `DefaultAppsChanged`; never sent to remote sessions.

### Simple MAPI (`nail_api::mapi`)

- `MapiMessage` = one `MAPISendMail(W)` call decoded by the DLL: subject,
  plain-text body, recipients (`MapiRecipient {class: to | cc | bcc, name,
  address}`; `MAPI_ORIG` dropped, unknown classes → `BadRecipType`;
  `email()` strips `SMTP:` and `<>`, falls back to a name that is an
  address), attachments (`MapiAttachment {path, file_name}`), `ui`
  (`dialog` / `modeless` / `none` from `flFlags`), the calling program's
  file name. `check()` = the checks without the file system (≤ 500
  recipients, ≤ 100 files, ≤ 8 MiB text, valid addresses, absolute paths).
- `MapiResult` = the `MAPI.h` codes Nail returns (`code()` /
  `from_code()`: 0 success, 1 user abort, 2 failure, 3 login failure, 5, 6,
  9, 10, 11 attachment not found, 12 open failure, 14, 15, 18, 25 invalid
  recipients, 26 not supported, 28 attachment too large) with
  `mapi-result-*` texts and `from_api_error`.
- **Transport**: the named pipe `\\.\pipe\nail-mapi-<user SID>`
  (`mapi_pipe_name`; `NAIL_MAPI_PIPE` overrides it in tests), created by the
  shell with `FILE_FLAG_FIRST_PIPE_INSTANCE` and an ACL for the current
  user; the DLL checks the server runs as the same user. Frames: 4-byte
  little-endian length + JSON (`MapiPipeRequest {version, message}` →
  `MapiPipeResponse {result}`; ≤ 16 MiB; `encode_frame` / `read_frame` /
  `write_frame`). When nothing answers, the DLL starts `Nail.exe` from
  `<install>\` (`app_exe_from_dll`: the DLL lives at
  `mapi\<arch>\NailMapi.dll`) and retries for 30 s; it waits up to 120 s for
  the answer. Registration (`mapi::registry`): `HKLM\Software\Clients\Mail\
  Nail` with `DLLPath` / `DLLPathEx` (the 32-bit view gets the x86 DLL); the
  default client is `Software\Clients\Mail`'s default value — WP111 / WP112
  confirm the details against current Windows docs.
- **Never silent**: the core (`Service::submit_mapi`, implemented now) reads
  every attachment before answering (programs delete temp files when the
  call returns; size limit = the composer's 25 MiB), saves a plain-text
  draft from the default identity (body above the signature) and queues
  `LaunchRequest::ComposeDraft {draft_id, program}`; the UI opens the draft
  (done in `features/launch` with `openRichDraft`). Requests without UI are
  handled the same way: Nail never sends mail for another program; the user
  presses Send. No account / identity → `LoginFailure`.

### Libraries (all MIT / Apache-2.0; `cargo deny check licenses bans sources` passes)

- `tauri-plugin-updater` 2.13.1 without default features, `rustls-tls`
  only (reqwest `rustls-no-provider`, rustls with ring; no `zip`, no
  system-proxy). It installs ring as rustls' process default only when no
  default exists — still ring, so ADR 0014 holds (`cargo tree -i aws-lc-rs`
  stays empty).
- `tauri` features `tray-icon` + `image-png`.
- `windows-sys` 0.61.2 (nail-mapi: pipes, process start, the MAPI
  structures; already in the lock), `windows-registry` 0.6.1 moved to the
  workspace.
- nail-app also gets `axum` / `futures-util` (dev endpoint) and
  `tokio-tungstenite` (tests); all were in the lock already. The lock is
  final for the parallel stage: packages add Win32 features freely but ask
  before new crates.

### Served now

The core serves every M4b request (`nail-core/src/desktop.rs`): settings
and the first run from `app_settings` (`desktop.updates`,
`desktop.settings`, `desktop.first-run`), the rest through the host, plus
`submit_mapi`. Without a host (`nail serve`, the e2e harness) desktop
requests answer `unsupported`. The shell does not pass a host until WP113.
Tests: `nail-core/tests/m4b.rs` (a fake host: policy wins, state checks,
settings applied, host changes broadcast, default apps, first run; MAPI
draft with a file deleted after the call, nothing queued, error codes),
nail-api unit tests, `nail-remote-auth` (all 12 desktop-only), nail-cli
(token header), the UI mock (`features/system/mock.test.ts`) and the
launch test (`ComposeDraft` opens the composer).

### Collision-free slots

| Package | Owns |
| --- | --- |
| WP111 | `crates/nail-mapi/` (cdylib `nail_mapi.dll` → shipped as `NailMapi.dll`, rlib for tests, bin `nail-mapi-harness`; `#![allow(unsafe_code)]` scoped to the FFI module) |
| WP112 | `app/src-tauri/windows/` (moved from `installer/`: `nsis/` template + hooks + strings, `wix/`), `app/src-tauri/tests/installer.rs`, `packaging/` (new: winget, update manifests), `scripts/`, `.github/workflows/`; bundle-config changes for `tauri.conf.json` go to WP115 as apiRequests (or into the overlay `scripts/build-installer.mjs` passes) |
| WP113 | the rest of `app/src-tauri` (`src/`, `Cargo.toml`, `tauri.conf.json` outside `bundle`, `capabilities/`), `locales/*/core-desktop.ftl` below its "Desktop shell" heading |
| WP114 | `ui/src/features/system/` (registered; mock routed; `data.features.system`), `locales/*/system.ftl` |

`crates/nail-config/src/widgets/system.rs` registers the future widgets
`update-settings`, `desktop-settings`, `default-apps` (any options) for
WP114; WP115 places them in `defaults/layout.kdl`. Contract changes
(nail-api, nail-core, shared UI files) go to WP115 as apiRequests.

## Consequences

- The integrator wires the host into the shell's `ServiceOptions`, the
  pipe server into `submit_mapi`, the UI widgets into the layout, and the
  Playwright / desktop scenarios of plan WP115.
- The x86 and ARM64 DLL / app builds need the `i686-pc-windows-msvc` /
  `aarch64-pc-windows-msvc` targets and MSVC tools; only x64 is installed
  locally — CI (WP112) builds the others.
- Programs that expect `MAPISendMail` without UI to send silently get the
  composer instead (documented in the user docs by WP114 / WP115).
- German texts of the new catalogs come later (empty `de` files).
