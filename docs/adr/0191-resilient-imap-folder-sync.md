# ADR 0191: Resilient IMAP folder sync — one bad message never fails a folder

Status: accepted · 2026-10-08 · owner report (the provider: "The folder INBOX could not be synchronized"; INBOX stopped
after 150 messages, every other folder synced)

## Context

In real-account testing, INBOX stopped at 150 stored messages, three full batches of 50 (`FetchBatchLimits::default`), and every
later pass failed at the same place. The status bar showed `sync-error-folders`, which `report_pass` emits only for a
folder error that is *not* fatal for the pass: `ImapError::No`, `ImapError::Protocol` or a `CoreError::Api`
(`is_fatal_for_pass`). Without a log nothing told which. The affected server was not examined directly; this ADR records
what the code showed.

### Confirmed in code

1. **A `FETCH` response the strict parser rejects fails the whole batch.** `conn.rs` handed every response to
   `imap-proto`; one it could not parse became `Incoming::Unparsed` and was dropped. `fetch_uids` then found the UID
   missing, `check_complete` saw it still on the server and returned `Protocol("server did not return messages N")`
   — a folder failure, on every retry at the same batch. `imap-proto` 0.16 rejects (a) 8-bit bytes in quoted strings
   (`quoted` allows only 7-bit `CHAR`): raw UTF-8 or Latin-1 subjects / names in `ENVELOPE`, common in spam and old
   mail; (b) `()` for an empty address list (`many1`); (c) keywords that are not atoms; and it **panics** on
   `INTERNALDATE NIL` (`unwrap()` in `msg_att_internal_date`). Nail asked for `ENVELOPE` although the core never
   used it (headers come from the message itself).
2. **A server `NO` / `BAD` for one message fails the whole batch.** Dovecot answers the whole `UID FETCH` with
   `NO [SERVERBUG] Internal error occurred` when one mail file is broken; other servers refuse long UID sets with
   `BAD`. Either failed the folder at the same batch forever.
3. **The same parser gap could delete messages locally.** A full flag scan (`UID FETCH 1:* (UID FLAGS)`) took an
   unparsable response as "message gone" and removed it from the store (`resync_flags` → `vanished`).
4. **A `CONDSTORE` / `QRESYNC` server that refuses the `SELECT` / `EXAMINE` parameter** (advertised but not
   supported for the mailbox) failed every folder at `plan_fetch` / `resync_flags`.
5. **An oversized literal** (> 512 MiB) returned an error in the middle of a response and left the connection out of
   step.

Not a cause: a message `nail-mime` cannot parse — `parse_message` already catches panics and returns an empty parse.
The checkpoint *is* written per batch (`FolderSink::store` writes `uidnext` with the batch), so a `NULL` `uidnext`
next to a set `uidvalidity` is not something the current code writes; a later look at the affected data should
re-check that observation.

### Hypotheses (not confirmable without the server's answers)

Which of 1 or 2 hit messages 151–200 of the affected INBOX (most likely 1a: a raw 8-bit subject in `ENVELOPE`, or 2: a `NO`
for one broken message), and whether the provider's server also refuses `(CONDSTORE)` or long UID sets. The connection
log (ADR 0190) now shows exactly that: the failing `UID FETCH`, the server's tagged answer and the skipped UIDs.

## Decisions

- **No `ENVELOPE`** in the fetch queries; `FetchedMessage.envelope` and `Envelope` are gone (unused).
- **Lenient `FETCH` reader** (`nail-proto-imap/src/salvage.rs`): when `imap-proto` rejects (or panics on) a
  `* n FETCH` response, a small reader takes `UID`, `FLAGS`, `INTERNALDATE`, `RFC822.SIZE`, `MODSEQ` and the
  `BODY[]` / `BODY[HEADER]` data and skips everything else whatever bytes it holds (nesting bounded at 64). Flag
  scans use it too, so an odd response is never taken as an expunge again.
- **Resilient batches** (`ImapClient::fetch_batch_resilient`): a refused batch is split in halves down to single
  messages; a single message that is refused, withheld, unreadable or too large is reported as a `FetchFailure`
  (kind, detail, the logged command and server answer) instead of failing the batch. Refusals that concern the
  mailbox (`[UNAVAILABLE]`, `[NOPERM]`, `[INUSE]`, `[LIMIT]`, …), errors that break the connection and more than five
  failing messages in one batch still fail it. `SEARCH SINCE` refused → whole messages; message sizes refused →
  batches by count.
- **Skipped, not lost**: the core records each failure in `sync_skipped` (schema v19, with its `ErrorDetails`) in
  the same store call that moves the folder's checkpoint past it; every later folder sync retries up to 50 skipped
  messages (least tried first) and forgets the ones that arrive or are gone; a `UIDVALIDITY` reset clears them.
  `ListSyncErrors` lists them, `SyncErrorsChanged` announces changes, the UI shows them under the account.
- **`(CONDSTORE)` / `(QRESYNC …)` refused** → the connection opens mailboxes without the parameter
  (`plain_select`) / resyncs without QRESYNC.
- **Oversized literals** are read and dropped (the connection stays in step) and arrive empty; the message is
  reported as too large.
- Tests inject every kind into the mock server (`MessageFault::{Refuse, Malformed, Withhold, Garbage}`,
  `limit_fetch_uids`, `refuse_select_parameters`) at the client (`tests/integration/faults.rs`) and the core
  (`nail-core/tests/sync.rs`); demo-multi has the controls `imap-fault` and `imap-deny`.

## Consequences

- One broken or unusual message costs that message (shown, retried), never the folder.
- The first sync of a large folder makes progress past any single bad message; a broken connection still resumes
  from the last stored batch.
