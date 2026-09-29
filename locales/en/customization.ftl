# Nail — English UI strings: customization (M1b WP24).
#
# Inspector, layout write-back, merge dialog, config error list, smart
# folders (saved searches). Message ids start with the prefixes listed in
# ui/src/features/customization/messages.ts.

## Shared

customize-source = { $file }:{ $line }:{ $column }
customize-open-file = Open in editor
customize-open-unsupported = Opening files needs the desktop app. Open the file in your editor yourself.

## Dialog titles

window-config-merge = Apply a theme or layout
window-config-errors = Configuration errors

## Inspector

inspector-title = Inspector
inspector-hint = Point at any part of the window and click to inspect it. Press Esc to stop.
inspector-hint-pinned = Showing the element you clicked.
inspector-pick-again = Pick another element
inspector-close = Close inspector
inspector-empty = Nothing picked yet.
inspector-part = Part path
inspector-states = States
inspector-no-states = No states or data conditions.
inspector-state = :{ $state }
inspector-condition = [{ $key }="{ $value }"]
inspector-condition-set = [{ $key }]
inspector-rules =
    { $count ->
        [0] Theme rules
        [one] Theme rules (1)
       *[other] Theme rules ({ $count })
    }
inspector-no-rules = No theme rule styles this part directly.
inspector-rule-when = only while { $states }
inspector-declaration = { $property }: { $value }
inspector-layout = Layout node
inspector-node = { $kind }
inspector-node-with-id = { $kind } #{ $id }
inspector-node-place = window “{ $window }”, breakpoint { $breakpoint }, path { $path }
inspector-no-source = No file position: only widgets written in a file have one.
inspector-no-node = This element was not placed by a layout node with an id.

## Config errors

config-error-list-label = Configuration errors
config-error-summary =
    { $count ->
        [one] 1 error
       *[other] { $count } errors
    } in { $files ->
        [one] 1 file
       *[other] { $files } files
    }
config-error-none = All configuration files are valid.
config-error-last-valid = Nail keeps using the last valid version of each file until it is fixed.
config-error-file =
    { $file } — { $count ->
        [one] 1 error
       *[other] { $count } errors
    }

## Smart folders (saved searches)

saved-search-label = Smart folders
saved-search-empty = No smart folders yet. Search for something, then save the search here.
saved-search-remove = Remove “{ $name }”
saved-search-name = Name of the smart folder
saved-search-save = Save
saved-search-cancel = Cancel
saved-search-add = Save current search
saved-search-add-hint = Search first, then save the search as a smart folder.

## Merge dialog

merge-dialog-intro = Load or paste a theme or layout someone shared. You choose what to take; nothing is written until you apply.
merge-dialog-file = Kind of file
merge-dialog-file-theme = Theme (theme.kdl)
merge-dialog-file-layout = Layout (layout.kdl)
merge-dialog-load = Load a file
merge-dialog-paste = Or paste its text
merge-dialog-compare = Compare with mine
merge-dialog-incoming-invalid =
    { $count ->
        [one] The file you loaded has an error. Fix it and compare again.
       *[other] The file you loaded has { $count } errors. Fix them and compare again.
    }
merge-dialog-error-at = Line { $line }, column { $column }: { $message }
merge-dialog-identical = The file is the same as yours; there is nothing to take.
merge-dialog-summary =
    { $differing ->
        [one] 1 difference
       *[other] { $differing } differences
    }, { $taken } taken, { $conflicts ->
        [0] no conflicts
        [one] 1 conflict
       *[other] { $conflicts } conflicts
    }
merge-dialog-keep-all = Keep all mine
merge-dialog-take-all = Take all theirs
merge-dialog-keep-section = Keep mine: { $section }
merge-dialog-take-section = Take theirs: { $section }
merge-dialog-live-preview = Live preview
merge-dialog-show-unchanged = Show unchanged items
merge-dialog-choice = Version of { $key }
merge-dialog-keep-mine = Keep mine
merge-dialog-take-theirs = Take theirs
merge-dialog-mine = Mine
merge-dialog-theirs = Theirs
merge-dialog-absent = (not present)
merge-dialog-back = Back
merge-dialog-apply = Apply
merge-dialog-applied = Applied. Your previous file was backed up.
merge-dialog-revert = Revert
merge-dialog-reverted = The previous version is back.
merge-dialog-close = Close
merge-dialog-backups = Earlier versions
merge-dialog-no-backups = No backups yet.

merge-section-header = Name
merge-section-tokens = Tokens
merge-section-modes = Colour mode
merge-section-parts = Part styles
merge-section-icons = Icons
merge-section-message = Message display and sounds
merge-section-raw-css = Raw CSS
merge-section-windows = Windows
merge-section-actions = Actions
merge-section-keys = Key bindings
merge-section-toolbars = Toolbars
merge-section-menus = Menus

merge-change-added = new
merge-change-removed = only in mine
merge-change-changed = changed
merge-change-unchanged = same

merge-conflict-duplicate-key = Key already in use
merge-conflict-unknown-action = Unknown action
merge-conflict-unknown-widget = Widget not installed
merge-conflict-drops-plugin-items = Drops plug-in items
merge-conflict-invalid = Cannot be taken as it is

## Action labels (for `action "…" label="@…"` in layout.kdl)

customize-action-inspector = Inspector
customize-action-merge = Apply a theme or layout…
customize-action-errors = Configuration errors
