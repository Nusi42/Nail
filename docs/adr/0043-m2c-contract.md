# ADR 0043: M2c contract — Sieve, triage, sync policies, menus, localized core messages, store v7, slots

Status: accepted · 2026-09-30 · M2c WP50

## Context

Wave M2c (plan section 7) adds server-side rules and vacation replies
(WP51), the junk classifier, snooze and new-mail notifications (WP52),
engine polish — sync policies with on-demand bodies, server-side search,
localized Rust-built messages, a fixed MCP port, `RuleFromMessage` (WP53) —
and UI polish with accessibility (WP54), built by four agents in parallel
and integrated by WP55. This ADR fixes the contract they build against, the
store schema they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M2"): no GPL / LGPL / AGPL
dependency, no OpenSSL / aws-lc; strings only from Fluent catalogs; the
core owns data and every write goes through the operation queue.

## Decisions

### Localized core messages (`nail_api::localized`)

`LocalizedMessage {id, args}` — a Fluent id (`is_message_id`: lowercase,
digits, single `-`) and named arguments (`MessageArg` = integer or text;
JSON `{"id": "…", "args": {"count": 3}}`, `args` omitted when empty).
**Every contract type that carries an English diagnostic now has an
optional `localized` sibling** (`#[serde(default, skip_serializing_if)]`,
optional in TypeScript), so the wire format stays compatible and
`PROTOCOL_VERSION` stays 1:

`ApiError` and `Response::Error` (boxed: `Option<Box<LocalizedMessage>>`,
keeps `Result<_, ApiError>` small), `engine::EngineError` (boxed, carried
into `ApiError`), `ConfigIssue`, `Event::{ConfigError, SyncFailed,
SendStatus, OAuthFinished}`, `AccountStatus`, `EndpointTestResult`,
`OutboxEntry`, `OpStatus`, `RuleRun`, `MergeConflict`, `McpAuditEntry`,
`RemoteUiStatus`, `RemoteAuditEntry`, `KeyImportIssue`, `KeyLookupFailure`,
`VaultStatus`, and the new `SieveAccountStatus`.

The UI shows `localized` when present, else the text chosen by the code
(`error-<code>`) or the type's kind / state; the English `message` stays
for logs. Producers: the core, `nail-config` (configuration errors), the
MCP server and the CLI (WP53), `nail-sieve` (WP51) and the notification
policy (WP52). Validation issues of the new settings are enums with a
Fluent id each (`VacationIssue`, `ManageSieveIssue`, `JunkIssue`,
`NotificationIssue`, `McpSettingsIssue`; `message_id()`, `ALL`), and the
core answers invalid settings with `invalid_request` + that id.
Rendering in Rust (CLI output, desktop OS notifications) uses
`fluent-bundle` over the same catalogs (slot `nail-config/src/l10n.rs`,
WP53); `LocalizedMessage::render_with` covers simple placeables.
`nail-api/tests/catalog.rs` checks that every id the contract names exists
in `locales/en` and that `locales/de` has the same files.

Catalog files by producer, so parallel packages never edit one file:
`core.ftl` (WP53), `core-sieve.ftl` (WP51), `core-triage.ftl` (WP52,
notification texts; `notification-test-*`, `notification-new-mail-*`,
`notification-snooze-woke-title` are pre-filled), UI catalogs
`sieve.ftl`, `triage.ftl`, `sync.ftl`, `menus.ftl` (WP54); `mcp.ftl`
gained `mcp-settings-issue-port`.

### Server-side rules and vacation (`nail_api::sieve`)

- **ManageSieve settings** are part of an IMAP account in `accounts.kdl`:
  `AccountConfig.sieve: Option<ManageSieveConfig {enabled, host, port,
  tls, script_name}>` (the `sieve` node; defaults: IMAP host, port 4190,
  STARTTLS, script `"nail"`, enabled). The login is always the account's
  IMAP login (`ManageSieveConfig::endpoint`). Read / written by
  `nail-config` (schema, round trip, `sieve` needs IMAP) and documented in
  `userDocs/en/accounts-reference.md`.
