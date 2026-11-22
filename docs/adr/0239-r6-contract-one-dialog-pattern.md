# 0239 — R6 contract: one dialog pattern everywhere

Status: accepted (2026-11-22)

## Context

Nail has about 90 places that confirm, save or close something, and they
behave in at least six ways. A design review of every dialog (inventory,
rules, per-dialog changes, before / after mocks) found:

- **Button order flips.** Some dialogs put Cancel first
  (`exchange/common.tsx`, `appearance/ThemeDialogs.tsx`, `rules/Confirm.tsx`,
  `pim/common.tsx`), others the action first (composer, folders, Gmail, mail,
  the PIM editors).
- **Save does not always close.** Automatic replies, Contact groups,
  Diagnostics, Print to PDF and the recovery check keep the window open after
  Save and show a second Close beside it. Error details and Message source
  style Close as the primary; Folder delete styles Cancel as the primary.
- **Focus can land on Delete.** Folder delete and Gmail label delete focus the
  danger button; "Cancel and notify" (scheduling) submits on Enter.
- **Unsaved changes are lost silently.** Only the rules window and the
  composer ask; every other editor drops its edits on × or Esc.
- **Labels.** A bare "OK" (PIN prompt, a composer state); "Apply" means two
  different things.
- **Destructive actions without a question or Undo**: event / task Delete,
  Delete group, Remove source, Delete virtual account, Outbox Discard,
  Connection log Clear.
- **Settings mix three models** (apply at once, a Save at the bottom of a
  long form, a dirty-disabled Save with Undo) and nothing marks a section
  with unsaved changes when it is left.
- **No shared footer**: about 45 `*.actions` / `*.buttons` parts in
  `theme.kdl`, each aligned its own way; the event editor's footer scrolls
  out of view.

The maintainer approved one pattern with the decisions below.

## Decision

### The rules

1. **Footer order per system, by role.** Windows and Linux: primary,
   secondary, Cancel (left to right, right-aligned; WinUI's ContentDialog,
   Windows Settings, Office). macOS: Cancel, secondary, primary (Apple's HIG:
   the default button at the far right). Web mode follows the **viewer's**
   system (the browser's client hints, then `navigator.platform`, then the user
   agent), not the server's `PlatformInfo.os`. An editor's `Delete…` sits alone
   at the start. Code names roles, never places (like Qt's
   `QDialogButtonBox`).
2. **× = Esc = Cancel**, everywhere. None of them commits. With unsaved
   changes they ask with **one shared prompt**: "Save changes to “X”?" — Save /
   Don't save / Cancel; when saving is impossible (the item is invalid):
   "Discard changes to “X”?" — Discard changes / Keep editing.
3. **Labels are verbs** (Save, Create, Move, Delete folder, Send, Snooze,
   Unlock). Never OK, Yes, No or Apply. A finished task shows one **Done**.
4. **The primary saves and closes**; a failure keeps the dialog open with an
   inline error. Exception: **tool windows** (rules, theme editor, toolbar
   customizer, logs) stay open after their actions and have no footer Close
   (× is enough).
5. **Destructive**: a red verb button ("Delete folder"), never the default:
   the focus starts on Cancel and Enter never runs it. Prefer Undo where the
   action can be undone; destructive actions that ask nothing today get a
   confirmation or an Undo.
6. **Enter** runs the primary only when it is not destructive. In
   multi-line fields Enter adds a line and **Ctrl+Enter** (Cmd+Enter) runs the
   primary.
7. **Wizards** have Cancel on every step and Done on the result step.
8. **Settings: nothing applies until Save.** Every settings section gets the
   pinned "Unsaved changes" bar (Save / Discard) while it has unsaved
   changes; leaving the section, closing Settings or its window asks with the
   shared prompt. This deviates from the review's proposal (apply at once with
   Undo) by the maintainer's decision: one model for every section. Tool
   windows outside Settings that apply at once with Undo (theme editor,
   toolbar customizer) stay as they are.

Focus on open: the first field of an editor (the window frame's rule, ADR
0116), the verb of a normal confirmation (rule 6: Enter runs it), Cancel of a
destructive one (rule 5). The review's "focus the safe button" and "Enter runs
the primary when not destructive" are reconciled this way: a normal
confirmation's verb is the safe default, a destructive one's is not. Focus
returns to the opener on close (the window frame, unchanged).

### The shared pieces (contract; packages only use them)

- **`DialogFooter`** (`ui/src/widgets/dialog/DialogFooter.tsx`): props by
  role — `primary`, `secondary?`, `cancel` (label defaults to Cancel; its
  `onClick` is the dialog's close, so it goes through the close guard),
  `start?` (`Delete…`), `busy` (everything but Cancel disabled,
  `aria-busy`), `done` (only Done). A `destructive` button is red
  (`data-variant="danger"`) and never `type="submit"`; a normal primary may be
  the form's submit button (`submit`). Parts `dialog.footer` (`data-os`,
  `data-state="done"`), `dialog.footer.start`, `dialog.footer.end`; every
  button is a `form.button` with `data-role` (`primary` / `secondary` /
  `cancel` / `start` / `done`) and the dialog's `data-action`. The order:
  `footerOrder(os)` in `order.ts` (pure); the viewer's system: `viewerOs()` /
  `osOfNavigator`; `DialogOsContext` pins one (tests, previews).
