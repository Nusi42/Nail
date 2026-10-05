# Nail — English UI strings: Exchange accounts (shared mailboxes and
# delegate folders, room finder, out-of-office editor, rule placement
# badges, Focused Inbox, To Do lists; M3c WP94; contract ADR 0072).
#
# Owned by WP94; message ids start with the prefixes listed in
# ui/src/features/exchange/messages.ts. Texts the core builds (warnings,
# problems, Focused Inbox and colour names) are in core-exchange.ftl.

## Dialog titles

window-exchange-mailbox-editor = Shared mailbox
window-exchange-mailbox-remove = Remove mailbox
window-exchange-category-editor = Category
window-exchange-category-delete = Delete category
window-exchange-automatic-replies = Automatic replies
window-exchange-classify = Focused Inbox
window-exchange-apply-categories = Apply categories

## Actions (layout labels @exchange.action.*)

exchange-action-automatic-replies = Automatic replies…
exchange-action-add-mailbox = Add shared mailbox…
exchange-action-new-category = New category
exchange-action-focus = Show Focused / Other
exchange-action-classify = Move to Focused / Other…
exchange-action-toggle-focused = Show Focused Inbox
exchange-action-apply-categories = Categories…

## Layout labels

exchange-settings-tab-rules = Exchange rules

## Shared words

exchange-save = Save
exchange-cancel = Cancel
exchange-close = Close
exchange-loading = Loading…
exchange-edit-short = Edit
exchange-delete-short = Delete
exchange-remove-short = Remove
exchange-pending = (not synced yet)
exchange-account = Account
exchange-no-accounts = There is no Exchange account.
exchange-nothing-chosen = Nothing is chosen. Close this window and start again.

## Categories

exchange-categories-label = Categories of { $account }
exchange-categories-title = Categories
exchange-categories-none = No categories yet.
exchange-category-new = New category
exchange-category-edit = Edit category { $name }
exchange-category-remove = Delete category { $name }
exchange-category-unread =
    { $count ->
        [one] 1 unread
       *[other] { $count } unread
    }
exchange-category-view =
    { $unread ->
        [0] { $name }, { $color }
        [one] { $name }, { $color }, 1 unread
       *[other] { $name }, { $color }, { $unread } unread
    }{ $pending ->
        [yes] , not synced yet
       *[no] {""}
    }
exchange-message-categories = Categories
exchange-category-name = Name
exchange-category-color = Colour
exchange-category-create = Create
exchange-category-rename-hint =
    { $count ->
        [0] The new name is used from now on.
        [one] The category is renamed on 1 message too.
       *[other] The category is renamed on { $count } messages too.
    }
exchange-category-delete-question = Delete the category “{ $name }”?
exchange-category-delete-hint =
    { $count ->
        [0] It is removed from the category list.
        [one] It is removed from the category list. The 1 message that has it keeps it, without a colour.
       *[other] It is removed from the category list. The { $count } messages that have it keep it, without a colour.
    }
exchange-category-delete = Delete
exchange-apply-intro = Categories of “{ $subject }”:
exchange-apply-nothing = Select a message of an Exchange account first.
exchange-apply = Apply

## Shared and delegate mailboxes

