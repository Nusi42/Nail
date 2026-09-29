# Nail — English UI strings (complete catalog).
#
# Message ids use hyphens. Layout labels written as "@mail.action.reply" map
# to the id "mail-action-reply" (dots become hyphens).
# Every file here has a counterpart with the same name in each other locale
# directory; missing keys fall back to English per key.

app-name = Nail

## Panes (layout labels)

layout-pane-folders = Folders
layout-pane-messages = Messages
layout-pane-message = Message

## Mail actions

mail-action-next = Next message
mail-action-previous = Previous message
mail-action-reply = Reply
mail-action-reply-all = Reply all
mail-action-forward = Forward
mail-action-archive = Archive
mail-action-delete = Delete
mail-action-toggle-read = Mark as read/unread
mail-action-toggle-flag = Flag

## View and sync actions

view-action-toggle = Show/hide pane
sync-action-now = Check for new mail

## Errors (from nail_api::ErrorCode, "error-" + code with "_" → "-")

error-invalid-request = The request was not valid.
error-not-found = The item no longer exists.
error-unsupported = This is not supported yet.
error-auth-failed = The server rejected the login.
error-network = The server could not be reached.
error-config = The configuration is invalid.
error-unavailable = Nail is busy. Please try again.
error-internal = Something went wrong inside Nail.
