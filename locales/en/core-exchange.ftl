# Nail — English texts about Exchange accounts built in Rust (M3c;
# contract ADR 0072): warnings, mailbox / category / automatic-reply
# problems, Focused Inbox, engine and colour names, and, from WP93, the
# common layer's texts (Autodiscover, Windows sign-in, rule placement
# reasons `exchange-rule-reason-*`).
#
# Owned by WP93 (nail-exchange). The ids named by the contract are
# pre-filled; nail-api/tests/catalog.rs checks them.

## Engines (ExchangeEngine::message_id)

exchange-engine-graph = Microsoft Graph (Exchange Online)
exchange-engine-ews = Exchange Web Services (Exchange Server)

## Warnings (ExchangeWarning)

exchange-warning-ews-retired-online = Microsoft is retiring Exchange Web Services in Exchange Online. Switch this account to Microsoft Graph.
exchange-warning-focused-inbox-unavailable = The server does not sort mail into Focused and Other.
exchange-warning-rooms-need-consent = Rooms need the "Place.Read.All" permission, which your organization's administrator has not granted.
exchange-warning-throttled = The server is limiting requests; syncing is slower for a while.

## Focused Inbox (InferenceClassification::message_id)

exchange-focus-focused = Focused
exchange-focus-other = Other

## Shared and delegate mailboxes (ExchangeMailboxIssue, ExchangeMailboxState)

exchange-mailbox-issue-missing-address = Enter the mailbox's address.
exchange-mailbox-issue-invalid-address = This is not an email address.
exchange-mailbox-issue-own-mailbox = This is the account's own mailbox.
exchange-mailbox-issue-duplicate = This mailbox is already open.
exchange-mailbox-issue-empty-name = The name cannot be blank.
exchange-mailbox-issue-nothing-shown = Choose at least one of mail, calendar, contacts or tasks.
exchange-mailbox-state-pending = Not synced yet
exchange-mailbox-state-synced = Synced
exchange-mailbox-state-access-denied = No access to this mailbox
exchange-mailbox-state-not-found = No such mailbox
exchange-mailbox-state-error = Syncing failed

## Categories (ExchangeCategoryIssue, CategoryColor::message_id)

exchange-category-issue-missing-name = A category needs a name.
exchange-category-issue-name-too-long = The category name is too long.
exchange-category-issue-invalid-name = Category names cannot contain commas or semicolons.
exchange-category-issue-duplicate-name = A category with this name already exists.
exchange-color-none = No colour
exchange-color-preset0 = Red
exchange-color-preset1 = Orange
exchange-color-preset2 = Brown
exchange-color-preset3 = Yellow
exchange-color-preset4 = Green
exchange-color-preset5 = Teal
exchange-color-preset6 = Olive
exchange-color-preset7 = Blue
exchange-color-preset8 = Purple
exchange-color-preset9 = Cranberry
exchange-color-preset10 = Steel
exchange-color-preset11 = Dark steel
exchange-color-preset12 = Gray
exchange-color-preset13 = Dark gray
exchange-color-preset14 = Black
exchange-color-preset15 = Dark red
exchange-color-preset16 = Dark orange
exchange-color-preset17 = Dark brown
exchange-color-preset18 = Dark yellow
exchange-color-preset19 = Dark green
exchange-color-preset20 = Dark teal
exchange-color-preset21 = Dark olive
exchange-color-preset22 = Dark blue
exchange-color-preset23 = Dark purple
exchange-color-preset24 = Dark cranberry

## Automatic replies (AutomaticRepliesIssue)

exchange-oof-issue-empty-internal-reply = Write the reply for people in your organization.
exchange-oof-issue-empty-external-reply = Write the reply for people outside your organization.
exchange-oof-issue-reply-too-long = The reply is too long.
exchange-oof-issue-invalid-time = The start or end time is not valid.
exchange-oof-issue-missing-schedule = Choose when the replies start and end.
exchange-oof-issue-end-before-start = The end must be after the start.
