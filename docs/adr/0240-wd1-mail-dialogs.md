# 0240 — R6 WD1: the mail dialogs in the shared pattern

Status: accepted (2026-11-22)

## Context

ADR 0239 set one dialog pattern and its shared pieces (`DialogFooter`,
`ConfirmDialog`, `DialogForm`, the window layer's close guard). WD1 moves
the mail dialogs to it: the composer, folders, Gmail labels, Exchange
categories / mailboxes / automatic replies / Focused, Print, the message
source, Snooze, the move window's question, the Outbox and the plug-in send
veto. A few of them needed a choice the contract left open.

## Decision

- **Composer close.** The composer's own close confirmation
  (`composer-close:<key>`) goes. Each composer window registers the close
  guard (`ComposerCloseGuard`): the shared prompt asks with **Save draft**
  (the guard's `saveLabel`), **Don't save** (the last saved draft stays) and
  **Cancel**. Only instance composers (`composer:<key>`) register it, as only
  they asked before; the plain `composer` window is replaced by an instance
  as soon as one opens. The composer sessions no longer put a closed unsaved
  composer back themselves: they keep its state and the close guard puts
  the window back. Because the guard changes the store while it notifies,
  the sessions reconcile from the state they last saw to the current one
  (`catchUp`), not from the notification's pair.
- **Send.** The composer's Send is a plain button: `Enter` in Subject never
  sends; `Ctrl+Enter` (the layout's `composer.send`) does.
- **Discard.** "Discard draft" is a destructive `ConfirmDialog`; its cancel
  role keeps the label **Keep editing**. In the Outbox a draft's Discard asks
  in the draft's row (a destructive `ConfirmDialog`), as a deleted draft
  cannot be restored (no Undo).
- **Move question.** The question before Drafts, Trash or Junk is a normal
  confirmation (Move focused, `Enter` moves), since the move can be undone;
  this replaces ADR 0230's "focus on Cancel". Its Cancel and `Esc` go back
  to the picker instead of closing the window.
- **Print.** Print (primary), Save as PDF… (secondary), Cancel; on the PDF
  step Save PDF is the primary and Print the secondary. A saved PDF shows
  where it went and one **Done** (rule 3: a finished task shows one Done),
  instead of closing without saying where the file is.
- **Editors** (folder name, Gmail label, Exchange category / mailbox,
  Automatic replies, Apply labels / categories) register the close guard;
  after their own Save they close with `useCloseNow`
  (`ui/src/widgets/DialogStates.tsx`, `closeWindowNow` for the dialog's
  window), so the guard does not ask about what was just saved. Apply labels
  / categories share `useTagDialog` / `useTagDialogGuard` there.
- **Empty states** (`EmptyDialog`, a gone composer, a gone source window)
  show one **Done**. `CloseButton` stays for the print view's toolbar and for
  the dialogs of other packages until they move to the footer.
- **Message source** is a tool window: Copy all, no footer Close.
- **Send veto**: a destructive `ConfirmDialog` — Send anyway (red), Cancel
  (focused); an expired veto disables Send anyway.

## Consequences

- The `composer.confirm.*`, `folder-dialog.actions`, `gmail.dialog.actions`,
  `snooze.actions`, `plugin-veto.buttons` and `plugin-veto.intro` parts are
  gone; the `DIALOG_BUTTON_ROWS` entries of the composer confirmation, the
  Gmail dialogs and Snooze go with them.
- End-to-end tests click "Discard draft", "Delete folder", "Save" (Apply
  labels / categories) and Cancel (send veto); the composer's unsaved prompt
  is the window `unsaved-changes:composer_<key>`.
