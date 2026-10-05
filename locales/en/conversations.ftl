# Nail — English texts of conversations (P2, contract ADR 0148): the words
# the conversation list and the reader share. The list's own texts are in
# conversation-list.ftl (WP-list), the reader's in conversation-reader.ftl
# (WP-reader).

## People

# The user among a conversation's participants and on their own messages.
conversations-me = me
# Between two names of a conversation row ("Ada, me").
conversations-names-separator = {", "}
# Where names are left out between the first ones and the newest
# ("Ada, Bob … Grace").
conversations-names-gap = {" … "}
# A message the user wrote (from Sent, or from one of their addresses).
conversations-from-me = Sent by you

## Counts and folds

# $count (Number) — messages of the conversation (the row's count pill).
conversations-count = { $count ->
    [one] One message
   *[other] { $count } messages
}
# $count (Number) — older messages folded away above the newest ones.
conversations-earlier = { $count ->
    [one] One earlier message
   *[other] { $count } earlier messages
}
# $count (Number) — read messages folded away in the middle of the card feed.
conversations-older = { $count ->
    [one] One older message
   *[other] { $count } older messages
}

## Opening and closing

# The chevron of an expandable conversation row (Design B).
conversations-expand = Show the conversation's messages
conversations-collapse = Hide the conversation's messages
# Expand or collapse every card of the reader's feed.
conversations-expand-all = Expand all
conversations-collapse-all = Collapse all

## The list's conversation modes (layout option threads=)

conversations-mode-off = One row per message
conversations-mode-collapsed = One row per conversation
conversations-mode-expandable = Conversations that open into their messages
