# ADR 0116: M5c contract — the window model, remembered window state, pop-out windows, slots

Status: accepted · 2026-10-03 · M5c WP170

## Context

Wave M5c (plan section 10) turns Nail's secondary windows into real
windows, as the approved UX proposal's "Dialogs" section shows
(a published page, not linked here): drag by the title bar,
8 resize handles, double-click to maximize, snap to halves, keyboard move /
resize, remembered geometry, non-modal by default, pop-out on the desktop.
Until now every secondary window was a modal dialog (`layout/WindowLayer.tsx`,
`UiState.windows` = names, topmost last), and the composer existed once.

Four packages build in parallel against this ADR: WP171 (the window manager
in the UI), WP172 (non-modal windows and several composers), WP173 (pop-out
into OS windows in the desktop app) and WP175 (the Gmail / Exchange engine
tests and the engine code they exercise). WP174 integrates.

Binding scope decisions (plan section 10): every structure stays
expressible in `layout.kdl`; components emit `data-part`s; the window state
lives in a UI-state file in the config directory, not in `layout.kdl`
(ADR 0109 had suggested `window.*` keys in `ui-state.json`; see below).

## Decisions

### Window ids (`nail_api::windows`, `ui/src/state/windows.ts`)

- A window **kind** is a layout window name (`window "<kind>"`) or a feature
  dialog name: kebab-case, at most 64 characters (`is_window_kind`).
- A window **id** is the kind, or `<kind>:<instance>` for one of several
  windows of a kind; the instance key is `[A-Za-z0-9_-]{1,64}`
  (`WindowId::parse`, `window_id`, `window_kind`; `newWindowInstance` in the
  UI picks `w1`, `w2`, … for windows without a natural key — composers use
  their draft id).
- `UiState.windows` keeps window ids in z-order (topmost = focused last),
  `openWindow(id)` opens or raises, `focusWindow(id)` raises, `closeWindow`
  is unchanged. Z-order and focus are session state and never remembered.
- The window layer resolves the layout window / dialog and the title
  (`window-<kind>`) by kind; every window's content is rendered inside
  `WindowInstanceContext` (`useWindowInstance()` → `{id, kind, instance}`).

### The window model

`WindowSpec { modal, resizable, min_size, size, pop_out, instances }`
(`WindowSize` in CSS pixels; `WindowInstances` `single` / `multiple`).
It is resolved per kind from three layers, later winning field by field:

1. `DEFAULT_WINDOW_SPEC` — non-modal, resizable, 720 × 520 (min 360 × 240),
   can pop out, single; or `CONFIRMATION_WINDOW_SPEC` — modal, fixed
   440 × 220, never popped out. **Only confirmations are modal.**
