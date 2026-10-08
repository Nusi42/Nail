# Nail — English UI strings: per-folder sync policies and bodies that are
# downloaded on demand (M2c WP54; contract ADR 0043).
#
# Owned by WP54; message ids start with the prefixes listed in
# ui/src/features/sync/messages.ts.

## Body policy (nail_api::accounts::BodyPolicy)

sync-bodies-full = Full messages
sync-bodies-headers-only = Headers only (bodies on demand)
sync-bodies-recent = Full messages of the last { $days ->
        [one] day
       *[other] { $days } days
    }
sync-bodies-default = As the account
sync-choice-full = { sync-bodies-full }
sync-choice-headers-only = Headers only
sync-choice-recent = Only recent messages in full

## Actions (layout labels "@sync.action.…")

sync-action-download-message = Download full message
sync-action-folder-policy = Synchronization…

## Settings → Synchronization and the folder's sync dialog

window-sync-folder = Folder synchronization
sync-settings-tab = Synchronization
sync-policies-label = Synchronization per folder
sync-intro = Choose per folder how much Nail keeps on this device. Bodies that are not kept are downloaded when you open a message, and server-side search still finds them.
sync-no-folder = Choose a folder first.
sync-account = { common-account }
sync-folder = { common-folder }
sync-loading = Loading the folder's settings…
sync-policy-label = Synchronization of { $path }
sync-current = Now: { $bodies }; { $synced ->
        [yes] synchronized
       *[no] not synchronized
    }{ $idle ->
        [yes] , watched for new mail
       *[no] {""}
    }.
sync-bodies-label = Download
sync-bodies-hint = "Only recent" keeps older messages as headers until you open them.
sync-days = Days to keep in full
sync-pop3-full = POP3 accounts always keep full messages.
sync-enabled-label = Synchronize this folder
sync-enabled-default = { sync-bodies-default }
sync-enabled-yes = { mcp-tool-value-yes }
sync-enabled-no = { mcp-tool-value-no }
sync-idle-label = Watch for new mail at once (IDLE)
sync-idle-default = As the account (only the inbox)
sync-idle-yes = { mcp-tool-value-yes }
sync-idle-no = { mcp-tool-value-no }
sync-error-days = Keep at least 1 day.

## Reader: bodies on demand (widget sync-message-body)

sync-message-body-label = Message download
sync-message-body-partial = Only the headers of this message are stored on this device.
sync-message-body-missing = This message was not downloaded yet.
sync-message-body-failed = The message could not be downloaded. Check the connection and try again.
sync-message-body-fetching = Downloading…
