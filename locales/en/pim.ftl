# Nail — English UI strings: calendars, contacts and tasks (calendar
# views, mini-month, agenda, event / contact / task editors, groups, merge
# dialog, task list, reminders, sources, import; M3a WP74; contract
# ADR 0058).
#
# Owned by WP74; message ids start with the prefixes listed in
# ui/src/features/pim/messages.ts. Texts the core builds (validation
# issues, source states, reminder notifications) are in core-pim.ftl.

## Dialog titles (window-<dialog name>)

window-pim-event-editor = Event
window-pim-edit-scope = Change a repeating event
window-pim-contact-editor = Contact
window-pim-contact-groups = Contact groups
window-pim-merge-contacts = Merge contacts
window-pim-task-editor = Task
window-pim-import = Import calendar or contacts
window-pim-sources = Calendars and address books
window-pim-calendar-settings = Calendar settings
window-pim-new-calendar = New calendar or task list
window-pim-remove-calendar = Remove calendar

## Common

pim-save = Save
pim-cancel = Cancel
pim-close = Close
pim-delete = Delete
pim-edit = Edit
pim-remove = Remove
pim-loading = Loading…
pim-editor-nothing = Nothing to edit.
pim-editor-read-only = This item is read-only.
pim-categories = Categories (separated by commas)
pim-categories-list = Categories: { $list }
pim-kind =
    { $kind ->
        [home] Home
        [work] Work
        [cell] Mobile
        [fax] Fax
        [voice] Phone
        [pager] Pager
        [text] Text
        [video] Video
        [internet] Internet
        [pref] Preferred
       *[other] Other
    }
pim-badge-recurring = Repeats
pim-op-failed = A change could not be saved on the server. It is kept on this computer.
pim-move-failed = The event could not be moved.
pim-sync-synced = Saved on the server.
pim-sync-pending = Waiting to be saved on the server.
pim-sync-conflict = Changed here and on the server. Choose which version to keep.
pim-conflict-keep-server = Keep the server's version
pim-conflict-keep-local = Keep my version
pim-range = { $start } – { $end }
pim-time-range = { $start } – { $end }
pim-all-day = All day
pim-today-with-date = Today, { $date }

## Date and time picker

pim-datetime-date = { $field }, date
pim-datetime-time = { $field }, time
pim-datetime-pick = Choose the date for { $field }
pim-datetime-pick-short = Choose…
pim-month-previous = Previous month
pim-month-next = Next month
pim-nav-previous-short = ‹
pim-nav-next-short = ›

## Calendar views

pim-calendar-label = Calendar, { $view }: { $range }
pim-nav-label = Navigate
pim-nav-previous = Previous
pim-nav-next = Next
pim-nav-today = Today
pim-views-label = View
pim-view-day = Day
pim-view-multi-day = Days
pim-view-week = Week
pim-view-work-week = Work week
pim-view-month = Month
pim-view-year = Year
pim-view-agenda = Agenda
pim-view-free-time = Free time
pim-new-event = New event
pim-go-to-day = Show { $day }
pim-all-day-events = All-day events
pim-new-all-day-event = New all-day event on { $day }
pim-slots-label = Times of day (press Enter to create an event)
pim-slot-label = { $day }, { $time }
pim-events-of-day =
    { $count ->
        [0] No events on { $day }
        [one] One event on { $day }
       *[other] { $count } events on { $day }
    }
pim-event-untitled = (No title)
pim-event-label =
    { $title }, { $time }, { $day }{ $calendar ->
        [none] {""}
       *[other] , { $calendar }
    }{ $recurring ->
        [yes] , repeats
       *[no] {""}
    }
pim-month-day-label =
    { $count ->
        [0] { $day }
        [one] { $day }, one event
       *[other] { $day }, { $count } events
    }
pim-more-events = { $count } more
pim-more-events-label = Show all events on { $day } ({ $count } more)
pim-agenda-empty = Nothing planned.
pim-agenda-today = Today's agenda
pim-agenda-next =
    { $days ->
        [one] Agenda for today
       *[other] Agenda for the next { $days } days
    }
pim-mini-month-label = Mini calendar, { $month }
pim-free-time-intro = Free stretches of at least { $minutes } minutes during the working day. Choose one to plan an event.
pim-free-time-none = No free time.
pim-free-slot-label = Free on { $day }, { $time }: plan an event