2. The feature's registration, `Feature.windows: Record<kind,
   WindowRegistration>` (any spec fields; `{ confirmation: true }` starts
   from the confirmation spec), merged as `appRegistry.windows` (duplicates
   refused like dialogs).
3. The layout: properties on the `window` node, `size="760x600"`,
   `min-size="480x360"`, `modal=`, `resizable=`, `pop-out=`,
   `instances="single|multiple"` (`WindowFrame`;
   `ResolvedLayout.window_frames`, only windows that set one). Across a
   window's breakpoints the properties merge; a sparse user file's
   properties override the default's field by field, so a pre-M5c file that
   redefines a window keeps the default sizes and `instances`. Checks:
   edges `MIN_WINDOW_EDGE` (120) … `MAX_WINDOW_EDGE` (16,384), `size` ≥
   `min-size` (`config-window-size-invalid`, `config-window-frame-*` in
   `core.ftl`). The merge dialog renders and takes the properties
   (`render::window`); the write-back keeps them (edits in place, default
   windows copied whole).

`resolveWindowSpec(window, layout, appRegistry.windows)` in the UI mirrors
`ResolvedLayout::window_spec` (`WindowSpec::with_frame`, `normalized`: edges
clamped, `size` at least `min_size`).

The default layout now sizes every secondary window and gives the composer
`instances="multiple"`; `move` does not pop out. No default window is modal.

Constants the UI packages share: `WINDOW_CASCADE_OFFSET` (24 px, a new
instance's offset), `WINDOW_KEYBOARD_STEP` (16 px per arrow key),
`WINDOW_MIN_VISIBLE` (48 px of a window stay on the page when it shrinks or
a window is dragged off an edge).

### Remembered window state (`window-state.json`)

- **Per kind**, not per instance: `WindowState { page: WindowGeometry?,
  pop_out: WindowGeometry?, popped_out: bool }`; `WindowGeometry { rect:
  WindowRect {x, y, width, height}, maximized, snap: left|right? }` — `rect`
  is the restored rectangle, kept while maximized or snapped; maximized and
  snapped are exclusive. `page` is in page coordinates, `pop_out` in screen
  coordinates.
- File `<config-dir>/window-state.json` = `WindowStateFile {"version": 1,
  "windows": {"<kind>": {...}}}` (`nail-config/src/window_state.rs`).
  Missing → empty; unreadable, another version or not parsable → empty
  (logged) and replaced by the next write; single invalid entries are
  dropped on read; at most 256 kinds. Atomic writes under the config
  manager's write lock. Not configuration: neither watched nor backed up.
- **Why not `ui-state.json` keys** (ADR 0109's suggestion): the geometry is
  typed and validated (sizes, coordinates, exclusive flags), it changes on
  every drag end (a separate event keeps `UiStateChanged` consumers quiet),
  and a versioned file format can evolve on its own. The UI-state file
  keeps its free-form keys.
- Requests: `GetWindowStates`, `SetWindowState {window: kind, state|null}`
  (an empty state forgets the kind; instance ids are refused:
  `window-state-issue-invalid-window`), `ResetWindowStates` → all answered
  with `Response::WindowStates {states}`; `Event::WindowStateChanged
  {window, state|null}` on every change (also from pop-out reports).
  Remote sessions: `session` access, events forwarded (shared with the
  desktop like the UI state; the UI keeps windows on its own page).
- UI: `ui/src/state/windowState.ts` (`useWindowStates`, `useWindowState`,
  `useSetWindowState`, `useResetWindowStates`, `applyWindowStateChange`;
  `App.tsx` applies the event); mock `ui/src/api/windowStateMock.ts`.

### Pop-out windows (desktop only)

- `PopOutWindow {window: id, title?, context?}` opens the window as its own
  OS window: the core asks the shell (`DesktopHost::open_pop_out(PopOutOpen
  {window, label: "popout-<id>", route: "#/window/<id>", title, geometry})`)
  at the kind's remembered pop-out geometry, records the pop-out
  (`Inner::pop_outs`), sets `popped_out` for the kind and sends
  `PopOutChanged {window, change: opened, pop_outs}`. An open window is
  focused instead. `context` (≤ 16 KiB, e.g. `{"draft": "d42"}`) is how the
  new webview learns what to show: it reads its entry from `ListPopOuts`.
- `ReturnPopOut` closes the OS window and puts the window back into the page
  (forgets `popped_out`); `ClosePopOut` closes it (the window is gone; a
  closed kind stays `popped_out`, so it opens as an OS window again);
  `FocusPopOut` raises it; `ListPopOuts` lists them. Answers:
  `Response::PopOuts {pop_outs}`. Not open → `conflict`
  `pop-out-issue-not-open`; bad id / too large / more than 32 →
  `invalid_request` `pop-out-issue-*`. Without a desktop host (`nail serve`)
  `PopOutWindow` and friends answer `unsupported` (`desktop-unavailable`),
  `ListPopOuts` an empty list; remote sessions get `forbidden`
  (`DesktopOnly`), and `PopOutChanged` is never sent to them.
- The shell reports what the user does with a pop-out: `DesktopChange::
  PopOut(PopOutReport {window, change: moved|focused|closed|…, geometry})`
  (`DesktopChange` lost `Copy`). The core keeps the geometry per kind
  (`pop_out`), updates the list and sends `PopOutChanged`; reports for
  windows that are not open are ignored. The shell reports a move / resize
  when it ends (or at most every 500 ms).
- The pop-out webview loads the normal UI at `#/window/<id>`; `App.tsx`
  renders `features/popout/PopOutHost.tsx` (the window's content inside its
  `WindowInstanceContext`) instead of the main window. Each webview has its
  own UI store; shared data goes through the core (drafts are saved before a
  composer pops out — WP172 / WP173).
