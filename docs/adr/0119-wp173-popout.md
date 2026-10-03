# ADR 0119: pop-out windows in the desktop app (M5c WP173)

Status: accepted · 2026-10-03 · M5c WP173

## Context

ADR 0116 fixed the contract: the core keeps the list of open pop-outs and
drives the shell through `DesktopHost::open_pop_out` / `close_pop_out` /
`focus_pop_out`; the shell reports what the user does as
`DesktopChange::PopOut`; a pop-out webview loads the UI at `#/window/<id>`
(`App.tsx` renders `features/popout/PopOutHost`). WP173 implements the
shell's side (`app/src-tauri`) and the UI's pop-out feature
(`ui/src/features/popout`).

## Decisions

### Shell (`app/src-tauri/src/popout.rs`)

- **One Tauri `WebviewWindow` per pop-out**, label `popout-<id>`, URL
  `index.html#/window/<id>`, sharing the main window's WebView2 data folder
  (seeded runs keep it in their temporary directory). It talks to the same
  in-process core through the same `nail_call` command. Core events are
  emitted to every webview (`AppHandle::emit`), so each pop-out keeps its
  own UI store current exactly like the main window; Tauri answers each
  `nail_call` to the webview that sent it. The capability `default` now
  lists `popout-*`.
- **`PopOutManager`** holds everything that does not need Tauri, behind the
  trait `PopOutWindows` (`TauriPopOuts` in the app, a fake in tests):
  placement, the restored rectangle of maximized windows, move reports,
  focus / close reports, closing everything with the main window.