## Event editor

pim-event-title = Title
pim-event-calendar = Calendar
pim-event-when = When
pim-event-start = Start
pim-event-end = End
pim-event-time-zone = Time zone
pim-time-zone-floating = No time zone (same clock time everywhere)
pim-event-end-zone-toggle = The end is in another time zone
pim-event-end-time-zone = Time zone of the end
pim-event-secondary-zone = Also show the times in
pim-secondary-zone-none = No other time zone
pim-secondary-time = In { $zone }: { $time }
pim-event-location = Location
pim-event-description = Description
pim-event-url = Web page
pim-event-flags = Status and visibility
pim-event-status = Status
pim-event-transparency = Show me as
pim-event-visibility = Visibility
pim-status-confirmed = Confirmed
pim-status-tentative = Tentative
pim-status-cancelled = Cancelled
pim-transparency-busy = Busy
pim-transparency-free = Free
pim-visibility-public = Public
pim-visibility-private = Private
pim-visibility-confidential = Confidential
pim-delete-scope-this = Delete this occurrence
pim-delete-scope-this-and-following = Delete this and following
pim-delete-scope-all = Delete all occurrences

## Edit scope

pim-scope-legend = Change which occurrences?
pim-scope-this = This occurrence
pim-scope-this-and-following = This and following occurrences
pim-scope-all = All occurrences
pim-scope-apply = Apply
pim-scope-nothing = There is no change to apply.
pim-scope-move-intro = Move “{ $title }” to { $time }?
pim-scope-delete-intro = Delete “{ $title }”?

## Recurrence

pim-recurrence-legend = Repeat
pim-recurrence-repeat = Repeats
pim-recurrence-freq-none = Does not repeat
pim-recurrence-freq-daily = Daily
pim-recurrence-freq-weekly = Weekly
pim-recurrence-freq-monthly = Monthly
pim-recurrence-freq-yearly = Yearly
pim-recurrence-freq-custom = Custom rule
pim-recurrence-every =
    { $freq ->
        [daily] Every how many days
        [weekly] Every how many weeks
        [monthly] Every how many months
       *[yearly] Every how many years
    }
pim-recurrence-on-days = On
pim-recurrence-monthly-legend = On which day
pim-recurrence-monthly-day = On day { $day }
pim-recurrence-monthly-weekday =
    On the { $ordinal ->
        [1] first
        [2] second
        [3] third
        [4] fourth
       *[-1] last
    } { $weekday }
pim-recurrence-ends = Ends
pim-recurrence-end-never = Never
pim-recurrence-end-count = After a number of times
pim-recurrence-count = Number of times
pim-recurrence-end-until = On a date
pim-recurrence-until = Last date
pim-recurrence-none = Does not repeat.
pim-recurrence-custom = Repeats with a custom rule (kept as it is).
pim-recurrence-replace = Replace with a simple rule
pim-recurrence-locked = Only the whole series can change how it repeats. Choose “This and following” or “All occurrences” to change it.
pim-recurrence-summary =
    { $freq ->
        [daily] { $interval ->
            [1] Every day
           *[other] Every { $interval } days
        }
        [weekly] { $interval ->
            [1] Every week on { $days }
           *[other] Every { $interval } weeks on { $days }
        }
        [monthly] { $interval ->
            [1] Every month
           *[other] Every { $interval } months
        }{ $mode ->
            [weekday] { $ordinal ->
                [1] {" "}on the first { $weekday }
                [2] {" "}on the second { $weekday }
                [3] {" "}on the third { $weekday }
                [4] {" "}on the fourth { $weekday }
               *[-1] {" "}on the last { $weekday }
            }
           *[day] {" "}on day { $monthday }
        }
       *[yearly] { $interval ->
            [1] Every year
           *[other] Every { $interval } years
        }{ $mode ->
            [weekday] { $ordinal ->
                [1] {" "}on the first { $weekday } of the month
                [2] {" "}on the second { $weekday } of the month
                [3] {" "}on the third { $weekday } of the month
                [4] {" "}on the fourth { $weekday } of the month
               *[-1] {" "}on the last { $weekday } of the month
            }
           *[day] {""}
        }
    }{ $end ->
        [count] { $count ->
            [one] , once
           *[other] , { $count } times
        }
        [until] , until { $until }
       *[never] {""}
    }.

