# Nail — English UI strings: server-side rules (Sieve placement per rule,
# ManageSieve status) and vacation replies (M2c WP54; contract ADR 0043).
#
# Owned by WP54; message ids start with the prefixes listed in
# ui/src/features/sieve/messages.ts. Messages built by nail-sieve itself
# (reasons, server errors) live in core-sieve.ftl.

## Rule placement (nail_api::sieve::RulePlacement)

sieve-placement-server = Server
sieve-placement-client = { common-nail }
sieve-placement-not-expressible = Nail only

## Script state (nail_api::sieve::SieveState)

sieve-state-disabled = Server-side rules are off
sieve-state-unavailable = The server has no ManageSieve service
sieve-state-pending = Waiting to upload
sieve-state-uploading = Uploading…
sieve-state-in-sync = Up to date on the server
sieve-state-failed = Upload failed

## ManageSieve settings (nail_api::sieve::ManageSieveIssue)

sieve-issue-host = Enter a host name or IP address without spaces.
sieve-issue-port = Enter a port between 1 and 65535.
sieve-issue-script-name = Use 1 to 128 characters without slashes or control characters.

## Vacation reply (nail_api::sieve::VacationIssue)

vacation-issue-empty-body = Write the reply text.
vacation-issue-days = Choose between 1 and 365 days.
vacation-issue-date = Enter dates as YYYY-MM-DD.
vacation-issue-end-before-start = The last day is before the first day.
vacation-issue-address = One of the addresses is not an email address.
vacation-issue-subject = The subject must fit on one line.

## Settings → Server rules (widget sieve-status)

sieve-settings-tab = Server rules
sieve-status-label = Server-side rules
sieve-intro = With ManageSieve, rules that the server understands run on the server, so they work while Nail is closed. Each rule shows where it runs.
sieve-no-imap = Server-side rules and vacation replies need an IMAP account.
sieve-account = { common-account }
sieve-connection-title = { common-managesieve }
sieve-enabled = Run rules on the server when possible
sieve-host = { common-server }
sieve-host-hint = Leave empty to use the IMAP server.
sieve-port = { common-port }
sieve-port-hint = Leave empty for 4190.
sieve-script-name = Script name
sieve-script-name-hint = Nail keeps its rules in this script and leaves other scripts alone. Leave empty for "nail".
sieve-script = Script
sieve-script-label = Sieve script
sieve-uploaded = Uploaded
sieve-extensions = Server supports
sieve-other-scripts = Other scripts on the server
sieve-vacation-state = Vacation reply
sieve-vacation-on = Active on the server
sieve-vacation-off = Not active
sieve-upload = Upload now
sieve-show-script = Show script
sieve-hide-script = Hide script
sieve-rules-title = Where your rules run
sieve-rules-empty = No rule applies to this account.
sieve-rules-placement = Runs in

## Settings → Vacation reply (widget vacation-editor)

vacation-settings-tab = { sieve-vacation-state }
vacation-label = { sieve-vacation-state }
vacation-intro = While you are away, the server answers new mail for you — also while Nail is closed.
vacation-account = { common-account }
vacation-loading = Loading the vacation reply…
vacation-needs-sieve = Turn on server-side rules for this account (Server rules tab) so the reply reaches the server.
vacation-exchange-hint = For a Microsoft Exchange account this sets its automatic replies (out of office), with the same text for people inside and outside your organization.
vacation-enabled = Send a vacation reply
vacation-subject = { common-subject }
vacation-subject-hint = Leave empty for the server's default ("Auto:" and the original subject).
vacation-body = { common-message-body }
vacation-start = First day
vacation-end = Last day
vacation-days = Answer the same sender again after (days)
vacation-days-hint = Between 1 and 365 days.
vacation-addresses = Also my addresses
vacation-addresses-hint = Separate addresses with commas. Your identities are included automatically.
vacation-from = Reply from
vacation-from-hint = Leave empty to reply from the address the mail was sent to.
vacation-skip-lists = Do not answer mailing lists and bulk mail
vacation-saved = Saved. Nail uploads the reply with your rules.
