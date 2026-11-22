# 0238 — R5 review: list rows, the membership mark, vanished messages, select-all keys

Status: accepted (2026-11-22)

## Context

The review of wave R5 (ADRs 0235–0237) found:

- A filter chip without a typed search (Inbox plus "Unread") is a search
  list. The core turned its conversation rows into conversations **without**
  a folder, while the UI's rows (`rowTarget`, the exceptions, the loaded
  rows) carried the Inbox: "select all" then reached the conversations'
  messages in the user's other folders, which the same rows picked one by
  one never do. The row rule existed twice (Rust `hit_row`, TS `rowTarget`)
  without a vector pinning them.
- The `as_of` mark was the highest `rowid`. A message moved into the listed
  folder after the count (a snooze waking up, an undo, a rule, another
  session) keeps its row, so the apply acted on mail the question never
  counted.
- A message removed while a whole list was read or applied (a sync expunge,
  a delete on another device) failed the whole action with `not_found`, also
  through a stale exception; reading a list page by page over separate store
  calls could skip or repeat rows; a later part that failed dropped the rest
  of the action with only a log line.
- The "All N selected" live region sat inside the list box (axe
  `aria-required-children`) and was mounted with its text (often not
  announced); Shift + arrow keys silently ended "select all"; no key left a
  row out; "The only row is selected" was wrong once rows were left out; the
  two new actions' icons were defined twice.

## Decision

### One rule for a list's rows

- `MailList::Search` gains `listed_folder_id` (`serde(default)`): the folder
  its conversation rows are listed in — the folder for a filter chip
  without a typed search, `null` for a typed search. `MailList::row_folder_id`
  / `conversation_rows` / `row_target` are the rule; the core lists every
  kind of list through `row_target`.
- The UI derives the same from the list: `ui/src/actions/rowTargets.ts`
  (`rowTarget`, `rowFolderOf`, `listRowTarget`; no imports but the contract's
  types, so the mock core uses it too). The message list sets its scope's
  folder from `rowFolderOf(list)` and `mailListOf` writes
  `listed_folder_id` (`ListView.typedSearch`). The `mail-actions`
  ts-vectors pin `rowFolderOf` and `listRowTarget` against Rust.

### The mark is when a message entered its folder

- Schema v24 (`thread/targets.rs` `V24`): `messages.listed_seq` and the
  one-row counter `message_listed_seq`; triggers give a message the next
  number when it is stored and whenever its `folder_id` changes, so every
  path (sync, queue, snooze, undo, rules, other sessions) is covered.
  `Store::message_mark` is the counter, `TargetMessage::listed_seq` the
  message's number. A message that entered a folder after the mark — new or
  moved in — is left out of the apply. The counter is monotonic, which also
  closes ADR 0236's known limit (a reused `rowid`). Rows stored before v24
  keep 0.
- Not covered on purpose: a message that enters a *search* list by a flag
  change (marked unread elsewhere while the question is open) is acted on;
  bumping the number on flag changes would instead drop counted messages
  the user only read meanwhile.

### Vanished messages and partial results

- A whole list is read in one store read (`Service::mail_list_rows`:
  `ListThreads` / `ListMessages` with no page limit, a search through
  `Service::local_hits` = `nail_search::matching_ids` plus
  `Store::message_threads`), so rows removed or added meanwhile never shift a
  page.
- A list's message rows and its exceptions resolve through
  `Store::stored_target_messages`: a message no longer stored reaches
  nothing. Messages named one by one still fail with `not_found`.
- Each part re-checks which of its messages are still stored, plans them,
  and plans once more when one vanishes while planning;
  `Store::apply_mail_batch_part` leaves a message gone inside the
  transaction out (`MailBatchApplied::gone`) instead of failing the part.
- `MailActionResult::left_out` (new, `serde(default)`) counts the messages
  a large action did not reach because a later part failed; the undo toast
  then says "N messages changed; M messages could not be changed"
  (`mail-actions-done-partial`).
- The UI does not prune exceptions by the loaded rows: a left-out row that
  only scrolled out of the loaded pages would be selected again; a stale
  exception is harmless now that the core skips it.

### Select all, keyboard and screen readers

- The visible "All N selected" line stays at the top of the list without a
  live region. A separate polite live region (`message-list.announcer`,
  visually hidden) sits beside the list box, is mounted empty and changes
  its text: "All N selected", and "Selection cleared" when it ends.
  It has no `role="status"`: other screens keep their own status line
  as the page's only status role.
- Shift + ↑ / ↓ under "select all" works like Shift + click (rows left out
  or taken back, the list stays selected). `list.toggle-row` (default
  `ctrl+space` in the list) is the keyboard's Ctrl + click: it leaves the
  selected row out of "select all" or takes it back (picks it besides the
  other rows otherwise). The lists register it through
  `messageListControl.ts` (`expand` / `collapse` now only for expandable
  lists).
- `message-list-all-selected` says "1 selected" for one row; the icons of
  the list actions come only from `defaults/layout.kdl`.

## Consequences

- Tests: `nail-store` (`a_message_moved_into_a_folder_counts_as_after_the_mark`,
  `a_message_gone_before_its_part_applies_is_left_out`, the reads of a
  list), `nail-core` unit tests (`apply_resolved` with a vanished message
  in a later part, a later part that fails → `left_out`) and
  `tests/mail_actions.rs` (the chip's rows keep their folder and leave a copy
  in another folder alone, a message moved in after the mark, a gone
  exception), the ts-vectors, Vitest (`selectAll.app.test.tsx`: announcer,
  axe, Ctrl+Space, Shift+↓, the row menu, "1 selected") and
  `e2e/tests/r5-select-all.spec.ts` (axe after Ctrl+A, the row menu,
  Ctrl+Space, "Selection cleared").
