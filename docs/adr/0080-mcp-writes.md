# ADR 0080: MCP write tools, confirmations, elicitation, idempotency, rate limits and prompts

Status: accepted · 2026-10-02 · M4a WP101

## Context

ADR 0079 fixed the M4a contract: the MCP tool table with the write
tools and their argument types, action kinds and policies,
confirmations, `McpRateLimits`, idempotency keys, `McpWriteResult` and the
four prompts. WP101 builds them in `crates/nail-mcp` (and the stdio
bridge in `crates/nail-cli`) against the `McpData` trait, without touching
the core, so WP105 only has to wire what is left.

## Decisions

### Writes go through `McpData::write` as Client API requests

`McpData` gains `write(by: &ActionAttribution, request: Request)`,
`query(request)` (the reads a plan needs: `NewDraft`, `GetEvent`,
`GetContact`, `GetTask`, `RuleFromText`, `TestRule`, …; default `pim`),
`confirmations()` and `render(&LocalizedMessage)`. A write tool
translates the agent's arguments into the **same requests the UI sends**
(`UpdateFlags`, `MoveMessages`, `SnoozeMessages`, `DeleteMessages`,
`UploadAttachment` + `SaveDraft`, `SendDraft`, `CreateEvent` /
`UpdateEvent` + `SendInvitations`, `RespondToInvitation`,
`CreateContact` / `UpdateContact`, `CreateTask` / `UpdateTask` /
`CompleteTask`, `SaveRule`), so every write runs through the core's
operation queue and is undoable like the user's own. `CoreAccess` gains
`handle_write` (default: `handle`), `confirmations` (default `None`) and
`render` (default English); `CoreData` maps them. **With these defaults the
write tools already work in the running app**; WP105 only overrides
`handle_write` to attribute operations to the client and `render` to use
the core's locale. `edit_open_draft` is not a request: it emits
`ComposerEditRequested` (+ `McpContextAccess {edit: true}`) and the
composer applies and saves the edit.

### The steps of a write call (`writes/mod.rs`)

1. **Idempotency.** With `idempotency_key` (checked with
   `is_idempotency_key`; a bad key is a protocol error) calls with the same
   key run one after the other (a lock per client and key); a stored
   result (`mcp_idempotency`, 24 h, sealed) is returned as it was —
   audited as a replay — and the same key for another tool is a
   `conflict`. Results are stored when the write ends (`done`, `denied`,
   `expired`, `vetoed`); errors are not, so a retry can succeed. The key
   is also the confirmation's key, so a retry while the first call still
   waits joins the same confirmation instead of asking twice.
2. **Rate limits** (`limits.rs`): sliding windows in memory per client,
   read fresh from `McpClient.rate_limits` on every call.
   `requests_per_minute` counts every `resources/read`,
   `resources/subscribe`, `tools/call` (including MRTR retries) and
   `prompts/get`; `writes_per_minute` every write call that is not an
   idempotent replay (also ones the grant refuses); `sends_per_hour` each
   `send_message`, mail to attendees and invitation answer that mails the
   organizer. A refused call counts nothing, fails with `rate_limited`
   (tool error with `limit`, `max`, `retry_after_secs`; a JSON-RPC error
   with `data.nail = "rate_limited"` for resources and prompts) and is
   audited as denied. The broker's pending cap (20 per actor) is a
   `rate_limited` too.
