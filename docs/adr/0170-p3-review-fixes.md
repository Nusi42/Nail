# ADR 0170: P3 review fixes — Delete never expunges, Gmail's Delete and restore, stale EWS ids

Status: accepted · 2026-10-05 · P3 review fixes (amends ADRs 0164, 0165, 0167, 0169)

## Context

The two P3 reviews found five must-fix problems and four minor ones in the conversation actions:

1. `MailAction::Trash` on messages already in Trash took the queue's "delete for good" branch: a conversation
   listed in Trash was expunged, without undo, by anyone with the `organize` scope (remote sessions, MCP
   `delete_messages` with `permanent=false`, whose confirmation said "move to Trash").
2. The "Delete permanently?" window counted the resolved messages but deleted the re-resolved conversation, so
   mail that reached Trash / Junk while it was open was deleted uncounted.
3. Undoing a Delete on a Gmail conversation failed for the user's replies: the inverse Trash → Sent was an
   unsupported Gmail move (permanently failed operation; server and Nail disagreed).
4. EWS gives a moved item a new `ItemId`. An undo queued before the forward move replayed recorded the old id,
   got `ErrorItemNotFound` (= gone = success) and left the message in the target folder on the server.
5. Delete reached the listed folder plus Sent, not Gmail's whole thread (archived copies stayed; Delete in Sent
   left the Inbox copies), and a later restore from Trash put the user's own replies into the Inbox.

Minor: the undo history was purged across actors (an agent could evict the user's undo), MCP conversation
confirmations counted conversations wholly outside the grant, the label / category windows computed their
state from the selected message only, and the toast said "labelled" for removals and Exchange categories.

## Decisions

- **Trash never deletes for good.** `ApplyMailAction` `trash` leaves messages already in Trash out of its
  targets (resolution, counts and plan; a conversation wholly in Trash answers `no-messages`). Deleting from
  Trash stays `DeletePermanently` with its own scope (`permanent-delete`) and question. The older
  `DeleteMessages {permanent: false}` keeps its local behaviour, but a remote session needs `permanent-delete`
  for it when a named message is in Trash, and MCP `delete_messages` without `permanent` refuses messages in
  Trash.
- **Delete is Gmail's whole thread.** New reach `ConversationReach::Thread` for `trash`: `whole` plus the
  thread's messages in every other folder of the listed folders' accounts (without a folder: of every account)
  but Drafts, Trash and Junk. Threads span accounts (the same mail received twice); Delete in one account's
  folder does not reach another account's copy. Read state and tags keep `whole` (what the row counts).
- **Restore keeps sent mail in Sent.** `Move` from Trash to a folder with the Inbox role moves each message
  whose sender is one of its account's addresses — configured, or found in the newest 200 messages of the account's Sent folder (the demo corpus writes from another address than the account's) — to the account's Sent folder instead, like Gmail's restore.
  Other targets move everything (the user chose that folder).
- **Gmail Trash / Spam → Sent** is a label change: remove `TRASH` / `SPAM` (`users.messages.untrash` for
  Trash); the message keeps `SENT`. Other moves into Sent stay unsupported.
- **Remote ids follow EWS moves.** `Store::rebase_remote_ids` rewrites the recorded `remote_id` of every waiting
  move / delete of a message when the server answers a new id (`set_remote` after a replayed move, and a move
  found by sync), like IMAP's `rebase_later_ops` does for UIDs. Graph uses immutable ids and is unaffected.
- **Delete permanently acts on what it counted.** The window keeps the resolved message ids and applies
  `DeletePermanently` to them as message targets (in requests of at most 500 targets; the rows still leave the
  list as a conversation). A message that arrived meanwhile stays.
- **Undo history per actor.** The newest `MAIL_UNDO_KEEP` (50) batches are kept per actor; the one-hour limit is
  unchanged.
- **MCP counts after the grant.** Each conversation of `update_messages` / `delete_messages` is resolved on its
  own; the confirmation counts only conversations with a message the grant covers.
- **Label and category windows over the targets.** The windows keep the targets they opened with and count
  each tag over their messages (`GetConversation` per conversation, cached summaries for messages): checked =
  all, mixed (`indeterminate`) = some. An untouched mixed box changes nothing; ticking adds to all, clearing
  removes every spelling from all.
- **Toast wording.** Tag changes say what happened and what the tags are called:
  `mail-actions-done-tags-{added,removed,changed}` with `$noun` label / category / tag.

- **Rust 1.99 lints.** The machine's stable toolchain moved from 1.97 to 1.99 during the fix stage (CI uses
  `stable` too), and its clippy adds `assert_is_empty`, `unused_async_trait_impl`, `chunks_exact_to_as_chunks`,
  `single_range_in_vec_init` and `manual_is_variant_and`, and no longer needs the `float_cmp` expectation in
  nail-bench. They are fixed, not allowed: an `assert!(x.is_empty())` now prints the value (`assert!(x.is_empty(),
  "{x:?}")`, or `assert_eq!(call().len(), 0)` where the receiver is a call), test fakes of async traits return
  `std::future::ready(…)`, `fetch_update` (deprecated) became a compare-exchange loop in `nail-ical`. One narrow
  `#[expect(clippy::unused_async_trait_impl)]` covers the methods rmcp's `#[tool_handler]` generates in a test.

## Consequences

- No schema change, no new dependency. `ConversationReach` gains `thread` (TypeScript regenerated; the mock
  core mirrors it, the restore rule and the Trash rule).
- Tests: store (`thread` reach incl. accounts, per-actor purge, `rebase_remote_ids`), core
  (`mail_actions.rs`: Trash in Trash, remote `DeleteMessages`, whole-thread Delete and restore to Sent;
  `gmail.rs`: Delete + undo of a Sent message; `exchange.rs`: EWS undo queued before the move replayed), MCP
  (counts after the grant, Trash refusal), UI (mock reach, Delete permanently applies the counted ids and
  chunks, mixed label boxes, tag toasts), Playwright (`p3-actions.spec.ts`: restore from Trash returns the
  replies to Sent; the toast text of labels).
