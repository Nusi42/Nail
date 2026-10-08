# Nail — English texts of the conversation reader (P2 WP-reader, ADR 0148):
# the card feed, its folds and keys, the reader stack's layout migration.
# Shared words ("me", counts, folds) are in conversations.ftl.

## The card feed

# $names (String) — the recipients of an open card ("to Ada, me").
thread-view-card-to = to { $names }
# The group of an open card's buttons; $name (String) — its sender.
thread-view-card-actions = Actions for the message from { $name }

## Keys (command palette)

thread-view-action-next-card = Next message card
thread-view-action-previous-card = Previous message card
thread-view-action-next-message = Next message in the conversation
thread-view-action-previous-message = Previous message in the conversation
thread-view-action-toggle-card = Open or close message card
thread-view-action-expand-all = Open all message cards
thread-view-action-collapse-all = Close all message cards
thread-view-action-open-in-reader = Read the selected conversation (move to the reader)

## The reader stack's migration (layouts of your own)

thread-view-offer-title = Read conversations as cards
thread-view-offer-text = Your layout shows a conversation as a strip above the message. Nail can show it as cards instead: every message with its own badges, banners and actions, oldest first. You can undo this under Settings → Layout & toolbars.
thread-view-offer-apply = Use cards