3. **Plan** (`writes/{mail,pim,rules}.rs`, `plan.rs`): scope checks per
   tool and per item kind (`ActionKind::mcp_scopes`; permanent delete
   `organize` + `permanent-delete`; mail to attendees `calendar` + `send`),
   the items' accounts and folders (`Access::require_place`, which does
   not ask for `read`: a write tool needs its own scope instead), argument
   validation, item resolution. A reply / forward / quoting draft needs
   the original to be **readable** (`read`, `encrypted` for encrypted
   mail); a forward of a message with attachments and attachments copied
   from a message need `attachments`. PIM writes reach the calendars,
   address books and task lists of sources the grant reaches (ADR 0058);
   read-only ones answer `forbidden`. Rules need a grant for all accounts;
   `create_rule` always creates (empty id) and never replaces a rule.
   The plan holds the confirmation's view (summary
   `confirmation-summary-*`, targets, before / after with `action-field-*`
   labels plus `mcp-tool-field-*` for fields the contract has no label
   for; values such as yes / no rendered in the core's locale), the
   content-free audit lines and the `Action` to run (also the
   confirmation's sealed payload).
4. **Policy.** Destructive kinds always ask (enforced here regardless of
   the broker's answer); otherwise `ConfirmationBroker::policy`.
5. **Confirmation.** `submit` stores the confirmation (the UIs get
   `ConfirmationRequested`); a **background task** waits for the final
   state (`wait`), runs the action on approval, records the outcome
   (`finish`: approved stays approved, a failure or a plug-in veto marks it
   `failed` with a localized reason), stores the idempotency result and
   writes the audit entry. The tool call only waits for that task, so an
   approval given after the client went away still runs (the agent asked,
   the user approved) and is never run twice.
6. **Elicitation** where the client declares form elicitation: in-band
   (`elicitation/create` raced against the decision, in the handler's own
   task as SEP-2260 requires) for sessions (≤ 2025-11-25), and as an MRTR
   `input_required` result for stateless 2026-07-28 clients (the request
   state is a random token kept server-side, bound to the client and the
   tool; the retry applies the answer and waits). Answers: *accept* with
   `approve` (default true) approves kinds `ConfirmationSource::Elicitation`
   may approve, `approve: false` and *decline* deny anything, *cancel* /
   errors leave the decision to Nail. Destructive kinds get a form without
   fields saying they are approved in Nail only; accepting it changes
   nothing. Nail's own confirmation is always raised; the first decision
   wins (the store decides each confirmation once).

### Confirmation broker

`broker::ConfirmationBroker` (dyn-compatible, boxed futures: `policy`,
`submit`, `wait`, `answer`, `finish`, `cancel`, `wake`). When the core
brings none, the server uses `StoreBroker`: store APIs of ADR 0079, events
through `McpData::emit`, waits that poll the store
(`WriteSettings::poll_interval`, default 500 ms) and wake at once on
`wake()` — `McpServer::on_event` calls it for every
`ConfirmationResolved`, which the core already forwards — and expire
overdue confirmations themselves (`expire_confirmations`, then
`ConfirmationResolved`). `WriteSettings::confirmation_timeout_secs`
(default 300) is the `NewConfirmation.timeout_secs`;
`McpServer::start_with` / `NailMcp::for_client_with` take the settings,
`start` / `for_client` keep their signatures.

### Audit with before / after, without mailbox content

One audit entry per write when it ends: `allowed` (done), `declined`
(denied, expired, vetoed), `error` (failed or invalid), `denied` (grant or
rate limit). The detail is `write ` + JSON: kind, target ids (≤ 50 +
count), before / after lines **without mailbox content** (flags, folder
ids, tag names, counts, times, "changed" markers), status, op ids,
created ids, confirmation id. `mcp_audit.detail` is not sealed, so
subjects, addresses and bodies stay in the confirmation (whose view is
sealed). `messages::audit_detail` maps the details to `mcp-tool-audit-*`
texts, so `ListMcpAudit` shows them localized.

### Results stay agent-facing English, content stays untrusted

Results are `McpWriteResult` as structured content (`done` / `vetoed` /
`denied` / `expired`, the last three as tool errors with an English
`hint`). Write results carry ids only; `test_rule` and prompt data use the
untrusted-data envelope.

### Prompts

`prompts/list` names the four prompts with descriptions from
`core-mcp.ftl`; `prompts/get` needs `read`, counts against the rate limit,
is audited (`McpCallKind::Prompt`) and answers the rendered instructions
(plus `mcp-prompt-draft-reply-target` / `-wishes` for `draft_reply`) and
the data as an embedded `nail://` resource in the untrusted envelope:
`summarize_thread` (the visible messages of the thread, newest 50, with
their text where readable), `triage_folder` (unread messages the grant
shows, default 25, at most 100, scanning at most 1000), `draft_reply` (the
message and its thread, the user's wishes), `prepare_meeting` (the event
or occurrence, attendees with their contact if any, related visible mail
found by the title). Prompt arguments arrive as strings; numbers and
booleans are accepted and turned into strings.

### Strings

New user-facing texts go to `locales/en/core-mcp.ftl` (the contract's
WP101 slot, already compiled into `nail-config`'s catalog and pre-filled
with the prompt templates): `mcp-tool-value-*`, `mcp-tool-field-*`,
`mcp-tool-elicit-*`, `mcp-tool-error-vetoed`, `mcp-tool-audit-*`,
`mcp-tool-bridge-confirmation-pending`, `mcp-prompt-draft-reply-target` /
`-wishes`. A separate `mcp-prompts.ftl` (as the task text suggested) would
need a new entry in `nail-config`'s embedded list (integrator-owned) and
split the prompt texts from their pre-filled templates, so it was not
created. `locales/de/core-mcp.ftl` stays empty.

### Stdio bridge

The bridge stays a transparent pump: in-band elicitation requests ride the
upstream SSE stream and their answers go back as POSTs; MRTR is plain
request / response. A test runs a confirmation's form through the bridge.
`nail mcp` running the core headless prints each `ConfirmationRequested`
to stderr (`mcp-tool-bridge-confirmation-pending`): without a window only
the client's form can approve (non-destructive kinds), the rest expires.

### Dependencies

No new crate: rmcp gains its `elicitation` feature (adds `url`, MIT /
Apache-2.0, already in the lock); nail-mcp now depends on `nail-config`
(rendering; was a dev-dependency) and `nail-ical` (`parse_rrule` for
`create_event`'s `recurrence`).

## Consequences

- WP105: override `CoreAccess::handle_write` (attribution in the
  operation queue / undo history), `render` (core locale) and optionally
  `confirmations` (a broker that also runs approved plug-in writes);
  cancel a client's pending confirmations and remove its policies on
  revoke (the waiting calls then answer `expired`); keep forwarding events
  to `McpServer::on_event` so decisions wake waiting calls at once.
- `McpGrant::skip_draft_confirmation` is ignored (drafts follow the
  `draft` policy).
- Tests: `tests/writes.rs` (scopes, drafts without asking, approve / deny
  / expire, permanent delete always asks, policies, idempotency, rate
  limits, elicitation in a session and via MRTR, vetoes, PIM and rule
  writes), `tests/prompts.rs`, `tests/bridge.rs` (elicitation through the
  bridge), `tests/core_data.rs`, unit tests per module; nail-cli
  `mcp_bridge.rs` / `mcp.rs`.
