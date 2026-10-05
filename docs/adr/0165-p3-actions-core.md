# ADR 0165: P3 WP-actions-core — conversation actions in store, core and engines

Status: accepted · 2026-10-05 · P3 package WP-actions-core (contract ADR 0164; the integrator renumbers)

## Context

ADR 0164 fixed the semantics of Gmail-like conversation actions
(`ApplyMailAction`, `ResolveMailTargets`, `UndoMailAction`, `MailAction::reach`),
the undo tables of schema v17 and the slots of this package. The contract's
core stub resolved targets from the conversation view (one copy per
Message-ID), forwarded them to the per-message writes and kept no batch.
This package replaces the stub: every stored copy, one batch with one undo,
the actor's own undo, grouped replay in every engine, and conversation
targets for MCP clients and plug-ins.

## Decisions

### Resolution (`nail_store::thread::targets`)

- `Store::conversation_targets(thread, folders, own, keep, reach)` reads
  every stored copy of the thread (`thread_id` index) with its folder role
  and keeps, per reach: **listed** = copies in the listed folders (without a
  folder: everything but Drafts, Trash, Junk and Sent), **whole** = listed
  plus the joined Sent folders (never drafts; without a folder: everything
  but Drafts, Trash, Junk), **newest** = the row's `latest_message_id` and
  its copies among the listed messages. Copies are **not** collapsed: an
  action reaches every stored copy (duplicates with the same Message-ID).
