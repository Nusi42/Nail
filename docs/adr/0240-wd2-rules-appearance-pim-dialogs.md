# 0240 — R6 WD2: rules, appearance, customization and PIM dialogs

Status: accepted (2026-11-22)

## Context

ADR 0239 defines one dialog pattern and its shared pieces. This package
moves the dialogs of the rules window, Appearance and the theme windows,
the customization editors and merge wizard, the calendar, contact and task
editors and dialogs, and scheduling onto them. A few places needed a
decision the contract leaves open.

## Decision

### Rules window

- The rule editor registers its unsaved changes **where it is shown**
  (`features/rules/RulesGuard.tsx`): in a window (the rules window, a
  popped-out one) it registers the window's close guard; inside the settings
  page (Settings → Mail & junk, where the page's guard key is the settings
  store's `guardKey`) it registers a settings draft, so the page's save bar
  and guard cover it. The two never register on the same key.
- Picking another rule with unsaved changes asks through the same prompt:
  the rules store's `leave` (set by the editor) calls `askToLeave` with the
  editor's guard key. `features/rules/closeGuard.ts`, the store's
  `pending` / `discard` / `keepEditing` / `asking` and the inline
  `rules/Confirm.tsx` are gone.
- Delete rule and a run that moves or deletes mail ask in **modal
  confirmation windows** (`rule-delete`, `rule-run`; `Questions.tsx`), not
  over the editor's content: the window layer makes the rules window inert
  and returns the focus to the button that asked. Both are destructive.
- The footer is the shared `DialogFooter`: `Delete rule…` at the start,
  Run on, Run and Test rule in the middle (`rules.footer.tools`; Run's label
  changes with the rule's state, so it is not a role button and Save never
  moves), Save rule (the primary, the form's submit button) at the end, no
  Close.
  The status line (problem badge, "Saved", the last run with Undo) stays
  above it in `rules.footer`.
- `FooterButton` has no `aria-disabled`: Save rule stays enabled while the
  rule has problems (pressing it saves nothing; the problem badge says
  why), Run is disabled. Asked of the integrator: an `ariaDisabled` /
  `describedBy` option, so Save can again be focusable but marked.

### Appearance (a settings section)

- The whole section is **one settings draft** (`appearance-settings`): a
  picked theme, Follow Windows and its pair, density and the overrides.
  Save writes the theme first (`SetTheme`), then the preferences on top of
  the appearance that call returned, so the two writes never undo each
  other. One draft instead of several avoids two `SetThemePreferences`
  writes racing with stale preferences.
- The accent's **Apply** is gone; the sliders and the colour picker no
  longer write after a pause (the live restyling goes: nothing applies
  before Save). The theme cards still preview each theme.
- Undo / Redo act on what was saved; an unsaved draft stays shown.

### Customization

- The toolbar and column editors (`customization/Editors.tsx`) register
  settings drafts (`toolbar-editor:<node>`, `column-editor:<node>`); their
  Save / Undo changes buttons go. Choosing another toolbar or list asks
  with `askToLeave`. "Back to the default toolbar" stays an immediate
  action.
- The merge wizard: Next (was "Compare with mine"), then **Apply theme** /
  **Apply layout** with Back (secondary) and Cancel on every step; the result
  shows Revert beside one Done.

### Calendars, contacts, tasks

- The PIM questions (delete an event or a task, delete a contact group,
  remove a source) are modal confirmation windows `pim-delete-event`,
  `pim-delete-task`, `pim-delete-group`, `pim-remove-source`
  (`features/pim/Confirm.tsx`): the component that asks words the question
  (`askPim`), the window runs it and keeps itself open with the core's
  reason when it fails. Remove source asks rather than offering Undo: a
  removed source cannot be restored by the core.
- The event, task and contact editors share `useEditorGuard`
  (`pim/common.tsx`): unsaved changes are what differs from the editor's
  state when it opened. Their footer is `DialogFooter` (`Delete…` at the
  start for a saved, writable item; Save; Cancel) inside `DialogForm`, so it
  is pinned (`dialog.footer` in `DIALOG_BUTTON_ROWS`) and Ctrl+Enter saves
  from the notes. An editor's store target is cleared when the editor
  unmounts (unless another target replaced it).
- The edit-scope question: a move is a form with **Save changes**; a delete
  is a destructive `ConfirmDialog` with **Delete events**.
- Contact groups and Calendars and address books are tool windows: no
  footer Close; their per-item rows (a group's Save, Delete group…; Sync,
  Remove…; Test, Add) stay inline (`pim.editor.actions`).

### Scheduling

- Cancel meeting is a destructive `ConfirmDialog`: **Cancel meeting** (red)
  and **Keep meeting** as the cancel role with the focus; the scopes,
  people and note are its detail.
- Invitation settings: `DialogForm`, Save / Cancel, a close guard.
- Find a time: one footer (Create meeting; Cancel in the dialog).

## Consequences

- Obsolete theme parts (no component emits them any more; their removal
  from `defaults/theme.kdl` and the regenerated theme fixture come with the
  integration): `rules.footer.start`, `rules.footer.end`, `rules.confirm`,
  `rules.confirm.question`, `rules.confirm.actions`, `merge-dialog.actions`,
  `column-editor.actions`, `pim.scope.actions`, `scheduling.dialog.actions`,
  `scheduling.find-time.actions`, and the `DIALOG_BUTTON_ROWS` entries
  `merge-dialog.actions` / `scheduling.dialog.actions`. The feature skins
  dropped `rules.confirm`, `theme-dialog.actions`, `theme-dialog.question`,
  `theme-dialog.note` and `pim.calendar-dialog.question`.
- Settings → Appearance no longer restyles the app while a slider moves.
- E2e: `r6-wd2-dialogs.spec.ts`; `e2e/support/appearance.ts`
  (`saveSettingsSection`, `pickTheme`) for specs that change Appearance.
