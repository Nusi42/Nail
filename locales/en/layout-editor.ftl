# Nail — English texts of the layout editor (M5d WP181): customize-layout
# mode with pane headers, drag handles, dock and swap targets, the hide /
# show eye, the widget palette ("Add to layout"), presets, Undo / Reset to
# default / Done and the keyboard alternative (message ids
# `layout-editor-*`). Texts named by the contract (undo entries, editor
# issues, preset names) are in core-editors.ftl.

## Actions (defaults/layout.kdl `actions`)

layout-editor-action-customize = Customize layout
layout-editor-action-customize-keywords = arrange panes move swap dock hide show presets widgets edit layout
layout-editor-palette = Layout palette

## Customize-layout mode: the banner

# $window = the window being customized ("Mail")
layout-editor-title = Customizing layout: { $window }
layout-editor-hint = Drag panes by their handle onto another pane, or press a handle and choose where it goes. Changes are saved to layout.kdl.
layout-editor-undo = Undo
layout-editor-redo = Redo
# $entry = what Undo / Redo would take back ("Swap panes")
layout-editor-undo-entry = Undo: { $entry }
layout-editor-redo-entry = Redo: { $entry }
layout-editor-reset = Reset to default
layout-editor-done = Done
layout-editor-undo-anyway = Undo anyway
layout-editor-redo-anyway = Redo anyway

## Status messages (announced)

layout-editor-undone = Change undone.
layout-editor-redone = Change redone.
layout-editor-cancelled = Nothing was moved.
layout-editor-moving = Choose where it goes: Tab to a target and press Enter. Escape cancels.
# $window = the window's name
layout-editor-reset-done = { $window } is back to the default layout.
# $pane = the pane's name
layout-editor-shown = { $pane } is shown.
layout-editor-hidden-done = { $pane } is hidden.
layout-editor-removed = { $pane } was removed from the layout.
# $preset = the preset's name
layout-editor-preset-applied = Layout preset “{ $preset }” applied.
# $moving = what was moved or added, $pane = the pane it was dropped on
layout-editor-dropped-left = { $moving } is now left of { $pane }.
layout-editor-dropped-right = { $moving } is now right of { $pane }.
layout-editor-dropped-top = { $moving } is now above { $pane }.
layout-editor-dropped-bottom = { $moving } is now below { $pane }.
layout-editor-dropped-swap = { $moving } and { $pane } swapped places.
layout-editor-dropped-into = { $moving } was added to { $pane }.

## Pane headers ($pane = the pane's name)

layout-editor-pick-up = Move { $pane }
layout-editor-put-back = Stop moving { $pane }
layout-editor-handle-hint = Drag to move, or press to choose where it goes
layout-editor-hidden = Hidden
layout-editor-hide = Hide { $pane }
layout-editor-show = Show { $pane }
layout-editor-remove = Remove { $pane } from the layout
layout-editor-targets = Drop targets on { $pane }

## Drop targets: short texts, then the full names ($moving, $pane)

layout-editor-zone-left = Left
layout-editor-zone-right = Right
layout-editor-zone-top = Above
layout-editor-zone-bottom = Below
layout-editor-zone-swap = Swap here
layout-editor-zone-into = Add here
layout-editor-target-left = Put { $moving } left of { $pane }
layout-editor-target-right = Put { $moving } right of { $pane }
layout-editor-target-top = Put { $moving } above { $pane }
layout-editor-target-bottom = Put { $moving } below { $pane }
layout-editor-target-swap = Swap { $moving } with { $pane }
layout-editor-target-into = Add { $moving } to { $pane }

## Names of panes without a label of their own

layout-editor-kind-folder-tree = Folders
layout-editor-kind-message-list = Message list
layout-editor-kind-message-view = Message
layout-editor-kind-list-header = List header
layout-editor-kind-thread-view = Conversation
layout-editor-kind-toolbar = Toolbar
layout-editor-kind-mini-month = Mini month
layout-editor-kind-agenda = Today's agenda
layout-editor-kind-task-list = Task list
layout-editor-kind-smart-folders = Smart folders
layout-editor-kind-calendar = Calendar
layout-editor-kind-calendar-list = Calendars
layout-editor-kind-contact-list = Contacts
layout-editor-kind-contact-card = Contact
layout-editor-kind-nav-rail = Navigation rail
layout-editor-kind-view-host = Views
layout-editor-kind-status-bar = Status bar
layout-editor-kind-settings-page = Settings
layout-editor-kind-plugin-panel = Plug-in panel
# $panel = "<plug-in>.<panel>"
layout-editor-kind-plugin-panel-named = Plug-in panel { $panel }
# $name = the node id or widget kind
layout-editor-pane-unnamed = Pane “{ $name }”
layout-editor-window-main = Main window
# $name = the window's name in layout.kdl
layout-editor-window-other = Window “{ $name }”

## The palette

layout-editor-presets = Presets
layout-editor-presets-none = This window has no presets.
layout-editor-presets-missing = Presets that need a pane this window no longer has are unavailable. Reset to default brings the panes back.
layout-editor-add = Add to layout
layout-editor-add-hint = Drag a widget onto a pane, or press it and choose where it goes.
layout-editor-widget-mini-month = Mini month
layout-editor-widget-mini-month-description = Small calendar for any pane
layout-editor-widget-agenda = Today's agenda
layout-editor-widget-agenda-description = Events and tasks for today
layout-editor-widget-task-list = Task list
layout-editor-widget-task-list-description = Next to your inbox
layout-editor-widget-smart-folders = Smart folders
layout-editor-widget-smart-folders-description = Saved searches
# $plugin = the plug-in's name
layout-editor-widget-plugin-panel-description = Panel of { $plugin }
layout-editor-toolbars = Toolbars
layout-editor-toolbars-hint = Right-click any toolbar and choose “Customize toolbar” to move buttons between the bar and the “More” menu.

## Settings → Layout & toolbars (widget layout-editor-settings)

layout-editor-settings-title = Window layout
layout-editor-settings-hint = Rearrange the panes of a view, apply a preset or go back to the default. Nail writes your changes to layout.kdl and keeps your comments.
layout-editor-settings-window = View
layout-editor-settings-no-windows = This layout has no views to customize.
layout-editor-settings-keywords = layout panes arrange customize presets reader below wide focus three