exchange-mailboxes-label = Other mailboxes of { $account }
exchange-mailboxes-title = Other mailboxes
exchange-mailboxes-none = No shared or delegate mailboxes are open.
exchange-mailbox-add = Add shared mailbox…
exchange-mailbox-edit = Edit mailbox { $name }
exchange-mailbox-remove = Remove mailbox { $name }
exchange-mailbox-automapped = added by your organization
exchange-mailbox-kind-shared = Shared mailbox
exchange-mailbox-kind-delegate = Delegate access
exchange-mailbox-kind-shared-long = A shared mailbox (for example a team's support address)
exchange-mailbox-kind-delegate-long = Another person's mailbox you have delegate access to
exchange-mailbox-show-mail = Mail
exchange-mailbox-show-calendar = Calendar
exchange-mailbox-show-contacts = Contacts
exchange-mailbox-show-tasks = Tasks
exchange-mailbox-intro = Open a mailbox you have permission to. Its folders appear under this account; its calendar, contacts and tasks with yours.
exchange-mailbox-address = Mailbox address
exchange-mailbox-name = Name
exchange-mailbox-name-hint = Leave empty to use the name the server gives it.
exchange-mailbox-kind = Kind of access
exchange-mailbox-shows = Show
exchange-mailbox-open = Open mailbox
exchange-mailbox-remove-question = Remove the mailbox “{ $name }”?
exchange-mailbox-remove-hint = Its folders, calendar, contacts and tasks disappear from Nail. Nothing changes on the server, and you can add it again.
exchange-mailbox-remove-confirm = Remove
exchange-mailbox-hide-question = Hide the mailbox “{ $name }”?
exchange-mailbox-hide-hint = Your organization added this mailbox for you, so Nail hides it instead of removing it. Add it again to show it.
exchange-mailbox-hide = Hide

## Focused Inbox

exchange-focused-label = Focused Inbox
exchange-focus-all = All
exchange-focused-switch = Focused Inbox
exchange-move-to-focused = Move to Focused
exchange-move-to-other = Move to Other
exchange-classify-question-focused =
    { $count ->
        [one] Move this message to Focused?
       *[other] Move these { $count } messages to Focused?
    }
exchange-classify-question-other =
    { $count ->
        [one] Move this message to Other?
       *[other] Move these { $count } messages to Other?
    }
exchange-classify-always-sender = Always move messages from { $sender }
exchange-classify-always = Always move messages from these senders

## Automatic replies (out of office)

exchange-oof-intro = Exchange answers messages for you while you are away, even when Nail is closed.
exchange-oof-status = Automatic replies
exchange-oof-status-disabled = Do not send automatic replies
exchange-oof-status-always-enabled = Send automatic replies
exchange-oof-status-scheduled = Send automatic replies only during this time
exchange-oof-schedule = When
exchange-oof-start = Start
exchange-oof-end = End
exchange-oof-zone = Times are in { $zone }.
exchange-oof-internal = Reply to people in my organization
exchange-oof-audience = Reply to people outside my organization
exchange-oof-audience-none = Do not reply
exchange-oof-audience-contacts-only = Only to my contacts
exchange-oof-audience-all = To everyone
exchange-oof-external = Reply to people outside my organization
exchange-oof-plain-text = Replies are plain text; formatting set in Outlook is replaced when you save here.
exchange-oof-pending = Saved in Nail; not on the server yet.
exchange-oof-fetched = Read from the server { $time }.
exchange-oof-saved = Saved.
exchange-oof-banner-on = Automatic replies are on for { $account }.
exchange-oof-banner-until = Automatic replies are on for { $account } until { $end }.
exchange-oof-banner-upcoming = Automatic replies for { $account } start { $start }.
exchange-oof-turn-off = Turn off
exchange-oof-edit = Edit…

## Rules

exchange-rule-badges-label = Where this rule runs on Exchange accounts
exchange-placement-server = { $account }: on the server
exchange-placement-client = { $account }: in Nail
exchange-placement-not-expressible = { $account }: in Nail only
exchange-placement-short-server = On the server
exchange-placement-short-client = In Nail
exchange-placement-short-not-expressible = In Nail only
exchange-rules-label = Exchange inbox rules
exchange-rules-intro = Rules that Exchange can run itself are placed as inbox rules on the server, so they work while Nail is closed. The others run in Nail.
exchange-rules-server-switch = Run rules on the server where Exchange can
exchange-rules-state-disabled = Rules run in Nail only.
exchange-rules-state-pending = Rule changes wait to be written to the server.
exchange-rules-state-syncing = Writing rules to the server…
exchange-rules-state-in-sync = The server's rules are up to date.
exchange-rules-state-failed = Writing rules to the server failed.
exchange-rules-synced-at = read { $time }
exchange-rules-sync = Sync now
exchange-rules-placements = Where your Nail rules run
exchange-rules-rule = Rule
exchange-rules-placement = Runs
exchange-rules-why = Why
exchange-rules-none = No Nail rule applies to this account.
exchange-rules-server-title = Inbox rules on the server
exchange-rules-server-none = The server has no inbox rules.
exchange-rules-from-nail = placed by Nail
exchange-rules-from-server = created in Outlook or on the web
exchange-rules-unsupported = Nail cannot show all of this rule; you can only switch it on or off or delete it.
exchange-rules-in-error = The server reports this rule as broken.
exchange-rules-delete = Delete
exchange-rules-delete-named = Delete inbox rule { $name }
exchange-rules-delete-question = Delete the inbox rule “{ $name }” from the server?
exchange-rules-delete-placed-question = Delete “{ $name }” from the server? Nail places it again on the next sync unless you change the Nail rule.

## Rooms

exchange-rooms-title = Rooms
exchange-rooms-booked = Booked rooms
exchange-rooms-find = Find a room…
exchange-rooms-hide = Hide rooms
exchange-rooms-list = Room list
exchange-rooms-all = All rooms
exchange-rooms-capacity = At least this many seats
exchange-rooms-only-free = Only rooms that are free
exchange-rooms-searching = Looking for rooms…
exchange-rooms-none = No room matches.
exchange-rooms-results = Rooms for this time
exchange-rooms-invalid-time = The event must end after it starts to find a room.
exchange-room-capacity =
    { $count ->
        [one] 1 seat
       *[other] { $count } seats
    }
exchange-room-floor = floor { $floor }
exchange-room-accessible = Wheelchair accessible
exchange-room-features = Equipment
exchange-room-free = Free
exchange-room-busy = Busy
exchange-room-unknown = Availability unknown
exchange-room-busy-label = Busy times
exchange-room-busy-period = { $start }–{ $end }
exchange-room-add = Add
exchange-room-add-named = Add room { $name }
exchange-room-remove = Remove
exchange-room-remove-named = Remove room { $name }
exchange-room-response-needs-action = Not answered yet
exchange-room-response-accepted = Accepted
exchange-room-response-declined = Declined
exchange-room-response-tentative = Tentative
exchange-room-response-delegated = Delegated

## Task lists (Microsoft To Do)

exchange-tasks-list = Task list
exchange-tasks-all = All lists
exchange-tasks-group-todo = Microsoft To Do — { $account }
exchange-tasks-group-mailbox = { $mailbox }
exchange-tasks-group-other = Other lists
exchange-tasks-kind-todo = To Do
exchange-tasks-kind-flagged = Flagged email
exchange-tasks-kind-tasks = Exchange tasks
exchange-tasks-kind-mailbox = Shared from { $mailbox }

## Account setup and settings

exchange-setup-type = Kind of account
exchange-setup-type-auto = Find the settings automatically
exchange-setup-type-graph = Microsoft 365 or Exchange Online
exchange-setup-type-ews = Exchange Server in my organization
exchange-setup-host = Exchange server
exchange-setup-host-hint = For example mail.example.com. Ask your administrator if you do not know it.
exchange-setup-found = Microsoft Exchange
exchange-setup-source = Found by
exchange-setup-engine = Connection
exchange-setup-server = Server
exchange-setup-url = EWS address
exchange-setup-url-hint = Leave empty for https://{ $host }/EWS/Exchange.asmx.
exchange-setup-engine-choice = Connect with
exchange-setup-redirect = The settings for { $domain } point to { $host }, which is not part of { $domain }. Continue only if your organization uses that server.
exchange-setup-redirect-confirm = I trust { $host }
exchange-sign-in = Sign in
exchange-sign-in-graph-hint = Microsoft 365 accounts sign in with Microsoft in your browser.
exchange-sign-in-windows-current = As the user signed in to Windows
exchange-sign-in-windows-account = With another Windows account
exchange-sign-in-password = With a password
exchange-sign-in-oauth2 = With Microsoft (modern authentication)
exchange-sign-in-windows-current-hint = Nail signs in like Outlook does and stores no password.
exchange-sign-in-windows-login = Windows account (DOMAIN\user)
exchange-sign-in-login = User name
exchange-sign-in-secret = Password
exchange-sign-in-package = Sign-in protocol
exchange-sspi-negotiate = Automatic (Kerberos, else NTLM)
exchange-sspi-kerberos = Kerberos only
exchange-sspi-ntlm = NTLM only
exchange-sign-in-basic-hint = Sent to the server over an encrypted connection; stored in the system's credential store.
exchange-sign-in-keychain-hint = Stored in the system's credential store.
exchange-sign-in-oauth-hint = You sign in with Microsoft in your browser; Nail never sees your password.
exchange-options-title = Exchange
exchange-options-poll = Check for changes every (seconds)
exchange-options-max-poll = Check at least every (seconds) when nothing changes
exchange-options-default = Leave empty for { $seconds } seconds.
exchange-options-streaming = Get changes at once (streaming notifications)
exchange-status-label = Exchange status
exchange-status-version = Server version
exchange-status-push = Changes arrive
exchange-push-streaming = at once (streaming)
exchange-push-polling = by checking regularly
exchange-push-none = only when you sync
exchange-status-last-sync = Last synced
exchange-status-never = never
exchange-status-folders = Folders synced
exchange-status-folders-value = { $synced } of { $total }
exchange-status-pending = Changes waiting for the server
exchange-status-throttled = Slowed down by the server until
