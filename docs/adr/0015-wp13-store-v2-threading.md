# ADR 0015: Store v2 — schema, operation queue per account, threading, drafts and outbox

Status: accepted · 2026-09-29 · M1a WP13 (`crates/nail-store`)

(Numbered with a package suffix because four packages ran in parallel; the
integrator may renumber.)

## Context

M1a needs the store to hold what the mail engine produces and the UI shows:
threads (JWZ, with a subject fallback, spanning folders and the members of
virtual accounts), tags (IMAP keywords), message sizes for `size>` searches,
drafts and an outbox with undo-send. Two M0 follow-ups also land here
(ADR 0007, "Consequences"): a blocked account must not stall the replay of
other accounts, and a sync must not overwrite local changes whose operations
are still queued.

## Decisions

### Schema v2 (migration 2)

- `messages` gains `message_id` (without angle brackets), `in_reply_to_json`,
  `references_json`, `thread_id`, `subject_key` + `subject_is_reply`
  (normalized subject for the fallback) and `size`. New indexes on
  Message-ID, thread, subject key and a global date order (search).
- `message_tags(message_id, tag)`, `WITHOUT ROWID`, cascade-deleted with
  the message.
- `thread_refs(msgid, thread_id)`: every Message-ID ever seen (own and
  referenced) mapped to its thread — the incremental threading state.
- `op_queue.message_id` (backfilled from the JSON payload) and an index
  `(account_id, state, id)` for per-account passes.
- `drafts` (the `nail_api::Draft` JSON) and `outbox` (state machine
  `scheduled → sending → sent | failed`, `cancelled`).
- `folders.highestmodseq` already existed in v1; nothing to add.
- **Not added:** identities and virtual accounts. ADR 0012 made
  `accounts.kdl` their only source of truth; the store only needs member
  lists and folder sets, which the core passes into queries
  (`list_threads(&[FolderId])`, `get_thread(…, Some(&members))`,
  `list_messages_in(&[FolderId])`, search scopes).
- Migrations can now carry a Rust **backfill** that runs in the same
  transaction as the SQL (`Migration { sql, backfill }`). v2 uses it to
  compute subject keys and threads for existing rows; a v1 database keeps
  its M0 grouping because the M0 `thread_key` (root of the reference chain)
  is fed into the thread assignment.
- Upgrade tests open a database built from a verbatim copy of the released
  v1 schema with data (`tests/fixtures/store-v1.sql`).

### Threading

Two parts:

1. **Membership, incremental, in SQL.** On every message write the store
   looks up the thread of each of the message's ids (own Message-ID,
   `References`, `In-Reply-To`; for legacy rows the M0 thread key). None →
   new thread (id derived from the root reference, so `rethread_all` is
   stable); one → join; several → merge into the thread with the oldest
   message. Then all ids are mapped to it. This is the connected-component
   view of JWZ: missing parents, loops and duplicate Message-IDs need no
   special handling, and threads are global, so they span folders and
   accounts; a virtual account filters by its members. Deleting messages
   does not split threads (`rethread_all` rebuilds).
2. **Layout, pure JWZ** (`thread::jwz::build_threads`): the full algorithm
   (containers, reference chain linking without overriding links, loop
   refusal, fresh containers for duplicate ids, pruning of empty
   containers, subject grouping, date-sorted siblings) for
   `Store::thread_tree`, i.e. reply indentation. Tested with the classic
   edge cases.

**Subject fallback:** only for messages with no parent reference and an
unreferenced own id. `Re: x` joins the newest thread with subject `x`
within 60 days; an original `x` joins a thread holding only orphan replies
of `x` (it arrived late). Forwards (`Fwd:`, `WG:`, …) and empty subjects
never join. Prefixes are stripped in the common languages (`Re`, `AW`,
`SV`, `VS`, `Antw`, `Odp`, `Rif`, `Res`, `YNT`; `Fwd`, `Fw`, `WG`, `TR`,
`Enc`, `RV`, `VL`, `Doorst`), with counters (`Re[2]:`) and list tags
(`[list]`). The window keeps unrelated "Hello" threads apart; JWZ itself
has none.

