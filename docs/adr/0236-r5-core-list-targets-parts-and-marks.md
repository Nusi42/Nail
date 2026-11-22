# 0236 — R5 WT1: list targets in the core, actions in parts, the `as_of` mark

Status: accepted (2026-11-22)

## Context

ADR 0235 (the R5 contract) added list targets ("this list, except these")
and left the core's part to WT1: run actions above
`MAIL_ACTION_PART_MESSAGES` (10 000) messages in parts recorded as one batch
with one undo, make the `as_of` mark in `ResolveMailTargets` and honour it in
`ApplyMailAction`, list large lists without one store hop per row, and keep
the R4 guarantees (one `OpsChanged` per batch, no event per message, the
queue summary back at zero).

## Decision

### Resolving a list

- `resolve_targets` resolves rows in bulk (`resolve_rows`): the
  conversation scope of each listed folder is read once, and the rows are
  resolved `ROWS_PER_STORE_CALL` (500) at a time in one store call each
  (`conversation_targets` per thread, `Store::target_messages` per message),
  so a list of 25 000 rows costs about 50 store calls and other requests
  get the store between them. A list's exceptions resolve the same way.
- `plan_moves` looks the target folder up once per account instead of once
  per message.
- The store keeps 64 prepared statements per connection (rusqlite's default
  is 16) and the statements of applying and queueing an operation are
  cached: applying 25 000 moves went from about 23 s to about 5 s in a debug
  build.

### The `as_of` mark

- The mark is the store's message mark: the highest `rowid` of `messages`
  (`Store::message_mark`), sent as `mark-<n>` (opaque to clients; anything
  else is refused as an invalid request). `TargetMessage::row` is the
  message's `rowid`; every message stored later has a greater one (a moved
  message keeps its row, the upsert updates in place).
- `ResolveMailTargets` with a list takes the mark first and counts only the
  messages at or below it, so the apply with that mark acts on exactly what
  the confirmation counted. It answers the list's own mark when the request
  carried one, else the new one.
- `ApplyMailAction` leaves out a list's messages above its mark; a list
  without a mark is the list as it is when the request runs. A message the
  sync stores anew after the mark (for example under a new id) counts as
  later and is left out too: the mark only ever narrows an action.
- Amended by ADR 0238: the mark is now a monotonic number of when each
  message entered its folder (schema v24, `listed_seq`), so a message moved
  into the list after the count is left out too, and the limit below is
  gone.
- Known limit (before ADR 0238): SQLite may hand the `rowid` of the newest message, when that
  message is deleted, to the next one stored. Mail that arrives just after
  such a deletion can then share the mark's number and be included. This
  needs the newest message deleted between the confirmation and the apply,
  and only that one arrival is affected; a monotonic column was not worth a
  schema change.

### Parts with one undo

- `ApplyMailAction` splits the resolved messages (sorted by account and
  folder) into parts of `MAIL_ACTION_PART_MESSAGES`. Each part is planned,
  applied locally, queued and recorded in one store transaction
  (`Store::apply_mail_batch_part`); later parts join the first part's batch
  (`into`), so the answer has one `batch_id` and one undo reverses every
  part. Each part sends one `MailChanged` and one `OpsChanged`
  (`announce_enqueued`), never one event per message.
- A part that cannot be undone (an operation without an inverse) drops the
  batch: the action as a whole is then not undoable, like a single
  transaction that had such an operation.
- A part joins only an open batch of the same actor (`Conflict` otherwise).
- A failure in the first part fails the action as before (nothing changed).
  A failure in a later part stops the action: the parts done stay applied,
  queued and undoable, the answer names exactly what was done
  (`message_ids`, `op_ids`, the batch), and a WARN line is logged. ADR 0238:
  the answer also counts what was left (`left_out`), and a message that
  vanished meanwhile is skipped instead of failing its part. The rows
  left over stay in the list, so the user sees them and can act again.
- `UndoMailAction` undoes in parts of the same size:
  `Store::undo_mail_batch_part` queues the newest inverses and marks the
  batch `undone` at once (it is never undone twice, also not by a second
  request during a long undo), and `Store::undo_mail_batch_rest` queues the
  older ones part by part. A failure in a later part stops the undo with a
  WARN line; the parts done stay queued.

### MCP

- The MCP write tools keep naming messages and conversations and do not
  accept list targets: every MCP write is confirmed by the user with the
  messages it touches listed, the agent's grant is checked per message, and
  the tools have their own limits. A list target would hand an agent an
  unbounded write that the confirmation cannot show. They use
  `ResolveMailTargets` without lists, so its `message_ids` stay complete for
  them.

## Consequences

- Measured on the development machine (release build; debug in brackets):
  the core test `a_list_of_25000_messages_moves_in_parts_with_one_undo`
  resolves a 25 000-message list in 0.5 s (0.6 s), applies the action in
  three parts in 4.8 s (5.4 s) and queues the undo in 4.7 s (5.2 s); one
  part holds the store for about 1.6 s. Against the mock IMAP server
  (`sync::a_list_of_25000_messages_is_archived_and_deleted_with_one_undo_each`)
  Archive applies in 9.6 s and the server has every move after 31 s, Delete
  applies in 7.3 s (on the server after 29 s), each undo is queued in about
  7 s. The time goes into SQLite (applying, queueing and recording each
  operation), not into Nail's code: release and debug differ little.
- The 25 000-message IMAP test runs about four minutes in a debug build (the
  first sync of 25 000 messages and four replays of 25 000 moves).
