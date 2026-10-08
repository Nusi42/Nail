# Nail — English texts of mail actions on conversations named by the P3
# contract (ADR 0164): what an action did (labels for undo toasts,
# confirmations and audit lines) and why an action or an undo was refused.
#
# The ids named by nail_api::mail_actions are pre-filled;
# nail-api/tests/catalog.rs checks them. The UI's own texts (toasts, the
# undo button, confirmations) are in mail-actions.ftl (WP-actions-ui).

## What an action did (nail_api::mail_actions::MailActionKind)

mail-action-kind-archive = Archived
mail-action-kind-trash = Moved to Trash
mail-action-kind-delete-permanently = Deleted permanently
mail-action-kind-move = Moved
mail-action-kind-mark-read = Marked as read
mail-action-kind-mark-unread = Marked as unread
mail-action-kind-flag = { common-flagged }
mail-action-kind-unflag = Unflagged
mail-action-kind-tags = Tags changed
mail-action-kind-snooze = Snoozed
mail-action-kind-junk = Marked as junk
mail-action-kind-not-junk = Marked as not junk

## Why an action or an undo was refused (MailActionIssue)

mail-action-issue-no-targets = Nothing is selected.
mail-action-issue-too-many-targets = Too many conversations at once; select fewer.
mail-action-issue-too-many-messages = Too many messages at once; select fewer conversations.
mail-action-issue-no-messages = There is nothing to do for this selection.
mail-action-issue-nothing-to-change = Choose at least one tag to add or remove.
mail-action-issue-not-in-trash = Only messages in Trash or Junk can be deleted permanently.
mail-action-issue-undo-expired = This action can no longer be undone.
mail-action-issue-undone-already = This action was undone already.
mail-action-issue-not-undoable = Messages deleted permanently cannot be restored.

## Confirmations of agents and plug-ins acting on conversations
## (update_messages / delete_messages with `conversations`, nail.mail.update /
## nail.mail.delete; WP-actions-core): the action and the message count.

mail-action-confirmation-organize = { $actor } wants to change { $conversations ->
    [one] a conversation
   *[other] { $conversations } conversations
} ({ $count ->
    [one] one message
   *[other] { $count } messages
}).
mail-action-confirmation-trash = { $actor } wants to move { $conversations ->
    [one] a conversation
   *[other] { $conversations } conversations
} to Trash ({ $count ->
    [one] one message
   *[other] { $count } messages
}).
mail-action-confirmation-permanent-delete = { $actor } wants to delete { $conversations ->
    [one] a conversation
   *[other] { $conversations } conversations
} permanently ({ $count ->
    [one] one message
   *[other] { $count } messages
}). This cannot be undone.
