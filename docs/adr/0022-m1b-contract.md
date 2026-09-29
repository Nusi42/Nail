# ADR 0022: M1b contract — protocol, store v4, crate stubs, feature slots

Status: accepted · 2026-09-29 · M1b WP20

## Context

Wave M1b (plan section 6) adds the rich composer (WP21), the rules engine
(WP22), the read-only MCP server (WP23) and customization tooling (WP24),
built by four agents in parallel, then integrated by WP25. This ADR fixes
the contract they build against and the "slots" that keep the four
packages out of each other's files.

## Decisions

### Protocol additions (`crates/nail-api`)

Conventions of ADR 0002 / 0012 hold (tagged enums, snake_case fields,
kebab-case data enums, `null` for absent options, opaque ids, large
variants boxed). `PROTOCOL_VERSION` stays 1. New modules: `rules`, `mcp`,
`context` (UI context), `customize` (config errors, layout patches, merge
dialog, backups); `compose` and `search` grow. New ids: `RuleId`,
`RuleRunId`, `McpClientId`, `SavedSearchId`, `ConfigBackupId`.

**Rich drafts.** `Draft` gains `context: DraftContext {mode: reply |
reply-all | forward, message_id}`, `signature: identity | none` and
`markdown_part` (send the source as `text/markdown`); `html`, `markdown`
and `attachments` (`DraftAttachment {filename, mime, size, blob, content_id}`,
inline images by `content_id`) are live. The M1a field `in_reply_to` is
**replaced** by `context` (a deliberate incompatible change inside wave
M1: no client outside this repository exists; the store migrates stored
drafts, see below). `Draft::replied_message()` gives the replied message;
`Draft::new(identity)` an empty plain draft. Until WP21 lands the core
still refuses rich drafts with `unsupported`.

| Request | Response | Package |
| --- | --- | --- |
| `RetryOp {op_id}` | `Queued {op_ids}`; `conflict` unless failed | core (done) |
| `NewDraft {identity_id?, account_id?, mode?, context?}` | `Draft {draft}` (unsaved; quote, recipients, signature, forwarded attachments filled in) | WP21 |
| `RenderPreview {markdown, identity_id?}` | `PreviewRendered {html}` (sanitized, stylesheet inlined — what would be sent) | WP21 |
| `ConvertDraft {draft, mode}` | `DraftConverted {draft, lossy}` | WP21 |
| `UploadAttachment {filename, mime, data (base64), content_id?}` | `AttachmentUploaded {attachment}` | WP21 |
| `SuggestAddresses {prefix, limit}` | `AddressSuggestions {addresses}` | WP21 |
| `ListRules` / `SaveRule {rule, position?}` / `DeleteRule` / `ReorderRules {rule_ids}` | `Rules` / `RuleSaved` / `Done` / `Done` | WP22 |
| `TestRule {rule, folder_id?, limit}` | `RuleTested {scanned, matches: RuleMatch[]}` | WP22 |
| `RunRule {rule_id, message_ids, folder_id?}` | `RuleRan {run}` | WP22 |
| `ListRuleRuns {rule_id?, limit}` / `UndoRuleRun {run_id}` | `RuleRuns` / `Queued` | WP22 |
| `RuleToText {rule}` / `RuleFromText {text}` | `RuleText {text}` / `RuleParsed {rule?, errors}` | WP22 |
| `ListMcpClients` / `CreateMcpClient {name, grant}` | `McpClients` / `McpClientCreated {client, token}` (token shown once) | WP23 |
| `UpdateMcpGrants {client_id, grant}` / `RevokeMcpClient` | `McpClientSaved` / `Done` | WP23 |
| `ListMcpAudit {client_id?, before?, limit}` / `GetMcpServerInfo` | `McpAudit {entries}` / `McpServer {info}` | WP23 |
| `ReportUiContext {context}` / `GetUiContext` | `Done` / `UiContext {context}` | core (done); UI reporting WP23 |
| `UpdateLayout {patches: LayoutPatch[]}` | `Done` (+ `ConfigChanged`) | WP24 |
| `PreviewConfigMerge {file, incoming}` | `ConfigMergePreview {preview}` | WP24 |
| `ResolveConfigMerge {file, incoming, choices, default}` | `ConfigMergeResolved {theme?, layout?}` (live preview) | WP24 |
| `ApplyConfigMerge {…}` / `ListConfigBackups {file}` / `RevertConfig {file, backup_id?}` | `ConfigMergeApplied {backup}` / `ConfigBackups` / `Done` | WP24 |
| `OpenConfigFile {file, line, column}` | `Done` (inspector "open file at line") | WP24 |
| `ListSavedSearches` / `SaveSavedSearch {search}` / `DeleteSavedSearch` | `SavedSearches` / `SavedSearchSaved` / `Done` | core (done); UI WP24 |

