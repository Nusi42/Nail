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
