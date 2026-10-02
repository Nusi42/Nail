# Nail — English UI strings: the command palette (M4c WP124; contract
# ADR 0094).
#
# Owned by WP124; message ids start with the prefixes listed in
# ui/src/features/palette/messages.ts. The contract's ids
# (nail_api::palette) are pre-filled.

palette-title = Command palette
window-command-palette = Command palette

## Contract ids (ActionAvailability, core errors)

action-availability-global = Anywhere
action-availability-list = Message list
action-availability-reader = Reader
action-availability-composer = Composer
action-availability-calendar = Calendar
action-availability-contacts = Contacts
action-availability-tasks = Tasks
action-availability-desktop = Desktop app
palette-invalid-action = This is not an action.

## The palette

palette-search-label = Search actions
palette-search-placeholder = Type what you want to do
palette-list-label = Actions
palette-count = { $count ->
    [0] No actions
    [one] 1 action
   *[other] { $count } actions
}
palette-no-match = No action matches “{ $query }”.
palette-recent = Recent
palette-source-plugin = Plug-in
palette-unavailable = Not available here
palette-not-now = This action cannot run right now.
palette-needs = This action works in: { $places }.
palette-keys-hint = ↑ ↓ to choose, Enter to run, Esc to close
palette-clear-recent = Forget recent actions
palette-action-open = Command palette
palette-action-clear-recent = Forget the command palette’s recent actions
palette-action-clear-recent-keywords = history recent clear palette

## Built-in actions (titles where the layout has no label, and search words)

palette-action-account-add = Add an account
palette-action-account-add-keywords = new account imap pop3 gmail exchange setup
palette-action-agents-open-center = Open the confirmation center
palette-action-agents-open-center-keywords = agents ai mcp plug-ins approve deny confirmations
palette-action-calendar-calendars = Calendars and sources
palette-action-calendar-event-from-message = Create an event from this message
palette-action-calendar-event-from-message-keywords = appointment meeting schedule
palette-action-calendar-goto-today = Go to today
palette-action-calendar-new-event = New event
palette-action-calendar-new-event-keywords = appointment meeting create calendar
palette-action-calendar-next = Next period
palette-action-calendar-previous = Previous period
palette-action-composer-attach = Attach a file
palette-action-composer-attach-keywords = attachment add file
palette-action-composer-discard = Discard the draft
palette-action-composer-save = Save the draft
palette-action-composer-save-keywords = draft store
palette-action-composer-send = Send
palette-action-composer-toggle-preview = Show or hide the preview
palette-action-composer-toggle-source = Show or hide the source
palette-action-contacts-groups = Contact groups
palette-action-contacts-merge = Merge duplicate contacts
palette-action-contacts-merge-keywords = duplicates combine
palette-action-contacts-new = New contact
palette-action-contacts-new-keywords = person address book create
palette-action-customize-errors = Show configuration errors
palette-action-customize-merge = Apply a shared theme or layout
palette-action-customize-merge-keywords = import theme layout merge
palette-action-exchange-add-mailbox = Add a shared mailbox
palette-action-exchange-add-mailbox-keywords = exchange delegate shared
palette-action-exchange-apply-categories = Set categories
palette-action-exchange-apply-categories-keywords = exchange colour tag
palette-action-exchange-automatic-replies = Automatic replies
palette-action-exchange-automatic-replies-keywords = out of office vacation away oof
palette-action-exchange-classify = Move to Focused or Other
palette-action-exchange-new-category = New category
palette-action-exchange-toggle-focused = Switch between Focused and Other
palette-action-gmail-apply-labels = Set labels
palette-action-gmail-apply-labels-keywords = gmail tag label
palette-action-gmail-category = Gmail category
palette-action-gmail-new-label = New label
palette-action-inspector-toggle = Show or hide the inspector
palette-action-inspector-toggle-keywords = customize theme parts style
palette-action-junk-mark = Mark as junk
palette-action-junk-mark-keywords = spam
palette-action-junk-not-junk = Not junk
palette-action-mail-archive = Archive
palette-action-mail-compose = New message
palette-action-mail-compose-keywords = write compose email create
palette-action-mail-delete = Delete
palette-action-mail-delete-keywords = trash remove
palette-action-mail-delete-permanently = Delete permanently
palette-action-mail-forward = Forward
palette-action-mail-move = Move to folder…
palette-action-mail-move-keywords = file folder put
palette-action-mail-next = Next message
palette-action-mail-previous = Previous message
palette-action-mail-reply = Reply
palette-action-mail-reply-keywords = answer respond
palette-action-mail-reply-all = Reply to all
palette-action-mail-toggle-flag = Flag or unflag
palette-action-mail-toggle-flag-keywords = star important mark
palette-action-mail-toggle-read = Mark as read or unread
palette-action-mail-toggle-read-keywords = seen unread
palette-action-outbox-undo = Undo send
palette-action-pim-import = Import a calendar or contacts file
palette-action-pim-import-keywords = ics vcf vcard
palette-action-pim-sources = Calendar and contact sources
palette-action-pim-sources-keywords = caldav carddav subscription webcal
palette-action-pim-sync = Synchronize calendars and contacts
palette-action-plugins-install = Install a plug-in
palette-action-plugins-load-dev = Load a plug-in folder for development
palette-action-plugins-load-dev-keywords = developer dev plugin
palette-action-plugins-manage = Manage plug-ins
palette-action-plugins-manage-keywords = extensions add-ons plugin
palette-action-remote-content-allow = Load remote content
palette-action-remote-content-allow-keywords = images pictures external
palette-action-remote-logout = Sign out
palette-action-remote-pair = Pair a device
palette-action-remote-pair-keywords = phone tablet remote access qr
palette-action-rules-create-from-message = Create a rule from this message
palette-action-rules-create-from-message-keywords = filter sort automatic
palette-action-rules-open = Rules
palette-action-rules-open-keywords = filters sort automatic
palette-action-scheduling-accept = Accept the invitation
palette-action-scheduling-decline = Decline the invitation
palette-action-scheduling-find-time = Find a time
palette-action-scheduling-find-time-keywords = free busy meeting availability
palette-action-scheduling-settings = Scheduling settings
palette-action-scheduling-tentative = Accept tentatively
palette-action-search-clear = Clear the search
palette-action-search-focus = Search
palette-action-search-focus-keywords = find query look
palette-action-security-autocrypt-import = Import an Autocrypt setup message
palette-action-snooze-list = Snoozed messages
palette-action-snooze-open = Snooze
palette-action-snooze-open-keywords = later remind postpone
palette-action-sync-download-message = Download this message
palette-action-sync-folder-policy = Folder synchronization
palette-action-sync-folder-policy-keywords = offline download headers
palette-action-sync-now = Check for new mail
palette-action-sync-now-keywords = sync refresh fetch receive
palette-action-system-about = About Nail
palette-action-system-about-keywords = version help
palette-action-system-check-updates = Check for updates
palette-action-system-check-updates-keywords = update upgrade version
palette-action-system-default-apps = Default apps
palette-action-system-default-apps-keywords = mailto default program windows
palette-action-system-first-run = Show the welcome screen
palette-action-tasks-from-message = Create a task from this message
palette-action-tasks-new = New task
palette-action-tasks-new-keywords = todo to-do create
palette-action-window-close = Close the window
