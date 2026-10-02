# Nail — English UI strings: plug-ins (install dialog, plug-in manager,
# permissions, dialogs, panels, notifications; M2d WP62; contract ADR 0050).
#
# Owned by WP62; message ids start with the prefixes listed in
# ui/src/features/plugins/messages.ts. The permission labels are named by
# the contract (PluginPermission::label). Texts the core builds (manifest
# problems, why a plug-in stopped, invalid settings) are in core-plugins.ftl.

## Permissions (install dialog and plug-in manager)

plugin-permission-mail-read = Read your mail: accounts, folders, senders and subjects
plugin-permission-mail-read-body = Read the text of your messages
plugin-permission-calendar-read = Read your calendar events
plugin-permission-ui-dialogs = Show dialogs
plugin-permission-notifications = Show notifications
plugin-permission-storage = Store its own data on this device
plugin-permission-network = Connect to { $domain }
plugin-permission-mail-organize = Flag, tag, move, archive and delete your messages
plugin-permission-mail-send = Write drafts and send mail for you
plugin-permission-calendar-write = Create, change and delete calendar events
plugin-permission-contacts-write = Create, change and delete contacts
plugin-permission-tasks-write = Create, change and delete tasks
plugin-permission-hooks-arrival = Look at new mail as it arrives and sort it
plugin-permission-hooks-before-send = Check outgoing mail before it is sent, add to it or stop it
plugin-permission-hooks-open = Show notes and badges on messages you open
plugin-permission-composer = Add actions and content blocks to the composer
plugin-permission-attachments = Preview attachments and add attachment actions
plugin-permission-background = Run in the background on a schedule or when things happen
plugin-permission-mcp-expose = Offer tools to connected AI agents (MCP clients)
plugin-permissions-label = Permissions
plugin-permissions-none = This plug-in asks for no permissions.
plugin-permission-allow = Allow
plugin-permission-deny = Deny
plugin-permission-needs-read = Needs the permission to read your mail.
plugin-permission-accounts = Accounts for “{ $permission }”
plugin-permission-all-accounts = All accounts
plugin-permission-some-accounts = Only these accounts
plugin-permission-no-accounts = Choose at least one account.

## Windows

window-plugin-install = Install a plug-in
window-plugin-manager = Plug-ins

## Layout labels (@plugin.…)

plugin-manager-tab = Plug-ins
plugin-action-install = Install plug-in…
plugin-action-load-dev = Load plug-in folder…
plugin-action-manage = Manage plug-ins

## Install dialog

plugin-install-intro = Install a plug-in from a .nailplugin file or a web address. Plug-ins run separately from your mail and can only do what you allow.
plugin-install-source = Install from
plugin-install-source-file = A file on this computer
plugin-install-source-url = A web address
plugin-install-source-dev = A development folder
plugin-install-location-file = Path of the .nailplugin file
plugin-install-location-url = Web address (https)
plugin-install-location-dev = Folder with manifest.json
plugin-install-preview = Check the plug-in
plugin-install-cancel = Cancel
plugin-install-back = Back
plugin-install-name = { $name } { $version }
plugin-install-author = By { $author }
plugin-install-homepage = Web page: { $url }
plugin-install-package = Package: { NUMBER($size) } bytes, fingerprint { $hash }…
plugin-install-update = Version { $installed } is installed; this updates it to { $version }. Its data is kept.
plugin-install-dev-note = Development folder: the plug-in reloads when its files change.
plugin-install-issues =
    { $count ->
        [one] This plug-in cannot be installed because of a problem:
       *[other] This plug-in cannot be installed because of { $count } problems:
    }
plugin-install-adds = What it adds
plugin-install-command = Command “{ $title }”
plugin-install-panel = Panel “{ $title }”
plugin-install-permissions = What it may do
plugin-install-enable = Turn it on right away
plugin-install-install = Install
plugin-install-update-button = Update
plugin-install-changed = The package changed after it was checked. Check it again before installing.
plugin-install-done = { $name } is installed.
plugin-install-close = Close

## Placements (proposed on install, confirmed like a merge)

