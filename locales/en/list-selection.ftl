# Nail — the message list's "select all" texts (ADR 0235).
#
# Ctrl+A in the message list selects the whole list, rows not loaded yet
# included; the list's older texts stay in app.ftl (message-list-*).

# The whole list is selected; $count is the true number of rows (all
# conversations or messages of the list, minus the deselected ones), so
# one row may be left while other rows are shown.
message-list-all-selected =
    { $count ->
        [one] 1 selected
       *[other] All { $count } selected
    }
# The whole list is selected and the core is still counting its rows
# (shown instead of a count that might belong to another list).
message-list-all-selected-counting = All selected
# Announced to screen readers when "select all" ends (Esc, another list).
message-list-selection-cleared = Selection cleared

## Command palette and the row's menu

message-list-action-select-all = { edit-action-select-all }
message-list-action-select-all-keywords = select all every whole list conversations messages ctrl+a
message-list-action-clear-selection = Clear selection
message-list-action-clear-selection-keywords = deselect unselect none escape
# Ctrl+Space: under "select all" leaves the selected row out or takes it
# back; otherwise picks it besides the other rows or takes it out.
message-list-action-toggle-row = Select or leave out row
message-list-action-toggle-row-keywords = toggle pick deselect leave out row ctrl+space
