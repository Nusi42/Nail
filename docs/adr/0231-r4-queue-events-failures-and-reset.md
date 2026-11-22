# 0231 — R4 WS1: queue events, failure classes, deferred-pass logs, no duplicates after a reset

Status: accepted (2026-11-21)

## Context

ADR 0230 (the R4 contract) added the queue summary (`OpsSummary`,
`GetOpsSummary`, `OpsChanged`, `Resync`) and left the core's part of the
follow-up work to WS1: stop the per-operation `OpStatus` events of message
writes, keep a failure's real reason, count the other write queues, make
deferred replays visible in a diagnostics bundle, and stop moved messages
from showing twice after a UIDVALIDITY reset.

## Decision

### Events of message writes

- Message writes send no `OpStatus` at all: not when they are queued
  (`Service::announce_enqueued`), not when one is retried (`retry_op`) and
  not while they are replayed (`replay::EventReplayer`, neither `in-flight`,
  `done`, `pending` nor `failed`). Clients follow `OpsChanged`: once per
  queued batch, once per retry or dismissal, at most once per replayed group
  and `OPS_CHANGED_INTERVAL`, and at the end of each pass.
- A bulk move and delete of 2,000 messages on the IMAP mock sends a few
  dozen events in total (the test asserts fewer than `EVENT_CAPACITY / 8`
  and that the receiver never lags).
- Folder changes keep `OpStatus` (ADR 0196: the folder dialogs wait for their
  one operation).

### Failure classes (schema v22)

- `op_queue` gets `error_code` (the `ErrorCode` wire name) and
  `error_localized` (a `LocalizedMessage` as JSON) next to `last_error`.
- `nail_store::ReplayOutcome::Retry` / `Fail` carry an `OpError { code,
  localized, message }` instead of a bare text. `From<String>` / `From<&str>`
  give a text-only error the class `internal`, so replayers without a class
  (tests, the PIM-free paths) keep working unchanged.
- An attempt's error replaces all three columns; a success keeps them.
  `OpFailure` in the summary now carries the stored class and text, also
  after a restart; rows that failed before schema v22 read as `internal`.
- The diagnostics bundle's `store/operations.json` lists `error_code` too.

### Other write queues in the summary

- `Store::write_queue_counts` (was `op_queue_counts`) counts, per account,
  the message writes (`pending`, `in_flight`, `failed`) plus the folder
  changes (`folder_ops`), Gmail label changes (`gmail_label_ops`) and
  Exchange writes (`exchange_ops`) still `pending`. Those three queues have
  no in-flight state.
- Their **failures are left out** of `failed`: `RetryFailedOps` and
  `DismissFailedOps` act on message writes only, so a counted folder or
  label failure could never be cleared from the status bar. Folder changes
  report their failure in the folder dialog (`OpStatus`), Gmail labels and
  Exchange categories / settings show their sync state in their own screens.
- Queuing and finishing those writes sends `OpsChanged` (folder requests,
  `gmail::queue_label_op`, `exchange::queue`, after each Gmail label and
  Exchange replay loop, and when the folder replay ends a pass early).

### Logs of deferred passes

- A replay pass that ends because a write must wait (offline, timeout)
  logs one INFO line per pass: `writes deferred; retrying later` with the
  account, the number of writes still waiting and the error class. The
  error's text stays at DEBUG (it may name messages, servers or
  addresses). Giving up after the last attempt stays WARN.

### No duplicates after a UIDVALIDITY reset

- `ingest::resolve_id` no longer stores a server message as a new row when
  a move or delete of a message with the same `Message-ID` out of that
  folder is still waiting for replay (`Store::has_waiting_move_out`, which
  reads the operation's recorded source; ADR 0233 replaced it with
  `Store::held_move_outs`, which also holds failed writes, and lists the
  skipped UID to fetch again). After a UIDVALIDITY reset the sync
  fetches the folder again with new UIDs; without the check the moved
  message showed in its new folder and, as a new row, in the old one until
  the next sync after the replay.
- A row that is already there (same UID, or a row moved in by the user
  without a UID yet) is found before the check, as before.
- Trade-off: a second copy of the same message (same `Message-ID`) in the
  source folder is not stored while the move waits. The sync then fetches
  it again only with the next full fetch of the folder; such duplicates are
  rare, and the moved copy is where the user put it.

## Consequences

- Protocol clients that still followed `OpStatus` for message writes must
  use `OpsChanged` (WS2 switches the desktop and web UI).
- Schema v22; no new crates or dependencies.
- `nail-server`'s web socket already sends `resync_events`; its test
  `a_lagging_client_is_resynced` (`tests/integration/server.rs`) checks that
  a lagging socket receives `OpsChanged` and then `Resync`.
