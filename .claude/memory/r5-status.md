---
name: r5-status
description: "Wave R5 (select all and large mail actions; 2026-11-22): what was integrated (ADRs 0235-0237), timings and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADRs 0235 contract, 0236 core, 0237 UI), release notes "Nail 0.8.10":**

- Contract: `MessageTarget::List` (`ListTarget { list: MailList, except, as_of }`), `MAX_MAIL_ACTION_TARGETS` =
  4 096 rows named one by one, `MAIL_ACTION_PART_MESSAGES` = 10 000, `MailTargetResolution::{messages, as_of}`.
- Core: list rows resolve in bulk (`ROWS_PER_STORE_CALL` = 500, `Store::target_messages`); the `as_of` mark is
  `mark-<n>` (`Store::message_mark`, the highest message rowid; any other format = invalid request); actions above
  10 000 messages run in parts (`apply_mail_batch_part`, one batch, one undo; undo also in parts); one `MailChanged`
  and one `OpsChanged` per part; `STATEMENT_CACHE` = 64. MCP takes no list targets (every MCP write is confirmed
  per message).
- UI: `list.select-all` (`ctrl+a` / `meta+a` in the list, palette, row menu), `list.clear-selection` (`escape`);
  selection `all: { list, except }`; "All N selected" (`message-list.selection-summary`, sticky); every whole-list
  action asks once (`mail-bulk-confirm`) unless the Move window's Drafts / Trash / Junk question or "Delete
  permanently?" already asked (both count `resolution.messages`, a list resolution has no ids). `applyInChunks` is
  gone: one request per action.

**Timings (25 000 messages; release, debug in brackets):** resolve 0.5 s (0.6 s), apply in three parts 4.8 s
(5.4 s), undo queued 4.7 s (5.2 s); each part holds the store about 1.6 s. IMAP mock: Archive applies in 9.6 s, the
server has every move after about 31 s; Delete 7.3 s / about 29 s; each undo queued in about 7 s.

**Pitfalls:**

- `nail-core` `sync::a_list_of_25000_messages_is_archived_and_deleted_with_one_undo_each` runs about 4 minutes in
  debug; the whole `sync.rs` binary about 260 s.
- `tests/exchange.rs` (`categories_automatic_replies_and_rooms`, `shared_mailboxes_sending_and_free_busy`, the two
  interrupted-listing tests) failed once each, only when the machine was about 5x slower (a parallel release
  build): rerun alone before suspecting code.
- A new modal confirmation must also be added to `CONFIRMATIONS` in `ui/src/features/composer/instances.test.tsx`.
- The move window needs `selectedMessageId` (it says "Select a message first" otherwise), also for a list target.
- Review fixes (ADR 0238): the mark is `messages.listed_seq` (schema v24, triggers on insert and folder change), not
  the rowid; a chip list's conversation rows carry `listed_folder_id` (one rule `MailList::row_target` =
  `ui/src/actions/rowTargets.ts`, ts-vectors); vanished list rows / exceptions are skipped, `left_out` reports a
  failed later part; the select-all live region is `message-list.announcer` beside the list box (a live region
  inside `role=listbox` is an axe violation); `ctrl+space` = `list.toggle-row`.
- A new table needs its name in `schema::tests::migrates_empty_database_to_latest`.

**Follow-ups (plan status line):** tag states for a whole list; rules "run on selected" with a list target; smaller
store transactions inside a part; profile the replay of 25 000 IMAP moves; a 1 200-row e2e gate variant; the
maintainer's UX review of how dialogs confirm and close.

Related: [[r4-status]]
