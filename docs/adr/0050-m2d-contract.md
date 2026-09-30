# ADR 0050: M2d contract — plug-in manifest, permissions, declarative UI, admin requests, store v9, slots

Status: accepted · 2026-09-30 · M2d WP60

## Context

Wave M2d (plan section 7) ships the plug-in preview of the design
("Plug-ins"): commands, UI contributions, dialogs, read access,
notifications, storage. Four packages build it in parallel — the plug-in
host `nail-plugins` (WP61), the plug-in UI (WP62), the SDK, starter
template, example and user docs (WP63), and a carry-over package (WP65) —
and WP64 integrates. This ADR fixes the contract they build against, the
store schema they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M2"): no GPL / LGPL / AGPL
dependency, no OpenSSL / aws-lc; strings only from Fluent catalogs; the
core owns data and every write goes through the operation queue; write
access, hooks and the full extension-point list stay in M4.

## Decisions

### Module `nail_api::plugins`

Split into `manifest`, `permission`, `ui`, `runtime` and `admin`.

**Naming.** Everything a plug-in author writes or receives is camelCase
JSON (the design's example, JavaScript convention): the manifest,
component trees, dialog specs and results, notification specs, command
contexts and the mail view. The protocol around them (requests, responses,
events, admin types such as `PluginInfo`) stays snake_case like the rest of
the API. `nail_api::export_plugin_typescript` writes the plug-in-facing
types into `packages/plugin-api/src/generated/` (the SDK's types are
generated from the Rust contract); `npm run gen:api` regenerates both
trees, `tests/bindings.rs` fails when either is stale.

**Ids.** `PluginId` (the manifest's `id`, chosen by the author) and
`PluginDialogId` (opaque, core-made) in `ids.rs`. Plug-in ids and names of
commands, panels and settings are kebab-case, 1–64 characters, no dots
(`is_plugin_id`, `is_contribution_name`). Commands are the layout actions
`plugin.<plugin>.<command>` (`plugin_command_id` / `parse_plugin_command_id`),
panels the layout widget `plugin-panel "<plugin>.<panel>"`
(`plugin_panel_id` / `parse_plugin_panel_id`). Plug-ins see messages as
`nail://messages/<id>` (same as MCP).

### Manifest (`manifest.json`)

`PluginManifest {id, name, version (semver), apiVersion ("1"), main (a .js
/ .mjs path in the package), description?, author?, homepage? (https),
permissions, contributes}`; `contributes` = `commands` (`{id, title, icon?,
description?, contexts: [list | reader | composer | global]}`),
`placements` (exactly one of `toolbar` / `menu` (`app` or
`context:<part>`) / `panel`, `command` for toolbar and menu, at most one of
`after` / `before` naming an action or widget kind), `keys` (`{key,
command, context = global}`, contexts of `layout.kdl`), `panels` (`{id,
title, icon?}`), `settings` (typed schema: `string` / `number` (integers) /
`boolean` / `select`, each with `key`, `title`, `description?`, `default?`
and bounds / options; values are JSON) and `locales` (language tag → `.ftl`
file in the package).

User-visible manifest texts may be `%fluent-id%` references into the
plug-in's own catalogs (`text_key`); the host resolves them for the core's
locale before `PluginInfo` reaches the UI (user's language → English → the
id). Plug-in texts are the plug-in's data, not Nail strings, so they are
not in `locales/`.

`PluginManifest::parse` validates everything and returns every problem as
`PluginManifestIssue {kind, path, detail}` (25 kinds, Fluent ids
`plugin-manifest-*` with `$path` / `$detail`); permission names are checked
one by one so an unknown or not-yet-available permission is reported as
such. Limits: 64 KiB, 100 commands / placements / keys, 50 panels /
settings / locales, titles 100 characters, descriptions 1000. API versions:
`SUPPORTED_API_VERSIONS = ["1"]` (current and previous major; there is no
previous one yet).

### Permissions and grants

`PluginPermission`: `mail.read` (headers, accounts, folders — no bodies),
`mail.read.body` (needs `mail.read`; never the decrypted content of
end-to-end encrypted mail — that is M4's `mail.read.encrypted`),
`calendar.read` (placeholder until M3: accepted, grants nothing),
`ui.dialogs`, `notifications`, `storage`, `network:<host>` (one host per
permission: `api.example.com`, `*.example.com` for subdomains, IPv4
literals; no scheme, port or path; `allows_host`). On the wire a permission
is its name; TypeScript gets the literal union with
`` `network:${string}` ``. The design's M4 names are refused as
`permission-not-in-preview`.

**Deviation from design.md.** The design lets dialogs, commands and
storage go without a permission. The preview (plan WP60) names
`ui.dialogs`, `notifications` and `storage` as permissions so the install
dialog shows every capability a plug-in uses; commands need none.

`PluginGrant {permission, accounts?}`: `accounts` only for the two mail
permissions (`null` = every account, never empty). Grants are a subset of
the declared permissions (`nail_store::check_plugin_grants`).

### Declarative UI (`PluginUiNode`)

A tagged union with `type`: `text` (style body / heading / muted / strong /
error), `markdown`, `input` (kind text / email / url / number), `textarea`,
`select` (`multiple`), `checkbox`, `date` (`YYYY-MM-DD`, `time` adds
`THH:MM`; default also `today` / `tomorrow`), `contact-picker`,
`folder-picker`, `message-list` (`messageIds`; the host fills `messages`
with `PluginMessage` rows the plug-in may read), `table` (text cells),
`button` (sends a UI action; variant default / primary / danger),
`progress` (0–100 or indeterminate), `image` (`data:` URI of PNG / JPEG /
GIF / WebP only; package assets are inlined by the host; no SVG, no remote
URLs) and `stack` (row / column container — not in the design's list, but
a tree needs one). Optional fields are omitted from the JSON when unset.
The value each input reports is fixed (`value_kind`, table in the module
docs). `validate_tree` enforces 500 nodes, depth 8, 64 KiB texts, 500
items, unique well-formed ids, valid defaults and dates, row lengths,
images ≤ 512 KiB (`PluginUiIssue`, Fluent ids `plugin-ui-issue-*`, logged
to the plug-in's log).

### Runtime types

`PluginCommandContext {surface, selection {messageIds, openMessageId?,
threadId?}, accountId?, folderId?, draftId?}` (`from_ui` builds it from the
UI context; selected text and the draft cursor are not passed on).
`PluginDialogSpec {title, body, buttons [{id, label, primary}], modal =
true, size small | medium | large}` → `PluginDialogResult {button | null,
values}`. `PluginNotificationSpec {title, body?, level info | success |
warning | error, system}` (`system`: also an OS / Web Notification, subject
to quiet hours). `PluginMessage` (headers view: id, uri, account, folder,
thread, subject, from, to, cc, date, flags, tags, hasAttachments,
encrypted) and `PluginMessageBody` (text, sanitized html?, attachment names
/ types / sizes).

**Which UI.** Several UIs may be connected. A UI sends a random
`client_id` with `RunPluginCommand`, `PluginUiAction` and `GetPluginPanel`;
dialogs and notifications raised while handling the call carry it, and a
UI shows only those with its own id or `null` (background work).

### Requests, responses, events

| Request | Response | Served at the contract stage |
| --- | --- | --- |
| `ListPlugins`, `GetPlugin {plugin_id}` | `Plugins {plugins}`, `Plugin {plugin}` | yes (store; enabled plug-ins are `stopped`, `crashed` with a recorded error) |
| `PreviewPluginInstall {source}` | `PluginInstallPreview {preview}` (manifest, issues, sha256, size, installed version) | `unsupported` (WP61 → WP64) |
| `InstallPlugin {source, package_sha256, grants, enable}` | `Plugin` + `PluginsChanged` | `unsupported`; `conflict` when the hash differs from the preview |
| `LoadDevPlugin {folder, grants}` | `Plugin` | `unsupported`; desktop only |
| `EnablePlugin`, `DisablePlugin`, `UpdatePluginGrants {grants}` | `Plugin` + `PluginsChanged` | yes (flag / grants in the store) |
| `UninstallPlugin` | `Done` + `PluginsChanged` | yes (store rows; package files by WP64); items in `layout.kdl` stay |
| `ListPluginLogs {plugin_id?, after_seq?, limit}`, `ClearPluginLogs` | `PluginLogs {entries}`, `Done` | yes (limit clamped to 1–1000) |
| `GetPluginSettings`, `UpdatePluginSettings {values}` | `PluginSettings {plugin_id, schema, values}` | yes (`invalid_request` + `plugin-settings-invalid`) |
| `PreviewPluginPlacements {plugin_id}` | `PluginPlacements {plugin_id, incoming, preview}` (applied with `ApplyConfigMerge`) | `unsupported` |
| `RunPluginCommand {command, surface, context, client_id?}`, `RespondPluginDialog {dialog_id, button?, values}`, `PluginUiAction {target, component_id, values, client_id?}` | `Done` | `unsupported` |
| `GetPluginPanel {panel, context, client_id?}` | `PluginPanel {panel, tree}` | `unsupported` |

Sources: `PluginPackageSource` = `file {path}` | `url {url}` (https;
http only for loopback) | `dev {folder}`. Install is two-step so the user
approves exactly the package that was previewed (hash). An update keeps
storage, logs and valid settings and replaces the grants with the dialog's.

Events: `PluginsChanged`, `PluginDialogOpened {dialog}`,
`PluginDialogClosed {dialog_id, plugin_id}`, `PluginNotification
{notification}`, `PluginPanelChanged {panel, tree}`, `PluginLogLine
{entry}`, `PluginStopped {plugin_id, reason, error?, localized?}`
(`PluginStopReason`: disabled, uninstalled, reloaded, shutdown, and the
crashes error / cpu-budget / memory-limit; Fluent `plugin-stopped-*`).
`PluginRunState`: disabled, starting, running, stopped, crashed,
incompatible.

**Remote access.** Plug-ins read mail with their own grants, which could
reach past a remote session's narrowed grant. So remote sessions see and
use plug-ins only with a grant for every account (`ListPlugins` is
narrowed to nothing, `GetPlugin` and the runtime requests are refused
otherwise; their events go to full grants only); administration (install,
enable, grants, logs, settings, placements) is `admin`; packages are
installed remotely from URLs only; `LoadDevPlugin` is desktop only; log
and stop events go to admin sessions.

### Store schema v9 (`nail-store/src/plugins.rs`)

`plugins` (manifest JSON as installed, source kind + location, package
hash, enabled, settings JSON of changed values, storage quota, times, last
error), `plugin_grants`, `plugin_storage` (per-plug-in KV: JSON values,
`size` = key + plain JSON, quota default 5 MiB, 256-byte keys, 1 MiB
values; `Error::QuotaExceeded` → `too_large`), `plugin_logs` (2000 lines
per plug-in, 8 KiB each, millisecond times). Storage values and log
messages are **sealed at rest** (`SEALED_COLUMNS`), since both may quote
mail. Everything cascades with the plug-in. Accessors: `install_plugin`,
`plugin(s)`, `set_plugin_enabled`, `set_plugin_error`, `remove_plugin`,
`plugin_grants` / `set_plugin_grants`, `plugin_settings` /
`update_plugin_settings`, `plugin_storage_{get,set,delete,keys,clear,usage}`,
`set_plugin_quota`, `append_plugin_log`, `plugin_logs`,
`clear_plugin_logs`. Parallel packages add no migrations; WP64 appends v10
if a package reports a need.

### Layout

`plugin-panel "<plugin>.<panel>"` is a widget of `nail-config` (argument
stored as the `panel` option; `panel="…"` works too; invalid ids are
`config-plugin-panel-invalid`). The earlier placeholder form — any widget
kind `plugin.<id>.<name>` — still parses; WP62 decides whether the UI
renders it. Sizes come from the parent split's `sizes` (no `width=` on
widgets). Placements become layout text in `nail-config/src/plugins.rs`
(WP62) and are confirmed in the existing merge dialog.

### Crates and libraries

Licences checked (`cargo deny --offline check licenses bans sources`: ok;
`cargo tree -i openssl-sys` / `aws-lc-rs` empty):

- `rquickjs` 0.14 (MIT; QuickJS-NG C sources, MIT, compiled by `cc`; the
  crate ships pre-generated bindings for `x86_64-pc-windows-msvc`, so no
  bindgen / libclang). Features `futures`, `loader`, `macro`, `classes`,
  `properties`. Verified on Windows MSVC by the stub's tests: evaluation,
  an interrupt handler stopping `for (;;) {}`, the memory limit stopping
  runaway allocation, no `require` / `fetch` / `process` / `std` / `os`
  in a full context.
- `zip` 8.6 (MIT) with only `deflate-flate2-zlib-rs` (zlib-rs, Zlib
  licence, pure Rust): no bzip2 / zstd / lzma C code, no AES.
- `nail-plugins` also uses the workspace tokio, serde_json, sha2 / hex,
  url and notify-debouncer-mini (hot reload).

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| Plug-in host (runtime, gate, API bindings, packages, dev reload) | `crates/nail-plugins/` | WP61 |
| `nail plugin dev` | `crates/nail-cli/src/plugin.rs` (subcommand wired; arguments live there) | WP61 |
| Host texts (manifest / UI issues, stops, CLI) | `locales/{en,de}/core-plugins.ftl` (contract ids pre-filled) | WP61 |
| Plug-in UI (renderer, dialogs, commands, notifications, install dialog, manager) | `ui/src/features/plugins/` (registered in `features/registry.ts`, mock routed in `api/mock.ts`, state `data.features.plugins`) | WP62 |
| Layout widgets and placements | `crates/nail-config/src/widgets/plugins.rs`, `crates/nail-config/src/plugins.rs` | WP62 |
| UI texts | `locales/{en,de}/plugins.ftl` (permission labels pre-filled) | WP62 |
| SDK, template, example, user docs | `packages/plugin-api/` (except `src/generated/`), `packages/create-nail-plugin/`, `examples/`, `userDocs/en/plugins/` | WP63 |

`packages/*` are npm workspaces; the root `npm test` also runs their `test`
scripts (`npm run test -w packages --if-present`), `npm run lint` their
`lint` scripts. `packages/plugin-api/src/generated/` is generated (not
formatted by Prettier, never edited by hand).

WP65 (carry-over) builds in the directories its work-package description
names; the plug-in contract adds nothing there, and everything of M2d lives
in the new files above, so no plug-in slot overlaps a carry-over area.

Integrator-owned during M2d (report, do not edit): `Cargo.toml` (the
dependencies above are pre-declared), `Cargo.lock` beyond a package's own
new dependency, `nail-api` (incl. the generated TypeScript),
`nail-store` migrations, `nail-core` (`src/plugins.rs` serves the store
part now), `nail-remote-auth`, `nail-server`, `app/src-tauri`,
`crates/nail-api/fixtures`, `e2e/`, `defaults/`, the root `package.json`,
`docs/adr` numbering.

## Consequences

- WP61–WP63 compile against fixed types; the suites stay green in between.
  The UI mock answers every new request like the core does now.
- WP64 wires: the host into the core (a `nail-core` adapter implementing
  the host's trait over the store, the Client API with the permission gate,
  events), install / preview / dev loading / uninstall of package files,
  resolved manifest texts, placements through `nail-config`,
  `nail plugin dev`, OS notifications for `system` notifications, remote
  event routing by `client_id`, and the M2d Playwright scenarios.
- The preview never gives plug-ins decrypted end-to-end encrypted content,
  write access or hooks; those arrive with M4's permission model.
