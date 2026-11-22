---
name: r4-status
description: "Wave R4 (bulk actions: queue summary, resync, Move window, drafts safety; 2026-11-21): what was integrated (ADRs 0230-0232) and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADRs 0230 contract, 0231 core, 0232 UI + drafts safety), release notes "Nail 0.8.9":**

- Core: `OpsSummary` / `GetOpsSummary` / `OpsChanged` / `Resync`; message writes send no `OpStatus` any more
  (folder changes keep it). Failure class and localized text stored with the write (schema v22), so the status bar
  names the real reason after a restart. `Store::write_queue_counts` (was `op_queue_counts`) also counts pending
  folder, Gmail label and Exchange writes; their failures are not in `failed` (bulk Retry / Dismiss act on message
  writes only). One INFO line per deferred replay pass. `ingest::resolve_id` skips a server message while a move-out
  of the same Message-ID waits or failed (`held_move_outs`, once per batch) and lists its UID in `sync_refetch`
  (schema v23; fetched by `retry_skipped` once nothing holds it back): no duplicates after a UIDVALIDITY reset, no
  lost copy. Dismiss puts failed moves / deletes back (`revert_move_out`, ADR 0233).
- `EditDraftMessage` on a message without `\Draft` copies it into a new draft without a server copy: the original
  is never replaced or deleted (`draft_edit.rs`, test `m1b::a_non_draft_in_drafts_is_copied_and_never_replaced`).
  Review (ADR 0233): a draft's server copy is removed only if it is Nail's own or flagged `\Draft`
  (`draft_copy_removal_in`); schema v23 unlinks 0.8.8 links to received mail (maintainer lost drafts that way);
  `m1b::an_old_link_to_a_received_message_never_deletes_it`.
- UI: `ui/src/state/opsSummary.ts` tracks the summary (start, reconnect, `OpsChanged`, `Resync`) and re-asks after
  `OPS_RECHECK_MS` (2.5 s) while changes wait; `pendingOps` only holds ids queued since the last summary. `Resync`
  refetches lists, conversations, searches and the open message and prunes a stale selection. Move window =
  combobox / listbox (`matchRank`, Enter only on the highlighted or exactly named folder, current folder left out,
  confirmation for Drafts / Trash / Junk via `ASKS_BEFORE_MOVE`). Only `\Draft` messages continue in the composer;
  `mail.move-out-of-drafts` in the reader toolbar, reader / message menus and the palette.
- SSOT: `opsWaiting` (TS) is pinned to `OpsSummary::waiting` by the `ops` ts-vectors
  (`crates/nail-api/fixtures/ts-vectors/ops.json`). The mock (`mockOps.ts`) sends no `OpStatus` for message writes;
  `mockDrafts.ts` links only `\Draft` messages.

**Pitfalls:**

- After rebasing branches that add `.ftl` or theme files, delete `node_modules/.vitest-cache` (stale catalog glob).
- Changing `defaults/layout.kdl` / `theme.kdl` needs `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test
  integration defaults::` and `npx prettier --write crates/nail-api/fixtures`.
- A WS package's commit-msg hook refusal does not stop a chained `git push` in the same command: chain with `&&`.
- e2e `m5d` "customize layout … Undo and Redo" failed twice under load in WS2's runs (passes alone): watch it.

**Follow-ups (plan status line):** failures of folder / Gmail label / Exchange writes in the summary with bulk
Retry / Dismiss; coalesce the per-operation WARN of a mass permanent failure; a separate text for Retry / Dismiss
errors.

Related: [[r3-status]], [[next-steps]]