`LayoutPatch` (tag `op`): `split-sizes {node, sizes}`, `column-order {node,
columns}`, `toolbar-order {toolbar, items}`, `add-key-binding {context,
key, action, args}`, `remove-key-binding {context, key}`. Nodes are
addressed by `LayoutNodeRef {window, breakpoint, path}` (child indexes),
which works for nodes that exist only in the defaults of a sparse file.

The merge dialog is stateless: every request carries the incoming text, so
the core keeps no merge sessions. Items have ids stable within one
preview; choices default to `mine`. Sections and conflict kinds are enums;
the UI shows `merge-section-<section>` and `merge-conflict-<kind>`.

**Rules model** (`nail_api::rules`): `Rule {id, name, enabled, triggers,
scope, conditions, actions, source}`; triggers `arrival | on-send |
schedule {interval_minutes} | manual | older-than {days}`; nested
`Condition`s (`all`, `any`, `not`, `header`, `address`, `subject`, `body`,
`query`, `attachment`, `size`, `date`, `age`, `flag`, `tag`, `account`,
`folder`, `auth`, `list-id`) with `TextMatch {op: equals | contains |
starts-with | ends-with | regex, value, case_sensitive}`; actions `move`,
`copy`, `delete`, `archive`, `tag`, `untag`, `flag`, `mark-read`,
`forward`, `redirect`, `reply` (template), `notify`, `stop`. Folders are
`FolderRef` (`role` or `path`, optional account) because `rules.kdl` is
written by people and must not contain opaque folder ids. `RuleRun` carries
cause, state (`running | done | failed | undone`), matched count, op ids
and `undoable`.

**MCP** (`nail_api::mcp`): `McpScope` (`read`, `raw-source`,
`attachments`, `encrypted`, `context`, `organize`, `permanent-delete`,
`draft`, `send`, `calendar`, `pim`, `rules`), `McpGrant {scopes,
all_accounts, accounts, excluded_folders, skip_draft_confirmation}` whose
default grants nothing (decided: no access until granted), `McpClient`,
`McpAuditEntry`, `McpServerInfo`.

**New events:** `ConfigErrors {file, errors: ConfigIssue[]}` (every error
of a file; empty = valid again) supersedes `ConfigError`, which stays
until WP24/WP25 switch the core and UI over; `RulesChanged`;
`RuleRunFinished {run}`; `McpContextAccess {client_id, client_name, edit}`
(on-screen indicator); `McpClientsChanged`; `SavedSearchesChanged`.

### Store schema v4 (`crates/nail-store`)

The plan called this "v3"; v3 already exists (POP3 bookkeeping, added by
WP14-core), so M1b is **v4**. Tables:

- `drafts` + columns `mode`, `context_mode`, `context_message_id`,
  `server_folder_id`, `server_uid`, `server_message_id` (server Drafts copy
  for autosave, WP21); `draft_attachments` (blob refs). The full draft
  stays JSON in `payload`. A Rust backfill turns M1a `in_reply_to` into a
  reply `context`. Blob garbage collection (`remove_unreferenced_blobs`,
  `blob_is_referenced`) now also keeps blobs a draft attachment uses.
- `collected_addresses` (autocomplete: `record_addresses`,
  `suggest_addresses`).
- `rule_state`, `rule_runs`, `rule_run_actions` (each action with its
  queued op id and the inverse `Operation`, so a run is undone as one
  unit), `rule_inbox` (new mail waiting for arrival rules, survives a
  restart). Rules themselves live in `rules.kdl`, the source of truth, as
  accounts do in `accounts.kdl` (ADR 0012).
