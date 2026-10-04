# ADR 0121: M5c integration — every dialog a window, confirmations that block their own flow

Status: accepted · 2026-10-04 · M5c WP174

## Context

The M5c packages (ADRs 0117 window manager, 0118 non-modal windows and
several composers, 0119 desktop pop-outs, 0120 Gmail / Exchange sync races)
were built in parallel against the contract (ADR 0116). WP174 integrates
them, migrates every dialog of the app to the window manager and adds the
scenarios of plan section 10. This ADR records what the integration
decided beyond the package ADRs.

## Decisions

### Wiring

- `theme/compile.ts` emits `WINDOWS_SKIN_CSS` and `COMPOSER_SKIN_CSS`
  after the shared skin (before the theme's part rules) and appends
  `POPOUT_BASE_CSS` to the base CSS. The M1 modal-frame rules (flex-centred
  `window-layer`, `window:not(active) { display: none }`, the layer's
  padding and scrim in `theme/skin.ts`) are gone, and so are the
  `part "window"` size rules of `defaults/theme.kdl`: sizes come from the
  window spec only.
- The app and the test harness provide `WindowSpecsContext`
  (`appRegistry.windows`) and a new `WindowTitlesContext`
  (`appRegistry.windowTitles`, from the new optional `Feature.windowTitles`:
  `(state, id) => string | null`). The window layer shows the composer's
  subject and the plug-in dialogs' own titles through it. Contexts, not
  registry imports: the registry imports every feature, and the features
  import the layout (`PopOutRouter` / `PopOutButton` read the specs from
  the context, which broke the import cycle that kept
  `state/windowStore.test.tsx` from loading).
- `PopOutButton` sits in every window's title bar (desktop app only, specs
  with `pop_out`).
- The composer registers its pop-out preparer (save the draft, hand over
  `{draft}`), names its OS window with `usePopOutTitle(subject)` and, in a
  pop-out page, opens the draft from the context in the route's window id
  (`openComposer` now takes a given window id when no composer has it — the
  pop-out page opens its window before the composer loads). "Return to main
  window" runs the kind's preparer first (the draft is saved) and the main
  page reopens the returned composer from the draft it popped out
  (`features/composer/popOut.ts`).
- The security toggles and the program-draft notice read the composer of
  their own window (`features/composer/windowOf.ts`: `composerOf`,
  `useComposerWindowOf`, outside the composer's flows so no import cycle
  reaches them); the sign / encrypt choice stays the focused composer's.
- `UiState.composer` is documented as the focused composer (ADR 0118).

### Modality: a confirmation blocks only its own flow

ADR 0116 made confirmations the only modal windows; the WP171 layer made a
modal window cover the whole page. The plan's scenario is "a modal
confirmation blocking only its own flow", so:

- Every modal window has an **owner** (`modalOwner` in
  `layout/WindowLayer.tsx`): an open non-modal window with the same
  instance key (`composer-close:w1` → `composer:w1`), else the window that
  had the focus when it opened, else the main window. It is decided once,
  when the modal window first shows (a layout effect; until then it counts
  as page-modal).
- An owner window is `inert`, `data-blocked` and dimmed by the skin; other
  windows and the main window stay usable, and Ctrl+F6 skips it. A
  page-modal window (owner `main`) puts `window-scrim` *below every window*
  (only the main window is covered), keeps focus from straying into the
  main window and is the only kind with `aria-modal="true"` (a window-modal
  confirmation must not hide the rest from assistive technology).
  `data-owner` names the owner.
- Modal windows stack above all non-modal ones, so raising a window never
  covers an open question.
- `ActionsProvider` applies only window bindings while a page-modal window
  is open (`[data-owner="main"]`); with non-modal windows, or a question of
  another window, focus in the main window keeps the main window's and the
  global bindings (Escape there no longer closes the topmost window).

### Every dialog is a window

No dialog keeps a fixed modal frame of its own any more:

