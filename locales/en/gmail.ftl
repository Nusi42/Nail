# Nail — English UI strings: Gmail accounts (label management, label-based
# views, categories as a filter; M3b WP84; contract ADR 0065).
#
# Owned by WP84; message ids start with the prefixes listed in
# ui/src/features/gmail/messages.ts. Texts the core builds (label
# problems, category names, Gmail errors) are in core-gmail.ftl.

## Dialog titles (window-<dialog name>)

window-gmail-label-editor = Label
window-gmail-label-delete = Delete label
window-gmail-apply-labels = Apply labels

## Actions (layout labels @gmail.action.*)

gmail-action-new-label = New label
gmail-action-apply-labels = Labels…
gmail-action-label-view = Show label
gmail-action-category = Show inbox category

## Common

gmail-save = Save
gmail-create = Create
gmail-cancel = Cancel
gmail-close = Close
gmail-loading = Loading…
gmail-no-subject = (No subject)
gmail-edit-short = Edit
gmail-delete-short = Delete

## Label list

gmail-labels-label = Labels of { $account }
gmail-labels-title = Labels
gmail-labels-none = No labels yet.
gmail-label-new = New label
gmail-label-edit = Edit label { $name }
gmail-label-remove = Delete label { $name }
gmail-label-unread =
    { $count ->
        [one] 1 unread
       *[other] { $count } unread
    }
gmail-label-pending = (not synced yet)
gmail-label-view =
    { $unread ->
        [0] { $name }
        [one] { $name }, 1 unread
       *[other] { $name }, { $unread } unread
    }{ $pending ->
        [yes] , not synced yet
       *[no] {""}
    }
gmail-message-labels = Labels

## Categories

gmail-categories-label = Inbox categories
gmail-category-all = All
gmail-category-tab =
    { $unread ->
        [0] { $name }
        [one] { $name }, 1 unread
       *[other] { $name }, { $unread } unread
    }
gmail-category-unread =
    { $count ->
        [one] 1 unread
       *[other] { $count } unread
    }

## Label editor

gmail-editor-nothing = No label to edit.
gmail-label-name = Name
gmail-label-parent = Nest under
gmail-label-parent-none = (Top level)
gmail-label-color = Colour
gmail-label-list-visibility = In the label list
gmail-label-message-visibility = On messages
gmail-label-move-hint = Labels nested under this one move with it.
gmail-list-visibility-show = Show
gmail-list-visibility-show-if-unread = Show if it has unread mail
gmail-list-visibility-hide = Hide
gmail-message-visibility-show = Show
gmail-message-visibility-hide = Hide
gmail-color-sample = Aa
gmail-color-none = No colour
gmail-color-current = Current colour
gmail-color-red = Red
gmail-color-orange = Orange
gmail-color-yellow = Yellow
gmail-color-green = Green
gmail-color-teal = Teal
gmail-color-blue = Blue
gmail-color-purple = Purple
gmail-color-pink = Pink
gmail-color-light-red = Light red
gmail-color-light-orange = Light orange
gmail-color-light-yellow = Light yellow
gmail-color-light-green = Light green
gmail-color-light-teal = Light teal
gmail-color-light-blue = Light blue
gmail-color-light-purple = Light purple
gmail-color-light-pink = Light pink
gmail-color-dark-red = Dark red
gmail-color-dark-orange = Dark orange
gmail-color-dark-yellow = Dark yellow
gmail-color-dark-green = Dark green
gmail-color-dark-teal = Dark teal
gmail-color-dark-blue = Dark blue
gmail-color-dark-purple = Dark purple
gmail-color-dark-pink = Dark pink
gmail-color-gray = Grey
gmail-color-light-gray = Light grey
gmail-color-black = Black

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
gmail-apply = Apply
