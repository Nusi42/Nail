# Nail — English UI strings: filter rules (M1b WP22).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/rules/messages.ts.

window-rules = Filter rules
rules-action-open = Filter rules
rules-action-create-from-message = Create rule from message

## Rule list

rules-title = Filter rules
rules-new = New rule
rules-empty = No rules yet. Create one, or use “Create rule from message” on a message.
rules-select-hint = Choose a rule to edit it, or create a new one.
rules-list-label = Rules, in the order they run
rules-list-separator = {", "}
rules-enable-rule = Rule “{ $name }” is on
rules-move-up = Up
rules-move-down = Down
rules-move-rule-up = Move “{ $name }” up
rules-move-rule-down = Move “{ $name }” down
rules-when-arrival = when mail arrives
rules-when-on-send = when sending
rules-when-manual = by hand
rules-when-schedule = on a schedule
rules-when-older-than = when mail gets old

## Editor

rules-editor-title = Rule
rules-view = View
rules-view-visual = Editor
rules-view-text = Text
rules-name = Name
rules-enabled = Rule is on
rules-section-triggers = When it runs
rules-trigger-arrival = When mail arrives
rules-trigger-on-send = When I send mail
rules-trigger-manual = When I run it by hand
rules-trigger-schedule = On a schedule
rules-trigger-older-than = When mail gets old
rules-schedule-minutes = Every (minutes)
rules-older-than-days = Older than (days)
rules-section-scope = Where it applies
rules-scope = Account
rules-scope-everywhere = All accounts
rules-scope-custom = This rule applies to specific folders. Edit where it applies in the text view.
rules-section-conditions = Conditions
rules-section-actions = Actions
rules-group-all = All of these
rules-group-any = Any of these
rules-group-not = None of these
rules-group-mode = Match
rules-add-condition = Add condition
rules-add-action = Add action
rules-condition-kind = Condition
rules-action-kind = Action
rules-remove = Remove
rules-choose = Choose…

## Condition kinds

rules-cond-all = All of a group
rules-cond-any = Any of a group
rules-cond-not = None of a group
rules-cond-header = Header
rules-cond-address = Address
rules-cond-subject = Subject
rules-cond-body = Message text
rules-cond-query = Search query
rules-cond-attachment = Attachment
rules-cond-size = Size
rules-cond-date = Date
rules-cond-age = Age
rules-cond-flag = Flag
rules-cond-tag = Tag
rules-cond-account = Account
rules-cond-folder = Folder
rules-cond-auth = Sender check (DKIM, SPF, DMARC)
rules-cond-list-id = Mailing list

## Condition fields

rules-field = Field
rules-field-from = From
rules-field-to = To
rules-field-cc = Cc
rules-field-recipients = To or Cc
rules-field-reply-to = Reply-To
rules-field-sender = Sender
rules-op = Comparison
rules-op-contains = contains
rules-op-equals = is
rules-op-starts-with = starts with
rules-op-ends-with = ends with
rules-op-regex = matches the regular expression
rules-op-exists = exists
rules-op-any-name = any file name
rules-op-any-type = any type
rules-value = Text
rules-case-sensitive = Match upper and lower case
rules-header-name = Header name
rules-query = Search query
rules-compare = Comparison
rules-size-greater = larger than
rules-size-less = smaller than
rules-size-kb = Size (KB)
rules-date-before = before
rules-date-after = after
rules-date = Date
rules-age-greater = older than
rules-age-less = newer than
rules-days = Days
rules-flag = Flag
rules-flag-seen = Read
rules-flag-flagged = Flagged
rules-flag-answered = Answered
rules-flag-draft = Draft
rules-flag-state = State
rules-flag-state-set = is set
rules-flag-state-unset = is not set
rules-tag = Tag
rules-account = Account
rules-folder-mode = Folder
rules-folder-by-path = By name
rules-folder-by-role = Special folder
rules-folder-path = Folder path
rules-folder-role = Special folder
rules-folder-account = In account
rules-folder-account-own = The message’s account
rules-role-inbox = Inbox
rules-role-sent = Sent
rules-role-drafts = Drafts
rules-role-archive = Archive
rules-role-junk = Junk
rules-role-trash = Trash
rules-auth-method = Check
rules-auth-dkim = DKIM signature
rules-auth-spf = SPF
rules-auth-dmarc = DMARC
rules-auth-result = Result
rules-auth-result-pass = passed
rules-auth-result-fail = failed
rules-auth-result-softfail = soft fail
rules-auth-result-neutral = neutral
rules-auth-result-none = no result
rules-auth-result-temperror = temporary error
rules-auth-result-permerror = permanent error

## Actions

rules-do-move = Move to folder
rules-do-copy = Copy to folder
rules-do-archive = Archive
rules-do-delete = Delete
rules-do-tag = Add tag
rules-do-untag = Remove tag
rules-do-flag = Flag
rules-do-mark-read = Mark read or unread
rules-do-forward = Forward
rules-do-redirect = Redirect
rules-do-reply = Reply with a template
rules-do-notify = Notification
rules-do-stop = Stop processing more rules
rules-delete-permanent = Delete permanently (cannot be undone)
rules-set-flag-label = Flag
rules-set-flag-on = Add the flag
rules-set-flag-off = Remove the flag
rules-set-read-label = Read state
rules-set-read-on = Mark as read
rules-set-read-off = Mark as unread
rules-set-notify-label = Notification
rules-set-notify-on = Show the notification
rules-set-notify-off = Suppress the notification
rules-recipients = Recipients (separated by commas)
rules-reply-template = Reply text (Markdown)

## Text view

rules-text-label = The rule in rules.kdl syntax
rules-text-apply = Check text
rules-text-error = Line { $line }, column { $column }: { $message }

## Saving, testing, running

rules-problems = Before saving:
rules-error-name = Give the rule a name.
rules-error-no-actions = Add at least one action.
rules-error-condition = Fill in every condition.
rules-error-action = Fill in every action.
rules-error-trigger = Schedules and ages need a number of at least 1.
rules-save = Save rule
rules-saved = The rule is saved.
rules-delete = Delete rule
rules-test = Test rule
rules-test-title = Test results
rules-test-summary = { $matched ->
    [0] No match in { $scanned } messages.
    [one] One message of { $scanned } matches.
   *[other] { $matched } messages of { $scanned } match.
}
rules-test-none = No existing message matches this rule.
rules-test-actions = What would happen
rules-test-why = Why it matches
rules-hit = { $condition } matched
rules-hit-value = { $condition } matched: { $value }
rules-run = Run now
rules-run-started = The rule is running on your mail.
rules-runs-title = Recent runs
rules-runs-none = This rule has not run yet.
rules-run-summary = { $cause }: { $matched ->
    [one] one message
   *[other] { $matched } messages
}, { $state }
rules-run-cause-arrival = New mail
rules-run-cause-on-send = Sent mail
rules-run-cause-schedule = Schedule
rules-run-cause-manual = By hand
rules-run-state-running = running
rules-run-state-done = done
rules-run-state-failed = failed
rules-run-state-undone = undone
rules-undo = Undo this run
rules-undone = The run was undone.
