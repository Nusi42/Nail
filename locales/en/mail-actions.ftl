# Nail — English texts of the conversation actions in the UI (P3
# WP-actions-ui, ADR 0164): undo toasts, the Undo button, confirmations of
# destructive actions on conversations, the card menu's own actions. The
# labels of what an action did in general are the core's
# `mail-action-kind-*` (core-mail-actions.ftl).

## Undo toast: what happened to how many conversations / messages
## ($conversations = conversations acted on, 0 for a card's own message;
## $messages = messages touched).

mail-actions-done-archive = { $conversations ->
    [0] { $messages ->
        [one] Message archived
       *[other] { $messages } messages archived
    }
    [one] Conversation archived
   *[other] { $conversations } conversations archived
}
mail-actions-done-trash = { $conversations ->
    [0] { $messages ->
        [one] Message moved to Trash
       *[other] { $messages } messages moved to Trash
    }
    [one] Conversation moved to Trash
   *[other] { $conversations } conversations moved to Trash
}
mail-actions-done-delete-permanently = { $conversations ->
    [0] { $messages ->
        [one] Message deleted permanently
       *[other] { $messages } messages deleted permanently
    }
    [one] Conversation deleted permanently
   *[other] { $conversations } conversations deleted permanently
}
mail-actions-done-move = { $conversations ->
    [0] { $messages ->
        [one] Message moved
       *[other] { $messages } messages moved
    }
    [one] Conversation moved
   *[other] { $conversations } conversations moved
}
mail-actions-done-mark-read = { $conversations ->
    [0] { $messages ->
        [one] Message marked as read
       *[other] { $messages } messages marked as read
    }
    [one] Conversation marked as read
   *[other] { $conversations } conversations marked as read
}
mail-actions-done-mark-unread = { $conversations ->
    [0] { $messages ->
        [one] Message marked as unread
       *[other] { $messages } messages marked as unread
    }
    [one] Conversation marked as unread
   *[other] { $conversations } conversations marked as unread
}
mail-actions-done-flag = { $conversations ->
    [0] { $messages ->
        [one] Message flagged
       *[other] { $messages } messages flagged
    }
    [one] Conversation flagged
   *[other] { $conversations } conversations flagged
}
mail-actions-done-unflag = { $conversations ->
    [0] { $messages ->
        [one] Message unflagged
       *[other] { $messages } messages unflagged
    }
    [one] Conversation unflagged
   *[other] { $conversations } conversations unflagged
}
mail-actions-done-tags = { $conversations ->
    [0] { $messages ->
        [one] Tags of the message changed
       *[other] Tags of { $messages } messages changed
    }
    [one] Tags of the conversation changed
   *[other] Tags of { $conversations } conversations changed
}
# A tag change: $noun = label (Gmail), category (Exchange) or tag; $tags = how
# many were added or removed.
mail-actions-done-tags-added = { $noun ->
    [label] { $tags ->
        [one] Label
       *[other] Labels
    }
    [category] { $tags ->
        [one] Category
       *[other] Categories
    }
   *[tag] { $tags ->
        [one] Tag
       *[other] Tags
    }
} added to { $conversations ->
    [0] { $messages ->
        [one] the message
       *[other] { $messages } messages
    }
    [one] the conversation
   *[other] { $conversations } conversations
}
mail-actions-done-tags-removed = { mail-actions-done-tags-added }
mail-actions-done-tags-changed = { mail-actions-done-tags-added }
mail-actions-done-snooze = { $conversations ->
    [0] { $messages ->
        [one] Message snoozed
       *[other] { $messages } messages snoozed
    }
    [one] Conversation snoozed
   *[other] { $conversations } conversations snoozed
}
mail-actions-done-junk = { $conversations ->
    [0] { $messages ->
        [one] Message marked as junk
       *[other] { $messages } messages marked as junk
    }
    [one] Conversation marked as junk
   *[other] { $conversations } conversations marked as junk
}
mail-actions-done-not-junk = { $conversations ->
    [0] { $messages ->
        [one] Message marked as not junk
       *[other] { $messages } messages marked as not junk
    }
    [one] Conversation marked as not junk
   *[other] { $conversations } conversations marked as not junk
}

# Kept for older layouts and tools: "<what was done> · N conversations".
mail-actions-toast-conversations =
    { $action } · { $conversations ->
        [one] 1 conversation
       *[other] { $conversations } conversations
    }
mail-actions-toast-messages =
    { $action } · { $messages ->
        [one] 1 message
       *[other] { $messages } messages
    }
mail-actions-undone = Action undone

## "Delete permanently?" for conversations: the count the core resolved.

mail-actions-delete-permanently-conversations = { $conversations ->
    [one] Delete this conversation's { $messages ->
        [one] message
       *[other] { $messages } messages
    } permanently?
   *[other] Delete { $messages } messages in { $conversations } conversations permanently?
}

## Command palette

mail-actions-action-mark-read = { common-mark-as-read }
mail-actions-action-mark-read-keywords = seen read conversation
mail-actions-action-mark-unread = { common-mark-as-unread }
mail-actions-action-mark-unread-keywords = unseen new unread conversation
mail-actions-action-undo = Undo last action
mail-actions-action-undo-keywords = revert restore back archive delete move

## A card's own menu ("More" on a card of the reader's feed): these act on
## that one message, not on the conversation.

mail-actions-card-more = More actions for this message
mail-actions-card-delete = Delete this message
mail-actions-card-archive = Archive this message
mail-actions-card-move = Move this message
mail-actions-card-mark-read = Mark this message read
mail-actions-card-mark-unread = Mark this message unread
mail-actions-card-toggle-read = Mark this message read / unread
mail-actions-card-toggle-flag = Flag this message
mail-actions-card-junk = Mark this message as junk
