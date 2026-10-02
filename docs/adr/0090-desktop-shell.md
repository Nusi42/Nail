# ADR 0090 (WP113): Desktop shell — host, tray, startup, updater, Simple MAPI pipe, dev endpoint

Status: accepted · 2026-10-02 · M4b WP113 (the integrator renumbers)

## Context

ADR 0087 fixed the M4b contract: the core serves the desktop requests
through a `DesktopHost` the shell passes as `ServiceOptions::desktop`, and
`Service::submit_mapi` turns a Simple MAPI message into a saved draft.
WP113 builds the shell side in `app/src-tauri`: tray, close to tray and
start hidden, single instance, the updater client, "Choose Nail in Default
apps", the Simple MAPI pipe server and the `nail plugin dev` endpoint.

## Decisions

### Structure (`app/src-tauri/src`)

| Module | What |
| --- | --- |
| `registry.rs` | `Registry` trait over the Windows registry: `WindowsRegistry` (real; with `NAIL_REGISTRY_ROOT` both hives map below `HKCU\<root>\HKCU|HKLM`) and `MemoryRegistry` |
| `install.rs` | `InstallationInfo` from the installers' values + policy; `UpdateSource` (update endpoints + key) |
| `default_apps.rs` | `DefaultAppsStatus` from `RegisteredApplications`, Capabilities, `UserChoice(Latest)`, `Software\Clients\Mail` |
| `startup.rs` | the Run key (`"<exe>" --autostart`), "start hidden" rule |
| `updater.rs` | the update state machine over an `UpdateBackend` trait (scripted in tests) |
| `updater_tauri.rs` | the real backend: tauri-plugin-updater; error → `UpdateFailure` mapping |
| `dev_endpoint.rs` | loopback WebSocket for `nail plugin dev` |
| `mapi_server.rs` | the Simple MAPI named-pipe server |
| `win.rs` | user SID, SDDL security descriptors, random tokens (BCryptGenRandom), `ms-settings:` via `ShellExecuteW` |
| `host.rs` | `ShellHost: DesktopHost` (the core's view of all the above) |
| `tray.rs` / `shell.rs` | tray actions and labels; Tauri wiring (tray icon, window show / hide, second start, MAPI handler) |

No new crates: everything uses dependencies the contract added (tauri
`tray-icon`, tauri-plugin-updater, axum, futures-util, windows-registry) or
new features of the existing `windows` crate (`Win32_Security`,
`Win32_Security_Authorization`, `Win32_System_Threading`, `Win32_UI_Shell`)
and tokio's `test-util` in dev-dependencies. `Cargo.lock` is unchanged by
WP113. The URI opener is `ShellExecuteW` limited to `ms-settings:` URIs
(the `open` crate would have added a lock entry for nail-app; the Tauri
opener plug-in is not in the lock).

### Installation facts

- The copy that runs is matched by directory: the hive (`HKCU` first,
  then `HKLM`) whose `Software\Nail\Install\InstallDir` is the executable's
  folder decides `kind` (`Installer`, default `nsis`), `scope` (`Scope`,
  default from the hive), `mapi_installed` (`Mapi`). No match =
  `development` (a dev build on a machine with Nail installed stays one);
  the `portable` marker wins.
- `updates_disabled`, first match: policy `DisableUpdates` (`HKLM` before
  `HKCU`) → `policy`; MSI with `DisableUpdates` missing or non-zero →
  `msi-managed`; portable; development (unless the test override below);
  no endpoint or key → `not-configured`. Values may be `REG_DWORD` or
  `REG_SZ` (`1` / `true` / `yes`).
- `forced_channel` = policy `UpdateChannel` (`HKLM` before `HKCU`).
- Facts are read once at start (policy changes apply on restart).

### Updater

- Source: `plugins.updater` of `tauri.conf.json` (`pubkey`, `endpoints`;
  committed empty = not configured; the release overlay of
  `scripts/build-installer.mjs` fills it). An endpoint may contain
  `{{channel}}`, or end in `<channel>/latest.json` (what WP112's overlay
  writes), which is switched to the effective channel.
- Test override: `NAIL_UPDATE_ENDPOINT` + `NAIL_UPDATE_PUBKEY`, accepted
  only for a loopback URL; it also enables a development build (the policy
  still wins). Plain http works in debug builds; release builds need the
  overlay's `dangerousInsecureTransportProtocol` (WP112's
  `--insecure-loopback-updates`).