plugin-placements-loading = Looking for places in your layout…
plugin-placements-intro = The plug-in suggests adding these to your layout. Choose what to add; you can move or remove them later in layout.kdl.
plugin-placements-apply =
    { $count ->
        [one] Add 1 item to the layout
       *[other] Add { $count } items to the layout
    }
plugin-placements-skip = Not now
plugin-placements-applied = Added to your layout. The previous layout was backed up.
plugin-placements-undo = Undo
plugin-placements-close = Close
plugin-placements-nothing = The plug-in has nothing to add to your layout.
plugin-placements-invalid =
    { $count ->
        [one] The suggested layout has an error, so nothing can be added.
       *[other] The suggested layout has { $count } errors, so nothing can be added.
    }
plugin-placements-unavailable = Suggested places cannot be shown yet.

## Plug-in manager

plugin-manager-label = Plug-ins
plugin-manager-install = Install a plug-in…
plugin-manager-load-dev = Load a development folder…
plugin-manager-empty = No plug-ins are installed.
plugin-manager-name = { $name } { $version }
plugin-manager-enabled = { $name } is on
plugin-manager-source-file = Installed from { $path }
plugin-manager-source-url = Installed from { $url }
plugin-manager-source-dev = Loaded from the development folder { $folder }
plugin-manager-crashed = The plug-in stopped: { $error }
plugin-manager-show-permissions = Permissions
plugin-manager-show-settings = Settings
plugin-manager-show-log = Log
plugin-manager-show-layout = Place in layout
plugin-manager-grants-save = Save permissions
plugin-manager-grants-saved = Permissions saved.
plugin-manager-uninstall = Remove
plugin-manager-uninstall-confirm = Remove { $name }? Its settings, data and log are deleted. Items it added to your layout stay until you remove them.
plugin-manager-uninstall-yes = Remove
plugin-manager-uninstall-no = Keep

## Run states

plugin-state-disabled = Off
plugin-state-starting = Starting
plugin-state-running = Running
plugin-state-stopped = Not running
plugin-state-crashed = Stopped after a problem
plugin-state-incompatible = Not compatible with this version of Nail

## Log viewer

plugin-logs-label = Plug-in log
plugin-logs-level = Show from level
plugin-logs-refresh = Refresh
plugin-logs-clear = Clear log
plugin-logs-empty = The log is empty.
plugin-log-level-debug = Debug
plugin-log-level-info = Info
plugin-log-level-warn = Warning
plugin-log-level-error = Error
plugin-log-source-plugin = Plug-in
plugin-log-source-host = Nail
plugin-log-source-api = API call

## Settings page

plugin-settings-label = Plug-in settings
plugin-settings-none = This plug-in has no settings.
plugin-settings-save = Save settings
plugin-settings-saved = Settings saved.

## Plug-in dialogs

plugin-dialog-from = from { $plugin }
plugin-dialog-close = Close
plugin-dialog-invalid =
    { $count ->
        [one] Please correct the marked field.
       *[other] Please correct the { $count } marked fields.
    }

## Plug-in UI elements

plugin-field-required = This field is required.
plugin-field-email = Enter a mail address like name@example.org.
plugin-field-url = Enter a web address starting with https://.
plugin-field-number = Enter a number.
plugin-field-too-long = This text is too long.
plugin-field-date-range = This date is outside the allowed range.
plugin-select-none = (none)
plugin-folder-none = (no folder)
plugin-contact-placeholder = Name <address>, separated by commas
plugin-contact-remove = Remove { $address }
plugin-contact-remove-short = ×
plugin-message-list-label =
    { $count ->
        [one] 1 message
       *[other] { $count } messages
    }
plugin-message-no-subject = (no subject)
plugin-table-label = Table
plugin-table-select = Select
plugin-table-select-row = Select row { $row }
plugin-progress-label = Progress
plugin-link-confirm = Open { $url } in your browser?
plugin-link-open = Open
plugin-link-cancel = Cancel

## Notifications

plugin-notifications-label = Plug-in notifications
plugin-notification-dismiss = Dismiss

## Panels

plugin-panel-missing = The plug-in for this panel is not installed or not enabled.
plugin-panel-label = Plug-in panel

## M4a (WP104, ADR 0079): permissions that change data, contributions

