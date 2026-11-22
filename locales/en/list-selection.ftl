# Nail — the message list's "select all" texts (ADR 0235).
#
# Ctrl+A in the message list selects the whole list, rows not loaded yet
# included; the list's older texts stay in app.ftl (message-list-*).

# The whole list is selected; $count is the true number of rows (all
# conversations or messages of the list, minus the deselected ones).
message-list-all-selected =
    { $count ->
        [one] The only row is selected
       *[other] All { $count } selected
    }

## Command palette and the row's menu

message-list-action-select-all = { edit-action-select-all }
message-list-action-select-all-keywords = select all every whole list conversations messages ctrl+a
message-list-action-clear-selection = Clear selection
message-list-action-clear-selection-keywords = deselect unselect none escape
