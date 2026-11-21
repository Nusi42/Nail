# Nail — English UI strings: filter rules (M1b WP22).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/rules/messages.ts.

window-rules = { common-filter-rules }
rules-action-open = { common-filter-rules }
rules-action-create-from-message = Create rule from message
rules-action-run-menu = Run rule

## Rule list

rules-title = { common-filter-rules }
rules-new = New rule
rules-empty = No rules yet. Create one, or use “Create rule from message” on a message.
rules-select-hint = Choose a rule to edit it, or create a new one.
rules-list-label = Rules, in the order they run
rules-list-separator = {", "}
rules-enable-rule = Rule “{ $name }” is on
rules-when-arrival = when mail arrives
rules-when-on-send = when sending
rules-when-manual = by hand
rules-when-schedule = on a schedule
rules-when-older-than = when mail gets old

## Editor

rules-view-visual = Editor
rules-view-text = Text
rules-name = { common-name }
rules-enabled = { common-on }
rules-trigger-arrival = When mail arrives
rules-trigger-on-send = When I send mail
rules-trigger-manual = When I run it by hand
rules-trigger-schedule = On a schedule
rules-trigger-older-than = When mail gets old
rules-schedule-minutes = Every (minutes)
rules-older-than-days = Older than (days)
rules-scope = { common-account }
rules-scope-everywhere = { common-all-accounts }
rules-scope-custom = This rule applies to specific folders. Edit where it applies in the text view.
rules-group-all = All of these
rules-group-any = Any of these
rules-group-not = None of these
rules-group-mode = Match
rules-add-condition = Add condition
rules-condition-kind = Condition
rules-action-kind = { common-action }
rules-choose = { common-choose }

## Condition kinds

rules-cond-all = All of a group
rules-cond-any = Any of a group
rules-cond-not = None of a group
rules-cond-header = Header
rules-cond-address = { common-email-address-short }
rules-cond-subject = { common-subject }
rules-cond-body = Message text
rules-cond-query = Search query
rules-cond-attachment = { common-attachment }
rules-cond-size = Size
rules-cond-date = { common-date }
rules-cond-age = Age
rules-cond-flag = { rules-flag }
rules-cond-tag = Tag
rules-cond-account = { common-account }
rules-cond-folder = { common-folder }
rules-cond-auth = Sender check (DKIM, SPF, DMARC)
rules-cond-list-id = Mailing list

## Condition fields

rules-field = { common-field }
rules-field-from = { common-header-from }
rules-field-to = { common-header-to }
rules-field-cc = { common-cc }
rules-field-recipients = To or Cc
rules-field-reply-to = Reply-To
rules-field-sender = Sender
rules-op = { rules-compare }
rules-op-contains = contains
rules-op-equals = is
rules-op-starts-with = starts with
rules-op-ends-with = ends with
rules-op-regex = matches the regular expression
rules-op-exists = exists
rules-op-any-name = any file name
rules-op-any-type = any type
rules-value = { action-field-body }
rules-case-sensitive = Match upper and lower case
rules-header-name = Header name
rules-query = { rules-cond-query }
rules-compare = Comparison
rules-size-greater = larger than
rules-size-less = smaller than
rules-size-kb = Size (KB)
rules-date-before = before
rules-date-after = after
rules-age-greater = older than
rules-age-less = newer than
rules-days = { common-days }
rules-flag = Flag
rules-flag-seen = { action-field-read }
rules-flag-flagged = { common-flagged }
rules-flag-answered = Answered
rules-flag-draft = { common-draft }
rules-flag-state = { common-state }
rules-flag-state-set = is set
rules-flag-state-unset = is not set
rules-tag = { rules-cond-tag }
rules-account = { common-account }
rules-folder-mode = { common-folder }
rules-folder-by-path = By name
rules-folder-by-role = Special folder
rules-folder-path = Folder path
rules-folder-role = { rules-folder-by-role }
rules-folder-account = In account
rules-folder-account-own = The message’s account
rules-role-inbox = { common-inbox }
rules-role-sent = { common-folder-sent }
rules-role-drafts = { common-drafts }
rules-role-archive = { common-folder-archive }
rules-role-junk = { common-junk }
rules-role-trash = { common-trash }
rules-auth-method = Check
rules-auth-dkim = DKIM signature
rules-auth-spf = { common-spf }
rules-auth-dmarc = { common-dmarc }
rules-auth-result = { common-result }
rules-auth-result-pass = passed
rules-auth-result-fail = failed
rules-auth-result-softfail = { common-soft-fail }
rules-auth-result-neutral = { common-neutral }
rules-auth-result-none = no result
rules-auth-result-temperror = { common-temporary-error }
rules-auth-result-permerror = { common-permanent-error }

## Actions

rules-do-move = { common-move-to-folder }
rules-do-copy = Copy to folder
rules-do-archive = { common-archive }
rules-do-delete = { common-delete }
rules-do-tag = Add tag
rules-do-untag = Remove tag
rules-do-flag = { common-flag }
rules-do-mark-read = Mark read or unread
rules-do-forward = { common-forward }
rules-do-redirect = Redirect
rules-do-reply = Reply with a template
rules-do-notify = Notification
rules-do-stop = Stop processing more rules
rules-delete-permanent = Delete permanently (cannot be undone)
rules-set-flag-label = { rules-flag }
rules-set-flag-on = Add the flag
rules-set-flag-off = Remove the flag
rules-set-read-label = Read state
rules-set-read-on = { common-mark-as-read }
rules-set-read-off = { common-mark-as-unread }
rules-set-notify-label = { rules-do-notify }
rules-set-notify-on = Show the notification
rules-set-notify-off = Suppress the notification
rules-recipients = Recipients (separated by commas)
rules-reply-template = Reply text (Markdown)

