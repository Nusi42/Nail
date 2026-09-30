# ADR 0047: M2c WP54 — context menus, Customize editors, triage / Sieve / sync UI, lazy editors, accessibility

Status: accepted · 2026-09-30 · M2c WP54 (parallel package; the integrator renumbers)

## Context

Plan section 7, WP54: right-click menus from `layout.kdl`, toolbar / key /
column editors in the Customize tab, lazy-loaded TipTap / CodeMirror,
inline images of reopened HTML drafts, UI for vacation, junk, snooze,
notifications and per-folder sync policies, a themed login page and an
accessibility pass. The contract is ADR 0043; WP54 owns `ui/`,
`locales/` (except `core*.ftl`), `defaults/` and the widget slots
`crates/nail-config/src/widgets/{sieve,triage,sync}.rs`.

## Decisions

### Context menus (`ui/src/features/menus/`)

- **Parts.** The contract's context-menu parts are logical names;
  `targets.ts` maps DOM parts to them (`message-list.row`,
  `thread-view.item` → `thread-view.message`, `message-view`,
  `message-view.attachment`, `folder-tree.item` → `folder-tree.folder`,
  `composer`). The innermost match wins; a listbox's
  `aria-activedescendant` counts (Shift+F10 on the message list opens the
  selected row's menu). Text fields keep the browser's menu (spell check,
  paste); parts without a layout menu do too.
- **Target as context.** A message row's menu selects the row first, so
  every existing `mail.*` action works unchanged. A folder's menu does not
  open the folder: while a menu is open, `contextTarget(uiStore)` names its
  target and the new actions read it (`sync.folder-policy`,
  `junk.*`, `snooze.open`, `sync.download-message`). Handlers read it
  synchronously; the menu closes right after dispatching.
- **Keyboard.** Shift+F10 and the menu key are key bindings of a new
  action `menu.context` (rebindable), not hard-wired. The menu follows the
  WAI-ARIA menu pattern: roving focus, Up / Down / Home / End, Enter /
  Space, Right / Left for submenus, Escape (focus returns to where it was,
  or to the focusable around the target for pointer menus), Tab closes,
  typeahead. Disabled entries stay focusable (`aria-disabled`). Menu keys
  are stopped before the app's key bindings (`data-key-modal`).
- **Rendering.** An effect (`ContextMenuHost`) listens for `contextmenu`
  and renders the menu; position through `useGeometry` (no inline style),
  kept inside the window, closed on outside pointer-down, blur and resize.
  Shortcuts are shown with localized key names (`keyLabels.ts`, shared with
  the key editor) and `aria-keyshortcuts` for chords.

### Customize editors (`features/customization/Editors.tsx`)

- Toolbar, key-binding and column editors are layout widgets
  (`toolbar-editor`, `key-binding-editor`, `column-editor`) placed in a
  `tabs` node of the Customize stack, registered in
  `nail-config/src/widgets/customization.rs` (no other package edits it in
  M2c; WP53 owns the rest of `nail-config`).
- Toolbars and columns are edited as a **draft** and written with **Save**
  (one `UpdateLayout`, one backup), not on every move; keys are written per
  add / remove / reset. Each view is keyed by the file's state, so a
  `ConfigChanged` shows what was written. Reordering is keyboard-first
  (`ReorderList`: Up / Down / Remove buttons, Alt+Up / Alt+Down, focus
  follows the entry).
- Keys are **recorded** in a read-only field (`recordKey`: chords via
  `comboFromEvent`, sequences up to three steps, Backspace removes the last
  step; Tab and Escape are not captured, so the field never traps focus);
  a conflict in the same context is announced before saving.
- **Columns** are now rendered: a `message-list` with `columns="…"`
  (`from subject date preview tags account`) shows one cell per column
  (templates per column, `message-list.header` with the titles,
  `aria-hidden`); without it the row templates apply as before.

### Triage, Sieve and sync UI

- Widgets `junk-settings`, `junk-verdict` (reader), `snooze-list` (window
  `snoozed`), `notification-settings`; `sieve-status`, `vacation-editor`;
  `sync-folder-policies`, `sync-message-body` (reader). Dialogs `snooze`
  (picker with presets in local time, RFC 3339 with offset) and
  `sync-folder`; `snoozed` is also a dialog so the action works with a
  layout that lacks the window. Actions `junk.mark`, `junk.not-junk`,
  `snooze.open`, `snooze.list`, `sync.download-message`,
  `sync.folder-policy`, `menu.context`.
- Writes go through the core: `MarkJunk` / `(Un)SnoozeMessages` use the
  shared `queued()` helper (op ids, optimistic removal from lists,
  refetch on failure); settings use their requests; ManageSieve settings
  are an `UpdateAccount` of the account's `sieve` node.
- Validation mirrors the contract (`manageSieveIssues`, existing
  `vacationIssues`, `junkIssues`, `notificationIssues`) and shows the same
  Fluent ids the core answers with; `NailError` now carries the error's
  `localized` text (`errorMessageOf`).
- **Web Notifications** (remote UI only; the desktop shell shows OS
  notifications from Rust): an effect shows `NotificationRequested` when
  the page has permission; the settings offer "Allow notifications in this
  browser".
- **Mocks behave like the core after WP55** (ADR 0043 allowed WP54 to
  extend them): `MarkJunk` trains per message, sets `$Junk` / `$NotJunk`
  and moves into / out of Junk; snoozing moves to a hidden per-account
  folder with `$Snoozed` (`wakeDue` wakes); `GetMessage` carries the
  verdict; `SyncSieve` / `PreviewSieveScript` answer (placements are a
  stand-in: arrival rules → server); `FetchMessageBodies` clears
  `headers_only`.

### Lazy editors and chunks

- `features/composer/lazyEditors.tsx` loads `HtmlEditor` / `MarkdownEditor`
  with `import()` (module cache instead of `React.lazy`, so an editor that
  is loaded renders synchronously — tests preload it); the composer starts
  both downloads when it mounts; a status line stands in meanwhile. A test
  keeps TipTap / CodeMirror out of every statically imported module.
- `vite.config.ts` splits `vendor` (React, TanStack, Zustand, Fluent) and
  `catalogs` (the `.ftl` files) into their own chunks. Result: main chunk
  ~470 kB (was ~1.7 MB), vendor ~290 kB, catalogs ~130 kB, HTML editor
  ~450 kB and Markdown editor ~500 kB on demand; no chunk over Vite's
  500 kB warning.

### Inline images of reopened HTML drafts

The stored HTML keeps `cid:` references; previews of images added in this
session were already kept. For drafts from another session the UI asks the
core for the bytes: **`GetDraftAttachment {draft_id, blob}` →
`AttachmentData {mime, data}`** — a contract addition requested from WP55
(only blobs attached to that draft; draft access rules for remote
sessions). Until it is generated, `inlineImages.ts` is the single place
that sends it (typed locally, cast once); a core without it answers an
error and the editor keeps the alt text. The HTML editor swaps `cid:`
sources for previews that arrive later without changing the stored HTML.
Only `image/*` (not SVG) becomes a `data:` preview.

### Login page and other pre-configuration screens

`theme/BootTheme.tsx` compiles the shipped default theme — the
`nail-api` reference fixture, which `nail-config`'s test keeps equal to
`defaults/theme.kdl` — for the remote login page, "Loading…" and
"unavailable". It is a separate chunk loaded at once (nothing is shown
for those few milliseconds instead of unstyled content); the URL's
`?mode=` applies.

### Accessibility

- **axe-core in Vitest** (`src/a11y.test.tsx`, `axe-core` 4.13, MPL-2.0,
  dev dependency): every window of the default layout and every tab of its
  tab containers, every feature dialog, every M2c widget, the open context
  menu, the HTML and Markdown composer and the pairing login page; colour
  contrast and landmarks-per-page stay with the Playwright scan (jsdom has
  no layout). Fixed on the way: a duplicate "Devices" landmark in
  `remote-devices`.
- **Focus management**: modal windows trap Tab / Shift+Tab (`trapTab`,
  skipping hidden tab panels) in addition to the existing initial focus
  and focus return; toolbars move with Left / Right / Home / End.
- **Reduced motion**: the compiled theme's base CSS stops animations and
  transitions under `prefers-reduced-motion: reduce`, whatever the theme.
- **High contrast**: `defaults/themes/high-contrast.kdl`, an example theme
  (`extends="default" mode="high-contrast"`, same colours in every mode,
  larger type, 2 px borders, 3 px cyan focus rings, no motion), documented
  in `userDocs/en/accessibility.md`. The default theme keeps following
  Windows' contrast themes by itself.

### Default layout and theme

`defaults/layout.kdl`: reader stacks gain `junk-verdict` and
`sync-message-body`; window `snoozed`; settings tabs Server rules,
Vacation reply, Junk mail, Notifications, Synchronization; the Customize
stack gains the editors' tabs; the new actions; keys `b`
(`snooze.open`), `shift+j` (`junk.mark`), `shift+f10` / `contextmenu`
(`menu.context`); `snooze.list` on the main toolbar, `snooze.open` /
`junk.mark` on the reader toolbar; menus for `message-list.row` (extended),
`thread-view.message`, `message-view`, `folder-tree.folder`.
`defaults/theme.kdl`: icons `snooze`, `download`, `chevron-right` and the
parts of all of the above.

## Consequences

- The reference fixtures (`crates/nail-api/fixtures/default-*.json`) and
  the write-back golden files must be regenerated by WP55 (see the
  report); UI tests pass with both the old and the regenerated fixtures.
- Follow-ups: `GetDraftAttachment` in the contract; a Playwright pass for
  the new windows (WP55 scenarios: right-click menu, rebinding a key, axe
  on every window); a rule's placement badge inside the rule manager (the
  Server rules tab lists them); `folder-tree.account` has no DOM part yet
  (the account switcher is not a tree); application menu (`menu "app"`)
  still unused in the web UI.
