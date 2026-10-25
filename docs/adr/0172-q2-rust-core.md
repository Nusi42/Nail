# ADR 0172: Q2 rust-core — SSOT / ODR / DRY in the core crates

Status: accepted · 2026-10-08 · Q2 package WP-rust-core (wave Q2, ADR 0171)

## Context

ADR 0171 measured the copied code and the repeated facts of the existing code
and gave WP-rust-core the crates nail-api, nail-core, nail-store, nail-config,
nail-mcp, nail-server, nail-cli, nail-search, nail-rules, nail-triage,
nail-sieve, nail-compose, nail-mime, nail-vault, nail-remote-auth, nail-proxy,
nail-plugins, nail-portability, nail-bench and nail-util, and the catalogs
`locales/*/core*.ftl` and `itip.ftl`: 584 findings at the contract commit
(529 clones, 18 repeated constants, 13 repeated Fluent texts, 24 repeated
numbers). Behaviour does not change; every suite stays green.

## Decisions

### 1. One home per kind of shared code

- **Contract facts and sample values live in nail-api.** `nail_api::testing`
  (feature `testing`) is the one definition of the accounts, folders,
  messages, reminders and confirmations tests build (`account`, `folder`,
  `summary`, `imap_account`, `reminder`, `send_confirmation`,
  `dev_installation`, the instants `T0` and `TUESDAY_NOON`, the
  `inert_desktop_host!` macro). Request groups that several dispatchers
  match (`mcp_admin_requests!`, `remote_admin_requests!` with
  `remote_auth_requests!`, `desktop_requests!`, `portability_requests!`,
  `plugin_admin_requests!`, the three PIM groups, `config_merge_requests!`)
  are `#[macro_export]` or-patterns in `nail_api::request_groups`; a match arm
  using one carries `#[expect(clippy::unnested_or_patterns, reason = …)]`.
  `Request::wire_type` / `Response::wire_type` name a request in messages
  (it replaces three hand-written `serde_json` lookups and the core's
  `request_name`). `nail_api::hosts` holds the web ports (`HTTPS_PORT`,
  `HTTP_PORT`, `web_port`), `nail_api::search::RESTRICTED_SCAN` the scan
  limit of restricted searches (MCP, remote sessions, plug-ins).
  `ActionItem` is the one shape of a toolbar / menu action (flattened into
  `ToolbarItem::Action`, `MenuItem::Action(ActionItem)`), `PluginTextField`
  the one shape of a plug-in text input (flattened into `Input` and
  `Textarea`); the generated TypeScript follows (`npm run gen:api`).
- **Test support of a crate is its `testing` module** behind a `testing`
  feature that the crate's own dev-dependencies enable: `nail_core::testing`
  (`TempService`, `quiet_options`, `plain_options`, `error_of`),
  `nail_store::testing` (`add_inbox`), `nail_rules::testing` (`mailbox`),
  `nail_portability::testing` (`PASSPHRASE`), `nail_proxy::testing`
  (`listen`). Integration tests share helpers through `tests/common/mod.rs`
  (`#[expect(dead_code, reason = …)] mod common;` where a crate uses a part).
- **Small pure helpers go to nail-util** (civil dates, Unix times).
- **Catalog checks** use `nail_config::l10n::assert_message_ids_in_catalog`.

### 2. Shared logic extracted (examples)

- nail-core: one `ingest_api_messages` and `API_INGEST_CHUNK` for Gmail and
  Exchange, `replay::{group_slots, group_results, Grouped, recorded_or}`,
  `pinned_http` (SSRF-safe hops shared by subscriptions and `nail.http`)
  over `nail_proxy::pin_host`, `editing::on_config` for every blocking
  config request, `Service::broadcast_config`, `write_collection` /
  `update_collection` for PIM collections, the `pim_replayer!` macro and
  `replayed_object` for both PIM replayers, `rooms_answer`, scheduling's
  `sent_record` / `mail_organizer` / `organized_event` / `one_sent`, the
  Exchange `delete_target` / `delete`, `StoreFolders` over
  `nail_sieve::FolderMap`, `check_vacation`, remote access's
  `narrowed`.
- nail-mcp: `Shared::new_confirmation` / `run_approved` for MCP and actor
  writes, `counted_resource` for resource reads and subscriptions, mail
  `requests_plan` / `deletion_changes` / `draft_summary`, PIM
  `event_plan_head` / `scoped` / `Ctx::task`; tests: `common::{transport,
  call, pending, set_grant}` and per-file helpers (`claude`, `spawn_call`,
  `finished`, `approve`, `spawn_run`, `deny`, `bridged`, `context_data`).
- nail-store, nail-config, nail-search, nail-plugins, nail-portability,
  nail-remote-auth, nail-server, nail-cli, nail-vault, nail-sieve,
  nail-triage, nail-mime, nail-compose: shared migrations / row readers,
  KDL write helpers, test stores and harnesses (see the commit).

### 3. One Fluent text per meaning: message references

Where two ids mean the same (import / export format names, the theme kind
and the OS colour scheme, the Unread filter and favorite, an invalid colour,
the plaintext-needs-loopback and unknown-account config errors, a switched-off
rule for Sieve and Exchange), the second id references the first
(`export-format-mbox = { import-format-mbox }`). Both ids stay (the enums'
label families stay complete); the text is written once. Message references
keep the caller's arguments (pinned by
`l10n::tests::message_references_render_the_shared_text`).

### 4. Reasoned exceptions (`scripts/duplicates/allow.json`)

Findings that are no violation, each with its reason: four constants that
agree by chance or differ (two plug-in / MCP name limits, two parsers'
nesting limits, two different test calendars the check cuts at `//`, two
audit page sizes, the 30 s test wait of two Cargo workspaces), two Fluent
texts with two meanings (`Snoozed`, `Comment`), and thirteen numbers whose
occurrences are unrelated limits that happen to agree (300, 512, 600, 4096,
10000, 120, 128, 366, 2000, 254, 365, 150, 250). Numbers with one meaning
became constants instead (the web ports, the IMAPS port via
`default_port`, `RESTRICTED_SCAN`, `MAX_UNDO_SEND_SECS` in the schema,
`CONFIRMATION_TIMEOUT_SECS`, `Ipv4Addr::LOCALHOST`, `is_ascii_graphic`).

### 5. Copies that removing a third copy exposed

Removing the core's copy of a three-way clone makes jscpd pair the two
remaining (engine) copies: a new finding that fails `npm run check`. Two
such pairs (the civil-date code of the updater and of nail-proto-imap, the
HTML escaping of nail-itip and nail-proto-graph) are fixed here with the
same hunks as q2-rust-engines (commit 6fa5c72: `nail_util::markup`,
`nail_util::time`), so the merge of both packages agrees; one engine number
(`365`) has a transitional `allow.json` entry that q2-rust-engines makes
obsolete.

## Consequences

- `node scripts/check-duplicates.mjs --strict --owner WP-rust-core` reports
  0 (from 584).
- Tests of other crates build their sample values from `nail_api::testing`;
  a new sample value goes there, not into a test file.
- `userDocs/en/plugins/components.md` lists the text-field properties of
  `input` / `textarea` in the order of the shared `PluginTextField`.
