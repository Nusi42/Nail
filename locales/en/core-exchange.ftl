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
exchange-color-preset0 = { common-red }
exchange-color-preset1 = { common-orange }
exchange-color-preset2 = Brown
exchange-color-preset3 = Yellow
exchange-color-preset4 = { common-green }
exchange-color-preset5 = Teal
exchange-color-preset6 = Olive
exchange-color-preset7 = { common-blue }
exchange-color-preset8 = { common-purple }
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
exchange-oof-issue-end-before-start = { common-end-after-start }

## Autodiscover (WP93, nail-exchange autodiscover)

exchange-autodiscover-invalid-address = Enter a complete email address, such as name@example.com.
exchange-autodiscover-not-found = No Exchange settings were found for this address.
exchange-autodiscover-confirm-redirect = The server for { $domain } sends you to { $host }. Do you want to send your sign-in to { $host }?
exchange-autodiscover-needs-login = { $host } asks you to sign in to look up your settings.
exchange-autodiscover-too-many-redirects = The setup lookup was redirected too many times.
exchange-autodiscover-online = Exchange Online found. The account uses Microsoft Graph.
exchange-autodiscover-on-premises = Exchange Server found at { $host }. The account uses Exchange Web Services.

## Windows and server sign-in (WP93, nail-exchange negotiate / sspi)

exchange-signin-needs-login = The server asks you to sign in.
exchange-signin-no-common-method = The server offers no sign-in method Nail can use ({ $methods }).
exchange-signin-insecure-basic = Nail does not send your password over a connection without encryption.
exchange-signin-rejected = The server rejected the sign-in. Check the user name and password.
exchange-signin-windows-failed = Windows sign-in failed on this computer.
exchange-signin-needs-password = Enter the password for this user name.

## Rule placement: why a rule runs in Nail (WP93, nail-exchange mapping::rules)
## $position is the condition's place in the rule (1 = the whole rule), $action the action's number.

exchange-rule-reason-query = Condition { $position } is a search, which Exchange rules cannot run.
exchange-rule-reason-condition = Exchange rules cannot test condition { $position }.
exchange-rule-reason-text-test = Condition { $position } compares text in a way Exchange rules cannot (they only test "contains", without case).
exchange-rule-reason-address-field = Exchange rules cannot test the address field of condition { $position }.
exchange-rule-reason-header = Exchange rules cannot test the value of the header in condition { $position }.
exchange-rule-reason-tag = Condition { $position } tests a tag, which a new message does not have yet.
exchange-rule-reason-attachment = Exchange rules cannot test attachment names or types (condition { $position }).
exchange-rule-reason-folder = Condition { $position } tests a folder other than the Inbox.
exchange-rule-reason-structure = Exchange rules cannot combine the conditions this way (condition { $position }).
exchange-rule-reason-untag = Exchange rules cannot remove a tag (action { $action }).
exchange-rule-reason-flag = Exchange rules cannot flag or unflag messages (action { $action }).
exchange-rule-reason-mark-unread = Exchange rules cannot mark messages as unread (action { $action }).
exchange-rule-reason-reply = Replies with a template are sent by Nail (action { $action }).
exchange-rule-reason-forward-identity = Forwarding from a chosen identity is done by Nail (action { $action }).
exchange-rule-reason-notify = Notifications are shown by Nail (action { $action }).
exchange-rule-reason-other-account = Action { $action } moves or copies into another account.
exchange-rule-reason-targets = The rule moves, archives or deletes in more than one way (action { $action }).
exchange-rule-reason-category-name = The tag of action { $action } cannot be an Exchange category (commas and semicolons are not allowed).
exchange-rule-reason-address = Action { $action } forwards to something that is not an email address.
exchange-rule-reason-no-actions = The rule has no action Exchange can run.
exchange-rule-reason-disabled = { sieve-reason-disabled }
exchange-rule-reason-trigger = The rule also runs at other times than on arrival.
exchange-rule-reason-no-arrival = The rule does not run when mail arrives.
exchange-rule-reason-scope = The rule is limited to other folders or accounts.
exchange-rule-reason-folder-unknown = The folder "{ $folder }" is not on the server yet.
exchange-rule-reason-server-rules-off = Server rules are switched off for this account.
exchange-rule-reason-pending = The rule is not on the server yet.
exchange-server-rule-unsupported = This server rule uses settings that Nail rules cannot show.

## Automatic replies and the vacation settings (WP93, nail-exchange mapping::oof)

exchange-oof-note-subject = Exchange chooses the subject of automatic replies; your subject is not used.
exchange-oof-note-days = Exchange decides how often the same sender gets a reply.
exchange-oof-note-addresses = Extra addresses and the sender address are not used for Exchange accounts.
exchange-oof-note-external = People outside your organization get a different reply, set in Exchange.
exchange-oof-note-open-end = Without an end date, replies stop after one year.
exchange-oof-note-rounded = The schedule's times were rounded to whole days.

## Calendars, tasks and contacts (WP93, nail-exchange mapping)

exchange-pim-reason-recurrence = Exchange cannot store this repeat pattern.
exchange-pim-reason-zone = The time zone "{ $zone }" is unknown; the time is shown in UTC.
exchange-pim-reason-extra-dates = Exchange cannot store extra dates or several repeat rules.
exchange-pim-reason-floating = A time without a time zone was saved in the mailbox's time zone.
