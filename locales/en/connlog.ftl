# Nail — English texts of the connection log viewer, the error details
# window and the log settings (ADR 0190). Contract ids (protocols, levels,
# error steps) are in core-connlog.ftl.

## Windows

window-connection-log = Connection log
window-error-details = Error details
window-application-log = Application log

## The viewer

connlog-viewer-title-account = Connection log of { $account }
connlog-viewer-title-source = Connection log of { $source }
connlog-viewer-title-setup = Connection log of account setup
connlog-viewer-note = What Nail sent to the servers and what they answered. Message content, passwords and tokens are never recorded; addresses are shortened.
connlog-viewer-in-memory = Kept in memory only and gone when Nail closes.
connlog-viewer-on-disk = Also kept on disk (Settings › Data & backup).
connlog-viewer-filters = Filters
connlog-viewer-protocol = Protocol
connlog-viewer-protocol-all = All protocols
connlog-viewer-level = Show
connlog-viewer-level-from-info = Everything
connlog-viewer-level-from-warning = Warnings and errors
connlog-viewer-level-from-error = Errors only
connlog-viewer-actions = Connection log actions
connlog-viewer-pause = Pause
connlog-viewer-copy = Copy
connlog-viewer-export = Export…
connlog-viewer-clear = Clear
connlog-viewer-copied = The connection log was copied.
connlog-viewer-saved = The connection log was saved.
connlog-viewer-downloaded = The connection log was downloaded.
connlog-viewer-export-unsupported = The connection log could not be saved here.
connlog-viewer-cleared = The connection log was cleared.
connlog-viewer-paused =
    { $count ->
        [0] Paused.
        [one] Paused — one new line is waiting.
       *[other] Paused — { $count } new lines are waiting.
    }
connlog-viewer-count =
    { $total ->
        [one] { $shown } of one line shown.
       *[other] { $shown } of { $total } lines shown.
    }
connlog-viewer-empty = Nothing recorded yet. Lines appear as soon as Nail talks to the server.
connlog-viewer-filtered = No lines match the filters.
connlog-viewer-list = Recorded lines
connlog-viewer-connection = Connection { NUMBER($id, useGrouping: "false") }
connlog-viewer-unknown = This connection log is no longer available.
connlog-export-title = Save the connection log
connlog-filter-text = Text files

## Show details

connlog-show-details = Show details
connlog-details-step = What Nail was doing
connlog-details-protocol = Protocol
connlog-details-folder = Folder
connlog-details-command = Command
connlog-details-server = Server's answer
connlog-details-at = When
connlog-details-code = Error class
connlog-details-detail = Technical detail
connlog-details-redacted = Passwords, tokens and message content are never shown here; addresses are shortened.
connlog-details-copy = Copy details
connlog-details-copied = The details were copied.
connlog-details-show-log = Show in connection log
connlog-details-close = Close
connlog-details-gone = The details of this error are no longer available.

## Settings › Accounts

connlog-account-title = Connection
connlog-account-hint = The connection log shows what Nail said to this account's servers, without message content or passwords.
connlog-account-open = Connection log
connlog-account-problems = Recent problems
connlog-account-no-problems = No problems recorded.
connlog-account-problem = { $step } failed ({ $time }).
connlog-account-problem-folder = { $step } failed for { $folder } ({ $time }).

## Settings › Data & backup › Diagnostics

connlog-settings-title = Connection log
connlog-settings-intro = Nail keeps the last 2,000 protocol lines of each account in memory, without message content or passwords.
connlog-settings-disk = Keep connection log on disk
connlog-settings-disk-hint = Off by default. When on, the same lines are also written to files in Nail's data folder; turning it off deletes them.
connlog-settings-app-log = Application log
connlog-settings-app-log-path = Written to { $path }
connlog-settings-app-log-none = This Nail writes no application log file.
connlog-settings-app-log-show = Show recent lines
connlog-app-log-empty = No lines yet.
connlog-app-log-lines = Recent application log lines
connlog-app-log-refresh = Refresh
