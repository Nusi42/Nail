# Nail — English texts of the appearance UI (M5a; WP153): theme switching,
# Follow Windows, density, text size / corner radius / accent, and the
# restyled primitives' labels. Texts named by the contract are in
# core-appearance.ftl.

## Appearance (WP153)

appearance-title = Appearance
# The Settings tab of the appearance page (layout label "@settings.tab.appearance").
settings-tab-appearance = Appearance
appearance-loading = Loading themes…

appearance-themes-title = Theme
appearance-follow-system = Follow Windows
appearance-follow-system-hint = Switch between a light and a dark theme when Windows changes between light and dark mode.
appearance-light-theme = Light theme
appearance-dark-theme = Dark theme
appearance-gallery-label = Themes
appearance-gallery-hint = Picking a theme here turns Follow Windows off and keeps that theme.
# $kind (string) — "Light", "Dark" or "High contrast"
appearance-theme-meta = { $kind }
# $kind (string) — "Light", "Dark" or "High contrast"
appearance-theme-meta-user = { $kind } · your theme

appearance-density-title = Density
appearance-density-hint = Compact shows more messages at once: smaller rows and text, and no tag line in the message list.

appearance-overrides-title = Accent, text size and corners
appearance-overrides-hint = These apply to every theme (High contrast keeps its own accent). The sliders and the colour picker change the app as you move them; type a colour and choose Apply.
appearance-font-size = Text size
appearance-radius = Corner radius
# $value (number) — a size in pixels
appearance-pixels = { $value } px
# $value (number) — the theme's own size in pixels (no override)
appearance-pixels-default = { $value } px (theme default)
appearance-accent = Accent colour
appearance-accent-pick = Pick the accent colour
appearance-accent-high-contrast = High contrast uses its own colours.
appearance-override-default = Theme default
appearance-overrides-not-a-number = Enter whole numbers for the text size and the corner radius.
appearance-overrides-reset = Use theme defaults
appearance-overrides-apply = Apply

## Appearance additions (M5d WP183)

appearance-new-theme = New theme
appearance-edit-theme = Edit theme
appearance-editor-hint = Edit theme opens every colour, size and part style of the theme with a live preview. Built-in themes stay as they are: duplicate one to make your own.
appearance-history-label = Undo and redo appearance changes
appearance-undo = Undo
appearance-redo = Redo
# $action (string) — what the step undoes, e.g. "Edit theme"
appearance-undo-named = Undo { $action }
# $action (string) — what the step redoes, e.g. "Edit theme"
appearance-redo-named = Redo { $action }
appearance-undo-anyway = Undo anyway
appearance-redo-anyway = Redo anyway
# The inspector's button: open the theme editor at the picked element's part.
appearance-edit-style = Edit style
appearance-action-edit-theme = Edit theme
appearance-action-edit-theme-keywords = theme editor colours colors tokens style parts appearance customize
appearance-action-new-theme = New theme
appearance-action-new-theme-keywords = create theme duplicate copy appearance
# Palette action (M5d review): back to the built-in themes (Follow Windows,
# no overrides) — the way out of an unreadable theme.
appearance-action-reset = Reset appearance
appearance-action-reset-keywords = default built-in theme restore unreadable contrast colours colors appearance

## Theme editor (M5d WP183)

