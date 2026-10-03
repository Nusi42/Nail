# ADR 0117: the window manager (M5c WP171)

Status: accepted · 2026-10-04 · M5c WP171

## Context

ADR 0116 fixed the window model (ids, specs, remembered state per kind,
pop-outs). WP171 builds the window manager in the UI: every secondary window
is a real window over the main window, as the approved proposal's "Dialogs"
section shows (drag by the title bar, 8 resize handles, double-click to
maximize, snap to halves, keyboard move / resize, remembered geometry,
non-modal by default). WP171 owns `ui/src/layout/` and
`ui/src/features/windows/`.

## Decisions

### Files

- `features/windows/geometry.ts` — pure geometry: `defaultRect` (spec size,
  centred, cascaded by `WINDOW_CASCADE_OFFSET` per open window of the kind),
  `initialGeometry` (remembered → fitted to the page; a further instance is
  cascaded and opens normal), `shownRect` (maximized = the page, snapped = a
  half at least `min_size` wide, else the remembered rectangle fitted to the
  page), `keepOnPage` (`WINDOW_MIN_VISIBLE` px stay on the page, the title
  bar never leaves the top edge), `moveRect`, `resizeRect` (8 edges, opposite
  edges fixed, minimum size, moved edges stay on the page), `snapTarget`
  (`SNAP_EDGE` = 24 px, the proposal's value), `looseRect` (a maximized /
  snapped window dragged away comes loose at its restored size, the pointer
  keeping its relative place), `keyboardMove` / `keyboardResize`
  (`WINDOW_KEYBOARD_STEP`; resize from the bottom-right corner).
- `layout/WindowFrame.tsx` — one window: frame, title bar, buttons,
  handles, snap preview, pointer and keyboard handling, debounced saves.
  `tabbables` / `trapTab` moved here (re-exported from `WindowLayer.tsx`,
  which plug-in dialogs import).
- `layout/WindowLayer.tsx` — the layer: z-order, modality, focus, Ctrl+F6,
  remembering through `useSetWindowState`, popped-out windows left out.
- `layout/windowSpecs.ts` — `WindowSpecsContext` (the features'
  `appRegistry.windows`). A context, not an import of the registry: the
  registry imports every feature, and `features/plugins` imports
  `layout/WindowLayer.tsx`, so importing the registry here would be a cycle
  evaluated at start-up.
- `features/windows/commands.ts` — the layer registers itself per UI store;
  the actions `window.cycle`, `window.main`, `window.maximize`,
  `window.reset-positions` send it commands.
- `features/windows/skin.ts` — `WINDOWS_SKIN_CSS` (token-only) for the
  window-manager parts.

### Geometry and styling

- Position, size and stacking order are a geometry rule per window
  (`useGeometry("wm-<id>")`), never inline styles. Because a theme part rule
  like `window[window="move"] { width … }` is more specific than
  `[data-geom]`, the rule pins the size with `min-` / `max-width` /
  `-height` as well, so the window manager's size always wins over a theme.
- The DOM order of windows is the opening order (`openingOrder`); raising
  only changes `z-index`. Moving DOM nodes would reload iframes (HTML mail)
  and drop focus.
- The remembered rectangle is never changed by a smaller page: the window
  is *shown* fitted (`shownRect`), so it gets its place back when the page
  grows again. Only user actions are saved.
- Saves are debounced (`SAVE_DELAY_MS` = 400 ms: drags end once, key
  repeats coalesce) and flushed when the window closes. A save keeps the
  kind's `pop_out` / `popped_out` and is skipped when nothing changed.
  Remembered state that loads after a window opened is applied unless the
  user already changed the window.

### Interaction

- Move: pointer capture on the title bar (`window.header`), a 4 px
  threshold before a press counts as a drag, `pointercancel` goes back.
  Presses on title-bar buttons never start a move.
- Snap: the pointer within 24 px of the left / right page edge shows
  `window-snap-preview` (`data-snap`); dropping there snaps
  (`data-snap`, `data-state~="snapped"`) and keeps the pre-drag rectangle
  for restoring. Only resizable windows maximize or snap.
- Keyboard: the title bar is focusable (`role="group"`, named "Title bar of
  <title>", described by a hidden hint). Arrow keys move, Shift+arrow keys
  resize. Alt+F7 / Alt+F8 anywhere in the window start move / size mode
  (`data-mode`): arrow keys without Shift, Enter keeps, Escape goes back
  (and does not close the window), Tab / leaving keeps. A live region
  (`window.status`) announces modes, maximize / restore and snapping.
- Focus and z-order: a press or focus inside a window raises it
  (`focusWindow`), so the topmost window is the one with focus and the
  layout's `escape → window.close` closes it. Of several windows that open
  together only the topmost takes focus. Tab stays inside a window (like an
  OS window); Ctrl+F6 goes from the main window to the topmost window, from
  a window raises the bottom one (all windows come round), with one window
  back and forth to the main window; Ctrl+Shift+F6 returns to the main
  window (its last focused element). The layer handles Ctrl+F6 itself
  (capture phase, `preventDefault`, so a later layout binding of the
  actions never runs twice).
- Modality: non-modal windows have no `aria-modal`, and the layer lets the
  pointer through around them (`pointer-events: none` on `window-layer`).
  Modal windows (only `{ confirmation: true }`) get `aria-modal="true"`,
  `data-modal`, a `window-scrim` right below them (z-index between), the
  windows behind become `inert`, focus that strays outside (other than into
  menus / pop-ups, `[data-key-modal]`) comes back, and Ctrl+F6 does not
  leave.

## Consequences / integration

- The compiled theme needs `WINDOWS_SKIN_CSS` after the shared skin
  (`theme/compile.ts`), and `App.tsx` / the test harness must provide
  `WindowSpecsContext` with `appRegistry.windows`; until then every window
  is non-modal and in jsdom the base rule hides non-active windows (the
  window-manager tests inject the skin themselves).
- The base rules `window-layer` (flex centring) and
  `window:not(active) { display: none }` in `BASE_CSS` and the
  `window-layer` padding / scrim in `theme/skin.ts` are obsolete (the skin
  overrides them); the theme's `part "window"` width / height rules are
  superseded by the spec sizes.
- `ActionsProvider` still treats any open window as "only window bindings
  apply", also while focus is in the main window next to a non-modal
  window; that belongs to WP174 (see the package report).
