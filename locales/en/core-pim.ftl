# Nail — English texts about calendars, contacts and tasks built in Rust
# (M3a; contract ADR 0058): validation issues of PIM requests and source
# settings, source states, and reminder notifications.
#
# Owned by WP73 (store, reminder scheduler). The ids named by the contract
# (PimIssue, PimSourceIssue, PimSourceState, the reminder notification)
# are pre-filled; nail-api/tests/catalog.rs checks them.

## Problems with calendar, contact and task data (PimIssue)

pim-issue-invalid-date = A date is not valid.
pim-issue-invalid-time = A time is not valid.
pim-issue-invalid-time-zone = The time zone is not known.
pim-issue-end-before-start = The end is before the start.
pim-issue-mixed-all-day = Start and end must both be all-day or both have a time.
pim-issue-invalid-recurrence = The repetition rule is not valid.
pim-issue-invalid-scope = Choose which occurrences to change.
pim-issue-invalid-email = An email address is not valid.
pim-issue-missing-name = A name is required.
pim-issue-invalid-priority = The priority or progress is out of range.
pim-issue-invalid-color = The colour is not valid.
pim-issue-invalid-alarm = A reminder is not valid.
pim-issue-invalid-url = A web address is not valid.
pim-issue-task-cycle = A task cannot be its own subtask.
pim-issue-too-many = There are too many entries in one list.

## Problems with calendar and contact sources (PimSourceIssue)

pim-source-issue-invalid-id = Use 1 to 64 lowercase letters, digits, "-", "_" or "." for the source name.
pim-source-issue-reserved-id = The name "local" is reserved.
pim-source-issue-empty-name = The name cannot be empty.
pim-source-issue-invalid-url = Use an https: address (or webcal: for subscriptions).
pim-source-issue-missing-url = Enter the server address, or choose the account it belongs to.
pim-source-issue-missing-auth = Enter a user name for this server.
pim-source-issue-invalid-color = The colour is not valid.
pim-source-issue-refresh-too-short = The refresh interval must be at least 5 minutes.

## Source states (PimSourceState)

pim-source-state-idle = Up to date
pim-source-state-syncing = Syncing…
pim-source-state-offline = Offline
pim-source-state-auth-error = Sign-in failed
pim-source-state-error = Sync failed
pim-source-state-disabled = Turned off

## Reminder notifications (NotificationKind::Reminder)

notification-reminder-title = { $title }
notification-reminder-body = { $when }
notification-reminder-body-location = { $when } · { $location }
notification-reminders-title = { $count } reminders
notification-reminders-body = { $more ->
    [one] { $title } and one more
   *[other] { $title } and { $more } more
}

## Snoozing reminders (nail-triage reminders)

reminder-error-not-in-future = Choose a time in the future.
reminder-error-too-far = Reminders can be snoozed for up to a year.
reminder-error-store = The reminder could not be saved.

## Built-in address books

pim-address-book-collected = Collected

## Default local collections (made on first start; M3a WP75)

pim-calendar-default-name = My calendar
pim-task-list-default-name = My tasks
pim-address-book-default-name = My contacts
