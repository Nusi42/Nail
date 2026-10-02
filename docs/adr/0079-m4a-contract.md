# ADR 0079: M4a contract — writes by agents and plug-ins: confirmations and policies, MCP write tools / prompts / rate limits, plug-in write and hook permissions, hooks, composer / attachment / link / MCP extensions, background jobs, store v14, slots

Status: accepted · 2026-10-02 · M4a WP100

## Context

Wave M4a (plan section 9) makes agents and plug-ins write: the MCP write
tools, confirmations and prompts (WP101, `nail-mcp` / `nail-cli`), the
plug-in host's writes, hooks and extension points (WP102, `nail-plugins`),
the SDK, examples and docs (WP103, `packages/`, `examples/`, user docs) and
the UI for confirmations and extensions (WP104), built in parallel and
integrated by WP105. This ADR fixes the contract they build against, the
store schema they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M4", design "MCP server" and
"Plug-ins"): every write by an agent or plug-in goes through the operation
queue and the audit log; destructive or outgoing actions (delete, send,
forward, invitations, rule changes) need a confirmation the user sees in
Nail unless the user relaxed it per client / plug-in for non-destructive
actions (drafts, flags, tags); permanent delete is never auto-confirmed;
MCP elicitation where the client supports it, Nail's own dialog always; rate
limits per client; message content stays marked as untrusted; plug-ins get
every extension point of the design in v1 (hooks with time budgets that can
never block delivery or sending, a veto that names the plug-in and offers
"Send anyway", composer and attachment extensions, background jobs, MCP
exposure); no GPL / LGPL / AGPL, no OpenSSL / aws-lc; no non-loopback
sockets; strings from Fluent catalogs; the core owns data.

## Decisions

### Action kinds, policies and confirmations (`nail_api::agents`)

- Every write is one **action** of an `ActionKind`: `organize` (flags, tags,
  read, move, archive, snooze), `trash`, `permanent-delete`, `draft`,
  `send`, `calendar` (no mail), `invite` (mail to attendees, answers to
  invitations), `contact`, `task`, `pim-delete`, `rule`.
  `is_destructive` = trash, permanent delete, PIM deletes (Trash counts: the
  user decides, not an agent); `is_outgoing` = send, invite.
  `mcp_scopes()` / `plugin_permissions()` say what an actor needs.
- **Policies** (`ActionPolicy`, per actor and kind, mode `ask` / `allow`):
  defaults (`default_mode`) — destructive, outgoing and `rule` ask; the
  rest (`organize`, `draft`, `calendar`, `contact`, `task`) run without
  asking. The user may make any kind ask, and may set "always allow" only
  where `can_always_allow` (every non-destructive kind, so sending and
  invitations can be relaxed per client / plug-in, deleting never).
  `check_policy` refuses `allow` for destructive kinds with
  `action-policy-issue-destructive`; the store refuses it too (API check and
  a table `CHECK`), and `ActionPolicy::effective` never honours a stored one.
  An actor is `ActionActor` `{kind: mcp-client, client_id}` /
  `{kind: plugin, plugin_id}`.
- **Drafts run without asking by default** (design: only `delete_messages`
  and `send_message` are "confirm"; plan WP105: "creates a draft (no
  confirmation needed)"). `McpGrant::skip_draft_confirmation` (M1b) is
  superseded by the policies; it stays in the wire format and is ignored.
- **Confirmations** (`Confirmation`): attribution (`ActionAttribution`:
  actor, its name, and for a plug-in serving an MCP tool call the client it
  acts for), kind, operation (`send_message`, `nail.mail.send`), a summary
  sentence (`confirmation-summary-*`), the affected items (`ActionTarget`,
  at most 50 plus a count; labels are mailbox content shown as plain text),
  before / after lines (`ActionChange` with `action-field-*` labels), the
  accounts concerned, times, state (`pending`, `approved`, `denied`,
  `expired`, `cancelled`, `failed` + `error`), where the decision came from
  (`ConfirmationSource` `nail` / `elicitation` / `system`) and whether
  "always allow" may be offered. Requesters submit `NewConfirmation` (Rust
  only: plus the payload to run once approved, an idempotency key and a
  timeout, default 300 s, at most 1 h; `normalize` applies the limits). At
  most 20 pending per actor (`rate_limited` beyond).
