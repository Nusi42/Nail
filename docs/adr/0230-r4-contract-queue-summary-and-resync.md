# 0230 — R4 contract: queue summary, coalesced queue events, resync

Status: accepted (2026-11-21)

## Context

A maintainer report on the installed 0.8.8 (one plain IMAP account): several
hundred messages were moved and deleted in a few bulk actions (one queued
operation per message, about 460 moves and 74 deletes within three minutes).
The diagnostics bundle showed every operation `done` on the server, but

1. the status bar kept saying "Saving 14 changes" until Nail was restarted;
2. the Drafts folder showed some of the moved messages (apparently selected),
   and they also still showed in the Inbox; after a restart those rows were
   gone: stale UI rows, never on the server.

What the code does (verified for this ADR):

- The status bar's count is a UI-only list of op ids
  (`ui/src/state/uiStore.ts` `pendingOps`): ids are added from each
  `Queued` answer (`features/mail-actions/run.ts`, `state/mailOps.ts`) and
  removed only by an `OpStatus` event with state `done` or `failed`
  (`App.tsx`). There is no request for the real queue.
- The core's event channel holds 256 events per subscriber
  (`service.rs` `EVENT_CAPACITY`). A bulk action sends one `OpStatus`
  (`pending`) per operation when it is queued (`mail.rs`
  `announce_enqueued`), then `in-flight` and `done` per operation while it is
  replayed (`replay.rs` `EventReplayer`, groups of up to 500 on IMAP, single
  operations on local and POP3 accounts). 460 moves are well over 1 000
  events within seconds.
- When the desktop forwarder (`app/src-tauri/src/lib.rs` `forward_events`)
  or the web socket (`nail-server` `ws.rs`) lags, it sends
  `Service::resync_events`: configuration, accounts and one `MailChanged`
  for every folder, but nothing about the queue and no signal that the
  client lost events. The `done` events it lost never come again, so the
  count stays up; open lists and the selection are not reconciled.
- The Move window (`ui/src/widgets/FolderPicker.tsx`) moves to the first
  folder whose name or path *contains* the typed text when Enter is
  pressed, so a short filter can move mail to an unexpected folder (Drafts
  among them), and nothing asks before moving into Drafts, Trash or Junk.
- `isDraftMessage` (`features/mail-actions/run.ts`) treats every message in
  a Drafts folder as a draft, so a received message that landed there opens
  in the composer instead of the reader.

## Decision

### The queue summary (core, this contract)

- `nail_api::OpsSummary { pending, in_flight, failed, accounts:
  [OpsAccountSummary { account_id, pending, in_flight, failed }],
  newest_failure: OpFailure | null }` with `OpFailure { op_id, account_id,
  code, localized?, message, failed_at }`. Totals are the sums of the
  accounts; accounts with nothing queued are left out; `waiting()` =
  `pending + in_flight` is the status bar's "Saving N changes".
  `OpsSummary::from_accounts` and `retain_accounts` (the remote UI's
  narrowed grants) are the only places that build or narrow one.
- `Request::GetOpsSummary` → `Response::OpsSummary { summary }` (read scope;
  the remote UI narrows it to granted accounts).
- `Request::RetryFailedOps { account_id? }` → `Queued` (every failed write of
  the account, or of all accounts) and `Request::DismissFailedOps
  { account_id? }` → `Done` (deletes the failed rows; ADR 0233: a dismissed
  move or delete is undone locally, so the message shows where the server
  still has it). Organize
  scope; without `account_id` a remote session needs every account.
  `RetryOp` stays for one operation.
- Store: `Store::write_queue_counts` (contract name `op_queue_counts`,
  renamed by WS1, ADR 0231), `newest_failed_op`, `retry_failed_ops`,
  `dismiss_failed_ops` (`crates/nail-store/src/queue.rs`). The contract's
  summary counts the message write queue (`op_queue`); `done` rows are not
  counted.

### Coalesced queue events (core, this contract)

