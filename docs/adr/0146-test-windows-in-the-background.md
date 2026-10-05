# ADR 0146: Desktop test runs keep their windows in the background

Status: accepted · 2026-10-05 · finding from desktop test runs (amends ADRs 0090, 0092, 0093, 0119)

## Context

While the desktop e2e suites ran (`desktop*.spec.ts`: `startDesktop` →
`nail-app --seed demo`, four Playwright workers), the test apps took the
mouse although their windows were not visible. The shell opened ordinary
windows on the developer's desktop:

- the main window was created shown and focused (`SW_SHOW`, activating),
  1280 × 800 at the default position, over whatever the developer was doing;
  during WebView2's start it is unpainted (looks transparent) but takes
  clicks;
- pop-outs (`popout.rs`) and "bring the main window up"
  (`shell::show_main_window`: tray, Simple MAPI, second start) called
  `set_focus()`. tao implements it as `SetForegroundWindow` and, when
  Windows refuses, **injects an Alt key press with `SendInput`** to take the
  foreground — into the developer's input stream;
- every window sat on the taskbar and in Alt+Tab; OS notifications, file
  dialogs and the Settings app could open too.

Off-screen placement was considered: it changes the window's monitor (and
DPI scale), Windows has no way to maximize without activation, and the
pop-out placement clamps to monitors, so geometry would need translating.

## Decision

A shell test variable, `NAIL_TEST_BACKGROUND_WINDOWS=1`, honoured only by
`test-overrides` builds (`shell::test_env`, like `NAIL_REGISTRY_ROOT`; ADR
0093) and set by `startDesktop` by default (`0` to watch the windows).
`app/src-tauri/src/background.rs` decides the mode once at start. In it
every window the shell creates (main, pop-outs, hidden print windows) is:

- **cloaked** with `DwmSetWindowAttribute(DWMWA_CLOAK)` before it is first
  shown: Windows still counts it as shown (`IsWindowVisible`, DWM keeps
  composing it, WebView2 keeps rendering, `window-visible` / `pop-outs`
  answer as before), but it is never drawn and the mouse reaches the windows
  below (`WindowFromPoint` skips it). Positions and sizes stay real, so the
  pop-out placement and remembered geometry are untouched;
- **never activated**: `focusable(false)` (`WS_EX_NOACTIVATE`),
  `focused(false)`, shown with `ShowWindow(SW_SHOWNOACTIVATE)` before
  Tauri's own `show()` (which then finds it shown and keeps tao's state
  right for a later hide), no `set_focus` / `unminimize`
  (`background::bring_up`); a pop-out asked to open maximized is only
  remembered (reported as maximized) — Windows cannot maximize without
  activating;
- **off the taskbar** (`skip_taskbar`).

The shell also shows no OS notifications, opens no file dialog (`PickPath`
answers cancelled), does not open the Settings app and shows no
start-refusal message box in this mode. The tray icon stays (the tests
drive its handlers through the test control).

The seeded-only test control gets `windows`: per webview window `visible`,
`cloaked`, `activatable`, `foreground`, `inBackground`, plus whether the
foreground window belongs to the app. `e2e/support/desktop.ts`
`expectWindowsInBackground` checks it in `connectDesktop` and in each
desktop spec (main window, shown after "start hidden", pop-outs, after the
print window).

## Consequences

- Release and installer builds are unchanged (no `test-overrides`; the
  variable is logged and ignored).
- tao's event-target window (14 × 14, `WS_EX_LAYERED | WS_EX_TRANSPARENT |
  WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`) is visible to `EnumWindows` in every
  tao app; it is never drawn and click-through, so it is left alone.
- The pages run without OS focus; Playwright's focus emulation keeps the
  pages' focus behaviour, as before for unfocused windows.
- `scripts/installer-smoke.ps1` and `scripts/msi-smoke.ps1` start an
  installed (release) Nail and show its real window for a few seconds: they
  are release gates run by hand or in CI, not part of `npm test` /
  `npm run e2e`. The web e2e, visual and perf suites use headless Chromium.
