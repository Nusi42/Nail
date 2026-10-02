# ADR 0092: M4b integration — wiring, fixes found by the end-to-end scenarios, desktop smoke hooks

Status: accepted · 2026-10-02 · M4b WP115

## Context

WP111–WP114 built the Simple MAPI DLL (ADR 0088), the installers and the
update pipeline (ADR 0089), the desktop shell (ADR 0090) and the desktop UI
(ADR 0091) in parallel against the contract (ADR 0087). WP115 wires them
together and adds the plan's scenarios: the MAPI harness sends a file and
the composer opens with it attached, the tray's "New message", start hidden,
the updater finding a newer version on a loopback server and installing on
restart, an MSI install with `DISABLEUPDATES=1` hiding the updater, and
`nail plugin dev` against the desktop app.

## Decisions

### Wiring

- **Composer and launch.** `LaunchRequest::ComposeDraft` goes through
  `openProgramDraft` (it remembers the program, then opens the draft as
  before); the composer shows `ProgramDraftNotice` above its fields (a
  `role="note"` with Dismiss; nothing for ordinary drafts).
- **Layout, theme, widgets.** Settings gets the tabs Updates, Tray and
  startup, Default apps; the actions `system.about` (key **F1**),
  `system.check-updates`, `system.first-run`, `system.default-apps` are
  declared. The layout has no application menu, so these are reached
  through F1, the settings tabs and the first-run screen. The dialogs
  `about-nail` / `first-run` stay feature dialogs (no layout windows).
  `SYSTEM_BASE_CSS` is part of the base CSS; `defaults/theme.kdl` styles
  every new part. `update-settings` / `desktop-settings` / `default-apps`
  have typed options (`show-installation`, `show-dev-endpoint`,
  `show-mapi`; booleans, default on) instead of the contract's future
  widgets.
- **One attachment limit.** `nail_api::compose::MAX_ATTACHMENT_BYTES`
  (25 MiB) is the core's `UploadAttachment` limit, `submit_mapi`'s check and
  `nail_mapi::staging::MAX_STAGED_FILE_BYTES` (no more private copy).
- **Start with Windows and the MSI.** The MSI's `AUTOSTART=1` writes the
  machine's Run key. The shell counts it (`start_with_windows` is on when
  the user's or the machine's entry exists), never writes a second user
  entry while the machine's exists (Nail would start twice; an old user
  entry is removed), and switching off removes only the user's entry — the
  machine's is the administrator's and keeps the switch on.

### Fixes found by the scenarios

- **The pipe server lost answers.** The shell disconnected each pipe
  instance right after writing the answer. tokio's `flush` does nothing on
  a named pipe, and `DisconnectNamedPipe` throws away what the client has
  not read yet, so `MAPISendMail` could report `MAPI_E_FAILURE` although the
  core had already saved the draft and opened the composer (it depended on
  timing). The server now waits until the client closes its end (at most
  10 s) before disconnecting (`mapi_server::wait_for_client_close`).
  Tests: a client that reads 500 ms late still gets its answer; the
  cross-package test `app/src-tauri/tests/mapi_wired.rs` drives the shell's
  server with the DLL's own client (`nail_mapi::forward` + `PipeTransport`:
  private copies, same-user check) and the real core.
- **A stale update answer overwrote a newer state.** The UI wrote the
  `CheckForUpdates` / `DownloadUpdate` answer into the query cache. With a
  fast server the host's `UpdateStatusChanged` events (available →
  downloading → ready) arrive before the answer, which still says
  "checking", so the panel fell back to "Checking for updates…". The UI now
  refetches `GetUpdateStatus` after a command instead of writing the answer
  (also for the `system.check-updates` action). Regression test in
  `system.test.tsx`.
- **The MSI smoke never saw an MSI install.** `msi-smoke.ps1` started the
  installed app with a `portable` marker, so the app reported itself as
  portable and the `DISABLEUPDATES` path was untested. It now passes
  `--data-dir` / `--config-dir` in the temporary directory instead and
  checks the shell's installation log line (`kind="msi"`,
  `updates_disabled=Some(MsiManaged)`).

