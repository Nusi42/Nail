# ADR 0149: P2 WP201 — conversations in the store and the core

Status: accepted · 2026-10-05 · P2 WP201 (implements ADR 0148's conversation contract)

## Context

ADR 0148 defined what a conversation is and left the core with a stub: `ListThreads` counted the listed folder only,
nobody was "me", and `GetConversation` answered with `GetThread`'s messages. The quick win's demo showed the
mismatch (the Nightingale row said 4, its strip 7). WP201 implements the contract in `nail-store` and `nail-core`,
keeps the 100k-message corpus within its budgets, and narrows conversations for remote sessions.

## Decisions

### Store (`nail-store/src/thread/conversation.rs`)

- **Folders first, then rows.** `Store::conversation_folders(listed)` classifies the folders a list shows
  (`ConversationFolders`): `threaded` (listed, grouped by thread), `singles` (listed Drafts folders: every draft a
  row of its own) and `sent` (the Sent folders of the threaded folders' accounts that are not listed themselves).
  Each entry carries its account (`ScopeFolder`), so the core can narrow them (`retain`) before listing.
- **One page query.** `list_conversations(folders, own, offset, limit)` groups a `UNION ALL` of the listed
  messages, the Sent messages of threads that have a listed message, and the drafts (`single-<message id>`) by
  conversation key and orders by the newest message (`MAX(date_sort)`, ties by key). The Sent part is written
  `+thread_id IN (…)` so SQLite reads the Sent folders through `messages_folder_date` and probes the listed threads
  once, instead of one thread lookup per listed message (EXPLAIN test). Totals: distinct threads in the listed
  folders plus the drafts — a thread only in Sent never makes an Inbox row.
- **Schema v16: `messages_folder_thread` (folder_id, thread_id, date_sort).** Every part of the page and count
  queries reads only this covering index (EXPLAIN tests). Measured on the 100k standard corpus (release build,
  page of 100 rows, median): INBOX (29,830 messages, 17,829 conversations) 76 ms for the old folder-only query
  without the index, 122 ms with the Sent join without the index, **20 ms** with it (offset 5,000: 25 ms);
  Archive (18,205) 16 ms; Sent (12,154) 8 ms; one conversation read < 0.3 ms. The bench after the migration:
  initial sync 81.6 s (budget 300), incremental 785 ms (3,000), list page p95 4.0 ms (50), search p95 116 ms
  (250), peak memory 116 MiB (800); the small corpus passes too.
- **Each row** reads its messages through `messages_thread_id` (EXPLAIN test), listed copies before joined Sent
  copies of the same moment, so collapsing copies with one Message-ID keeps the listed copy (what the row's
  actions act on); the kept copy is the user's when any copy sits in Sent.
- **`from_me`** = a copy sits in a Sent folder, or the sender is one of the message's account's addresses.
  **Participant `me`** = the address belongs to any of the conversation's accounts, or the person sent one of the
  user's messages (so a Sent alias counts without an identity). `latest_from.me` follows its participant.
- **`conversation(thread, folders, own)`**: with folders, the listed and joining Sent messages; without, every
  folder but Drafts, Trash and Junk (`NOT EXISTS` on the folder's role); a `single-` id is that one message.
  Unknown → `NotFound`. The row is built by `summarize_conversation` from the same messages, so the row and the
  reader agree (tested).
- `list_threads(folders, …)` keeps its signature (no identities known; Sent still marks "me"); `get_thread` is
  unchanged (every copy, every folder).

### Core

- `conversations.rs`: `own_addresses_by_account` (configured accounts' address + identities, stored accounts'
  addresses for local / registered accounts), `list_conversations` / `get_conversation` over a real or virtual
  folder with an optional `FolderFilter`, and `conversation_row` / `thread_row` to rebuild a row from the messages
  a caller may see. `ListThreads` = `list_conversations` without a filter; `GetThread` marks "me"
  (`OwnAddresses::mark`).
- **Drafts rows** have the id `single-<message id>` (as the UI mock), so two drafts of one thread are two rows and
  `GetConversation` answers each with the draft alone.

### Remote sessions and MCP

- A grant that does not cover everything lists and reads conversations through `grant_filter`: folders of
  ungranted accounts and excluded folders (a Sent folder, a virtual folder's member) drop out **before** the
  query, so counts, participants, order and `latest_message` never reflect them.
- `Response::Thread` / `Response::Conversation` answers for a narrowed grant rebuild their row from the granted
  messages (before, `GetThread`'s summary still counted and named messages outside the grant); nothing left =
  `forbidden` as before.
- MCP's thread resource keeps the participant shape `{name, email, me, unread}` for restricted grants too ("me"
  from the full row, unread from the visible messages).

## Consequences

- The demo Inbox's Nightingale row now says 7 (four in the Inbox, three replies from Sent) and names the user
  "me"; `e2e/tests/p2-quickwin.spec.ts` expects 4 and must change to 7 (WP204 / integrator).
- Sent mail with no Inbox counterpart still appears only in Sent.
- `nail-bench` measures `ListMessages` only; the conversation page cost was measured separately (see the WP201
  report) and `ListThreads` should join the bench's metrics (integrator follow-up; `nail-bench` is not WP201's).
