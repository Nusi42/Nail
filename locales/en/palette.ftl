# Nail — English UI strings: the command palette (M4c WP124; contract
# ADR 0094).
#
# Owned by WP124; message ids start with the prefixes listed in
# ui/src/features/palette/messages.ts. The contract's ids
# (nail_api::palette) are pre-filled.

palette-title = { palette-action-open }
window-command-palette = { palette-action-open }

## Contract ids (ActionAvailability, core errors)

action-availability-global = Anywhere
action-availability-list = { common-message-list }
action-availability-reader = { customize-keys-context-reader }
action-availability-composer = { customize-keys-context-composer }
action-availability-calendar = { common-calendar }
action-availability-contacts = { common-contacts }
action-availability-tasks = { common-tasks }
action-availability-desktop = { settings-section-desktop }
palette-invalid-action = This is not an action.

## The palette

palette-search-label = Search actions
palette-search-placeholder = Type what you want to do
palette-count = { $count ->
    [0] No actions
    [one] 1 action
   *[other] { $count } actions
}
palette-no-match = No action matches “{ $query }”.
palette-recent = Recent
palette-unavailable = Not available here
palette-not-now = This action cannot run right now.
palette-needs = This action works in: { $places }.
palette-keys-hint = ↑ ↓ to choose, Enter to run, Esc to close
palette-clear-recent = Forget recent actions
palette-action-open = Command palette
palette-action-clear-recent = Forget the command palette’s recent actions
palette-action-clear-recent-keywords = history recent clear palette

## Built-in actions (titles where the layout has no label, and search words)