- **`ConfirmDialog`** (`ConfirmDialog.tsx`): `kind` (`data-confirm`),
  `question`, `detail`, `action` (the verb), `risk` (`normal` |
  `destructive`), `cancel`, `secondary?`, `canConfirm`, `busy`, `error`. Focus
  and Enter as above; after a failed attempt the focus goes back to the same
  button. Parts `dialog.confirm` (`data-risk`), `dialog.question`,
  `dialog.detail`, `dialog.note`. It replaces `MailConfirm`, `rules/Confirm`,
  the composer's and Exchange's confirmations and the inline "Are you sure"
  rows as the packages migrate them.
- **`DialogForm`** (`DialogForm.tsx`): a dialog's `<form>`; Enter in a
  single-line field submits through the footer's submit button, Ctrl+Enter /
  Cmd+Enter does so from a `textarea`; a disabled submit button blocks both.
- **The close guard** (`ui/src/layout/closeGuard.ts`, generalised from the
  rules window's, ADR 0222): `useCloseGuard({ name, isDirty, save?, canSave?,
  discard?, saveLabel?, detail? })` registers the calling window (its
  `WindowInstanceContext` id; `MAIN_GUARD` outside a window).
  `installCloseGuard(ui)` (mounted once by the windows feature,
  `CloseGuardEffect`) watches `UiState.windows`: a guarded window that leaves
  while dirty is put back in the same store update (it keeps its React state)
  and the prompt window `unsaved-changes:<key>` opens (`promptWindowFor`; the
  key is the window id with `:` as `_`; a modal confirmation whose owner is
  that window, `modalOwner`). `askToLeave(ui, key, proceed)` asks the same way
  before moving on inside a window (another settings section, another rule);
  `closeWindowNow(ui, id)` closes without asking (after the window's own
  Save). The prompt (`UnsavedPrompt.tsx`): Save (the guard's `saveLabel`,
  focused, Enter) runs `save` and lets the window go when it returns `true`,
  else returns to the window with its inline error; Don't save (red) runs
  `discard` and lets it go; Cancel, × and Esc return to the window.
- **`SettingsSaveBar`** and the settings drafts (`SettingsSaveBar.tsx`,
  `settingsDrafts.ts`): a settings widget registers its unsaved changes with
  `useSettingsDraft(key, { dirty, canSave?, save, discard })`; the settings
  page (`features/settings/widgets.tsx`) shows one bar for the shown section
  ("Unsaved changes to “Mail”", Save / Discard; Save saves every dirty draft in
  turn and stops at the first failure), registers the page's close guard
  (closing Settings asks) and asks with `askToLeave` before another section
  shows — also for the `settings.section` action (deep links; the page's guard
  key is kept in the settings store).
- **Strings**: `locales/en/dialog.ftl` (`dialog-*`, the prompt's window title
  `window-unsaved-changes`); `common-discard-changes` and
  `common-unsaved-changes` join `common.ftl` (the rules' and the composer's
  texts refer to them).
- **Look**: structure in `ui/src/widgets/dialog/skin.ts` (`DIALOG_SKIN_CSS`);
  `dialog.footer` joins `DIALOG_BUTTON_ROWS` (the last button row of a window
  body sticks to its bottom); one rule set in `defaults/theme.kdl` (`dialog.*`,
  `settings-save-bar`). Packages remove their own `*.actions` / `*.buttons`
  parts as their dialogs move to the footer.
- **End-to-end support**: `emulateViewerOs(page, os)` and
  `expectDialogFooter(dialog, { os, roles, focused?, start? })` in
  `e2e/support/windowTour.ts` check any dialog's footer order (from the same
  `footerOrder`) and its focused button.

### Reference migrations (contract)

- "Delete permanently?" (`widgets/DeletePermanently.tsx`) is a destructive
  `ConfirmDialog`; ×, Esc and Cancel forget the pending question (it used to
  stay until the next one).
- New theme / Rename theme (`features/appearance/ThemeDialogs.tsx`) use
  `DialogFooter` (Create / Rename, Cancel); New theme is a `DialogForm` with a
  close guard (a typed name asks before it is lost).

## Scope decisions

- The packages migrate feature dialogs only; they do not change the shared
  pieces (a needed change goes to the integrator).
- Pickers (Move to folder, the command palette) keep no footer: a pick acts
  and closes, × cancels.
- About Nail, Configuration errors, the confirmation center and other
  information windows keep × only.
- The OS window's own close (the desktop app's title bar, a browser tab) is
  not guarded in this wave.
- Plug-in dialogs get roles from the plug-in and are ordered by Nail; the
  plug-in API stays as it is (Nail adds Cancel).

## Consequences

- One look and one behaviour for every dialog; themes style `dialog.*` once
  instead of about 45 parts.
- Many end-to-end tests click buttons by name; labels such as "Delete" →
  "Delete folder" and "OK" → "Unlock" change them in the packages' specs.
- Settings behave differently from before: switches and choices no longer
  apply at once; users press Save (or are asked when they leave).
