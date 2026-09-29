# Nail — English UI strings: server-side rules (Sieve placement per rule,
# ManageSieve status) and vacation replies (M2c WP54; contract ADR 0043).
#
# Owned by WP54; message ids start with the prefixes listed in
# ui/src/features/sieve/messages.ts. Messages built by nail-sieve itself
# (reasons, server errors) live in core-sieve.ftl.

## Rule placement (nail_api::sieve::RulePlacement)

sieve-placement-server = Server
sieve-placement-client = Nail
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
