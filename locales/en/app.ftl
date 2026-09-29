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

## App shell

app-loading = Loading…

## Layout engine

layout-missing-window = This layout has no window named “{ $name }”.
layout-unknown-widget = Unknown widget “{ $kind }”.
layout-divider = Resize panes
layout-tab-untitled = Tab { $index }

## Accounts and folders

account-switcher-label = Accounts
# Name of the built-in "All accounts" virtual account (id "all").
account-all = All accounts
folder-tree-label = Folders
folder-tree-unread = { $count ->
    [one] One unread message
   *[other] { $count } unread messages
}

## Message list

message-list-label = Messages
message-list-empty = No messages in this folder.
message-list-loading = Loading messages…

## Message view

message-view-label = Message
message-view-empty = Select a message to read it.
message-view-loading = Loading message…
message-view-body-title = Message content: { $subject }
message-view-remote-blocked = Remote content in this message was blocked to protect your privacy.
message-view-attachments = Attachments
message-view-attachment-unnamed = Unnamed attachment
message-no-subject = (no subject)
message-header-from = From
message-header-to = To
message-header-cc = Cc
message-header-reply-to = Reply to
message-header-date = Date

## Toolbar

toolbar-label = Message actions

## Status bar

status-ready = Up to date
status-offline = Offline – reconnecting…
status-syncing = Syncing… { $done } of { $total }
status-sync-failed = Synchronization failed: { $reason }
status-config-error = Configuration error in { $file }, line { $line }, column { $column }. Using the last valid version.
status-folder-counts = { $total ->
    [one] One message
   *[other] { $total } messages
}, { $unread } unread

## Errors (from nail_api::ErrorCode, "error-" + code with "_" → "-")

error-invalid-request = The request was not valid.
error-not-found = The item no longer exists.
error-unsupported = This is not supported yet.
error-auth-failed = The server rejected the login.
error-network = The server could not be reached.
error-config = The configuration is invalid.
error-unavailable = Nail is busy. Please try again.
error-internal = Something went wrong inside Nail.
error-conflict = This conflicts with the current state. Please reload and try again.
error-tls = A secure connection to the server could not be established.
error-too-large = The message is larger than the server accepts.
error-rejected = The server refused the request.
error-cancelled = The operation was cancelled.

## Account status (from nail_api::AccountState, "account-state-" + state)

account-state-online = Online
account-state-connecting = Connecting…
account-state-offline = Offline
account-state-auth-error = Sign-in required
account-state-disabled = Disabled

## Search query errors (from nail_api::SearchErrorKind, "search-error-" + kind)

search-error-unexpected-token = This word cannot appear here.
search-error-unexpected-end = The search ends too early.
search-error-unterminated-quote = A quotation mark is not closed.
search-error-unbalanced-parenthesis = The parentheses do not match.
search-error-unknown-field = Unknown search field.
search-error-invalid-value = This value does not fit the search field.