window-theme-editor = Theme editor
theme-editor-title = Theme editor
theme-editor-theme = Theme
# $name (string) — the name of one of the user's own themes
theme-editor-theme-user = { $name } (your theme)
theme-editor-loading = Loading the theme…
theme-editor-no-theme = There is no theme to edit.
theme-editor-sections = Theme sections
theme-editor-group-color = Colours
theme-editor-group-type = Type
theme-editor-group-spacing = Spacing
theme-editor-group-radius = Corners
theme-editor-group-motion = Motion
theme-editor-group-other = More
theme-editor-parts = Parts
theme-editor-mode = Values for
theme-editor-mode-base = All modes (base value)
theme-editor-mode-dark = Dark mode
theme-editor-mode-high-contrast = High contrast
theme-editor-mode-print = Printing
theme-editor-mode-compact = Compact density
theme-editor-use = Use this theme
theme-editor-in-use = In use
theme-editor-duplicate = Duplicate as new theme
theme-editor-rename = Rename
theme-editor-delete = Delete
theme-editor-saving = Saving…
# The edited colours make text hard to read (M5d review). $fg / $bg are
# token names (color.fg), $ratio / $needed contrast ratios (4.5).
theme-editor-contrast-warning = Hard to read: { $fg } on { $bg } has a contrast of only { $ratio }:1 (text needs at least { $needed }:1). Undo with Ctrl+Z, or run “Reset appearance” from the command palette (Ctrl+K).
# A token the Appearance page overrides for every theme; $value = the override.
theme-editor-overridden = Overridden in Appearance: { $value }
theme-editor-clear-override = Clear override
theme-editor-builtin-notice = This is a built-in theme, so it cannot be changed. Duplicate it to edit your own copy; the original stays as it is.
theme-editor-reload = Reload the theme
# $name (string) — the theme that is copied
theme-editor-copy-name = { $name } copy
theme-editor-origin-base = Base
theme-editor-origin-extended = Inherited
theme-editor-origin-own = This theme
# $token (string) — a token name such as "color.accent"
theme-editor-token-pick = Pick a colour for { $token }
# $token (string) — a token name such as "color.accent"
theme-editor-token-value = Value of { $token }
# $token (string) — a token name such as "color.accent"
theme-editor-token-reset = Use the inherited value of { $token }
theme-editor-token-filter = Find a setting
theme-editor-no-tokens = No settings match.
theme-editor-part-filter = Find a part
theme-editor-parts-label = Parts with styles
# $count (number) — parts not listed; the search narrows the list
theme-editor-parts-more = { $count ->
    [one] 1 more part. Search to narrow the list.
   *[other] { $count } more parts. Search to narrow the list.
}
theme-editor-add-part = Style another part
theme-editor-selector = Part
theme-editor-selector-invalid = Enter a part name such as message-list.row, optionally with :hover or [state="unread"].
theme-editor-add = Add
theme-editor-pick-part = Choose a part to see and change its style.
theme-editor-reset-part = Reset part
theme-editor-part-no-rules = No theme styles this part yet. Add a property to style it.
theme-editor-rule-empty = No properties.
# $property (string) — a style property such as "background-color"
theme-editor-property-value = Value of { $property }
# $property (string) — a style property such as "background-color"
theme-editor-property-remove = Remove { $property }
theme-editor-property-override = Override
# $property (string) — a style property such as "background-color"
theme-editor-property-override-label = Override { $property } in this theme
theme-editor-add-property = Add a property
theme-editor-property = Property
theme-editor-value = Value
theme-editor-property-invalid = Enter a property name such as background-color and a value.
# $name (string) — the theme's name
theme-editor-preview-label = Preview of { $name }
theme-editor-preview-caption = Preview with unsaved changes
theme-editor-preview-inbox = Inbox
theme-editor-preview-sent = Sent
theme-editor-preview-archive = Archive
theme-editor-preview-initials = AL
theme-editor-preview-from = Ada Lovelace
theme-editor-preview-snippet = Notes on the engine
theme-editor-preview-initials-2 = CB
theme-editor-preview-from-2 = Charles Babbage
theme-editor-preview-snippet-2 = Tables for Tuesday
theme-editor-preview-subject = Notes on the engine
theme-editor-preview-tag = Work
theme-editor-preview-body = The figures for the second table are attached.
theme-editor-preview-reply = Reply
theme-editor-preview-archive-action = Archive
theme-editor-preview-search = Search mail

## Theme windows (M5d WP183)

window-theme-new = New theme
window-theme-rename = Rename theme
window-theme-delete = Delete theme?
theme-dialog-name = Name
# $file (string) — the theme's file in the configuration folder
theme-dialog-file = Saved as { $file }
theme-dialog-new-hint = A new theme starts as a copy of another one. Change it afterwards in the theme editor.
theme-dialog-base = Start from
theme-dialog-use = Use the new theme now
theme-dialog-cancel = Cancel
theme-dialog-create = Create
theme-dialog-close = Close
# $file (string) — the theme's current file
theme-dialog-rename-file = Also rename the file ({ $file })
theme-dialog-rename = Rename
# $name (string) — the theme's name
theme-dialog-delete-question = Delete the theme “{ $name }”?
# $file (string) — the theme's file
theme-dialog-delete-note = { $file } is removed. Where the theme was in use, Nail switches to the default theme. You can undo this.
theme-dialog-delete = Delete

## Message list rows (WP153): the tag line under each message

message-list-tag-attachment = Attachment
# $count (number) — attachments of the message
message-list-tag-attachments = { $count ->
    [one] 1 attachment
   *[other] { $count } attachments
}
message-list-tag-invitation = Invitation
message-list-tag-warning = Warning
message-list-tag-flagged = Flagged

## Attachment cards in the reader (WP153)

# $name (string) — the attachment's file name
message-view-attachment-preview = Preview { $name }
# $name (string) — the attachment's file name
message-view-attachment-save = Save { $name }
message-view-attachment-save-saved = Saved.
message-view-attachment-save-downloaded = Downloaded.
# The title of the desktop's Save dialog for an attachment.
message-view-attachment-save-title = Save attachment
message-view-attachment-save-unsupported = Saving attachments is not available in this version of Nail yet.
message-view-attachment-save-failed = The attachment could not be saved.