- States as in ADR 0087. A check runs 20 s after start and then every
  24 h while `auto_check` is on (and on request); a found release downloads
  in the background when `auto_download` is on; progress is reported every
  1 % (256 KiB without a size). A later check that finds the release
  already downloaded stays `ready-to-install`. A channel change drops what
  the old channel found and checks again. Policy's channel is used even if
  the stored setting differs (the core passes the stored settings).
- Install: `InstallUpdate` answers first, then (after 400 ms) the plug-in
  runs the installer in passive mode and Nail restarts. **Quitting Nail with
  a downloaded update installs it without restarting** (the "install on
  restart" of the plan: `RunEvent::Exit`). Downloads stay in memory.
- Failures: minisign / base64 / signed-version errors → `signature`;
  manifest / target / semver / URL / arch errors → `manifest`; the rest by
  phase → `network` (check), `download`, `install`.

### Tray, close to tray, start hidden, single instance

- Tray icon (the window icon) with tooltip and menu *Open Nail*, *New
  message* (an empty `LaunchRequest::Compose`), *Check mail* (`SyncNow`
  for all accounts), separator, *Quit Nail*; texts `tray-*` in
  core-desktop.ftl rendered in the core's language. Left click opens the
  window.
- Close to tray: closing the main window hides it when the setting is on,
  a tray icon exists and the user did not pick *Quit*.
- Start hidden: started with `--autostart`, `start_hidden` on and a tray
  icon → the window is created invisible; otherwise `--autostart`
  minimizes as before.
- The stored desktop settings are applied once at start
  (`DesktopHost::apply_desktop_settings`: close to tray, Run key refreshed
  with the current exe path or removed, dev endpoint).
- Second start (single-instance plug-in; not for seeded runs): its
  arguments are parsed like the first start's (`AppArgs`, so `--` and
  flags work), launch requests go to the core, and the window comes up —
  except a bare second `--autostart`. Simple MAPI requests do not come as
  arguments: the DLL uses the pipe (and starts `Nail.exe` when nothing
  answers).
- Default apps: `OpenDefaultAppsSettings` opens the scope's
  `ms-settings:defaultapps?registeredApp…=Nail`; when the main window gets
  the focus again the shell reports `DesktopChange::DefaultApps` once.

### Simple MAPI pipe

- `\\.\pipe\nail-mapi-<SID>` (`NAIL_MAPI_PIPE` overrides it), created with
  `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS` and the
  protected DACL `D:P(A;;GA;;;<user SID>)` (no other account, not even
  administrators). If another process already owns the name the server
  does not start (logged) rather than share it.
- One frame in, one out (ADR 0087 frames; 30 s to send the request; other
  versions or malformed frames → `Failure` without reaching the core). The
  handler calls `Service::submit_mapi`; on success the main window comes
  up (the UI opens the composer from `ComposeDraft`).
- Seeded runs serve a pipe only with `NAIL_MAPI_PIPE` set, so a demo or an
  e2e run never takes the real Nail's pipe.

### `nail plugin dev` endpoint

- Started / stopped by `plugin_dev_endpoint`. Binds `127.0.0.1:0`
  (ADR 0057), writes `ui-endpoint.json` with a fresh 256-bit token, serves
  `/api/ws`: the upgrade needs `Authorization: Bearer <token>` (constant
  time) and no `Origin` header (browsers always send one). Only
  `LoadDevPlugin` is served (`forbidden` otherwise), only
  `PluginLogLine` / `PluginsChanged` are forwarded. Stopping removes the
  file.

### Seeded runs and tests

- Seeded runs (`--seed`) use the memory registry unless
  `NAIL_REGISTRY_ROOT` is set: a demo never writes the real Run key and is
  a development build (updates disabled unless the test override is set).
- Unit tests touch the real registry only below
  `HKCU\Software\NailTests\<name>-<pid>` and remove it.

## Consequences

- WP115 can drive the desktop scenarios with `NAIL_REGISTRY_ROOT` (stage
  installer facts / policies), `NAIL_UPDATE_ENDPOINT` /
  `NAIL_UPDATE_PUBKEY` (loopback update server; debug build or the
  insecure-loopback overlay), `NAIL_MAPI_PIPE` (the harness's pipe).
- Policy changes need a restart; the MAPI pipe is not restarted if it
  fails at start.
- Tests: `cargo test -p nail-app` (registry incl. the real one below a
  test root, installation facts, default apps, Run key, updater machine
  with a scripted backend and a paused clock, error mapping, the pipe
  server incl. a real pipe, first-instance and ACL checks, the dev
  endpoint with a real WebSocket client, the host through the core's
  requests, tray labels in en / de, second-start parsing).