- **Placement per rule**: `RuleSieveStatus {rule_id, placement: server |
  client | not-expressible, reasons: [LocalizedMessage]}`. A rule never
  runs in both places for the same message: `server` rules are skipped on
  arrival by the client (manual runs still work). No per-rule preference in
  `rules.kdl` in M2c (a later addition if needed).
- **Account state**: `SieveAccountStatus {state: disabled | unavailable |
  pending | uploading | in-sync | failed, script_name, uploaded_at,
  extensions, other_scripts, rules, vacation_active, error, message,
  localized}`. Nail owns one script per account, makes it active and
  leaves other scripts alone.
- **Vacation**: `VacationSettings {enabled, subject, body, start_date,
  end_date, days (1–365, default 7), addresses, from, skip_lists}` with
  `issues()`; compiled into the same script (`vacation`, dates through
  `date` when available).
- Uploading is not an operation-queue write: the user's write is the rule
  / vacation change (local first: `rules.kdl`, the store); the upload
  replays it to the server, retried like a queue replay and visible as
  `SieveState`.

| Request | Response | Served now |
| --- | --- | --- |
| `GetSieveStatus {account_id?}` | `SieveStatus {accounts}` | yes (state from settings + store; `rules` empty until WP55) |
| `SyncSieve {account_id}`, `PreviewSieveScript {account_id}` | `SieveStatus` / `SieveScript {account_id, script, rules}` | `unsupported` (WP51 → WP55) |
| `GetVacation {account_id}`, `UpdateVacation {account_id, settings}` | `Vacation {account_id, settings}` | yes (validated, stored; upload by WP55) |

### Junk, snooze, notifications (`nail_api::triage`)

- **Junk** per account: `JunkSettings {enabled (default false: the
  classifier is optional), threshold 50–99 (90), min_training (10),
  honor_server_flags, move_to_junk}`, `JunkStatus {settings,
  trained_junk, trained_ham, tokens, ready, last_trained_at}`,
  `JunkVerdict {junk, score?, source: server | classifier | user}` (optional
  `MessageBody.junk`). Keywords `$Junk` / `$NotJunk`. Never deletes.
  `MarkJunk {message_ids, junk}` → `Queued` (train + keyword + move through
  the queue). Training always happens; `enabled` switches classification
  of arriving mail.
- **Snooze**: keyword `$Snoozed` + move to the hidden folder `Snoozed`
  (both queue writes), wake time kept locally. `SnoozeMessages
  {message_ids, until, wake_folder_id?}` / `UnsnoozeMessages` → `Queued`;
  `ListSnoozed {account_id?}` → `Snoozed {messages: [SnoozedMessage]}`.
- **Notifications**: `NotificationSettings {enabled, show_preview, sound,
  folders: inbox | all, overrides: [{scope: account | folder, notify}],
  quiet_hours: {start "HH:MM", end, days}?}`. Precedence of the policy:
  off → quiet hours (dropped, not deferred) → a rule's `notify` action →
  folder override → account override → `folders`. The core pushes
  `Event::NotificationRequested {notification: MailNotification {id, tag,
  kind: new-mail | snooze-woke | test, account_id, folder_id, message_ids,
  title, body?, sound}}` with localized texts; the desktop shell shows an
  OS notification from Rust (`tauri-plugin-notification`, registered;
  the web content gets no permission), the remote UI a Web Notification.

| Request | Response | Served now |
| --- | --- | --- |
| `GetJunkStatus`, `UpdateJunkSettings`, `ResetJunkClassifier` | `JunkStatus {status}` | yes |
| `MarkJunk`, `SnoozeMessages`, `UnsnoozeMessages` | `Queued` | `unsupported` (WP52 → WP55) |
| `ListSnoozed {account_id?}` | `Snoozed` | yes |
| `GetNotificationSettings`, `UpdateNotificationSettings` | `NotificationSettings {settings}` | yes |
| `TestNotification` | `Done` + `NotificationRequested` (kind `test`) | yes |

### Sync policies and bodies (`nail_api::sync`)

