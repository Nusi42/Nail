# Nail — English texts of the toolbar customizer (M5d WP182): right-click a
# toolbar → Customize; drag buttons between the bar, "More" and a palette of
# available actions, plug-in actions included (message ids
# `toolbar-editor-*`). Texts named by the contract (undo entries, editor
# issues) are in core-editors.ftl.

## Actions (defaults/layout.kdl `actions`, menu `context:toolbar`)

toolbar-editor-action-customize = Customize toolbar
toolbar-editor-action-keywords = toolbar buttons actions customize edit more menu arrange

## The customizer window (`toolbar-customizer:<toolbar>`)

window-toolbar-customizer = { toolbar-editor-action-customize }
toolbar-editor-intro = Drag buttons between the toolbar, its More menu and the available actions. With the keyboard: the arrow keys choose a button, Ctrl+arrow keys move it, M moves it between the toolbar and More, Delete removes it. In Available actions, Enter adds the action to the toolbar and Shift+Enter adds it to More.
toolbar-editor-no-toolbars = This layout has no toolbars.
toolbar-editor-area-bar = { customize-toolbar-choose }
toolbar-editor-area-more = { customize-toolbar-more }
toolbar-editor-area-palette = Available actions
toolbar-editor-empty-bar = The toolbar is empty. Drag actions here.
toolbar-editor-empty-more = The More menu has no entries of its own. Drag buttons here.
toolbar-editor-search = Find an action
toolbar-editor-no-match = No action matches.
toolbar-editor-more-matches = Showing { $shown } of { $total } actions. Type to find the others.
toolbar-editor-drop-to-remove = Drop a button here to remove it from the toolbar.
toolbar-editor-badge-used = On toolbar
toolbar-editor-commands = Change the chosen button

## Items

toolbar-editor-item-header = Heading: { $label }
toolbar-editor-item-submenu = Submenu: { $label }

## Commands

toolbar-editor-move-earlier = Move earlier
toolbar-editor-move-later = Move later
toolbar-editor-move-to-more = Move to More
toolbar-editor-move-to-bar = Move to toolbar
toolbar-editor-add-to-bar = Add to toolbar
toolbar-editor-add-to-more = Add to More
toolbar-editor-undo-title = Undo: { $change }
toolbar-editor-redo-title = Redo: { $change }
toolbar-editor-undo-anyway = { appearance-undo-anyway }
toolbar-editor-redo-anyway = { appearance-redo-anyway }

## Status line (announced to assistive technology)

toolbar-editor-moved = Moved { $item }.
toolbar-editor-moved-to-more = Moved { $item } to More.
toolbar-editor-moved-to-bar = Moved { $item } to the toolbar.
toolbar-editor-added = Added { $item } to the toolbar.
toolbar-editor-added-to-more = Added { $item } to More.
toolbar-editor-removed = Removed { $item }.
toolbar-editor-reset-done = The toolbar is back to its default buttons.
toolbar-editor-undone = Undone: { $change }
toolbar-editor-redone = Redone: { $change }

## Names of the default toolbars

toolbar-editor-toolbar-main = { toolbar-main }
toolbar-editor-toolbar-reader = { common-message-actions }
toolbar-editor-toolbar-calendar = { common-calendar }
toolbar-editor-toolbar-customize = { settings-tab-customize }

## Settings card (`toolbar-editor-settings`)

toolbar-editor-settings-title = { customize-toolbar-tab }
toolbar-editor-settings-intro = Right-click any toolbar and choose Customize toolbar, or pick one here.
toolbar-editor-settings-count =
    { $count ->
        [one] 1 button
       *[other] { $count } buttons
    }
toolbar-editor-settings-customize = { settings-tab-customize }
toolbar-editor-settings-customize-named = Customize { $toolbar }
