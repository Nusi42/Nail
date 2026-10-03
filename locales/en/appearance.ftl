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

appearance-overrides-title = Text size, corners and accent
appearance-overrides-hint = These apply to every theme. Leave a field empty to use the theme's own value.
# $min, $max (number) — the allowed range in pixels
appearance-font-size = Text size in pixels ({ $min }–{ $max })
# $min, $max (number) — the allowed range in pixels
appearance-radius = Corner radius in pixels ({ $min }–{ $max })
appearance-accent = Accent colour
appearance-accent-pick = Pick the accent colour
appearance-override-default = Theme default
appearance-overrides-not-a-number = Enter whole numbers for the text size and the corner radius.
appearance-overrides-reset = Use theme defaults
appearance-overrides-apply = Apply

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
