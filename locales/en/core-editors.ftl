# Nail — English texts of the M5d in-app editors named by the contract
# (ADR 0131): the undo history (scopes and entry labels), editor issues,
# layout presets and errors of the `presets` section / `hidden` property in
# layout.kdl.
#
# The ids named by nail_api::editing are pre-filled; nail-api/tests/catalog.rs
# checks them. The UI packages' own texts are in layout-editor.ftl (WP181),
# toolbar-editor.ftl (WP182) and appearance.ftl (WP183).

## Undo histories (nail_api::editing::HistoryScope)

history-scope-layout = Layout
history-scope-appearance = Appearance

## Undo entries (nail_api::editing::EditKind; Undo / Redo labels and the history list)

edit-kind-split-sizes = Resize panes
edit-kind-column-order = Reorder columns
edit-kind-toolbar-order = Change toolbar
edit-kind-add-key-binding = Add keyboard shortcut
edit-kind-remove-key-binding = Remove keyboard shortcut
edit-kind-reset-toolbar = Reset toolbar
edit-kind-reset-key-binding = Reset keyboard shortcut
edit-kind-move-pane = Move pane
edit-kind-swap-panes = Swap panes
edit-kind-add-widget = Add to layout
edit-kind-remove-node = Remove from layout
edit-kind-set-hidden = Show or hide pane
edit-kind-apply-preset = Apply layout preset
edit-kind-reset-window = Reset window layout
edit-kind-move-toolbar-item = Move toolbar button
edit-kind-add-toolbar-item = Add toolbar button
edit-kind-remove-toolbar-item = Remove toolbar button
edit-kind-layout-edits = Change layout
edit-kind-set-theme = Choose theme
edit-kind-set-theme-preferences = Change appearance
edit-kind-edit-theme = Edit theme
edit-kind-duplicate-theme = Duplicate theme
edit-kind-rename-theme = Rename theme
edit-kind-delete-theme = Delete theme
edit-kind-merge = Merge configuration
edit-kind-revert = Restore backup

## Editor issues (nail_api::editing::EditIssue)

edit-issue-conflict = { $file } was changed outside Nail. Reload it, or undo anyway to replace those changes.
edit-issue-nothing-to-undo = There is nothing to undo.
edit-issue-nothing-to-redo = There is nothing to redo.
edit-issue-node-not-found = This part of the layout no longer exists. The layout may have changed; try again.
edit-issue-invalid-placement = The pane cannot be placed there.
edit-issue-unknown-widget = This widget is not available.
edit-issue-unknown-preset = This layout preset is not available for this window.
edit-issue-preset-pane-missing = This window has no pane "{ $pane }" that the preset needs.
edit-issue-toolbar-slot-invalid = The button cannot be placed there.
edit-issue-not-editable = Built-in themes cannot be changed. Duplicate the theme to edit your own copy.
edit-issue-theme-exists = A theme with this name exists already.
edit-issue-theme-not-found = The theme was not found.
edit-issue-invalid-theme-name = Enter a theme name of up to 64 characters.
edit-issue-invalid-token = This value cannot be used for this setting.
edit-issue-invalid-part = This style cannot be used for this part.

## Layout presets of the default layout (defaults/layout.kdl `presets`)

layout-preset-three-panes = Three panes
layout-preset-reader-below = Reader below
layout-preset-wide-reader = Wide reader
layout-preset-focus = Focus

## Errors of the `presets` section and `hidden=` in layout.kdl

config-preset-duplicate = preset "{ $preset }" is defined twice
config-preset-window-missing = preset "{ $preset }" needs a `window=` property
config-preset-one-root = preset "{ $preset }" needs exactly one node (split, stack, tabs or pane)
config-preset-pane-twice = preset "{ $preset }" names pane "{ $pane }" twice
config-preset-node-unknown = `{ $node }` cannot be used in a preset (use split, stack, tabs or pane)
config-preset-window-unknown = preset "{ $preset }" arranges window "{ $window }", which the layout does not have