## Conferencing

pim-conference-legend = Video calls and dial-in
pim-conference-uri = Link
pim-conference-label = Label
pim-conference-join = Join
pim-conference-add = Add a call link
pim-conference-remove = Remove the call link { $uri }

## Attendees

pim-attendees-legend = Attendees
pim-attendees-none = No attendees.
pim-attendee-named = { $name } <{ $email }>
pim-attendee-role = Role
pim-role-chair = Chair
pim-role-required = Required
pim-role-optional = Optional
pim-role-non-participant = For information
pim-partstat-needs-action = No answer yet
pim-partstat-accepted = Accepted
pim-partstat-declined = Declined
pim-partstat-tentative = Maybe
pim-partstat-delegated = Delegated
pim-attendee-remove = Remove { $email }
pim-attendee-email = Email address
pim-attendee-name = Name
pim-attendee-add = Add attendee
pim-attendee-suggestions = Suggestions from your contacts
pim-attendee-suggestion = Add { $name } <{ $email }>

## Alarms

pim-alarms-legend = Reminders
pim-alarms-none = No reminders.
pim-alarm-at = At { $time }
pim-alarm-amount = Reminder { $index }: how long
pim-alarm-unit = Unit
pim-alarm-unit-minutes =
    { $count ->
        [one] minute
       *[other] minutes
    }
pim-alarm-unit-hours =
    { $count ->
        [one] hour
       *[other] hours
    }
pim-alarm-unit-days =
    { $count ->
        [one] day
       *[other] days
    }
pim-alarm-unit-weeks =
    { $count ->
        [one] week
       *[other] weeks
    }
pim-alarm-when = When
pim-alarm-before-start = before the start
pim-alarm-after-start = after the start
pim-alarm-before-end = before the end (due)
pim-alarm-after-end = after the end (due)
pim-alarm-action = How
pim-alarm-action-display = Show a reminder
pim-alarm-action-audio = Play a sound
pim-alarm-action-email = Email (sent by the server)
pim-alarm-add = Add a reminder
pim-alarm-remove = Remove reminder { $index }

## Reminders

pim-toasts-label = Reminders
pim-reminder-label = Reminder: { $title }
pim-reminder-event = { $title }
pim-reminder-task = Task due: { $title }
pim-reminder-when = { $time }
pim-reminder-open = Open
pim-reminder-snooze-for = Remind me again in
pim-reminder-snooze = Snooze
pim-reminder-dismiss = Dismiss
pim-reminder-dismiss-all = Dismiss all { $count }
pim-snooze-minutes =
    { $minutes ->
        [60] 1 hour
       *[other] { $minutes } minutes
    }
pim-snooze-tomorrow = Tomorrow morning

## Calendars and sources

