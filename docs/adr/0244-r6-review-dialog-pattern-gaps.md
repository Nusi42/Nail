# 0244 — R6 review: the dialog pattern's gaps

Status: accepted (2026-11-22)

## Context

The review of wave R6 (ADRs 0239–0243) found places the one dialog pattern
did not reach, and copies of its logic:

- Deleting contacts (the card's Delete, the list's "Delete N" and its
  Delete key) asked nothing, and the contact editor had no `Delete…`.
- `MailConfirm` still rendered its own button row for the bulk, open-link
  and open-attachment questions (fixed order, Cancel always focused), and
  the window sweep skipped windows without `dialog.footer`.
- Settings → Keyboard (the key-binding editor), the plug-in manager (on /
  off, permissions, a plug-in's settings) and Settings → Server rules for
  Exchange (the account switch, each inbox rule's switch) applied at once;
  the inbox rule's delete was an inline "Are you sure" row.
- Calendar settings, New calendar / task list and the contact group editor
  dropped edits on ×, Esc or Cancel (a group also on picking another).
- The invitation answer's "Send?" step and the Propose / Forward forms had
  hand-written button rows.
- Popping Settings (or the rules window) out, or returning it, lost unsaved
  drafts: the close guard put the window back, the page then unmounted it
  and the prompt's Save wrote nothing.
- The "pending question per UI store" window was copied five times; the
  JSON equality helper four times plus inline copies.
- The settings save bar was silent for screen readers, and the focus fell
  to the page after Save or Discard.
- Cancel during the prompt's Save still closed the window when the save
  ended.

## Decision

- **One question window** (`ui/src/widgets/dialog/QuestionWindow.tsx`):
  `askQuestion(ui, kind, question)` / `questionOf` / `QuestionWindow`
  (question, detail, verb, `risk` — destructive by default —, optional
  check box, `run`; a failure stays with the core's reason; a window opened
  without a question says so and offers Done, `ConfirmGone`). The settings
  sections (`askSettingsQuestion`), the PIM deletes (`askPim`) and the
  Exchange inbox-rule delete use it. **One pending slot**
  (`widgets/dialog/pending.ts`): `pendingSlot`, `askInWindow`,
  `usePendingQuestion` (read once, forgotten however the window goes) for
  the windows with their own content (bulk, open link / attachment, Delete
  permanently, connection log Clear, plug-in removal). `MailConfirm` is
  removed; its questions are `ConfirmDialog`s — opening a flagged link or
  attachment is destructive (Cancel focused), a bulk action is destructive
  only for Trash and Delete permanently, otherwise its verb has the focus.
- **Contacts**: `pim-delete-contacts` joins `PIM_CONFIRMS`
  (`askDeleteContacts`: "Delete the contact “X”?" / "Delete N contacts?",
  verb "Delete contact" / "Delete N contacts"); the card, the list, its
  Delete key and the context menu ask; the contact editor gets `Delete…` at
  the start.
- **Settings drafts everywhere**: the key-binding editor keeps added,
  removed and reset shortcuts as `LayoutPatch` changes (shown in the table,
  marked "not saved") and writes them in one `UpdateLayout` with Save; keys
  recorded but not added count as a change (Save adds them). The plug-in
  manager's switch, permissions and settings use `useSettingsForm`; the
  manager's own window (`plugin-manager`) shows the save bar and registers
  the settings guard. The Exchange account switch and inbox-rule switches
  are drafts; Delete… asks ("Delete rule").
- **Guards**: the calendar dialogs save through `useEditorSave` (the shared
  close guard); the contact group editor registers `useCloseGuard` and
  asks with `askToLeave` before another group shows or a new one is
  created.
- **Footers**: the answer's "Send?" step is a `ConfirmDialog` (Send, "Only
  update my calendar", Back); Propose and Forward use `DialogFooter`.
- **Pop-outs**: `popOutWindow` and "Return to main window" first await
  `confirmLeave` (the close guard's prompt as a promise) unless the kind's
  preparer hands the changes over (`handsOverChanges`: the composer saves
  its draft); the window then leaves with `closeWindowNow`.
- **`sameJson`** (`ui/src/text/json.ts`) is the one plain-data equality;
  the former `sameArgs`, `sameList` and the dirty checks import it.
- **Save bar**: a polite live status (`settings-save-bar.status`, visually
  hidden) says "Unsaved changes to “X”", "Changes saved." or "Changes
  discarded."; after Save or Discard pressed in the bar the focus moves to
  `focusAfter` (the section's title), a failed Save gives it back to Save.
- **Prompt**: Cancel during Save marks the prompt cancelled; the save's end
  then neither closes nor moves on.
- The sweep (`expectDialogPattern`) fails on a `[data-confirm]` outside
  `dialog.confirm` and on a modal window with buttons but no
  `dialog.footer`. `DIALOG_BUTTON_ROWS` drops `mail-confirm.actions`.

## Scope decisions

- Settings drafts are kept per UI store, not per window: the plug-in
  manager window and the Settings page see the same drafts while both are
  open (both bars show them; either Save saves them).
- No marker on the settings navigation for a section with unsaved changes
  (the prompt asks when it is left).
- The contact group editor stays a tool window with its own Save row (no
  footer Close); the "New group" name is not guarded.

## Consequences

- Every confirmation renders through `ConfirmDialog`; a new kind of
  question is one `askQuestion` call plus a registered window kind.
- Tests: Vitest for each fix (contacts, bulk focus, key bindings, plug-ins,
  server rules, calendar / group guards, answer footer order, pop-out both
  ways, `confirmLeave`, the prompt's cancelled save, the save bar's status
  and focus, the pending slot and question window); e2e
  `r6-review-dialogs.spec.ts`.
