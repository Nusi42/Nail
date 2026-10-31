# Nail — English UI strings (complete catalog).
#
# Message ids use hyphens. Layout labels written as "@mail.action.reply" map
# to the id "mail-action-reply" (dots become hyphens).
# Every file here has a counterpart with the same name in each other locale
# directory; missing keys fall back to English per key.

app-name = { common-nail }

## Panes (layout labels)

layout-pane-folders = { common-folders }
layout-pane-messages = { common-messages }
layout-pane-message = { common-message }
layout-menu-view = { common-view }
toolbar-main = Main toolbar
toolbar-customize = Customization
settings-tab-accounts = { common-accounts }
settings-tab-virtual-accounts = { common-virtual-accounts }
settings-tab-customize = Customize

## Mail actions

mail-action-next = Next message
mail-action-previous = Previous message
mail-action-reply = { common-reply }
mail-action-reply-all = Reply all
mail-action-forward = { common-forward }
mail-action-archive = { common-archive }
mail-action-delete = { common-delete }
mail-action-toggle-read = Mark as read/unread
mail-action-toggle-flag = { common-flag }
mail-action-compose = { common-new-message }
mail-action-move = Move to…
mail-action-move-to = Move
mail-action-delete-permanently = Delete permanently

## Composer, outbox, search, window and account actions

composer-action-send = { common-send }
composer-action-save = { common-save-draft }
composer-action-discard = { common-discard }
outbox-action-open = { common-outbox }
outbox-action-undo = Undo send
search-action-focus = { common-search }
search-action-clear = { search-bar-clear }
window-action-open = Open window
window-action-close = Close window
account-action-add = { common-add-account }
settings-action-open = { common-settings }

## View and sync actions

view-action-toggle = Show/hide pane
sync-action-now = Check for new mail

## App shell

app-loading = { common-loading }

## Layout engine

layout-missing-window = This layout has no window named “{ $name }”.
layout-unknown-widget = Unknown widget “{ $kind }”.
layout-divider = { edit-kind-split-sizes }
layout-tab-untitled = Tab { $index }

## Accounts and folders

# Name of the built-in "All accounts" virtual account (id "all").
account-all = { common-all-accounts }
folder-tree-unread = { $count ->
    [one] One unread message
   *[other] { $count } unread messages
}
# Names of special-use folders in the folder pane and the list title
# (the server's own name, e.g. "INBOX", is the tooltip).
folder-role-inbox = { common-inbox }
folder-role-drafts = { common-drafts }
folder-role-sent = { common-folder-sent }
folder-role-archive = { common-folder-archive }
folder-role-junk = { common-junk }
folder-role-trash = { common-trash }

## Message list

message-list-empty = No messages in this folder.
message-list-loading = Loading messages…
message-list-search-results = { $count ->
    [0] No results for “{ $query }”
    [one] One result for “{ $query }”
   *[other] { $count } results for “{ $query }”
}
message-list-search-empty = No messages match this search.
message-list-search-invalid = The search could not be run. Check the query above.
# Column titles of a list with `columns` (Settings → Customize → Columns).
message-list-column-from = { common-header-from }
message-list-column-subject = { common-subject }
message-list-column-date = { common-date }
message-list-column-preview = { common-preview }
message-list-column-tags = { action-field-tags }
message-list-column-account = { common-account }

## Thread view

thread-view-label = Conversation
thread-view-show-quote = Show quoted text

## Search bar and query-language hints

search-bar-placeholder = Search, e.g. from:ada is:unread
search-bar-clear = Clear search
search-bar-hints = Search fields
search-error-position = At character { $column }
search-hint-from = Sender name or address
search-hint-to = Recipient name or address
search-hint-subject = Words in the subject
search-hint-has-attachment = Messages with attachments
search-hint-is-unread = Unread messages
search-hint-is-read = Read messages
search-hint-is-flagged = Flagged messages
search-hint-is-unflagged = Messages without a flag
search-hint-is-answered = Messages you replied to
search-hint-tag = Messages with a tag
search-hint-account = Messages of one account
search-hint-folder = Messages in a folder
search-hint-category = Gmail inbox category (primary, social, promotions, updates, forums)
search-hint-before = Sent before a date (YYYY-MM-DD)
search-hint-after = Sent on or after a date (YYYY-MM-DD)
search-hint-size = Larger than a size, e.g. size>2M (or size< for smaller)
search-hint-operators = Combine terms with AND, OR, NOT and parentheses; quote phrases with "…".

## Move to folder

folder-picker-filter = Find a folder
folder-picker-no-match = No folder matches.
folder-picker-no-selection = Select a message first.

## Secondary windows ("window-" + the layout window name)

window-mail = { common-mail }
window-composer = { common-new-message }
window-outbox = { common-outbox }
window-move = { common-move-to-folder }
window-account-setup = { common-add-account }
window-settings = { common-settings }
window-mail-delete-permanently = Delete permanently?

## "Delete permanently?" (a modal confirmation, M5c)

mail-delete-permanently-question = { $count ->
    [one] Delete this message permanently?
   *[other] Delete { $count } messages permanently?
}
mail-delete-permanently-note = This cannot be undone.

