# Nail — English texts about plug-ins built in Rust (M2d; contract ADR 0050):
# manifest problems (install dialog, `nail plugin dev`), refused UI trees
# (plug-in log), why a plug-in stopped, invalid settings, and the host's own
# log lines.
#
# Owned by WP61 (the plug-in host); ids start with `plugin-`. The ids named by
# the contract (nail_api::plugins) are pre-filled; nail-api/tests/catalog.rs
# checks they exist.

## Manifest problems (PluginManifestIssueKind; $path = where, $detail = the value)

plugin-manifest-too-large = The manifest is too large.
plugin-manifest-invalid-json = The manifest is not valid: { $detail }
plugin-manifest-invalid-id = “{ $detail }” is not a valid plug-in id (lowercase letters, digits and dashes).
plugin-manifest-invalid-name = The name or description at { $path } is empty or too long.
plugin-manifest-invalid-version = “{ $detail }” is not a version like 1.0.0.
plugin-manifest-unsupported-api-version = This plug-in needs plug-in API version { $detail }, which this version of Nail does not support.
plugin-manifest-invalid-main = “{ $detail }” is not a JavaScript file inside the package.
plugin-manifest-invalid-homepage = The homepage has to be an https address.
plugin-manifest-unknown-permission = Unknown permission { $detail }.
plugin-manifest-permission-not-in-preview = The permission { $detail } is not available yet.
plugin-manifest-invalid-network-domain = { $detail } is not a valid network permission (a host name, optionally starting with “*.”).
plugin-manifest-duplicate-permission = The permission { $detail } is listed twice.
plugin-manifest-body-without-read = Reading message bodies also needs the permission to read mail.
plugin-manifest-invalid-command = The command at { $path } has an invalid id or title.
plugin-manifest-duplicate-command = The command “{ $detail }” is declared twice.
plugin-manifest-invalid-placement = The placement at { $path } is not valid.
plugin-manifest-unknown-command = { $path } names the command “{ $detail }”, which the plug-in does not declare.
plugin-manifest-unknown-panel = { $path } names the panel “{ $detail }”, which the plug-in does not declare.
plugin-manifest-invalid-key = The key binding at { $path } is not valid.
plugin-manifest-invalid-panel = The panel at { $path } has an invalid id or title.
plugin-manifest-duplicate-panel = The panel “{ $detail }” is declared twice.
plugin-manifest-invalid-setting = The setting at { $path } is not valid.
plugin-manifest-duplicate-setting = The setting “{ $detail }” is declared twice.
plugin-manifest-invalid-locale = The language file at { $path } is not valid.
plugin-manifest-too-many-contributions = The plug-in declares too many entries at { $path }.
plugin-manifest-missing-required-permission = A permission needs another one the plug-in does not declare: { $detail }.
plugin-manifest-contribution-needs-permission = { $path } needs the permission { $detail }.
plugin-manifest-invalid-hook = The mail hook at { $path } has an invalid filter or time budget.
plugin-manifest-duplicate-hook = The plug-in declares the hook “{ $detail }” twice.
plugin-manifest-invalid-extension = The entry “{ $detail }” at { $path } has an invalid id, title, file type or host.
plugin-manifest-duplicate-extension = “{ $detail }” is declared twice at { $path }.
plugin-manifest-invalid-job = The background job “{ $detail }” has an invalid id or title, nothing that starts it, or a schedule that is invalid or runs more often than every 5 minutes.
plugin-manifest-duplicate-job = The plug-in declares the background job “{ $detail }” twice.
plugin-manifest-invalid-mcp-tool = The MCP tool “{ $detail }” has an invalid id, description, argument schema or scopes, or its name is too long.
plugin-manifest-duplicate-mcp-tool = The plug-in declares the MCP tool “{ $detail }” twice.

## Refused UI trees (PluginUiIssueKind; logged to the plug-in's log, $path = where)

plugin-ui-issue-too-many-nodes = The dialog or panel has too many elements.
plugin-ui-issue-too-deep = Elements are nested too deeply at { $path }.
plugin-ui-issue-text-too-long = A text at { $path } is too long.
plugin-ui-issue-invalid-id = The element at { $path } has an invalid id.
plugin-ui-issue-duplicate-id = The id of the element at { $path } is used twice.
plugin-ui-issue-missing-id = The selectable list at { $path } needs an id.
plugin-ui-issue-too-many-items = The element at { $path } has too many entries.
plugin-ui-issue-invalid-default = The default value at { $path } is not one of the choices.
plugin-ui-issue-invalid-date = The date at { $path } is not valid.
plugin-ui-issue-row-length = A table row at { $path } does not match its columns.
plugin-ui-issue-invalid-progress = The progress at { $path } is above 100 %.
plugin-ui-issue-invalid-image = The image at { $path } is not an embedded PNG, JPEG, GIF or WebP image, or it is too large.

## Why a plug-in stopped (PluginStopReason; $plugin = its name)

plugin-stopped-disabled = { $plugin } was turned off.
plugin-stopped-uninstalled = { $plugin } was removed.
plugin-stopped-reloaded = { $plugin } is being reloaded.
plugin-stopped-shutdown = { $plugin } stopped because Nail is closing.
plugin-stopped-error = { $plugin } stopped after an error.
plugin-stopped-cpu-budget = { $plugin } was stopped because it took too long.
plugin-stopped-memory-limit = { $plugin } was stopped because it used too much memory.