The policy stays in `accounts.kdl` (ADR 0013: account settings live
there). `GetFolderSyncPolicy {folder_id}` / `SetFolderSyncPolicy
{folder_id, bodies?, enabled?, idle?}` → `FolderSyncPolicy {policy}`
(resolved bodies / enabled / idle, the rule written for exactly this path,
`on_demand` = IMAP); `Set` writes the folder's `sync` rule through
`save_account` (all `null` removes it). `MessageSummary.headers_only`
marks messages whose body is not stored; `GetMessage` fetches on demand
(WP53 / WP55); `FetchMessageBodies {message_ids}` → `BodiesFetched
{fetched, failed}` (served now: stored bodies are `fetched`).
`Response::SearchResults.server: ServerSearch {accounts, hits, failed}`
describes the IMAP `SEARCH` fallback.

### UI polish

- **Context menus** (`nail_api::menus`): `ResolvedMenu {name, items:
  [ResolvedMenuEntry: action {action, args, label?, icon?, shortcut?} |
  separator | submenu]}` resolved from `layout.kdl`'s `menu
  "context:<part>"` by `resolve_menu` (labels / icons from the item, then
  the action table; shortcut = last binding of the same action + args in
  the part's key context, else `global`; separators normalized, empty
  submenus dropped). `CONTEXT_MENU_PARTS` names the parts and their key
  contexts. The UI port is `ui/src/features/menus/resolve.ts`, tested
  against the same cases.
- **Toolbar / key / column editors** reuse `UpdateLayout` with the existing
  `LayoutPatch`es (`toolbar-order`, `add-key-binding`,
  `remove-key-binding`, `column-order`) plus two new ones:
  `reset-toolbar {toolbar}` and `reset-key-binding {context, key}` (remove
  the file's override so the default applies; a full file gets the default
  written back). Implemented in `nail-config` write-back and the UI mock.

### MCP port and rules from messages

`McpSettings {port?}` (≥ 1024, `McpSettingsIssue`) in `app_settings`
(`MCP_SETTINGS_KEY`): `GetMcpSettings` / `UpdateMcpSettings` →
`McpSettings {settings, info}` + `Event::McpSettingsChanged` (served; the
server picks the port up on (re)start — WP53 / WP55; `--mcp-port`
overrides). `RuleFromMessage {message_id}` → `RuleSuggested {rule}`
(served through `nail_rules::rule_from_message` on the stored message;
WP53 extends it to all headers).

New events: `SieveStatusChanged {account_id}`, `JunkStatusChanged
{account_id}`, `SnoozeChanged`, `NotificationSettingsChanged`,
`NotificationRequested`, `McpSettingsChanged`.

**Remote access** (`nail-remote-auth::required_access`, core
`check_remote_request`, `remote_event`, narrowing): rule placements and
scripts need a grant for every account (like rules); message requests
check the messages (`MarkJunk`, `Snooze`, `FetchMessageBodies`,
`RuleFromMessage` with the `encrypted` gate); account requests the
account; settings changes, `SyncSieve`, `SetFolderSyncPolicy` and MCP
settings are `admin`; notification overrides, snoozed lists and server
search info are narrowed to the grant.

### Store schema v7 (`nail-store`, `src/triage.rs`)

- `messages.body_state` (`synced` | `headers-only` | `on-demand`, CHECK;
  existing rows `synced`), `set_body_state`, `body_state`,
  `headers_only_messages`; summaries carry `headers_only`.
- `snoozed_messages` (wake time, wake folder, `Message-ID` for rebinding;
  no FK to messages), `snooze` (future time, wake folder of the same
  account), `unsnooze`, `list_snoozed`, `due_snoozes`, `next_snooze_wake`,
  `rebind_snooze`.
- `junk_settings`, `junk_tokens` (**hashed** tokens: 64-bit SHA-256 prefix,
  `hash_junk_token`, CHECK on the shape — the table never spells out words
  of the mail; it does not resist a dictionary attack, full-file encryption
  is the vault follow-up), `junk_training` (per message key, so re-marking
  moves a message between classes), `junk_verdicts`; `train_junk`
  (`Trained` / `Retrained` / `Unchanged`), `junk_token_counts`,
  `junk_stats`, `reset_junk`, `set_junk_verdict`, `junk_verdict`.
- `notify_overrides` (removed with their account / folder by triggers) +
  global settings in `app_settings`; `notification_settings`,
  `set_notification_settings` (validated, unknown targets refused).
- `sieve_state` per account (script hash, upload time, extensions, other
  scripts, vacation JSON, last error); `sieve_record`, `set_vacation`,
  `record_sieve_upload`, `record_sieve_error`.

**Parallel packages add no migrations**; WP55 appends v8 if a package
reports a need.

### Crates and libraries

Licences checked with `cargo metadata` and `cargo deny check licenses
bans` (MIT / Apache-2.0 only; `self_cell` is "Apache-2.0 OR GPL-2.0-only",
used under Apache-2.0). `cargo tree -i openssl-sys` / `aws-lc-rs` /
`aws-lc-sys` stay empty.

- `nail-sieve` (WP51, stub): workspace tokio, tokio-rustls / rustls (ring),
  base64, regex, sha2, hex; smoke tests (ring client config, SASL
  encodings, script hash). No Sieve library: `sieve-rs` is AGPL, so WP51
  writes the compiler and a small parser for the round-trip harness.
- `nail-triage` (WP52, stub, pure logic): chrono, `unicode-segmentation`
  1.13; smoke tests.
- `fluent-bundle` 0.16 + `unic-langid` 0.9 (nail-config `l10n` slot,
  smoke test renders the shipped catalog).
- `tauri-plugin-notification` 2.5 (desktop shell; registered, events wired
  by WP55).

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| Sieve compiler, ManageSieve client | `crates/nail-sieve/` | WP51 |
| Mock ManageSieve server | `crates/nail-testkit-net/src/managesieve.rs` (+ its `pub mod` line) | WP51 |
| Texts built by nail-sieve | `locales/{en,de}/core-sieve.ftl` | WP51 |
| Classifier, snooze scheduler, notification policy | `crates/nail-triage/` | WP52 |
| Notification texts built in Rust | `locales/{en,de}/core-triage.ftl` | WP52 |
| Engines, MIME, store, config (incl. `src/l10n.rs`), MCP, CLI, rules, search | `crates/nail-proto-imap`, `nail-mime`, `nail-store`, `nail-config` (except `src/widgets/`), `nail-mcp`, `nail-cli`, `nail-rules`, `nail-search` | WP53 |
| Texts built by core / config / MCP / CLI | `locales/{en,de}/core.ftl` | WP53 |
| UI | `ui/` (feature slots `sieve/`, `triage/`, `sync/`, `menus/` registered in `features/registry.ts`; mocks routed in `api/mock.ts`), `locales/` except the `core*.ftl` files, `defaults/`, `crates/nail-config/src/widgets/{sieve,triage,sync}.rs` | WP54 |

Integrator-owned during M2c (report, do not edit): `Cargo.toml` (the
dependencies above are pre-declared — ask before adding more),
`Cargo.lock` beyond a package's own new dependency, `nail-api`,
`nail-store` migrations, `nail-core`, `nail-server`, `nail-remote-auth`,
`app/src-tauri`, `crates/nail-api/fixtures`, `e2e/`, `docs/adr` numbering.
WP53 may change `nail-store` accessors and `nail-config` APIs as long as
the workspace keeps compiling (it owns those crates; WP55 wires the core).

## Consequences

- WP51–WP54 compile against fixed types and generated TypeScript; the
  suites stay green in between. The UI mock answers every new request like
  the core does today (plus working MCP settings, `RuleFromMessage`, sync
  policies and the reset patches).
- WP55 wires: `nail-sieve` into the core (compile on rule / vacation
  changes, upload, placements in `GetSieveStatus`, skipping `server` rules
  on arrival, the mock ManageSieve server in demo-multi), `nail-triage`
  (training on `MarkJunk`, classification on arrival, `MessageBody.junk`,
  the snooze wake timer and hidden folder, the notification policy on
  `NewMail` and the rules' `notify` action, OS notifications in the desktop
  shell), headers-only sync and on-demand fetch, the server search
  fallback, the MCP port from the settings, `localized` texts from the
  packages, and the M2c Playwright scenarios.
- Construction sites of the migrated types gained `localized: None`
  (mechanical); producers fill it in during M2c. Where an `EngineError`
  is at hand the core already passes its text on (account tests, queued
  operation status). Diagnostics read back from the store (outbox entries,
  MCP and remote audit entries) have no stored `localized` yet: the UI
  shows the error code's text for them until a package adds the column.