- UI hooks: `usePopOuts`, `usePopOutActions` (`popOut`, `returnToPage`,
  `close`, `focus`); `App.tsx` applies `PopOutChanged`. The mock has no
  desktop host until a test calls `enableMockPopOuts(core)`;
  `simulatePopOutReport` plays the shell's reports.

### Several composers

*Superseded in part by ADR 0118 (WP172): `UiState.composer` is now the
focused composer, mirroring the topmost `composer:<key>` window.*
`UiState.composer` stays the composer of the window `composer`; further
composers live in `UiState.composers` by window id
(`setComposerOf` / `updateComposerOf`). WP172 moves the composer onto
instance ids (`composer:<draft>`), using `useWindowInstance()` to find its
state, and decides how the single-instance readers (security, plug-ins, MCP
context, palette) pick the focused composer.

### Strings

`locales/en/core-windows.ftl` (contract ids: `window-snap-*`,
`window-instances-*`, `window-state-issue-*`, `pop-out-change-*`,
`pop-out-issue-*`; checked by `nail-api/tests/catalog.rs`); `core.ftl` has
the `config-window-*` errors; new UI files `windows.ftl` (WP171) and
`popout.ftl` (WP173); every file is embedded in `nail-config/src/l10n.rs`;
German files exist and are empty. The coverage test already lists
`features/windows/messages.ts` and `features/popout/messages.ts`.

### Slots (who edits what in M5c)

| Package | Owns |
| --- | --- |
| WP171 | `ui/src/layout/` (the window layer `WindowLayer.tsx` and the view components: frame, title bar, handles, maximize / snap, keyboard move / resize, focus and z-order, minimum sizes, staying on screen, remembering geometry through `state/windowState.ts`) and `ui/src/features/windows/` (window manager logic, styles, `windows.ftl`) |
| WP172 | `ui/src/features/composer/` (several composers, non-modal; `composers` / instance ids) and, in the other `ui/src/features/*` folders, only the `windows` registration entries (`Feature.windows`: `{ confirmation: true }` for confirmations, sizes for the tool windows) |
| WP173 | `app/src-tauri/` (`DesktopHost::open_pop_out` / `close_pop_out` / `focus_pop_out`, `DesktopChange::PopOut` reports, `popout-*` windows in the capabilities, geometry per pop-out) and `ui/src/features/popout/` (`PopOutHost.tsx`, "Open in new window" / "Return to main window", `popout.ftl`) |
| WP175 | `crates/nail-core/tests/gmail.rs`, `crates/nail-core/tests/exchange.rs` and the engine sources those tests exercise (`crates/nail-core/src/gmail*.rs`, `crates/nail-core/src/exchange*` — the `exchange/` directory —, `crates/nail-core/src/sync*.rs` only if the root cause is there) |
| WP174 | integration: every dialog migrated to the window manager, scenarios (move, resize, snap, maximize, keyboard, remembered geometry across a reload, several composers, a desktop pop-out), docs |

Shared contract files nobody edits in M5c except through WP174:
`ui/src/state/{windows,windowState,uiStore}.ts`, `ui/src/api/*` (incl.
`windowStateMock.ts`), `ui/src/features/{types,registry}.ts`,
`ui/src/App.tsx`, `ui/src/i18n/coverage.test.ts`, `crates/` (except WP175's
files), `defaults/`, `e2e/`. Packages add Vitest tests in their folders;
Playwright scenarios are WP174's. Contract changes go to WP174 as requests.

## Consequences

- Layout windows and dialogs share one model; confirmations opt into
  modality, everything else is non-modal by default.
- Remembered geometry is per kind, so a new composer opens where the last
  one was (cascaded by `WINDOW_CASCADE_OFFSET` when another is open).
- Remote browsers share the desktop's in-page geometry (like the UI state);
  the window manager keeps windows on its own page (`WINDOW_MIN_VISIBLE`).
- Until WP171, the contract-stage window layer still shows windows as modal
  frames (ids with instances work already); until WP173 the desktop host
  answers `unsupported` for pop-outs (trait defaults).
- Every `ResolvedLayout` literal and fixture gained `window_frames`; the
  layout JSON Schema documents the new properties.