## Settings

plugin-settings-invalid = These settings have invalid values: { $keys }

## Configuration (layout.kdl)

config-plugin-panel-invalid = `{ $panel }` is not a plug-in panel (expected `plugin-panel "<plugin>.<panel>"`)
# A widget written the pre-M2d way (`plugin.<id>.<name>`); `panel` is `<id>.<name>`.
config-plugin-widget-renamed = `{ $node }` is not a widget: place plug-in panels as `plugin-panel "{ $panel }"`

## Plug-in notifications shown by the operating system: the plug-in's
## name comes first. `title` is the plug-in's own title.

plugin-notification-os-title = { $plugin }: { $title }

## The `nail plugin` command

cli-plugin-dev-no-nail = Nail is not running with its web UI here. Start `nail serve` (or pass --url with the address it printed, or --data-dir with its data directory).
cli-plugin-dev-folder = Cannot use the folder { $folder }.
cli-plugin-dev-not-http = { $url } is not an http:// or https:// address of Nail.
# `detail` is the reason in English (network library text).
cli-plugin-dev-connect = Cannot connect to Nail at { $url }: { $detail }
cli-plugin-dev-closed = The connection to Nail closed.
# `detail` is Nail's reason (already in the user's language).
cli-plugin-dev-refused = Nail did not load the plug-in: { $detail }
cli-plugin-dev-loaded = Loaded { $plugin } { $version } from { $folder }. Changes reload it; press Ctrl+C to close this console (the plug-in stays loaded).
# One console line: `level` (debug, info, warn, error), `source` (plugin, host, api) and the text.
cli-plugin-dev-line = [{ $level }] { $source }: { $text }

## The plug-in host's own log lines (WP61; shown in the plug-in manager and by `nail plugin dev`)

plugin-host-started = { $plugin } { $version } started.
plugin-host-start-failed = { $plugin } could not start: { $detail }
plugin-host-reloading = Files of { $plugin } changed; reloading.
plugin-host-reload-invalid = { $plugin } was not reloaded because its manifest has problems.
plugin-host-api-denied = The plug-in called { $call } without the permission { $permission }.
plugin-host-unhandled-rejection = An error was not handled: { $detail }
plugin-host-log-dropped = { $count ->
    [one] One log line was dropped because the plug-in wrote too many.
   *[other] { $count } log lines were dropped because the plug-in wrote too many.
}
# M4a (WP102): hooks, background jobs and extension handlers.
# `hook` is the hook's name (plugin-hook-*, already translated); `detail` is the reason in English.
plugin-host-hook-failed = { $plugin } failed in its hook “{ $hook }” and was skipped: { $detail }
plugin-host-hook-timed-out = { $plugin } took longer than { $ms } ms in its hook “{ $hook }” and was skipped.
plugin-host-hook-actions-denied = { $plugin } asked to change new mail in an account where it may not organize mail; nothing was changed.
plugin-host-job-overlap = The background job “{ $job }” of { $plugin } was skipped because its previous run has not finished.
plugin-host-job-cpu-quota = The background job “{ $job }” of { $plugin } was skipped because the plug-in used its processor time for this hour.
plugin-host-job-failed = The background job “{ $job }” failed: { $detail }
plugin-host-job-timed-out = The background job “{ $job }” took too long: { $detail }
# `name` is the plug-in's action, renderer, tool or banner (`<kind> <name>`).
plugin-host-handler-failed = { $name } failed: { $detail }
# M4a WP105 (the core): one line per plug-in write ($operation = the API, e.g. nail.tasks.create).
plugin-write-log = { $outcome ->
    [done] Changed your data: { $operation }
    [denied] You denied a change: { $operation }
    [expired] A change was not confirmed in time: { $operation }
    [vetoed] A before-send check stopped sending: { $operation }
   *[failed] A change failed: { $operation }
}

## Plug-in packages (.nailplugin) and folders ($path = the file, $detail = more detail)

plugin-package-too-large = The plug-in package is too large.
plugin-package-not-a-zip = The file is not a plug-in package.
plugin-package-too-many-files = The plug-in package contains too many files.
plugin-package-unsafe-path = The plug-in package contains a file with an unsafe name: { $path }
plugin-package-duplicate-path = The plug-in package contains { $path } twice.
plugin-package-link = The plug-in package contains a link, which is not allowed: { $path }
plugin-package-encrypted = The plug-in package contains an encrypted file: { $path }
plugin-package-compression = The plug-in package uses an unsupported compression for { $path }.
plugin-package-no-manifest = The plug-in package has no manifest.json.
plugin-package-unreadable = The plug-in package cannot be read. { $detail }
plugin-package-hash-mismatch = The plug-in package changed after it was checked. Check it again before installing.
plugin-package-missing-file = The plug-in file { $path } is missing.

## M4a (ADR 0079): hooks and background jobs (plug-in manager, logs)

plugin-hook-arrival = New mail
plugin-hook-before-send = Before sending
plugin-hook-open = Opening a message
plugin-job-outcome-ok = Finished
plugin-job-outcome-error = Failed
plugin-job-outcome-timeout = Took too long
plugin-job-outcome-cpu-quota = Skipped: the plug-in used its CPU time for this hour
plugin-job-outcome-overlap = Skipped: the previous run had not finished