## Text view

rules-text-label = The rule in rules.kdl syntax
rules-text-broken = The text has mistakes, so the editor cannot show it yet.
rules-text-discard = Discard text changes
## Saving, testing, running

rules-error-name = Give the rule a name.
rules-error-no-actions = Add at least one action.
rules-error-condition = Fill in every condition.
rules-error-action = Fill in every action.
rules-error-trigger = Schedules and ages need a number of at least 1.
rules-save = Save rule
rules-saved = Saved
rules-delete = Delete rule…
rules-test = Test rule
rules-test-title = Test results
rules-test-none = No existing message matches this rule.
rules-test-actions = What would happen
rules-test-why = Why it matches
rules-hit = { $condition } matched
rules-hit-value = { $condition } matched: { $value }
rules-run = Run
rules-runs-title = Recent runs
rules-runs-none = This rule has not run yet.
rules-run-summary = { $cause }: { $matched ->
    [one] one message
   *[other] { $matched } messages
}, { $state }
rules-run-cause-arrival = { common-new-mail }
rules-run-cause-on-send = Sent mail
rules-run-cause-schedule = { common-schedule }
rules-run-cause-manual = By hand
rules-run-state-running = running
rules-run-state-done = done
rules-run-state-failed = { rules-auth-result-fail }
rules-run-state-undone = undone
rules-undo = Undo this run
rules-undone = The run was undone.

## Running rules on demand (R1, R2): "Run on" in the footer, the "Run rule" menu, the toast

rules-target-legend = Run on
rules-target-scope = Where the rule applies
rules-target-folder = Current folder ({ $folder })
rules-target-no-folder = Open a folder first.
rules-target-searching = Not while the list shows search results: open a folder.
rules-target-messages = { $count ->
    [0] Selected messages
   *[other] Selected messages ({ $count })
}
rules-target-no-messages = Select messages in the list first.
rules-save-and-run = Save and run
rules-ran = { $rules ->
    [one] The rule ran
   *[other] { $rules } rules ran
}: { $matched ->
    [0] no message matched
    [one] one message matched
   *[other] { $matched } messages matched
}, { $changes ->
    [0] nothing to change.
    [one] one change queued.
   *[other] { $changes } changes queued.
}
rules-undo-all = { common-undo }
rules-undone-all = Everything the run did was undone.
rules-menu-all = All rules that are on
rules-menu-off = { common-off }
rules-menu-none = No rules yet: open { common-filter-rules }
rules-issues-errors = { $count ->
    [one] One error
   *[other] { $count } errors
}
rules-issues-warnings = { $count ->
    [one] One thing to fix
   *[other] { $count } things to fix
}

## The rules window (R2): list, header, form, lower panel, footer, questions

rules-filter = Find a rule
rules-filter-none = No rule matches.
rules-move-rule = Move “{ $name }” (drag, or Alt+Up and Alt+Down)
rules-reorder-hint = Drag a rule by its handle, or press Alt+Up or Alt+Down on it, to change the order.
rules-name-placeholder = Name the rule
rules-form-when = { itip-label-when }
rules-form-where = { itip-label-where }
rules-form-if = If
rules-form-then = Then
rules-lower-label = Preview and recent runs
rules-tab-preview = { common-preview }
rules-tab-runs = Recent runs ({ $count })
rules-preview-empty = Press Test to see which messages the rule matches and what it would do.
rules-test-summary-on = { $matched ->
    [0] No match in { $scanned } messages ({ $target }).
    [one] One message of { $scanned } matches ({ $target }).
   *[other] { $matched } messages of { $scanned } match ({ $target }).
}
rules-test-open = Open the message
rules-apply-matches = { $count ->
    [one] Apply to this message
   *[other] Apply to these { $count }
}
rules-target-matches = { $count ->
    [one] the message in the preview
   *[other] the { $count } messages in the preview
}
rules-do-value = { $action }: { $value }
rules-run-target-scope = where the rule applies
rules-run-target-messages = { $count ->
    [one] one selected message
   *[other] { $count } selected messages
}
rules-run-target-folder = folder “{ $folder }”
rules-run-why-undone = This run was undone already.
rules-run-why-running = This run is still running.
rules-run-why-failed = This run failed, so there is nothing to undo.
rules-run-why-nothing = This run changed nothing.
rules-run-why-final = This run sent, copied or permanently deleted mail, or later changes replaced what it did: that cannot be taken back.
rules-confirm-delete = Delete the rule “{ $name }”? Its recent runs are deleted with it.
rules-delete-confirm = Delete the rule
rules-confirm-run = “{ $name }” moves or deletes mail. It matches { $count ->
    [0] no message
    [one] one message
   *[other] { $count } messages
} in { $target }. Run it now?
rules-confirm-discard = Discard your changes to “{ $name }”?
rules-discard = Discard changes
rules-no-message = Select a message in the list first.
rules-starters = Start from a template
rules-starter-newsletters-name = Newsletters to a folder
rules-starter-newsletters-hint = Moves mail from mailing lists into a “Newsletters” folder.
rules-starter-newsletters-folder = Newsletters
rules-starter-from-person-name = Flag mail from someone
rules-starter-from-person-hint = Flags new mail from an address you fill in.
rules-starter-old-read-name = Archive old read mail
rules-starter-old-read-hint = Archives read mail once it is older than 30 days.