- **Elicitation** (WP101): an MCP client's elicitation answer may deny
  anything but approve only kinds that `can_always_allow`
  (`ConfirmationSource::may_approve`): destructive actions are approved in
  Nail only. Nail's own confirmation is always raised.
- Requests: `ListConfirmations {filter: pending | decided | all, actor?,
  before?, limit}`, `DecideConfirmation {confirmation_id, decision:
  approve | deny, always_allow}` (`conflict` when no longer pending;
  `always_allow` sets the actor's policy), `ListActionPolicies {actor}`,
  `SetActionPolicy {actor, kind, mode | null}`. Events
  `ConfirmationRequested`, `ConfirmationResolved`, `ActionPoliciesChanged`.

### MCP (`nail_api::mcp`)

- **Tool table** `mcp::tools::MCP_TOOLS` (`McpToolSpec`: name, scopes,
  kind): the read tools of M1b / M3a, `test_rule` (read + `rules`, changes
  nothing) and the writes `update_messages` (organize), `delete_messages`
  (trash; `permanent: true` = permanent-delete + the `permanent-delete`
  scope), `create_draft` / `update_draft` (draft), `send_message` (send),
  `create_event` / `update_event` (calendar; with mail to attendees =
  invite + `send`), `respond_to_invite` (invite; `send: false` = calendar),
  `upsert_contact` / `upsert_task` (pim), `create_rule` (rule) and
  `edit_open_draft` (context + draft; the design's table names it, nothing
  implemented it yet). Argument structs (`UpdateMessagesArgs`, …,
  `EditOpenDraftArgs`) are the agent-facing JSON (snake_case, ids as
  strings); with nail-api's `json-schema` feature they derive
  `schemars::JsonSchema` (schemars 1.2 = rmcp's), so the published input
  schemas are exactly these types (nail-mcp enables the feature; smoke test
  `tests/smoke.rs`). Every write takes an optional `idempotency_key`
  (1–128 printable ASCII, remembered 24 h per client). Results:
  `McpWriteResult` `done {op_ids, created, confirmation_id?}` / `denied` /
  `expired` / `vetoed {reasons}`.
- **Prompts** `mcp::prompts::McpPrompt`: `summarize_thread {thread_id}`,
  `triage_folder {folder_id, limit?}`, `draft_reply {message_id,
  instructions?, reply_all?}`, `prepare_meeting {event_id, occurrence?}`
  (prompt arguments are strings); `read` scope; templates in
  `core-mcp.ftl` (`mcp-prompt-<name>-description` / `-instructions`,
  pre-filled), rendered in the core's locale; data stays marked untrusted.
- **Rate limits** `McpRateLimits {requests_per_minute, writes_per_minute,
  sends_per_hour}` (default 120 / 20 / 10, `null` = no limit, 1–10 000;
  issues `mcp-rate-limit-issue-zero` / `-too-high`) on `McpClient.rate_limits`;
  `UpdateMcpRateLimits {client_id, limits}` → `McpClientSaved` (served by
  `nail_mcp::admin`). Over a limit a call fails with `rate_limited` and is
  audited as denied (WP101).
- Composer edits: `edit_open_draft` and plug-in composer actions both emit
  `ComposerEditRequested {draft_id, client_id?, by, edit: ComposerEdit}`
  (`op` insert / replace-selection / replace-all, `text`, `format`).

### Plug-ins (`nail_api::plugins`)

- **Permissions** (now accepted): `mail.organize` (needs `mail.read`),
  `mail.send`, `calendar.write`, `contacts.write`, `tasks.write` (the write
  permissions include listing the collections they write to),
  `hooks.arrival` / `hooks.open` (need `mail.read`), `hooks.beforeSend`,
  `composer`, `attachments`, `background`, `mcp.expose`. Account-limited:
  the mail permissions, the mail hooks and `attachments`
  (`takes_accounts`). Still refused as not available:
  `mail.read.encrypted`, `contacts.read`, `tasks.read`, `ui.webview`.
  `requires()` drives the new manifest issue `missing-required-permission`;
  `is_write()` lets the install dialog highlight writes. Plug-in writes
  follow the plug-in's action policies like MCP writes.
- **Manifest contributions** (camelCase): `hooks` (`PluginHookDef {hook:
  arrival | beforeSend | open, filter? (search language; not for
  beforeSend), timeoutMs?}`, one per kind), `composerActions`,
  `markdownBlocks` (fence info `<plugin>.<block>`), `attachmentPreviews`
  (MIME types / extensions, `maxBytes` ≤ 10 MiB), `attachmentActions`,
  `linkActions` (host patterns as for `network:`), `jobs` (`PluginJobDef
  {id, title?, schedule?, events}`), `mcpTools` (`PluginMcpToolDef {id,
  description, inputSchema?, scopes = [read]}`, MCP name
  `<plugin>__<tool>` ≤ 64 characters). Each needs its permission
  (`contribution-needs-permission`); new issue kinds `invalid-hook`,
  `duplicate-hook`, `invalid-extension`, `duplicate-extension`,
  `invalid-job`, `duplicate-job`, `invalid-mcp-tool`, `duplicate-mcp-tool`.
- **Hook results** (`plugins::hooks`): arrival → `PluginArrivalResult
  {actions: addTag | removeTag | setFlagged | setRead | move | archive}`
  (≤ 20, need `mail.organize`, never a delete); beforeSend gets
  `PluginOutgoingMessage` and answers `PluginBeforeSendResult {veto?:
  {reason}, headers (X- only, not X-Nail-*, ≤ 10, one line), addContent
  (top / bottom, ≤ 64 KiB)}`; open → `PluginOpenResult {banners (id,
  level, text, ≤ 3 buttons), badges}` (≤ 3 each). `problems()` validates an
  answer; an invalid answer, a throw or a time-out skips the hook and counts
  it (`PluginHookStats`, `PluginHookOutcome`). Budgets: arrival 2 s,
  beforeSend 3 s, open 1 s by default, 50 ms – 10 s.
- **Vetoes**: `SendDraft` may answer `SendVetoed {draft_id, veto_id,
  vetoes: [{plugin_id, plugin_name, reason}]}` (nothing queued);
  `OverrideSendVeto {veto_id}` = "Send anyway" with the original options →
  `SendQueued` (veto ids valid 10 minutes, kept in memory by the core).
  `SendDraft` itself is unchanged. A vetoed MCP `send_message` answers
  `vetoed`; the user sends from Nail.
- **Open hooks**: `GetMessagePluginAnnotations {message_id, client_id?}` →
  `MessagePluginAnnotations {banners, badges}` (`PluginBanner` /
  `PluginBadge` carry the plug-in's name); banner buttons are
  `PluginUiAction` with the new target `{kind: banner, message_id, banner:
  "<plugin>.<id>"}`.
- **Extensions** (`plugins::extensions`): `RunPluginAction {action:
  "<plugin>.<name>", target: composer {draft_id, selection} | attachment
  {message_id, index} | link {url, text?, message_id?}, context,
  client_id?}` (handlers get `PluginActionContext`); `RenderAttachmentPreview
  {message_id, index, renderer, client_id?}` → `AttachmentPreview {preview:
  PluginPreview}` = `{kind: tree, tree}` (validated like panels) or `{kind:
  image, mimeType: png | jpeg | gif | webp, data (base64, ≤ 5 MiB), alt?}`
  — never HTML, never SVG; Markdown blocks get `PluginMarkdownBlockInput`
  and answer `{html, text}` (HTML sanitized like a message body, ≤ 256 KiB).
- **Background jobs** (`plugins::jobs`): `CronSchedule` — five fields
  (minute, hour, day, month, weekday 0–7) with `*`, numbers, ranges, steps
  and lists, plus `@hourly` / `@daily` / `@weekly`; firing more often than
  every 5 minutes is refused (`CronIssue`); parsed in nail-api itself (no
  crate), next runs computed by WP102 with chrono. Events
  `message.received`, `message.sent`, `selection.changed`,
  `event.reminder`, `app.started` (coalesced 2 s; message events carry ids
  readable with `mail.read`). Handlers get `PluginJobTrigger` (schedule /
  event / manual). Limits: 60 s wall clock per run (pending promises
  included), a cumulative CPU quota of 120 s per hour per plug-in across
  all its calls, no overlapping runs, missed runs once at start.
  `GetPluginActivity {plugin_id}` → `PluginActivity {jobs:
  PluginJobStatus, hooks: PluginHookStats, cpu_ms_last_hour,
  cpu_quota_ms_per_hour}`; `RunPluginJob` (manual); event
  `PluginActivityChanged`.
- **`nail.l10n`** (WP102 prelude, WP103 SDK): `nail.l10n.locale` (the
  core's language), `nail.l10n.t(id, args?)` and `nail.l10n.has(id)` over
  the plug-in's own catalogs (`contributes.locales`, per-key English
  fallback, then the id) — the same catalogs `%id%` manifest texts use.
- **Binary `nail.http.fetch` bodies** (WP102 / WP103): request `body` may be
  a string, `Uint8Array` or `ArrayBuffer`; responses gain `bytes():
  Promise<Uint8Array>` and `arrayBuffer()` next to `text()` / `json()`
  (`body` stays the text). The host already moves bytes
  (`HttpRequest.body`, `HttpResponse.body`); rquickjs gains its
  `array-buffer` feature (no new dependency; smoke test in nail-plugins).
  Attachment bytes reach preview / action handlers the same way.

### Remote access

`required_access`: confirmations, policies and rate limits are `admin`
(the core also needs a full grant); plug-in activity and manual job runs
`admin`; annotations and plug-in actions `read`, previews `attachments`
(plug-ins need full grants, as in M2d); `OverrideSendVeto` `send`. Events:
confirmation / policy events only to admin sessions with full grants,
composer edits to full grants, plug-in activity to admins.

### Served now

The core (`nail-core/src/agents.rs`) serves the store-backed requests:
listing and deciding confirmations (decision recorded, events emitted;
waking the waiting requester is WP105's), policies, plug-in activity;
`UpdateMcpRateLimits` (nail-mcp admin). Before WP102 / WP105: annotations
are empty, `RunPluginJob` / `RenderAttachmentPreview` / `RunPluginAction`
answer `unsupported`, `OverrideSendVeto` `not_found`, `SendDraft` never
vetoes, banner actions answer `unsupported` in the host. Tests:
`nail-core/tests/m4a.rs`, `tests/remote.rs` (agent requests), the UI mocks
(`features/agents/mock.ts`, `features/plugins/extensionsMock.ts`) answer
the same.

### Store schema v14 (`nail-store`)

`confirmations` (actor, attribution, kind, operation, `view` and `payload`
**sealed**, idempotency key unique per actor, state, times, decision),
`action_policies` (only changed defaults; `allow` refused for destructive
kinds by a `CHECK`), `mcp_clients.rate_limits` (JSON, NULL = defaults),
`mcp_idempotency` (results by key, **sealed**, a day), `plugin_jobs`,
`plugin_hook_stats`, `plugin_cpu` (per minute; all cascade with their
plug-in). Store APIs (tested): `insert_confirmation` (idempotent per
actor key, pending cap), `get` / `list_confirmations`,
`decide_confirmation` (once, before expiry, source checked),
`take_confirmation_payload`, `finish_confirmation`, `cancel_confirmation(s_of)`,
`expire_confirmations`, `next_confirmation_expiry`, history pruned to 500;
`action_policies` / `action_policy` / `set_action_policy` /
`remove_action_policies`; `set_mcp_rate_limits`, `mcp_idempotent_result`,
`store_mcp_idempotent_result`; `plugin_job_states`,
`set_plugin_job_next_run`, `record_plugin_job_run`, `retain_plugin_jobs`,
`record_plugin_hook_call`, `plugin_hook_stats`, `add_plugin_cpu`,
`plugin_cpu_last_hour`. Parallel packages add no migrations; WP105 appends
v15 if needed (and removes an actor's policies / cancels its
confirmations on revoke / uninstall).

### Crates and libraries

One new direct dependency: `schemars` 1.2.2 (MIT, already in the lock as
rmcp's) behind nail-api's optional `json-schema` feature. rquickjs gains
`array-buffer` (a feature flag, no crate). No cron crate (nail-api parses
its subset; chrono computes times). `cargo deny` / `cargo tree -i
openssl-sys` / `aws-lc-rs` unchanged.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| MCP write tools, confirmations (elicitation + Nail), idempotency, rate limits, audit before / after, prompts, stdio bridge | `crates/nail-mcp/`, `crates/nail-cli/`, `locales/{en,de}/core-mcp.ftl` (prompt ids pre-filled) | WP101 |
| Host writes, hooks, composer / attachment / link extensions, jobs, CPU quota, `nail.l10n`, binary fetch, panel value filtering, MCP exposure | `crates/nail-plugins/`, `locales/{en,de}/core-plugins.ftl` | WP102 |
| SDK, scaffolder templates, examples (full Follow-up, before-send checker, PDF / CSV preview), user docs | `packages/`, `examples/`, `userDocs/en/plugins/`, `userDocs/en/mcp.md` | WP103 |
| Confirmation center, veto dialog, banners / badges, composer buttons, preview panes, action menus, MCP client settings, plug-in manager additions | `ui/src/features/agents/` (new, registered, mock routed, state `data.features.agents`, messages in the coverage test), `ui/src/features/plugins/` (incl. `extensionsMock.ts`), `ui/src/features/mcp/`, `crates/nail-config/src/widgets/agents.rs` (future widgets `confirmation-center`, `confirmation-indicator`), `locales/{en,de}/{agents,plugins,mcp}.ftl` | WP104 |

Shared, integrator-owned (report, do not edit): `Cargo.toml` (ask before
adding dependencies), `Cargo.lock` beyond a package's own new dependency,
`nail-api`, `nail-store`, `nail-core`, `nail-server`, `nail-remote-auth`,
`nail-config` (except the widget file), `app/src-tauri`, `defaults/`,
`e2e/`, `ui/src/widgets/`, other features' directories,
`locales/*/core-agents.ftl` (contract ids pre-filled: kinds, states,
summaries, fields), `docs/adr` numbering. Contract or schema changes go to
WP105 as `apiRequests`.

## Consequences

- WP101–WP104 compile against fixed types and generated TypeScript (UI and
  `packages/plugin-api/src/generated`); the suites stay green in between.
- WP101 and WP102 reach the core through their own traits (`McpData`,
  `PluginServices`); WP105 implements those with the store APIs above, a
  confirmation broker (wake the waiting call on `DecideConfirmation`,
  expiry timer on `next_confirmation_expiry`, elicitation answers),
  hooks in the arrival / send / open paths with budgets and the veto map,
  the job scheduler on the triage clock, MCP exposure of plug-in tools,
  and the M4a Playwright scenarios.
- Exhaustive matches (core dispatch, `check_remote_request`,
  `remote_event`, `required_access`, the UI mock) fail to compile when a
  request or event is added.
- The JS mirrors of the permission list (`packages/plugin-api/tools/manifest.mjs`,
  `src/testing.ts`) still refuse the M4a permissions until WP103 updates
  them; the UI's `grants.ts` already knows the account-limited ones.