plugin-permission-write = Can change your data
plugin-install-hook = Mail hook: { $hook }
plugin-install-composer-action = Composer action “{ $title }”
plugin-install-markdown-block = Markdown block “{ $title }”
plugin-install-attachment-preview = Attachment preview “{ $title }”
plugin-install-attachment-action = Attachment action “{ $title }”
plugin-install-link-action = Link action “{ $title }”
plugin-install-job = Background job “{ $title }”
plugin-install-mcp-tool = Tool for AI agents: { $tool }
plugin-manager-show-confirmations = Confirmations
plugin-manager-show-activity = Activity

## Plug-in manager: activity (hooks, background jobs, CPU time)

plugin-activity-cpu = CPU time in the last hour
plugin-activity-cpu-value = { $used } s of { $quota } s ({ $percent } %)
plugin-activity-cpu-exhausted = The plug-in used all its CPU time for this hour. Its jobs are skipped until the hour is over.
plugin-activity-hooks = Mail hooks
plugin-activity-hook = Hook
plugin-activity-calls = Calls
plugin-activity-failures = Failed
plugin-activity-timeouts = Too slow
plugin-activity-vetoes = Stopped mail
plugin-activity-time = Time (average / longest)
plugin-activity-time-value = { $average } ms / { $max } ms
plugin-activity-last = Last call
plugin-activity-count = { NUMBER($count) }
plugin-activity-never = Never
plugin-activity-last-error = Last problem: { $error }
plugin-activity-jobs = Background jobs
plugin-activity-schedule = Schedule
plugin-activity-events = Runs when
plugin-activity-last-run = Last run
plugin-activity-running = Running now
plugin-activity-last-run-value = { $time }: { $outcome }
plugin-activity-next-run = Next run
plugin-activity-no-next-run = Not scheduled
plugin-activity-runs = Runs
plugin-activity-runs-value =
    { $failures ->
        [0] { $runs }, none failed
       *[other] { $runs }, { $failures } failed
    }
plugin-activity-run-now = Run “{ $job }” now
plugin-activity-none = This plug-in has no mail hooks and no background jobs.

## Reader: banners and badges of open hooks, attachment previews, actions

plugin-banners-label = Notes from plug-ins
plugin-banner-from = { $plugin }:
plugin-badges-label = Plug-in badges
plugin-badge-from = Added by the plug-in { $plugin }
plugin-badge-tooltip = { $tooltip } (added by the plug-in { $plugin })
plugin-extension-from = { $plugin }
plugin-attachment-actions = Plug-in actions for { $name }
plugin-link-actions = Plug-in actions for this link
plugin-preview-label = Preview of { $name }
plugin-preview-by = { $renderer }, shown by the plug-in { $plugin }
plugin-preview-choose = Preview with
plugin-preview-option = { $renderer } ({ $plugin })
plugin-preview-loading = Preparing the preview…
plugin-preview-invalid = The plug-in { $plugin } returned a preview Nail cannot show.
plugin-preview-image-alt = Preview drawn by the plug-in { $plugin }

## Composer: plug-in actions, Markdown blocks, edits

plugin-composer-label = Plug-ins
plugin-composer-actions = Plug-in actions
plugin-composer-blocks = Insert block
plugin-composer-save-first = The draft could not be saved, so the plug-in cannot work on it yet.
plugin-composer-edited-insert = { $actor } inserted text.
plugin-composer-edited-replace-selection = { $actor } replaced the selected text.
plugin-composer-edited-replace-all = { $actor } replaced the whole text.
plugin-block-rendered-by = { $block }: rendered by the plug-in { $plugin } when the message is sent
plugin-block-unknown = No plug-in renders “{ $block }”; it is sent as a code block.

## Before-send veto

plugin-veto-title =
    { $count ->
        [one] A plug-in stopped this message
       *[other] { $count } plug-ins stopped this message
    }
plugin-veto-intro = The message was not sent. Check the reason, then keep editing or send it anyway.
plugin-veto-plugin = Plug-in “{ $plugin }”:
plugin-veto-keep-editing = Keep editing
plugin-veto-send-anyway = Send anyway
plugin-veto-expired = This was too long ago. Send the message again to check it once more.
