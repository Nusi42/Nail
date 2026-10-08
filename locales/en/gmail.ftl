# Nail — English UI strings: Gmail accounts (label management, label-based
# views, categories as a filter; M3b WP84; contract ADR 0065).
#
# Owned by WP84; message ids start with the prefixes listed in
# ui/src/features/gmail/messages.ts. Texts the core builds (label
# problems, category names, Gmail errors) are in core-gmail.ftl.

## Dialog titles (window-<dialog name>)

window-gmail-label-editor = Label
window-gmail-label-delete = { gmail-delete-confirm }
window-gmail-apply-labels = Apply labels

## Actions (layout labels @gmail.action.*)

gmail-action-new-label = New label
gmail-action-apply-labels = Labels…
gmail-action-label-view = Show label
gmail-action-category = Show inbox category

## Common

gmail-no-subject = (No subject)

## Label list

gmail-labels-label = Labels of { $account }
gmail-labels-none = No labels yet.
gmail-label-edit = Edit label { $name }
gmail-label-remove = Delete label { $name }
gmail-label-view =
    { $unread ->
        [0] { $name }
        [one] { $name }, 1 unread
       *[other] { $name }, { $unread } unread
    }{ $pending ->
        [yes] , not synced yet
       *[no] {""}
    }

## Categories

gmail-categories-label = Inbox categories
gmail-category-tab =
    { $unread ->
        [0] { $name }
        [one] { $name }, 1 unread
       *[other] { $name }, { $unread } unread
    }

## Label editor

gmail-editor-nothing = No label to edit.
gmail-label-parent = Nest under
gmail-label-parent-none = (Top level)
gmail-label-list-visibility = In the label list
gmail-label-message-visibility = On messages
gmail-label-move-hint = Labels nested under this one move with it.
gmail-list-visibility-show = { common-visibility-show }
gmail-list-visibility-show-if-unread = Show if it has unread mail
gmail-list-visibility-hide = { common-hide }
gmail-message-visibility-show = { common-visibility-show }
gmail-message-visibility-hide = { common-hide }
gmail-color-sample = Aa
gmail-color-none = { exchange-color-none }
gmail-color-current = Current colour
gmail-color-red = { common-red }
gmail-color-orange = { common-orange }
gmail-color-yellow = { exchange-color-preset3 }
gmail-color-green = { common-green }
gmail-color-teal = { exchange-color-preset5 }
gmail-color-blue = { common-blue }
gmail-color-purple = { common-purple }
gmail-color-pink = { common-pink }
gmail-color-light-red = Light red
gmail-color-light-orange = Light orange
gmail-color-light-yellow = Light yellow
gmail-color-light-green = Light green
gmail-color-light-teal = Light teal
gmail-color-light-blue = Light blue
gmail-color-light-purple = Light purple
gmail-color-light-pink = Light pink
gmail-color-dark-red = { exchange-color-preset15 }
gmail-color-dark-orange = { exchange-color-preset16 }
gmail-color-dark-yellow = { exchange-color-preset18 }
gmail-color-dark-green = { exchange-color-preset19 }
gmail-color-dark-teal = { exchange-color-preset20 }
gmail-color-dark-blue = { exchange-color-preset22 }
gmail-color-dark-purple = { exchange-color-preset23 }
gmail-color-dark-pink = Dark pink
gmail-color-gray = Grey
gmail-color-light-gray = Light grey
gmail-color-black = { exchange-color-preset14 }

## Delete

gmail-delete-nothing = No label to delete.
gmail-delete-intro =
    { $count ->
        [0] Delete the label “{ $name }”?
        [one] Delete the label “{ $name }”? The message keeps its other labels.
       *[other] Delete the label “{ $name }”? Its { $count } messages keep their other labels.
    }
gmail-delete-confirm = Delete label

## Apply labels

gmail-apply-nothing = Select a Gmail message first.
gmail-apply-intro = Labels of “{ $subject }”
gmail-apply-filter = Find a label
gmail-apply-legend = Labels
gmail-apply-none = No matching labels.