- **"Delete permanently?"** (`mail-delete-permanently`, widgets feature,
  `{ confirmation: true }`): `mail.delete-permanently` asks first; the
  messages are those selected when the action ran.
- **Plug-in dialogs** (`nail.ui.showDialog`) open
  `plugin-dialog-<size>:<key>` (non-modal) or `plugin-modal-<size>:<key>`
  (modal) by the dialog's `modal` and `size`; the window title is the
  dialog's; closing the window dismisses the dialog (`button: null`). The
  dialog's own backdrop, title, close button and Tab trap are gone (parts
  `plugin.dialog.backdrop` / `.title` / `.close` removed from
  `PLUGIN_PARTS`).
- **The send veto** is the confirmation `plugin-send-veto` (it blocks the
  composer it came from); closing it keeps editing.
- **Remote re-authentication** ("Confirm it’s you") is the confirmation
  `remote-reauth`; closing it cancels.
- A test (`ui/src/m5c.test.tsx`) opens every layout window and every
  feature dialog through the window manager and checks its role, its
  modality, the scrim, its default size and that keyboard resizing stops at
  its minimum size (fixed windows keep their size).

What stays outside: the remote login page (a gate before the app exists),
context menus, popovers and toasts.

### Smaller decisions

- The window's live region (`window.status`) is `aria-live` without
  `role="status"`, so a window's own status messages stay the window's only
  `status` role.
- The composer's default size is 820 × 720 (the security panel's key
  lookup row fits without scrolling).
- **API messages are stored in one transaction with their extras**:
  `Store::merge_with_remote` / `nail_store::exchange::RemoteExtras`
  (server id, Exchange item record, Gmail data) and
  `ingest::ingest_with_remote`; the Exchange and Gmail syncs use them, so no
  reader sees a new message without its remote id (WP175's request).
  ADR 0078 points to ADR 0120 for the changed removal rule.

### Tests

- Vitest: `m5c.test.tsx` (every window, titles, pop-out button, composer
  pop-out and return, per-window readers), `widgets/deletePermanently.test`,
  modality / owner tests in `layout/WindowLayer.test.tsx`, plug-in dialog,
  veto and re-auth tests through the window layer.
- Playwright `e2e/tests/m5c.spec.ts`: move, resize from each edge and
  corner, minimum size, snap left / right with the preview and coming
  loose, maximize by double-click and button, keyboard move / resize
  (arrows, Alt+F7 / Alt+F8 with Enter / Escape), geometry remembered across
  a reload (`window-state.json`) and Reset window positions, two composers
  side by side (one sent, the other kept), a modal confirmation blocking
  only its composer and a page-modal one leaving the composer usable,
  Escape. `desktop-m5c.spec.ts` (real desktop app): a composer pops out, is
  written and sent there and its OS window closes; a popped-out composer
  returns with what was typed.
- e2e helpers `composerWindow` / `composerWindows` / `discardComposer`
  (`e2e/support/views.ts`); `getByLabel` inside windows needs
  `exact: true` when the label is part of the title bar's "Title bar of …".
- Visual: the composer waits for its key check, a new `confirmation`
  screen, and every screen first marks the two messages other screens open
  unread again (the worker's server is shared by all variants; before, the
  read state depended on the variant order). Baselines regenerated.

## Consequences / follow-ups

- The composer still guards closing through its session sync (it puts an
  unsaved window back); a `beforeClose` in the window layer would be
  cleaner (WP172's request, deferred).
- `PopOutWindow` carries no size: a first pop-out opens at 720 × 520.
- No default key bindings for `popout.open` / `popout.return` (palette
  only); no snapping to the top edge or quarters; no backwards window cycle
  (needs a "lower window" in the UI store); geometry is per kind, shared by
  remote browsers (ADR 0116).
- A background composer's sign / encrypt report is recomputed when it is
  focused (ADR 0118).
- The visual `calendar-year` screen shows demo events relative to the
  real date: its baselines change with the weekday they were made on.