- **Geometry is in physical screen pixels** (the contract says "screen
  coordinates"): outer position + inner size, which is what Windows places
  windows by. A pop-out reopens exactly where it was on any monitor, whatever
  its scale factor; logical pixels would be converted with the scale factor
  of whichever monitor Tauri picks while the window is created, which moves
  windows on mixed-DPI setups.
- **Multi-monitor safe placement** (`place`): the remembered geometry is
  used when its title strip (32 CSS px high, at least `WINDOW_MIN_VISIBLE`
  wide) lies on a monitor's work area — shrunk to that work area. Otherwise
  (monitor unplugged, resolution changed, nothing remembered) the window
  opens at the remembered or default size (`DEFAULT_WINDOW_SPEC.size`,
  scaled by the main window's scale factor) centred over the main window on
  its monitor, or on the primary monitor when the main window is hidden.
  A window at the same place as an open pop-out cascades by
  `WINDOW_CASCADE_OFFSET` (two composers never cover each other exactly).
  The window is built hidden, placed, maximized if remembered so, then
  shown and focused (no flash at a default place).
- **Reports** (`DesktopChange::PopOut`): `moved` when a move / resize /
  maximize / restore / scale change ends — 300 ms of quiet, or at most every
  500 ms during a long drag (`ReportClock`) — with the restored rectangle
  and `maximized`; minimized windows report nothing (the restored geometry
  stays). Windows Snap is the OS's business: `snap` is always `null` for
  pop-outs. `focused` when the OS window gets the focus. `closed` when the
  OS window is destroyed by the user (close button, Alt+F4). Closes the core
  asked for (`ReturnPopOut` / `ClosePopOut`) are not reported again (the
  manager forgets the window before destroying it; the core would ignore
  them anyway). After opening, the real placement is reported once, so the
  kind remembers where it opened.
- **Taskbar titles**: the first title is `PopOutWindow.title` (the UI's
  localized window title) or the core's `window-<kind>` text. Afterwards the
  OS window follows the webview's `document.title`
  (`on_document_title_changed`, cleaned by `taskbar_title`: trimmed, no
  control characters, ≤ 256 characters). The pop-out page sets the document
  title to the window's title or to what the content names it with
  `usePopOutTitle` (e.g. the composer's subject).
- **The main window closing closes every pop-out** — when it hides to the
  tray as well as when Nail quits (no orphaned taskbar entries without a
  main window). They are reported `closed`, so their kinds stay
  `popped_out` and open as OS windows again next time. Drafts are not at
  risk: a composer saves its draft before it pops out, and its own autosave
  keeps running in the pop-out.
- `focus_pop_out` of a window that is gone reports it `closed` and answers
  `conflict` (`pop-out-issue-not-open`). An open failure answers `internal`
  with `popout-open-failed`; without the app (before `bind_app`)
  `unavailable`.
- Seeded runs' test control (`nail_test_shell`) gained `pop-outs`: the
  pop-out OS windows as `[{label, title, visible}]`, for the desktop smoke
  test.

### UI (`ui/src/features/popout`)

- **Availability** without a new request: `usePopOutsAvailable` sends
  `FocusPopOut` for the id `popout-probe` once — `conflict` (not open)
  means a desktop host exists; `unsupported` (`nail serve`) and `forbidden`
  (remote sessions) mean the browser UI, which keeps every window in the
  page and never shows the button. No side effects.
- **`PopOutButton`** (`window.pop-out`, icon `external-link`, "Open in new
  window") for windows whose resolved spec has `pop_out` (confirmations and
  `move` never). It runs the kind's **preparer**
  (`registerPopOutPreparer(kind, fn)`: save what only this page holds,
  return the context, ≤ 16 KiB), sends `PopOutWindow {window, title,
  context}` and closes the window in the page. Failures keep the window in
  the page; the button shows the reason in its label (`popout-open-error`).
- **`PopOutHost`** (the pop-out page): a header with the title and "Return
  to main window" (`ReturnPopOut`), the content in its
  `WindowInstanceContext`. The window is opened in the page's own UI store,
  so readers of the focused window find it; when the content closes it
  (Send, Cancel, a dialog's `close`), the OS window closes (`ClosePopOut`).
  `usePopOutContext()` returns the context it was opened with.
- **The main page follows `PopOutChanged`** (feature event handler):
  `opened` closes the window in the page, `returned` opens it there again;
  moves, focus and closes leave the page alone. Pop-out pages ignore it.
- **`PopOutRouter`** (effect, main page, desktop only): a window opened in
  the page that is popped out already is raised instead (`FocusPopOut`); a
  window whose kind was last shown popped out (`WindowState.popped_out`)
  pops out again. A window that fails stays in the page.
- Actions `popout.open` (the focused window or the id given; palette
  availability `desktop`) and `popout.return` (the pop-out page's window or
  the id given).
- Styles: `parts.ts` `POPOUT_BASE_CSS` (structure on the token schema: the
  page fills its OS window, the content scrolls).

## Desktop smoke scenario (for WP174)

A seeded desktop run (`npm run build:desktop`, the WP115 harness with
`nail_test_shell`):

1. Open Settings (or Filter rules) in the main window; its title bar shows
   "Open in new window" (`[data-part="window.pop-out"]`). Click it.
2. `nail_test_shell("pop-outs")` lists `popout-settings` with title
   "Settings", visible; the main page no longer shows the in-page window;
   `ListPopOuts` lists `settings`.
3. Attach to the new webview (its URL ends with `#/window/settings`): it
   shows `[data-part="pop-out"][data-window="settings"]` with the settings
   page, and changing a setting there reaches the main window (core event).
4. Move / resize the OS window (or drive it through the test control if
   the harness cannot); within a second `GetWindowStates` has
   `settings.pop_out` with the new rectangle and `popped_out: true`.
5. Close the OS window with its close button: `pop-outs` is empty,
   `PopOutChanged closed`. Open Settings again in the main window: it opens
   as an OS window at the remembered place (`popped_out` routing).
6. Click "Return to main window" in the pop-out: the OS window closes and
   Settings shows in the main page; `popped_out` is `false`.
7. Open two composers (`composer:<draft>`) and pop both out: two taskbar
   entries titled by the subjects (after WP172 calls `usePopOutTitle`),
   the second cascaded by 24 px; typing in one and closing it keeps the
   draft (Drafts folder).
8. Close the main window (close to tray off): every pop-out closes and the
   app exits; with close to tray on, the pop-outs close and the tray icon
   stays.

## Consequences

- WP171's title bar renders `<PopOutButton window={id} />` beside the close
  button; WP172's composer registers a preparer (save the draft, return
  `{draft}`) and calls `usePopOutTitle(subject)`; WP174 appends
  `POPOUT_BASE_CSS` in `theme/compile.ts`.
- Pop-outs open at the default 720 × 520 CSS px when nothing is remembered,
  not at the kind's spec size: `PopOutWindow` / `PopOutOpen` carry no size
  (a follow-up can add one).
- Remote browsers never see pop-outs (ADR 0116); their windows stay in the
  page.