## Links in mail (ADR 0206): the reader's link status and "Open this link?"

window-mail-open-link = Open this link?
# The status overlay's accessible text while a link is hovered or focused.
mail-link-status = Link to { $href }
mail-open-link-question = This link may not go where it seems to.
mail-open-link-open = Open anyway

## Message view

message-view-empty = Select a message to read it.
message-view-loading = Loading message…
message-view-body-title = Message content: { $subject }
# The reader leaves out the images the message body shows; this button lists them too. $count = all attachments.
message-view-attachments-show-all = Show all attachments ({ $count })
message-view-attachments-hide-embedded = Hide images shown in the message
message-header-from = { common-header-from }
message-header-to = { common-header-to }
message-header-cc = { common-cc }
message-header-reply-to = { common-reply-to }
message-header-date = { common-date }

## Status bar

status-ready = { pim-source-state-idle }
status-offline = Offline – reconnecting…
status-syncing = Syncing… { $done } of { $total }
status-sync-failed = Synchronization failed: { $reason }
status-config-error = Configuration error in { $file }, line { $line }, column { $column }. Using the last valid version.
status-account = { $account }: { $state }
status-accounts = { $account }: { $state } (and { $count } more)
status-ops-pending = { $count ->
    [one] Saving one change…
   *[other] Saving { $count } changes…
}
status-op-failed = A change could not be saved: { $reason }
# Queues a failed change again.
status-retry = Retry
status-send-countdown = { $seconds ->
    [one] Sending in one second
   *[other] Sending in { $seconds } seconds
}
status-send-scheduled = Scheduled to send { $time }
status-sending = Sending…
# Upload of a large message (Microsoft 365 sends it in pieces).
status-sending-progress = Sending… { $percent }% uploaded
status-sent = Message sent.
status-send-failed = Sending failed: { $reason }
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
error-cancelled = { common-operation-cancelled }
error-locked = A key is locked. Enter its passphrase and try again.
error-no-key = A needed key or certificate is missing or cannot be used.
error-crypto = Signing, encryption or verification failed.
error-forbidden = You are not allowed to do this.
error-reauth-required = Confirm it is you: sign in again to continue.
error-rate-limited = Too many attempts. Please wait a moment and try again.

## Account status (from nail_api::AccountState, "account-state-" + state)

account-state-online = Online
account-state-connecting = Connecting…
account-state-offline = { pim-source-state-offline }
account-state-auth-error = Sign-in required
account-state-disabled = Disabled

## Search query errors (from nail_api::SearchErrorKind, "search-error-" + kind)

search-error-unexpected-token = This word cannot appear here.
search-error-unexpected-end = The search ends too early.
search-error-unterminated-quote = A quotation mark is not closed.
search-error-unbalanced-parenthesis = The parentheses do not match.
search-error-unknown-field = Unknown search field.
search-error-invalid-value = This value does not fit the search field.
search-error-too-complex = This search is too long or has too many nested parts.

## OAuth2 sign-in: pages the browser shows after returning to Nail (rendered by the core)

oauth-callback-title = Nail sign-in
oauth-callback-success = Signed in. You can close this window and return to Nail.
oauth-callback-failure = Sign-in did not complete. Close this window and try again in Nail.

## Opening links and files (mailto:, .eml, .ics, .vcf; rendered by the core and the UI)

# Name of the local account that opened .eml files are shown in.
opened-files-account = Opened files
opened-files-folder = { opened-files-account }
launch-notice-label = Opening links and files
launch-open-failed = The file could not be opened: { $reason }

## Mail view (M5b WP162): toolbars, folder pane, list header

# Tooltip of a toolbar button with its key binding, e.g. "Archive (E)".
# $label (string) — the action; $shortcut (string) — the key, as shown in menus
toolbar-button-tooltip = { common-tooltip-shortcut }
# $count (number) — drafts in every account (Favorites → Drafts)
folder-tree-drafts = { $count ->
    [one] One draft
   *[other] { $count } drafts
}
folder-tree-outbox = { $count ->
    [one] One message waiting to be sent
   *[other] { $count } messages waiting to be sent
}
# The heading of an account's section in the folder pane.
# $name (string) — the account's name; $email (string) — its address
folder-tree-account-title = { $name } · { $email }
# $count (number) — unread messages in the folder the list shows
list-header-unread = { $count ->
    [0] Nothing unread
   *[other] { $count } unread
}
# $count (number) — accounts a unified folder covers ("All inboxes")
list-header-accounts = { $count ->
    [one] One account
   *[other] { $count } accounts
}
# $unread (string) — list-header-unread; $accounts (string) — list-header-accounts
list-header-counts = { $unread } · { $accounts }
# Accessible name of the filter chips (All, Unread, Flagged, Attachments).
list-header-filters = Filter messages
message-list-filter-empty = No messages match this filter.
# An attachment card while the attachment is being saved.
attachment-card-saving = { common-saving }
# Command-palette titles (actions with an argument, used through aliases).
list-action-filter = Filter the message list
list-action-filter-keywords = unread flagged attachments chips
folders-action-toggle-section = Collapse or expand a folder section