- `Event::OpsChanged { summary }` carries the whole summary, so a client that
  missed one is right again with the next. It is sent once per queued batch
  (`announce_enqueued`, one per request however many messages), once per
  retry or dismissal, at most once per replayed group and
  `OPS_CHANGED_INTERVAL` (250 ms) while a pass runs
  (`Replayer::group_recorded`, a new hook with a no-op default, called after
  a group's outcomes are recorded), and always at the end of each replay
  pass (`replay::replay_account`). Summaries are read and sent under one
  lock (`Inner::ops_events`), so a newer summary never goes out before an
  older one.
- **`OpStatus` for message writes is removed (WS1)**: no `pending`,
  `in-flight` or `done` per operation, and no `failed` per operation either
  (a mass failure would flood the channel the same way); `OpsChanged` with
  `newest_failure` replaces them, and the UI refreshes the lists when
  `failed` grows. `OpStatus` **stays for folder changes** (ADR 0196: the
  folder dialogs wait for their one operation with `runQueued`), and the
  `OpStatus` type stays in the contract. The contract itself still sends the
  old per-operation events next to `OpsChanged`, so the UI keeps working
  until WS2 has switched; WS1 removes them.
- `EVENT_CAPACITY` stays 256: with the flood gone, a lag is rare, and the
  resync below makes it harmless.

### Resync (core, this contract)

- `Event::Resync` (no fields) is sent last in `Service::resync_events`,
  right after an `OpsChanged` with the current summary. It tells the client
  it missed events: re-read the queue summary, refetch every open message
  list, conversation and reader, and drop selected messages that are no
  longer in their list. Both transports already send `resync_events` on a
  lag; remote sessions receive `Resync` always and `OpsChanged` narrowed.

### Failure details (WS1)

- The store keeps only the English text of a failure (`last_error`), so the
  contract's `OpFailure.code` is `internal` with that text as `message`. WS1
  records the error class (and localized text where the executor has one)
  with the failed operation so the summary names the real reason, also after
  a restart (a schema step adding the code to `op_queue` is allowed).
- WS1 also counts the other write queues the old status bar count included
  (folder changes; Gmail label and Exchange writes where they answer
  `Queued` op ids) in the summary, or documents why one is left out.

### The mock

- `ui/src/api/mockOps.ts` holds the mock core's queue (`MockData.opQueue`,
  replacing `failedOps`): `GetOpsSummary`, `RetryFailedOps`,
  `DismissFailedOps`, `OpsChanged` per queued batch and per replayed write;
  it keeps sending `OpStatus` until the UI no longer needs it.

### UI decisions (WS2, WS3 build them)

- **Status bar (WS2):** the count is `summary.waiting()` from
  `OpsChanged` / `GetOpsSummary` (read on start, after a reconnect and on
  `Resync`), never a list of op ids. `uiStore.opQueued(ids)` keeps its
  signature for its callers (mail actions, triage, Gmail, Exchange) but may
  only bump the count until the next summary arrives. A failure shows
  `newest_failure`'s reason and, with several, how many failed; Retry sends
  `RetryFailedOps`, Dismiss `DismissFailedOps` (both for every account).
  New texts go to `locales/en/status.ftl`.
- **Lists after a resync (WS2):** `Resync` invalidates message lists,
  conversations and the reader and drops selected ids that are no longer in
  the refetched list (`ui/src/actions/mailTargets.ts`, `MessageList.tsx`).
- **Move window (WS3):** Enter moves only to the highlighted row (arrow keys
  or the mouse) or to a folder whose name equals the typed text
  (case-insensitive); otherwise it does nothing and the list says why.
  Moving into Drafts, Trash or Junk asks first (folder and count).
- **Drafts safety (WS3):** only messages with the `\Draft` flag continue in
  the composer; any other message in a Drafts folder opens in the reader
  (`isDraftMessage`).

### Rejected

- Raising `EVENT_CAPACITY` only: a larger bulk action overflows any size.
- Keeping `OpStatus` per operation and batching only the UI: the channel
  overflows in the core, before any UI sees it.
- A UI-side timeout that clears "Saving" after a while: it would hide real
  waiting writes (offline) and never be right.

## Consequences

- The status bar shows the core's real queue, for all accounts; a lost
  event no longer leaves it wrong.
- Clients of the protocol (the web UI, remote sessions) that relied on
  `OpStatus` for message writes must use `OpsChanged`; folder changes are
  unchanged.
- No new crates, no new dependencies. A schema step only if WS1 stores the
  failure's error class.