### Desktop smoke scenarios and their hooks

`e2e/tests/desktop-m4b.spec.ts` drives the real `nail-app.exe --seed demo`
over WebView2:

- **Simple MAPI:** the app runs with `NAIL_MAPI_PIPE` and a config dir whose
  `accounts.kdl` has an identity (a switched-off account); `nail-mapi-harness
  send --dll nail_mapi.dll --no-ui --attach …` exits 0, the composer shows
  recipients, subject, text, the attachment and the program's note; the
  program's file is untouched.
- **Tray and start hidden:** Playwright cannot click the OS tray menu.
  Seeded runs (never a real profile) register one more Tauri command,
  `nail_test_shell`, that runs the tray menu's own handler for an item id
  (`tray-new-message`, `tray-quit`, …) or answers whether the main window
  is visible. `--autostart` (start hidden is on by default) keeps the window
  invisible until the tray's New message brings it up with an empty composer.
- **Updater:** a loopback HTTP server in the test serves
  `stable/latest.json` and a package signed with a throwaway minisign key
  (`e2e/support/updates.ts`, same format as `tauri signer`); the app gets
  `NAIL_UPDATE_ENDPOINT` / `NAIL_UPDATE_PUBKEY`. The package is
  `nail-fake-installer.exe` (new bin in `nail-testkit`), which records its
  command line instead of installing. "Check now" → downloaded and checked
  in the background → "Restart to install" → Nail runs the "installer" with
  `/P … /UPDATE` and quits.
- **MSI policy:** `HKLM\Software\Nail\Install` facts (`Installer=msi`,
  `InstallDir=<exe dir>`, `DisableUpdates=1`) staged below a throw-away
  HKCU key named by `NAIL_REGISTRY_ROOT` (removed afterwards); the Updates
  tab shows "Your organization installs updates for Nail." without update
  buttons, even with an update source. The real elevated silent MSI install
  runs in CI only (`msi-smoke.ps1`): it needs administrator rights and
  writes HKLM, which this test must not do on a developer's machine.
- **`nail plugin dev`:** the endpoint switched on in Tray and startup writes
  `ui-endpoint.json` with a token; `nail plugin dev <folder> --data-dir`
  loads the plug-in, which the plug-in manager lists as loaded from the
  development folder; switched off, the file is gone.
- **First run:** a fresh profile gets the welcome screen once per session;
  the existing desktop smoke test skips it (a `mailto:` start opens the
  composer instead). Shift+F10 opens the row's context menu in WebView2.

`npm run build:desktop` also builds `nail_mapi.dll`, `nail-mapi-harness`
and `nail-fake-installer` into the desktop target directory.

### CI

A `mapi` job runs the DLL's tests as x86 under WOW64, builds it for x64,
x86 and ARM64, and checks every export of the x64 and x86 DLLs with the
harness (an ARM64 DLL cannot be loaded on the x64 runner).

## Consequences

- Not verified locally: the elevated MSI install / upgrade, the ARM64 and
  offline-WebView2 bundles, Authenticode signing, which registry values the
  Windows MAPI stub reads in which order (DLLPathEx / DLLPath, HKCU / HKLM),
  and the ARM64 DLL link (no MSVC ARM64 tools here) — all CI or maintainer
  steps; the workflows have not run on GitHub yet.
- The OS notification look is not checked automatically (WebView2's CDP
  does not see Windows toasts); the notification path itself is unchanged
  since M2c.
- Follow-ups: an ARM64X `NailMapi.dll` for emulated x64 programs on ARM64;
  a low-integrity pipe label if sandboxed (AppContainer) callers matter;
  policies are read once at start; a squatted pipe name is only logged;
  German texts for `core-desktop.ftl` / `system.ftl`; the first-run screen
  closed with × comes back (by design); the import step waits for M4c's
  `import.open`.
