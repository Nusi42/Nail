# Nail — English texts of the conversation actions in the UI (P3
# WP-actions-ui, ADR 0164): undo toasts, the Undo button, confirmations of
# destructive actions on conversations. The labels of what an action did are
# the core's `mail-action-kind-*` (core-mail-actions.ftl).

# Undo toast: what happened to how many conversations / messages.
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
mail-actions-undo = Undo
mail-actions-undone = Action undone
