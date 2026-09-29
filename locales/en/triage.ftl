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
