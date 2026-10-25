# 0202 — Platforms beyond Windows: Linux build, platform report, OS-aware texts

Status: accepted (2026-10-25)

## Context

Nail is Windows first, but the workspace already gates every Windows crate
(`[target.'cfg(windows)'.dependencies]`) and has `cfg(not(windows))`
fallbacks for the Win32 / WebView2 / SSPI code. Since ADR 0201 nothing
compiled those fallbacks, and someone already runs Nail on macOS. A first
Linux build (Ubuntu 24.04 under WSL2) showed:

- the workspace builds unchanged, but `cargo clippy -D warnings` failed
  (unfulfilled `#[expect(unsafe_code)]` on modules whose unsafe code is
  Windows-only, Windows-only helpers unused elsewhere, a cast), and two tests
  assumed Windows path separators — one of them hid a real bug: an editor
  given as a Windows path (`"C:\…\notepad++.exe"`) was not recognised;
- the desktop shell answered file dialogs and Save as PDF with `internal`
  errors off Windows, so the UI's existing fallbacks (a typed path, a browser
  download), which react to `unsupported`, never ran;
- the UI offered Windows-only settings everywhere (start with Windows,
  default apps, Simple MAPI, the Windows certificate store, Windows sign-in
  for Exchange), and texts named Windows parts on every system ("Follow
  Windows", "the Windows Credential Manager").

## Decision

- **Lint and test on Linux.** `cfg(not(windows))` code is held to the same
  zero-warnings rule: Windows-only `expect`s are `cfg_attr(windows, …)`,
  Windows-only helpers and imports are `cfg(windows)`. Tests that use paths
  build them with the platform's separator.
- **Trait defaults instead of stubs.** The desktop shell implements
  `DesktopHost::pick_path` and `print_to_pdf` only on Windows; elsewhere the
  trait's defaults answer `unsupported`, and the UI falls back as in web
  mode.
- **Platform report.** `nail_api::platform` defines `Os` (`windows`, `macos`,
  `linux`, `other`) and `PlatformFeature` (`windows-sign-in`,
  `windows-certificate-store` in the core; `start-with-system`,
  `default-apps`, `simple-mapi` in the desktop app). Each feature is decided
  at compile time from the same `cfg` that builds it
  (`PlatformFeature::IN_CORE` / `IN_DESKTOP_APP`). `GetConfig` answers with
  `platform: PlatformInfo`: the core's features, plus the desktop app's when
  a desktop host is attached; never the desktop app's for a remote session.
  No new request: every UI loads the config at start.
- **The UI hides what the system lacks.** `usePlatformFeature` /
  `hasFeature` (`ui/src/state/platform.ts`); a settings widget declares
  `requires` (the whole card) and `platformLabels` (some of its labels) in
  its `SettingsWidgetInfo`, so the settings page and its search both leave
  them out. Gated: the startup part of the desktop settings and the
  first-run step, the default-apps card, first-run step and action, the
  Simple MAPI rows, the Windows store key tool, the Windows sign-in
  choices for Exchange (a Windows sign-in already configured stays visible)
  and the "look again with my Windows sign-in" answer to an Autodiscover
  sign-in prompt.
- **OS-aware texts.** Every localized message gets `$os` (the core's `Os`,
  `OS_MESSAGE_ARG`), in the core's renderer and in the UI's localizer alike;
  a message's own arguments win. Catalogs select on it
  (`{ $os -> [windows] … [macos] … *[other] … }`); shared parts are
  messages in `common.ftl` (`common-keychain`, `common-keychain-short`,
  `common-keychain-name`, `common-system-name`). A select covers only the
  part that differs, never the whole sentence. Until the config arrives the
  UI's system is unknown: the selects show their default variant, without
  a warning.
- **Bundles.** `tauri.linux.conf.json` (deb, AppImage) and
  `tauri.macos.conf.json` (app, dmg) replace the NSIS target there; the
  installer scripts stay Windows-only.
- **Linux checks run in WSL2** (`docs/dev-setup.md`, "Linux (WSL2)").

## Consequences

- Off Windows there is still no native file dialog and no PDF printing (the
  UI's fallbacks apply), no LAN address list for the remote UI, and S/MIME
  chains have no system trust anchors. Each is a follow-up with its own
  decision (a portable dialog crate, WebKit printing, the platform's root
  store).
- A new Windows-only setting needs a `PlatformFeature` and a `requires` /
  `platformLabels` entry; a new text naming a Windows part selects on `$os`.
- The Playwright suite still assumes Windows wording in older scenarios;
  `e2e/tests/platform.spec.ts` checks the platform behaviour on any system.
