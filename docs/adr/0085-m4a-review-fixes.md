# ADR 0085: M4a review fixes — approve what runs, plug-in tools within the client's grant, deletes by move, audit of plug-in writes

Status: accepted · 2026-10-02 · M4a review fixes

## Context

The review of M4a (ADR 0084) found ways for an agent or a plug-in to get
more than the user approved or granted:

- `send_message` showed the draft as it was at the request but sent
  whatever the draft held at approval; `update_draft` runs without asking
  by default, so a prompt-injected agent could swap recipients and body
  after the user saw a harmless confirmation.
- Plug-in MCP tools ran with the plug-in's full permissions and policies:
  a client limited to one account or with excluded folders could read
  and change other mail through the tool, its own confirmation policy and
  rate limits did not apply, and only the tool call was audited.
- `update_messages` with a move into Trash was "organize" (no
  confirmation); arrival hooks could trash every incoming message.
- An idempotency key reused for another request attached it to the
  earlier confirmation (approve A, run B); confirmations of an earlier run
  stayed approvable with nothing to run them.
- Before-send hooks got the plaintext of messages that will be
  end-to-end encrypted.
- The only record of plug-in writes was the plug-in's own log, which the
  plug-in can fill until old lines are dropped.
- The user docs described link actions that the reader does not offer.
- (Low) Pending confirmations survived disabling a plug-in or narrowing a
  grant; the examples' build docs wrote build output into the repository.

## Decisions

### What the user approves is what runs

- `send_message` plans `Action::SendDraft { draft_id, send_at, expected }`
  with the draft as the confirmation shows it. Running it reads the draft
  again and refuses (`conflict`, `mcp-tool-error-draft-changed`; the
  confirmation ends failed) when it differs. A whole-draft comparison
  rather than a hash: `Draft` is `PartialEq`, carries no timestamps, and
  the payload is sealed at rest anyway. The check and the core's
  `SendDraft` are two steps (a window of one request); making the core's
  send compare-and-send atomically is a follow-up.
- `Store::insert_confirmation` refuses (`invalid_data` →
  `invalid_request`) an existing idempotency key whose operation, kind or
  stored payload differ from the new request. Decided confirmations have
  no payload any more; they never run again, so operation and kind
  suffice. A keyed write whose key already has a confirmation goes
  through the confirmation path even when its policy would not ask, so
  the check also covers "another tool, no confirmation needed".
- A pending confirmation found under a key with no task of this server
  waiting for it is cancelled and answered `expired`.
- `Service::start` cancels every pending confirmation of an earlier run
  (`Store::cancel_pending_confirmations`); changing an MCP client's grant
  (`UpdateMcpGrants`), a plug-in's grants or disabling it cancels that
  actor's pending confirmations (`Service::cancel_confirmations_of`;
  policies stay). Uninstall / revoke still also remove policies.

### Plug-in MCP tools run within the overlap of both grants

- `nail_plugins::ClientLimit` (scopes, accounts, excluded folders of the
  client's grant, read fresh from the store by the core for every call)
  rides on the turn's `Origin` and on every pending call and timer of the
  turn. `Gate::limited` keeps only permissions the client's scopes cover
  (`mail.read` / `mail.read.body` / `calendar.read` ← `read`,
  `attachments` ← `attachments`, `mail.organize` ← `organize`,
  `mail.send` ← `draft` or `send`, `calendar.write` ← `calendar`,
  `contacts.write` / `tasks.write` ← `pim`, `composer` ← `context`; hooks
  never) and intersects every account scope with the client's accounts.
  The plug-in's own `storage`, `network:`, `notifications`, `ui.dialogs`
  stay (they reach no mailbox data the client could not read). Messages
  in excluded folders are `not_found` for `nail.mail.get` / `getBody` /
  `search` / `thread` and for the messages a write names.
- `ActorWrites` with `on_behalf_of` loads the client: its resolved grant
  is intersected with the plug-in's (`Access::intersect`: scopes and
  accounts of both, excluded folders of either; a revoked client reaches
  nothing), the write asks when the plug-in's **or** the client's policy
  asks, and it counts against the client's `writes_per_minute` /
  `sends_per_hour`. The limiter is one per core
  (`McpData::rate_limiter`, `AgentsCtl::limiter`), shared by the MCP
  server and plug-in writes.

### Moves that delete are deletes

`update_messages` with `move_to_folder_id` into a Trash- or Junk-role
folder plans `ActionKind::Trash` (summary `confirmation-summary-trash`),
so it always asks and cannot be always-allowed. `ActorWriteRequest` gained
`allow_destructive`; the core sets it `false` for `hooks.arrival`, whose
actions are never a delete: such an answer is refused (`forbidden`) before
anything is asked or run.

### Before-send hooks never see encrypted content

`PluginHost::before_send_hooks` hands hooks `before_send_view(message)`:
for a message that will be end-to-end encrypted, `text` is empty and
`html` absent (recipients, subject and attachment names stay; content the
hooks add is encrypted with the message as before).

### Plug-in writes are in the MCP audit log

Every `ActorWrites::run` appends one `mcp_audit` entry: name
`plugin:<id> <operation>`, `client_name` = the plug-in's name, `client_id`
= the MCP client it served (so the entry shows in that client's log),
outcome and the same content-free `write …` detail as the tools' entries
(refusals with their reason). The table is append-only for plug-ins and
purged only by the core's retention. The `plugin-write-log` line in the
plug-in's own log stays for the developer.

### Docs

- Link actions: the SDK, manifest and user docs say the reader does not
  offer them yet (`LinkActionMenu` stays deferred, ADR 0084).
- `examples/README.md` and `docs/dev-setup.md` build a copy of an example
  outside the repository; a test keeps the docs from telling anyone to
  build in place.

## Consequences

- A user's own edit of a draft between an agent's send request and the
  approval also stops the send; the user sends it from Nail.
- Agents get `invalid_request` for a reused key with other arguments, and
  `expired` for a key whose request was waiting when Nail stopped.
- A plug-in tool that needs mail of other accounts does not work for a
  client limited to some; the client's grant has to cover it.
- Tests: store (key mismatch, start cancel, key lookup), nail-mcp
  (`Access::intersect`, Trash/Junk roles; `tests/actor.rs` changed draft,
  Trash move + arrival refusal, plug-in for a client: accounts, scopes,
  policy, rate limit, audit, revoked; `tests/writes.rs` key reuse,
  orphaned confirmation, Trash move via MCP), nail-plugins (gate limits;
  a tool reading other accounts / excluded folders and writing without
  the scope; before-send redaction), core (`m4a.rs` start and grant /
  disable cancels; `m4a_wired.rs` tool narrowed by a changed grant, plug-in
  writes in the MCP audit log), create-nail-plugin (build docs).

## Follow-ups

- Compare-and-send inside the core's `SendDraft` to close the
  one-request window — done post-M4a (ADR 0086: content-hash revisions
  checked in the store transaction that queues the draft).
- An "all entries" view of the MCP activity log in the UI (plug-in writes
  of no client are listed only without a client filter).