- `keep` (a remote session's grant) drops copies before anything else, and
  the core narrows the listed folders with the same filter, so a session
  acts on exactly what it sees. A `single-<id>` row is that message.
- The core sorts the resolved messages by (account, folder) — stable — so
  the queue hands each folder's operations to the engine together. Without
  it a conversation alternating Inbox and Sent became one server call per
  message.
- Message targets stay the one message named (a card's own action).

### One batch (`nail_store::mail_batches`)

- `Store::apply_mail_batch(plan, operations)` plans each operation against
  the message's state **in the same transaction** that applies and queues
  it, and records it with its inverse in `mail_batches` / `mail_batch_ops`
  (schema v17; **no new migration** — v19 stays unused):
  - flag / tag change → the previous value of every flag it changes, added
    tags removed, removed tags added again; an operation that **changes
    nothing locally is not queued** (marking read mail read sends nothing);
  - move, delete to Trash → a move back. The inverse's `source.folder_id`
    names where the batch put the message (the field the queue fills itself
    on enqueue); undo skips the inverse when the message is no longer there
    or is gone, and clears `source` before enqueueing;
  - permanent delete (also Trash on a message already in Trash) → none; a
    batch is recorded only when every queued operation has an inverse, so
    such an action answers `batch_id: null`.
- Batches are purged before and after each write: older than
  `MAIL_UNDO_SECS`, beyond the newest `MAIL_UNDO_KEEP`.
- `Store::undo_mail_batch(id, actor, now)` answers `Undone` (inverses
  queued newest first, savepoint per inverse like a rule run's undo),
  `Expired` (unknown, purged, too old, **or another actor's** — never told
  apart) or `UndoneAlready`. The core maps them to `not_found` +
  `mail-action-issue-undo-expired` and `conflict` +
  `mail-action-issue-undone-already`.
- Batch ids are `batch-<n>` (`mail_batch_id` / `parse_mail_batch_id`).

### The core (`nail-core/src/mail_actions.rs`)

- Plans per action: Archive = moves to each account's Archive (messages
  already there are left out), Move as `MoveMessages` (`plan_moves`,
  shared), Trash / Delete permanently as deletes, read / flag / tags as
  flag changes, Snooze through the snooze scheduler (`plan_snooze`: records
  stored, `$Snoozed` + move to the hidden folder), Junk through the
  classifier (`plan_mark_junk`: training + verdict, `$Junk` + move).
- Undo side effects by the batch's kind: Snooze forgets the snoozes
  (`forget_snoozes`, the wake timer is gone), Junk / Not junk untrains the
  messages (`untrain_junk`). A failed batch after a snooze plan forgets the
  records again.
- Actors: `user` (`Service::handle`), `remote:<session>` (every remote
  session, full grant or not, so undo is per session), `mcp:<client>` and
  `plugin:<id>` (`McpCore::handle_write` routes `ApplyMailAction` /
  `UndoMailAction` with the attribution's actor). Rules and Sieve are
  untouched (they keep their own run undo).

### Engines: grouped replay

- The queue stays per message. `Operation::group_key` names what one server
  call can carry: the same flag change, or moves / deletes from the same
  folder to the same place (sources recorded). `Replayer` gains
  `max_group` (default 1) and `replay_group` (default: one by one, stop at
  the first retry); `Store::replay_account` claims the oldest pending
  operation plus directly following ones of the same account and key (each
  message once) and finishes each with its own outcome; an answer shorter
  than the group leaves the rest pending **without** counting the attempt.
  A retry still ends the pass after the group.
- `OpExecutor` gains `max_group` / `execute_group` (replay.rs); the
  `EventReplayer` reports every op's status.
- **IMAP** (500 per group): flag changes → one `UID STORE` per folder;
  moves and Trash → one `UID MOVE` (COPY + `\Deleted` + `UID EXPUNGE`
  without MOVE; the client chunks long sets) with `COPYUID` per message;
  permanent deletes → one expunge of the set; moves into another account
  stay one by one. `nail-proto-imap` needed no change.
- **Gmail** (`GmailEngine::write_many`, 1000 per group): flag / tag changes
  with the same label change and moves between two folders (not into or out
  of Trash) → one `messages.batchModify` per label change; trash, delete,
  append one by one. A permanently failing `batchModify` falls back to one
  call per write (each finds its own outcome); a retryable one answers its
  error for the group. `threads.modify` is **not** used: proving that a
  batch covers a whole Gmail thread costs a `threads.get` per thread, while
  `batchModify` with the same ids is one call either way.
- **Graph** (`GraphEngine::write_many`): flag changes without tags, moves,
  moves to Deleted Items and permanent deletes as `$batch` parts (20 per
  call; 404 → gone); tag changes (they read the categories) and appends one
  by one.
- **EWS** (`EwsEngine::write_many`, `EwsClient::{update_items, move_items,
  delete_items}`, 100 ids per request): one `UpdateItem` (AlwaysOverwrite,
  absolute fields) per distinct flag change, one `MoveItem` per target
  folder, one `DeleteItem` for permanent deletes; per-item errors stay per
  item (`ErrorItemNotFound` → gone).
- The `ExchangeEngine` trait (nail-api) has no multi-write call, so the
  core keeps the concrete engine beside the trait object: `RunningEngine.batch`
  (`exchange/batch.rs`, `BatchWrites` for `GraphEngine` / `EwsEngine`,
  built by `setup::engines_for`). Injected engines have none and replay one
  write at a time. Writes are grouped per mailbox.

### MCP and plug-ins

- `update_messages` / `delete_messages` with `conversations`: each component
  of the call (read, flag, tags, move, archive, snooze; Trash or permanent
  delete) is one `MailAction`, resolved by the core
  (`McpData::query(ResolveMailTargets)` with the named messages and the
  conversations as targets), narrowed to the messages the grant covers
  (left out, not refused — as for remote sessions), and run as one
  `ApplyMailAction` with exactly those message targets (one batch per
  component, the agent's to undo). A conversation listed in an excluded
  folder is refused (`denied`); at most 500 conversations. Calls with
  message ids only keep the per-message requests.
- Confirmations of conversation calls use `mail-action-confirmation-organize`
  / `-trash` / `-permanent-delete` (`core-mail-actions.ftl`; `$actor`,
  `$conversations`, `$count`). Kinds as before: moves into Trash or Junk and
  deletes always ask.
- Plug-ins: `nail.mail.update(targets, changes)` / `nail.mail.delete(targets,
  options)` take a message id, a list, or `{ messageIds?, conversations:
  [{ threadId, folderId? }] }` (prelude `mailTargets`, Rust
  `writes::targets`); they reach the same MCP plans through `ActorWrites`.
  The SDK types (`packages/plugin-api`) are left to the integrator (outside
  this package's folders).

## Consequences

- One undo restores a conversation exactly: each message back to its
  folder, each flag and tag as it was; messages moved since stay where the
  user put them.
- A conversation action costs one server call per folder (IMAP), per label
  change (Gmail), per 20 writes (Graph) or per 100 items (EWS).
- Messages already in the requested state cost nothing, also on undo.
- The queue's group claim adds one read of up to `max_group` pending rows
  per pass step; single-op replayers are unchanged.