- `mcp_clients` (SHA-256 of the token only, `hash_mcp_token`; the token
  is random, so no password hash is needed), `mcp_grants`, `mcp_audit`.
- `saved_searches`.

Ids formatted by the store: `run-<n>`, `mcp-<n>`, `search-<n>` (as
`draft-<n>` before). Agents must not add migrations in parallel: a package
that needs a schema change reports it and the integrator appends v5.

### Crate stubs and libraries

`crates/nail-compose` (WP21), `crates/nail-rules` (WP22) and
`crates/nail-mcp` (WP23) exist with smoke tests that exercise the chosen
libraries on Windows MSVC:

- `mail-builder` 1.0 (MIME), `comrak` 0.55 with `default-features = false,
  features = ["syntect-fancy"]` (syntect's pure-Rust regex engine; the
  default pulls in oniguruma's C build and a CLI), `css-inline` 0.21
  without default features (no `http` / `file` loading), `ammonia` 4.2
  (already used by nail-mime).
- `regex` 1.13.
- `rmcp` 3.5 with `server`, `macros`, `transport-io` (stdio) and
  `transport-streamable-http-server`; tests add `client` and
  `transport-streamable-http-client-reqwest`. reqwest stays on the
  workspace's `rustls-no-provider` build: no aws-lc (ADR 0014 holds;
  checked with `cargo tree -i aws-lc-rs`). The smoke test runs a
  Streamable HTTP round trip (axum on 127.0.0.1:0).

Expected shapes: `nail-compose` builds MIME from a `Draft` + `Identity`
(+ reply context) and renders Markdown for `RenderPreview`, replacing
`nail-core/src/mime.rs`; `nail-rules` evaluates rules against messages and
turns actions into `nail_store::Operation`s with inverses; `nail-mcp`
serves resources / tools over Streamable HTTP and stdio, checks grants on
every call and appends audit entries. None of them may depend on
`nail-core` (the core depends on them); they reach data through traits or
`nail-store` directly as WP23 decides.

### Collision-free slots

Each package gets files only it edits:

| Area | Slot |
| --- | --- |
| Core requests | `nail-core/src/{composer,rules,mcp,customize}.rs`: `handle(service, request)`; `Service::dispatch` routes each package's requests there in one arm |
| Widget kinds | `nail-config/src/widgets/{composer,rules,mcp,customization}.rs` (the table was split out of `layout.rs`) |
| `rules.kdl` | `nail-config/src/rules.rs` (declared in `lib.rs`, empty) |
| UI | `ui/src/features/{composer,rules,mcp,customization}/`: `index.ts` exports `widgets`, `actions`, `dialogs`, `events`, `effects`; `mock.ts` answers the feature's requests in `MockTransport`; `messages.ts` feeds the catalog coverage test. `features/registry.ts` merges them (duplicates throw); `App.tsx` and the test harness use it. See `ui/src/features/README.md` |
| Strings | `locales/en/{composer,rules,mcp,customization}.ftl` + empty `locales/de` twins; the loader bundles every `.ftl` |

"Dialogs" are a new, small mechanism: a named component shown in the
standard window frame when `window.open "<name>"` names no layout window.
They are for fixed-purpose confirmations only; arrangeable UI stays in
`layout.kdl`.

Integrator-owned during M1b (report, do not edit): `service.rs`,
`App.tsx`, `src/api/mock.ts`, `src/state/uiStore.ts`, `defaults/*.kdl`,
`nail-store` migrations, `nail-api` (contract changes go through the
integrator).

## Consequences

- WP21–WP24 compile against fixed types and generated TypeScript; every
  new request answers `unsupported` in the core and the mock until its
  package lands, so the suites stay green in between.
- `RetryOp`, the UI context and saved searches work end to end already
  (core + mock); their UI is WP23 / WP24 / WP25.
- The core still emits only `ConfigError`; emitting `ConfigErrors` (and
  `nail-config` reporting every error) is WP24's.
- The core's Fluent strings load only `app.ftl`; WP21 extends that when it
  generates localized quote headers.
