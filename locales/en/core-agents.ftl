# Nail — English texts about writes by AI agents (MCP clients) and plug-ins,
# built in Rust (M4a; contract ADR 0079): action kinds, confirmation
# summaries and their before / after lines, states, policy problems.
#
# Shared by the MCP server (WP101), the plug-in host (WP102) and the core
# (WP105); the integrator owns the file, packages report new ids as
# apiRequests. The ids named by the contract (nail_api::agents) are
# pre-filled; nail-api/tests/catalog.rs checks they exist.
# Arguments: $actor = the client's or plug-in's name, $count = how many
# items; others as noted.

## Action kinds (ActionKind)

action-kind-organize = Flag, tag, move or archive mail
action-kind-trash = Move mail to Trash
action-kind-permanent-delete = Delete mail permanently
action-kind-draft = Write drafts
action-kind-send = Send mail
action-kind-calendar = Change calendar events
action-kind-invite = Send invitations and answers
action-kind-contact = Change contacts
action-kind-task = Change tasks
action-kind-pim-delete = Delete events, contacts or tasks
action-kind-rule = Create or change filter rules

## Confirmation states (ConfirmationState)

confirmation-state-pending = Waiting for you
confirmation-state-approved = Approved
confirmation-state-denied = Denied
confirmation-state-expired = Expired
confirmation-state-cancelled = Cancelled
confirmation-state-failed = Failed

## Policy problems (ActionPolicyIssue)

action-policy-issue-destructive = Deleting always asks first. It cannot be allowed without asking.

## Summaries (CONFIRMATION_SUMMARY_IDS)

confirmation-summary-organize = { $actor } wants to change { $count ->
    [one] a message
   *[other] { $count } messages
}.
confirmation-summary-trash = { $actor } wants to move { $count ->
    [one] a message
   *[other] { $count } messages
} to Trash.
confirmation-summary-permanent-delete = { $actor } wants to delete { $count ->
    [one] a message
   *[other] { $count } messages
} permanently. This cannot be undone.
confirmation-summary-draft = { $actor } wants to write a draft: “{ $subject }”.
confirmation-summary-send = { $actor } wants to send “{ $subject }” to { $count ->
    [one] one recipient
   *[other] { $count } recipients
}.
confirmation-summary-calendar = { $actor } wants to change the event “{ $title }”.
confirmation-summary-invite = { $actor } wants to send the event “{ $title }” to { $count ->
    [one] one attendee
   *[other] { $count } attendees
}.
confirmation-summary-respond = { $actor } wants to answer the invitation “{ $title }”.
confirmation-summary-contact = { $actor } wants to change the contact “{ $name }”.
confirmation-summary-task = { $actor } wants to change the task “{ $title }”.
confirmation-summary-pim-delete = { $actor } wants to delete { $count ->
    [one] an item
   *[other] { $count } items
} from your calendars, contacts or tasks.
confirmation-summary-rule = { $actor } wants to save the filter rule “{ $name }”.
confirmation-summary-plugin-tool = { $actor } wants to run “{ $tool }”.

## Before / after lines (ACTION_FIELD_IDS)

action-field-folder = Folder
action-field-flagged = Flagged
action-field-read = Read
action-field-tags = Tags
action-field-snoozed-until = Snoozed until
action-field-to = To
action-field-cc = Cc
action-field-bcc = Bcc
action-field-subject = Subject
action-field-body = Text
action-field-attachments = Attachments
action-field-send-at = Send at
action-field-title = Title
action-field-start = Start
action-field-end = End
action-field-location = Location
action-field-attendees = Attendees
action-field-response = Answer
action-field-name = Name
action-field-emails = Email addresses
action-field-due = Due
action-field-completed = Done
action-field-rule = Rule
