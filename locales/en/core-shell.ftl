# Nail — English texts of the M5b shell named by the contract (ADR 0109):
# views and the navigation rail, toolbar "More" menus, the settings page's
# groups and sections, list filter chips, the folder pane's sections and
# favorites, UI-state issues.
#
# The ids named by nail_api::views / nail_api::ui_state and the labels the
# default layout uses are pre-filled; nail-api/tests/catalog.rs checks them.
# The UI packages' own texts are in shell.ftl (WP161), app.ftl (WP162),
# settings.ftl (WP163) and pim.ftl (WP164).

## Views (`@view.<id>`; nail_api::views::BUILTIN_VIEWS)

view-mail = Mail
view-calendar = Calendar
view-contacts = Contacts
view-tasks = Tasks
view-settings = Settings
# Accessible name of the navigation rail.
nav-rail-label = Sections

## View actions (default layout)

view-action-show = Show view
view-action-next = Next view
view-action-previous = Previous view

## Toolbars

# The overflow menu button of a toolbar (ToolbarItem::More without a label).
toolbar-more = More
toolbar-group-message = Message
toolbar-group-turn-into = Turn into

## Account colours (folder pane; nail_api::views::account_color_slot)

account-color-slot = Account colour { $slot }

## Settings page groups (`@settings.group.<id>`; nail_api::views::SETTINGS_GROUPS)

settings-group-general = General
settings-group-look-and-feel = Look & feel
settings-group-security = Security
settings-group-extensions = Extensions

## Settings page sections of the default layout (`@settings.section.<id>`)

settings-section-accounts = Accounts
settings-section-mail = Mail & junk
settings-section-calendar = Calendar & contacts
settings-section-server-rules = Server rules & vacation
settings-section-notifications = Notifications
settings-section-data = Data & backup
settings-section-desktop = Desktop app
settings-section-appearance = Appearance
settings-section-layout = Layout & toolbars
settings-section-keyboard = Keyboard shortcuts
settings-section-keys = Keys & encryption
settings-section-privacy = Privacy & data protection
settings-section-remote = Remote access
settings-section-agents = Agents (MCP)
settings-section-plugins = Plug-ins
# The configuration error list on the Layout & toolbars section.
settings-config-errors = Errors in your configuration files

## List filter chips (nail_api::views::ListFilter)

list-filter-all = All
list-filter-unread = Unread
list-filter-flagged = Flagged
list-filter-attachments = Attachments

## Favorites (nail_api::views::FavoriteFolder)

favorite-folder-all-inboxes = All inboxes
favorite-folder-unread = { list-filter-unread }
favorite-folder-flagged = Flagged
favorite-folder-snoozed = Snoozed
favorite-folder-drafts = Drafts
favorite-folder-sent = Sent
favorite-folder-outbox = Outbox

## Folder pane sections (nail_api::views::FolderPaneSectionKind)

folder-section-favorites = Favorites
folder-section-accounts = Accounts
folder-section-smart-folders = Smart folders

## UI state (nail_api::ui_state::UiStateIssue)

ui-state-issue-invalid-key = This is not a valid name for a remembered UI setting.
ui-state-issue-too-large = This UI setting is too large to remember.
ui-state-issue-too-many-keys = Too many UI settings are remembered already.