Alternatives considered: running full JWZ over a folder on every list
request (simple, but threads must be stable ids across folders, accounts
and restarts for `GetThread`, MCP and the UI), and storing parent links
only (needs recursive queries and still needs component ids).

### Operation queue

- **Per-account ordering and passes.** `replay_account(account, …)` replays
  one account's operations in order; a `Retry` ends only that account's
  pass. Each account has its own async lock. `replay_pending` runs one pass
  per account **concurrently** (`futures_util::future::join_all`), so an
  offline or hanging server never delays the others. `ReplayReport.deferred`
  became a list (one entry per blocked account).
- **New operations:** `MoveMessage` (same account: MOVE; other account:
  copy + delete as one op owned by the source account) and `DeleteMessage`
  (to the account's Trash, or expunge when permanent / already in Trash; no
  Trash → `not_found`, nothing queued). Enqueue records the source
  (`MessageSource {account, folder, uid}`) and the resolved Trash folder in
  the payload, because the local row moves (UID unknown) or disappears at
  once. `FlagChange` gained `add_tags` / `remove_tags` (empty lists are not
  serialized, so old payloads and their JSON stay identical).
  `enqueue_all` queues a selection atomically.
- **Sync-side merge** (`merge_messages`, and the same rule inside
  `upsert_message(s)`, `update_flags`, `update_flags_and_tags`): while an
  operation for a message is pending or in flight, the server's flags and
  tags are taken and the pending changes are **re-applied on top** (the
  user's change stays visible, other changes from the server still
  arrive); a message with a pending move or delete is not written at all.
  Once the operation is done, the server wins again (design: server wins
  for flags). The report lists `kept_local` and `skipped` messages.

### Headers without breaking the M0 API

`StoredMessage` keeps its four fields (nail-core builds it with a struct
literal). Threading headers and the size travel in a separate
`MessageMeta` via `merge_messages(&[SyncedMessage { message, meta }])`;
`upsert_message(s)` keeps the stored headers and threads by the M0
`thread_key`. `messages_missing_meta` lists rows to backfill from their
blobs.

### Drafts and outbox

Drafts are stored as `nail_api::Draft` JSON with ids `draft-<n>`; outbox
ids are `outbox-<n>`. A draft is locked (`conflict`) while scheduled or
sending. `claim_due_sends(now)` marks due entries `sending`;
`finish_send` records `Sent` (deletes the draft), `Retry {retry_at}` or
`Failed`. `cancel_send` works for scheduled and failed entries (undo send)
and returns the draft id. An entry left `sending` by a crash becomes
`failed` (`unavailable`) on the next open instead of being resent: it may
have reached the server, and a duplicate mail is worse than a retry
button. New store error `Conflict` maps to `ErrorCode::Conflict`.

### Store-side search filters

`MessageFilter` (flags, tags, account by id / name / address, folder by
name / path / role, dates, size, id sets, AND / OR / NOT) compiles to one
parameterized SQL condition; `filter_message_ids` returns ids newest
first. `nail-search` uses it for everything that is not full text
(ADR 0016).

## Consequences

- nail-core (WP14) should switch ingest to `merge_messages` with a
  `MessageMeta` from `nail_mime::ParsedMessage` (`message_id`,
  `in_reply_to`, `references`, raw length), backfill old rows with
  `messages_missing_meta`, replay per account with `replay_account` from
  each sync engine, and call `set_message_uid` after a replayed move learns
  the new UID (`COPYUID`); until then the moved row has no UID and the
  target folder's sync must match it by Message-ID instead of inserting a
  second row.
- `thread_refs` grows with every Message-ID ever seen; it is small (two
  short strings per id) and could be pruned by a later `rethread_all`.
- Cross-account moves change the row's account locally; the core's id
  scheme (derived from account/folder/UID) must tolerate a row whose id no
  longer matches its location until the next full sync.
