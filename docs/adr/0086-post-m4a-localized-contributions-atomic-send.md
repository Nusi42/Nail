# ADR 0086: Post-M4a — localized contribution texts, atomic compare-and-send for confirmed sends

Status: accepted · 2026-10-02 · post-M4a (verify finding, ADR 0085 follow-up)

## Context

- The independent verify of M4a failed on one bug: `%id%` texts of the
  M4a contributions reached the user unresolved. `PluginCatalogs::
  localize_manifest` covered only the name, description, commands, panels
  and settings, so the CSV preview example showed `%preview-title%` in
  the reader; job titles in the plug-in manager's Activity section came
  from the raw manifest (`GetPluginActivity`), and MCP clients got raw
  `%id%` tool descriptions (`PluginHost::mcp_tools`).
- ADR 0085 made a confirmed `send_message` compare the draft with the one
  the confirmation showed, but the comparison and the core's `SendDraft`
  were two steps: an edit landing between them would be sent although
  the user approved another draft.

## Decisions

### Every manifest text is localized

- `localize_manifest` resolves every user-visible text of the M4a
  contributions too: composer action titles and tooltips, Markdown block
  titles and templates, attachment preview titles, attachment and link
  action titles, job titles and MCP tool descriptions (cut to
  `MAX_MCP_TOOL_DESCRIPTION` after resolving,
  `PluginCatalogs::mcp_description`). Hook definitions, placements and
  keys carry no text.
- The host keeps the manifest localized in the core's language per start
  (`Slot::localized`, `Running::localized`); `mcp_tools()` lists its
  descriptions. `GetPluginActivity` resolves job titles with the
  plug-in's catalogs (`Service::plugin_catalogs`).
- Docs: the manifest and localization pages and the generated API
  reference say `template` and MCP tool descriptions may be `%id%` too.

### A confirmed send queues exactly the approved draft

- A draft's revision is a hash of its content, not a stored counter:
  `nail_store::draft_revision` = the first 128 bits of SHA-256 over the
  draft's JSON without its id (what the store keeps). No schema change;
  any edit (recipients, subject, body, attachments, crypto choice) gives
  another revision, and the same content always the same one.
- `Request::SendDraft` gained `expected_revision` (optional; the UI never
  sets it). `send_message` plans `Action::SendDraft { revision }` from
  the draft the confirmation shows and sends with
  `expected_revision: Some(revision)`; the early comparison before the
  request stays (same `mcp-tool-error-draft-changed`).
- The core checks the revision in the store transaction of every write on
  the send path and carries the new revision forward: before-send hooks
  run only on the approved draft; their added content is written with
  `Store::save_draft_at` (check + write in one transaction, returns the
  new revision); the crypto choice `SendDraft` writes into the draft the
  same; the outbox entry is created with `Store::schedule_send_at` (check +
  insert in one transaction). A queued draft cannot be edited
  (`save_draft` refuses it), so what the sender builds is that revision.
  "Send anyway" after a veto keeps the pinned revision.
- A mismatch is `nail_store::Error::DraftChanged` → `conflict` with
  `localized = mcp-tool-error-draft-changed` (the core reuses the MCP
  text: only confirmations pin revisions); nothing is queued, the
  confirmation ends failed.

## Consequences

- An edit of the draft by the user (or the agent through `update_draft`)
  at any moment after the confirmation was shown stops the send; the user
  sends it from Nail. Content added by the plug-ins' own before-send hooks
  does not.
- Tests: nail-plugins `l10n` (one per contribution kind, the examples'
  CSV preview title and follow-up job title), `tests/extensions.rs` (MCP
  tool descriptions in the core's language); nail-store (revisions,
  checked save / schedule refuse a changed draft and queue nothing);
  nail-core `error.rs` (mapping), `m4a_wired.rs` (localized job title and
  tool description through the core; a stale revision is refused with and
  without before-send hooks, the current one is sent with the hook's
  content; a plug-in's send approved after the user edited the draft
  fails with `draft-changed` and sends nothing); nail-mcp `tests/actor.rs`
  (the request carries the approved revision); Playwright
  `extensions.spec.ts` (the CSV preview's localized title, a `%id%` job
  title in the plug-in manager).
