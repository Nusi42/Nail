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

## The `nail plugin` command

cli-plugin-dev-unavailable = Plug-in development is not available yet in this version of Nail ({ $folder }).
