# Nail — English UI strings: junk mail, snooze and new-mail notifications
# (M2c WP54; contract ADR 0043).
#
# Owned by WP54; message ids start with the prefixes listed in
# ui/src/features/triage/messages.ts. Notification texts built by the core
# (nail-triage's policy) live in core-triage.ftl.

## Junk (nail_api::triage::JunkIssue, JunkSource)

junk-issue-threshold = Choose a threshold between 50 and 99 percent.
junk-issue-min-training = Choose between 1 and 10000 messages.
junk-source-server = Marked by the server
junk-source-classifier = Classified by Nail
junk-source-user = Marked by you

## Notifications (nail_api::triage::NotificationIssue)

notification-issue-quiet-hours-time = Enter times as HH:MM (24 hours).
notification-issue-empty-quiet-hours = Quiet hours must not start and end at the same time.
notification-issue-duplicate-override = An account or folder is listed twice.

## Actions (layout labels "@junk.action.…", "@snooze.action.…")

junk-action-mark = Mark as junk
junk-action-not-junk = Not junk
snooze-action-open = Snooze…
snooze-action-list = Snoozed messages

## Reader: the junk verdict bar

junk-verdict-label = Junk mail
junk-verdict-junk = This message is junk.
junk-verdict-score = This message looks like junk ({ $score } %).

## Settings → Junk mail

junk-settings-tab = { junk-verdict-label }
junk-settings-label = Junk mail settings
junk-settings-intro = Nail honours the Junk flags your server sets and can learn from your own "Mark as junk" and "Not junk" choices. Junk is only ever moved, never deleted.
junk-settings-account = { common-account }
junk-settings-loading = Loading the classifier…
junk-settings-classifier = Nail's own junk filter
junk-settings-enabled = Classify arriving mail with Nail's junk filter
junk-settings-threshold = Junk threshold (percent)
junk-settings-threshold-hint = Mail at or above this probability counts as junk (50–99).
junk-settings-min-training = Training needed
junk-settings-min-training-hint = The filter starts once it has seen this many junk and this many good messages.
junk-settings-honor-server = Trust the server's Junk flags
junk-settings-move = Move junk to the Junk folder
junk-settings-never-deletes = Nail never deletes junk; you can always find it in the Junk folder.
junk-settings-saved = { common-saved }
junk-settings-reset-done = The filter forgot everything it learned.
junk-reset = Reset the filter…
junk-reset-confirm = Forget everything the filter learned for this account?
junk-reset-yes = Reset
junk-status-label = Training status
junk-status-ready = Ready: classifying arriving mail
junk-status-learning = Learning (needs { $needed } junk and { $needed } good messages)
junk-status-trained = Trained
junk-status-trained-counts = { $junk } junk, { $ham } good
junk-status-tokens = Words known
junk-status-last-trained = Last trained

## Snooze

window-snooze = { common-snooze }
window-snoozed = { snooze-action-list }
snooze-intro = { $count ->
        [one] Hide the message until…
       *[other] Hide { $count } messages until…
    }
# P3: the selected conversation(s): their messages in the listed folder.
snooze-intro-conversations = { $count ->
        [one] Hide the conversation until…
       *[other] Hide { $count } conversations until…
    }
snooze-nothing-selected = Select a message to snooze first.
snooze-presets-label = Quick choices
snooze-preset-later-today = Later today
snooze-preset-tomorrow = Tomorrow
snooze-preset-weekend = This weekend
snooze-preset-next-week = Next week
snooze-custom-label = Date and time
snooze-error-past = { common-time-in-future }
snooze-error-time = Enter a date and a time.
snooze-list-empty = No snoozed messages.
snooze-list-until = Back { $time }
snooze-list-wake = Wake up now
snooze-list-wake-label = Wake up "{ $subject }" now

## Settings → Notifications

notification-settings-tab = { settings-section-notifications }
notification-settings-label = Notification settings
notification-settings-loading = Loading notification settings…
notification-general-title = { common-new-mail }
notification-enabled = Tell me about new mail
notification-show-preview = Show sender and subject
notification-sound = { common-play-a-sound }
notification-folders = Folders that notify
notification-folders-inbox = Inboxes only
notification-folders-all = Every folder
notification-quiet-title = Quiet hours
notification-quiet-enabled = No notifications during quiet hours
notification-quiet-start = { common-range-from }
notification-quiet-end = Until
notification-quiet-days-hint = No day chosen means every day. Hours that pass midnight belong to the day they start on.
notification-day-mon = Monday
notification-day-tue = Tuesday
notification-day-wed = Wednesday
notification-day-thu = Thursday
notification-day-fri = Friday
notification-day-sat = Saturday
notification-day-sun = Sunday
notification-overrides-title = Accounts and folders
notification-overrides-hint = A folder's choice wins over its account's; a rule's "notify" action wins over both.
notification-override-account = { $account }
notification-override-folder = { $folder } ({ $account })
notification-override-add = Add a folder of { $account }
notification-override-add-choose = Choose a folder…
notification-override-default = As set above
notification-override-on = Always notify
notification-override-off = Never notify
notification-browser-needed = This browser shows notifications only after you allow them.
notification-browser-denied = This browser blocks notifications from Nail. Allow them in the browser's site settings.
notification-browser-allow = Allow notifications in this browser
notification-settings-saved = { common-saved }
notification-test = Send a test notification
notification-test-sent = A test notification is on its way.
