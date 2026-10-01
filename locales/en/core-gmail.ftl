# Nail — English texts about Gmail accounts built in Rust (M3b; contract
# ADR 0065): label problems (GmailLabelIssue), category names and, from
# WP82, the Gmail engine's errors (`gmail-*`).
#
# Owned by WP82 (nail-proto-gmail). The ids named by the contract are
# pre-filled; nail-api/tests/catalog.rs checks them.

## Problems with label settings (GmailLabelIssue)

gmail-label-issue-missing-name = A label needs a name.
gmail-label-issue-name-too-long = The label name is too long.
gmail-label-issue-invalid-name = This name cannot be used for a label.
gmail-label-issue-invalid-color = The label colour is not valid.
gmail-label-issue-duplicate-name = A label with this name already exists.

## Inbox categories (GmailCategory::message_id)

gmail-category-primary = Primary
gmail-category-social = Social
gmail-category-promotions = Promotions
gmail-category-updates = Updates
gmail-category-forums = Forums

## Gmail API engine errors (GmailError::localized, WP82)

gmail-network = Gmail could not be reached. Check the internet connection.
gmail-timeout = Gmail did not answer in time.
gmail-tls = The secure connection to Gmail failed.
gmail-auth-failed = Google refused the sign-in. Sign in to the account again.
gmail-insufficient-scope = The Google sign-in does not allow access to the mailbox. Sign in again and allow mail access.
gmail-forbidden = Gmail does not allow this.
gmail-missing = The message or label no longer exists in Gmail.
gmail-rate-limited = Gmail asks Nail to slow down. Nail tries again later.
gmail-unavailable = Gmail is not available at the moment. Nail tries again later.
gmail-rejected = Gmail refused the request.
gmail-label-color-rejected = Gmail does not accept this label colour. Choose one of Gmail's colours.
gmail-label-conflict = A label with this name already exists in Gmail.
gmail-too-large = The message is too large for Gmail.
gmail-http-status = Gmail answered with an unexpected error (HTTP { $status }).
gmail-bad-response = Gmail sent an answer Nail does not understand.
gmail-insecure-url = The Gmail API address is not secure. Use https (plain http only for local test servers).
gmail-config = This account is not set up for the Gmail API with a Google sign-in.
gmail-token = Nail could not get a Google access token.
gmail-move-unsupported = Gmail cannot move messages this way.