palette-action-account-add = { setup-label }
palette-action-account-add-keywords = new account imap pop3 gmail exchange setup
palette-action-agents-open-center = Open the confirmation center
palette-action-agents-open-center-keywords = agents ai mcp plug-ins approve deny confirmations
palette-action-calendar-calendars = Calendars and sources
palette-action-calendar-event-from-message = Create an event from this message
palette-action-calendar-event-from-message-keywords = appointment meeting schedule
palette-action-calendar-goto-today = Go to today
palette-action-calendar-new-event = { pim-action-new-event }
palette-action-calendar-new-event-keywords = appointment meeting create calendar
palette-action-calendar-next = Next period
palette-action-calendar-previous = Previous period
palette-action-composer-attach = { composer-action-attach }
palette-action-composer-attach-keywords = attachment add file
palette-action-composer-discard = Discard the draft
palette-action-composer-save = Save the draft
palette-action-composer-save-keywords = draft store
palette-action-composer-send = { common-send }
palette-action-composer-toggle-preview = { composer-action-toggle-preview }
palette-action-composer-toggle-source = Show or hide the source
palette-action-contacts-groups = { pim-action-contact-groups }
palette-action-contacts-merge = Merge duplicate contacts
palette-action-contacts-merge-keywords = duplicates combine
palette-action-contacts-new = { pim-action-new-contact }
palette-action-contacts-new-keywords = person address book create
palette-action-customize-errors = Show configuration errors
palette-action-customize-merge = Apply a shared theme or layout
palette-action-customize-merge-keywords = import theme layout merge
palette-action-exchange-add-mailbox = Add a shared mailbox
palette-action-exchange-add-mailbox-keywords = exchange delegate shared
palette-action-exchange-apply-categories = Set categories
palette-action-exchange-apply-categories-keywords = exchange colour tag
palette-action-exchange-automatic-replies = { window-exchange-automatic-replies }
palette-action-exchange-automatic-replies-keywords = out of office vacation away oof
palette-action-exchange-classify = Move to Focused or Other
palette-action-exchange-new-category = { exchange-action-new-category }
palette-action-exchange-toggle-focused = Switch between Focused and Other
palette-action-gmail-apply-labels = Set labels
palette-action-gmail-apply-labels-keywords = gmail tag label
palette-action-gmail-category = Gmail category
palette-action-gmail-new-label = { gmail-action-new-label }
palette-action-inspector-toggle = Show or hide the inspector
palette-action-inspector-toggle-keywords = customize theme parts style
palette-action-junk-mark = { junk-action-mark }
palette-action-junk-mark-keywords = spam
palette-action-junk-not-junk = { junk-action-not-junk }
palette-action-mail-archive = { common-archive }
palette-action-mail-compose = { common-new-message }
palette-action-mail-compose-keywords = write compose email create
palette-action-mail-delete = { common-delete }
palette-action-mail-delete-keywords = trash remove
palette-action-mail-delete-permanently = { mail-action-delete-permanently }
palette-action-mail-forward = { common-forward }
palette-action-mail-move = Move to folder…
palette-action-mail-move-keywords = file folder put
palette-action-mail-next = { mail-action-next }
palette-action-mail-previous = { mail-action-previous }
palette-action-mail-reply = { common-reply }
palette-action-mail-reply-keywords = answer respond
palette-action-mail-reply-all = Reply to all
palette-action-mail-toggle-flag = Flag or unflag
palette-action-mail-toggle-flag-keywords = star important mark
palette-action-mail-toggle-read = Mark as read or unread
palette-action-mail-toggle-read-keywords = seen unread
palette-action-outbox-undo = { outbox-action-undo }
palette-action-pim-import = Import a calendar or contacts file
palette-action-pim-import-keywords = ics vcf vcard
palette-action-pim-sources = Calendar and contact sources
palette-action-pim-sources-keywords = caldav carddav subscription webcal
palette-action-pim-sync = Synchronize calendars and contacts
palette-action-plugins-install = { window-plugin-install }
palette-action-plugins-load-dev = Load a plug-in folder for development
palette-action-plugins-load-dev-keywords = developer dev plugin
palette-action-plugins-manage = { plugin-action-manage }
palette-action-plugins-manage-keywords = extensions add-ons plugin
palette-action-remote-content-allow = { remote-action-load-content }
palette-action-remote-content-allow-keywords = images pictures external
palette-action-remote-logout = { remote-action-logout }
palette-action-remote-pair = { remote-action-pair }
palette-action-remote-pair-keywords = phone tablet remote access qr
palette-action-rules-create-from-message = Create a rule from this message
palette-action-rules-create-from-message-keywords = filter sort automatic
palette-action-rules-open = Rules
palette-action-rules-open-keywords = filters sort automatic
palette-action-scheduling-accept = Accept the invitation
palette-action-scheduling-decline = Decline the invitation
palette-action-scheduling-find-time = { scheduling-action-find-time }
palette-action-scheduling-find-time-keywords = free busy meeting availability
palette-action-scheduling-settings = Scheduling settings
palette-action-scheduling-tentative = Accept tentatively
palette-action-search-clear = { settings-search-clear }
palette-action-search-focus = { common-search }
palette-action-search-focus-keywords = find query look
palette-action-security-autocrypt-import = Import an Autocrypt setup message
palette-action-snooze-list = { snooze-action-list }
palette-action-snooze-open = { common-snooze }
palette-action-snooze-open-keywords = later remind postpone
palette-action-sync-download-message = Download this message
palette-action-sync-folder-policy = { window-sync-folder }
palette-action-sync-folder-policy-keywords = offline download headers
palette-action-sync-now = { sync-action-now }
palette-action-sync-now-keywords = sync refresh fetch receive
palette-action-system-about = { system-action-about }
palette-action-system-about-keywords = version help
palette-action-system-check-updates = { system-action-check-updates }
palette-action-system-check-updates-keywords = update upgrade version
palette-action-system-default-apps = { system-default-apps-title }
palette-action-system-default-apps-keywords = mailto default program windows
palette-action-system-first-run = Show the welcome screen
palette-action-tasks-from-message = Create a task from this message
palette-action-tasks-new = { pim-action-new-task }
palette-action-tasks-new-keywords = todo to-do create
palette-action-window-close = Close the window