pim-calendars-label = Calendars
pim-calendar-kind-tasks = Tasks
pim-calendar-settings-of = Settings of { $name }
pim-calendar-name = Name
pim-calendar-color = Colour (for example #3b82f6)
pim-calendar-reminders = Show reminders
pim-calendar-create = Create
pim-sources-open = Calendars and address books…
pim-import-open = Import…
pim-sources-label = Sources
pim-source-local = On this computer
pim-source-kind-local = Local
pim-source-kind-ics = Subscription (ICS / webcal)
pim-source-kind-caldav = CalDAV server
pim-source-kind-carddav = CardDAV server
pim-source-kind-exchange = Exchange account
pim-source-synced = Last synced { $time }
pim-source-sync = Sync now
pim-source-sync-label = Sync { $name } now
pim-source-remove-label = Remove { $name }
pim-sources-sync-all = Sync everything
pim-source-add = Add a subscription or server
pim-source-kind = Kind
pim-source-name = Name
pim-source-account = Use the login of
pim-source-account-none = No mail account (enter a login)
pim-source-feed-url = Address of the calendar feed
pim-source-server-url = Server address (empty: find it from the account)
pim-source-username = User name
pim-source-password = Password
pim-source-refresh = Refresh every … minutes (empty: default)
pim-source-test = Test
pim-source-test-ok =
    { $count ->
        [0] Connected. No calendars or address books found yet.
        [one] Connected. Found one calendar or address book.
       *[other] Connected. Found { $count } calendars and address books.
    }
pim-source-add-submit = Add
pim-source-added = Added. Nail is syncing it now.

## Import

pim-import-intro = Import events and tasks from an iCalendar file (.ics) or contacts from a vCard file (.vcf).
pim-import-file = File
pim-import-too-large = The file is too large to import.
pim-import-preview = What the file holds
pim-import-summary =
    { $name ->
        [none] {""}
       *[other] { $name }:{" "}
    }events: { $events }, tasks: { $tasks }, contacts: { $contacts }, groups: { $groups }.
pim-import-sample = First items
pim-import-issues = Problems in the file
pim-import-issue-item = { $message } ({ $item })
pim-import-calendar = Import events and tasks into
pim-import-address-book = Import contacts into
pim-import-run = Import
pim-import-result = Imported: { $created } new, { $updated } updated, { $skipped } skipped.

## Contacts

pim-contacts-label = Contacts
pim-contacts-search = Search contacts
pim-contacts-book = Address book
pim-contacts-all-books = All address books
pim-contacts-group = Group
pim-contacts-all-groups = All contacts
pim-contacts-actions = Contact actions
# The "More" menu button of the Contacts view's header (M5b).
pim-view-more = More
pim-contacts-list = Contacts
pim-contacts-count =
    { $count ->
        [0] No contacts
        [one] One contact
       *[other] { $count } contacts
    }
pim-contacts-empty = No contacts yet.
pim-contacts-no-match = No contact matches.
pim-contacts-more = Show more ({ $count } left)
pim-contacts-delete =
    { $count ->
        [one] Delete
       *[other] Delete { $count }
    }
pim-contact-new = New contact
pim-contact-unnamed = (No name)
pim-contact-linked-count =
    { $count ->
        [one] Linked with one more contact
       *[other] Linked with { $count } more contacts
    }
pim-book-collected = Collected addresses
pim-groups-open = Groups…
pim-merge-find = Find duplicates…
pim-merge-selected = Merge { $count } contacts…

## Contact card

pim-contact-card-label = Contact
pim-contact-card-of = Contact: { $name }
pim-contact-none = Choose a contact to see it here.
pim-contact-photo-of = Photo of { $name }
pim-contact-in-book = In { $book } (vCard { $version })
pim-contact-actions = Contact actions
pim-contact-emails = Email
pim-contact-phones = Phone
pim-contact-addresses = Addresses
pim-contact-urls = Web pages
pim-contact-impp = Chat
pim-contact-related = Related people
pim-contact-dates = Dates
pim-contact-birthday = Birthday
pim-contact-anniversary = Anniversary
pim-contact-notes = Notes
pim-contact-custom = More fields
pim-contact-groups-list = Groups: { $list }
pim-contact-linked = Linked contacts in other address books
pim-contact-linked-item = { $name } ({ $book })
pim-contact-unlink = Unlink
pim-contact-unlink-label = Unlink { $name }
pim-contact-meetings = Meetings with this person

## Contact editor

pim-contact-book = Address book
pim-contact-move-to = Address book (choose another to move the contact)
pim-contact-kind = Kind
pim-contact-kind-individual = Person
pim-contact-kind-organization = Organization
pim-contact-kind-location = Place
pim-contact-name-legend = Name
pim-contact-prefix = Title before the name
pim-contact-given = First name
pim-contact-additional = Middle name
pim-contact-family = Last name
pim-contact-suffix = Suffix
pim-contact-display-name = Shown as
pim-contact-nicknames = Nicknames (separated by commas)
pim-contact-work-legend = Work
pim-contact-organization = Organization
pim-contact-unit = Department (separated by commas)
pim-contact-title = Job title
pim-contact-role = Role
pim-contact-photo = Photo
pim-contact-photo-choose = Choose a picture
pim-contact-photo-remove = Remove the photo
pim-contact-photo-too-large = Choose a picture (at most 512 KB).
pim-contact-date-known = { $field } is known
pim-contact-date-no-year = The year is not known
pim-entry-value = { $field } { $index }
pim-entry-kind = Kind
pim-entry-preferred = Preferred
pim-entry-remove = Remove { $field } { $index }
pim-entry-add = Add ({ $field })
pim-address-number = Address { $index }
pim-address-street = Street
pim-address-extended = Address line 2
pim-address-po-box = PO box
pim-address-postal-code = Postal code
pim-address-locality = City
pim-address-region = Region
pim-address-country = Country
pim-address-remove = Remove address { $index }
pim-address-add = Add an address
pim-custom-name = Field { $index }: name
pim-custom-value = Field { $index }: value
pim-custom-remove = Remove field { $index }
pim-custom-add = Add a field

## Groups

pim-groups-empty = No groups yet.
pim-group-entry =
    { $count ->
        [one] { $name } (one member)
       *[other] { $name } ({ $count } members)
    }
pim-group-new = New group
pim-group-name = Group name
pim-group-create = Create group
pim-group-edit = Edit the group { $name }
pim-group-members = Members
pim-group-member-emails = Other addresses (one per line)
pim-group-saved = Group saved.
pim-group-delete = Delete group

## Merging

pim-merge-duplicates-intro = These contacts look like the same person. Choose a set to merge.
pim-merge-duplicates = Possible duplicates
pim-merge-no-duplicates = No duplicates found.
pim-merge-reasons = Same { $list }
pim-duplicate-email = email address
pim-duplicate-phone = phone number
pim-duplicate-name = name
pim-merge-choose = Merge these
pim-merge-too-few = Choose at least two contacts to merge.
pim-merge-primary = Keep as the main contact
pim-merge-contact = { $name } ({ $book })
pim-merge-effect =
    { $folded ->
        [0] {""}
        [one] One contact in the same address book is merged into it.{" "}
       *[other] { $folded } contacts in the same address book are merged into it.{" "}
    }{ $linked ->
        [0] {""}
        [one] One contact in another address book stays there, linked to it.
       *[other] { $linked } contacts in other address books stay there, linked to it.
    }
pim-merge-field-display_name = Shown as
pim-merge-field-organization = Organization
pim-merge-field-title = Job title
pim-merge-field-role = Role
pim-merge-field-birthday = Birthday
pim-merge-field-anniversary = Anniversary
pim-merge-field-notes = Notes
pim-merge-field-photo = Photo
pim-merge-field-emails = Email addresses to keep
pim-merge-field-phones = Phone numbers to keep
pim-merge-field-urls = Web pages to keep
pim-merge-field-impp = Chat addresses to keep
pim-merge-field-related = Related people to keep
pim-merge-field-addresses = Addresses to keep
pim-merge-photo = Photo
pim-merge-submit = Merge { $count } contacts
pim-merge-back = Back

## Tasks

pim-tasks-label = Tasks
pim-tasks-list = Tasks
pim-task-list = Task list
pim-tasks-all-lists = All task lists
pim-tasks-show-completed = Show completed
pim-task-new = New task
pim-task-quick-add = Add a task to { $list }
pim-task-add = Add
pim-tasks-empty = No tasks.
pim-task-untitled = (No title)
pim-task-label =
    { $title }{ $status ->
        [completed] , done
        [cancelled] , cancelled
        [in-process] , in progress
       *[needs-action] {""}
    }{ $due ->
        [none] {""}
       *[other] , due { $due }
    }{ $priority ->
        [high] , high priority
        [low] , low priority
       *[other] {""}
    }
pim-task-complete = Mark “{ $title }” as done
pim-task-reopen = Mark “{ $title }” as not done
pim-task-expand = Show the subtasks of { $title }
pim-task-collapse = Hide the subtasks of { $title }
pim-task-subtask-count = { $done } of { $count } subtasks done
pim-priority-none = No priority
pim-priority-high = High
pim-priority-medium = Medium
pim-priority-low = Low
pim-priority-custom = Priority { $value }
pim-task-title = Title
pim-task-parent = Subtask of
pim-task-no-parent = (Not a subtask)
pim-task-status = Status
pim-task-status-needs-action = To do
pim-task-status-in-process = In progress
pim-task-status-completed = Done
pim-task-status-cancelled = Cancelled
pim-task-percent = Percent done
pim-task-priority = Priority
pim-task-start = Start
pim-task-due = Due
pim-task-has-date = { $field } date set
pim-task-with-time = At a time of day
pim-task-recurrence-needs-date = Set a due date to make the task repeat.
pim-task-description = Notes
pim-task-subtasks = Subtasks
pim-task-no-subtasks = No subtasks.
pim-task-open = Edit { $title }
pim-task-subtask-new = New subtask
pim-task-delete-with-subtasks =
    { $count ->
        [one] Delete with its subtask
       *[other] Delete with its { $count } subtasks
    }
pim-linked-message = Created from the email “{ $subject }”.
pim-linked-message-show = Show the email

## Layout labels for the default layout (WP75): actions ("@pim.action.…"),
## windows and panes

window-calendar = Calendar
window-contacts = Contacts
window-tasks = Tasks
pim-pane-calendars = Calendars and tasks
pim-toolbar-calendar = Calendar actions
pim-pane-contacts = Contacts
pim-pane-contact = Contact
pim-tab-calendar = Calendar
pim-tab-contacts = Contacts
pim-tab-tasks = Tasks
pim-action-open-calendar = Calendar
pim-action-open-contacts = Contacts
pim-action-open-tasks = Tasks
pim-action-calendar-today = Today
pim-action-calendar-next = Next
pim-action-calendar-previous = Previous
pim-action-calendar-view = Calendar view
pim-action-new-event = New event
pim-action-event-from-message = Create event from email
pim-action-new-task = New task
pim-action-task-from-message = Create task from email
pim-action-new-contact = New contact
pim-action-contact-groups = Contact groups
pim-action-merge-contacts = Merge contacts
pim-action-import = Import calendar or contacts
pim-action-sources = Calendars and address books
pim-action-sync = Sync calendars and contacts

## Calendar view, calendar list and settings section (M5b, WP164)

# The calendar list's group of all ICS / webcal subscriptions.
pim-calendar-group-subscriptions = Subscriptions
# The "+" menu of a group in the calendar list.
pim-calendar-add-to = Add to { $group }
pim-calendar-new-calendar = New calendar
pim-calendar-new-task-list = New task list
pim-calendar-new-subscription = New subscription…
pim-calendar-new-calendar-in = A new calendar in { $source }
pim-calendar-new-task-list-in = A new task list in { $source }
# The "…" menu of a calendar in the calendar list.
pim-calendar-actions-of = Actions for { $name }
pim-calendar-settings-open = Settings…
pim-calendar-hide = Hide in the calendar
pim-calendar-show = Show in the calendar
pim-calendar-color-label = Colour
pim-calendar-color-other = Other colour (for example #3b82f6)
pim-calendar-color-invalid = Enter a colour such as #3b82f6, or choose one above.
pim-calendar-remove-menu = Remove…
pim-calendar-unsubscribe-menu = Unsubscribe…
pim-calendar-remove = Remove
pim-calendar-unsubscribe = Unsubscribe
pim-calendar-remove-of = Remove { $name }
pim-calendar-remove-question = Remove the calendar “{ $name }” with all its events? This cannot be undone.
pim-calendar-remove-task-list-question = Remove the task list “{ $name }” with all its tasks? This cannot be undone.
pim-calendar-unsubscribe-question = Unsubscribe from “{ $name }”? Its events disappear from Nail; you can subscribe again at any time.
pim-calendar-cannot-remove = This calendar cannot be removed here.
pim-calendar-gone = This calendar no longer exists.
pim-calendar-read-only-hint = This calendar is read-only: name, colour and reminders change only on this computer.
# Colours offered for calendars and task lists.
pim-color-blue = Blue
pim-color-cyan = Cyan
pim-color-green = Green
pim-color-amber = Amber
pim-color-orange = Orange
pim-color-red = Red
pim-color-pink = Pink
pim-color-purple = Purple
pim-color-slate = Slate
# The settings page's "Calendar & contacts" section (layout label "@pim.settings.section").
pim-settings-section = Calendar & contacts
pim-settings-calendars = Calendars and task lists
pim-settings-sources = Subscriptions and servers
pim-settings-import = Import
pim-settings-import-hint = Add events, tasks and contacts from .ics and .vcf files.
pim-settings-keywords = calendar contacts tasks address book subscription webcal ics vcf caldav carddav colour reminders import
