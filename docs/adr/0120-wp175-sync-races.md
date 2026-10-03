# ADR 0120: Gmail / Exchange tests that failed under load: two engine races, three tests waiting on the wrong signal (M5c WP175)

Status: accepted · 2026-10-04 · M5c WP175

## Context

Under load (the full suite, or several copies of a test binary in
parallel) these `nail-core` integration tests failed:

- `gmail --test gmail_syncs_folders_labels_and_categories` (account state
  `Connecting` instead of `Online`): about half of the runs with 16 copies
  in parallel.
- `exchange`: `a_message_moved_by_another_client_keeps_its_local_state`
  (the moved message lost its Nail-only tag),
  `an_ews_message_moved_elsewhere_is_moved_not_downloaded_again` (a second
  copy with a new id), `an_ews_account_syncs_streams_and_tests` (the
  streamed message never arrived), `shared_mailboxes_sending_and_free_busy`
  (mailbox `Pending` instead of `Synced`),
  `a_graph_account_syncs_mail_with_focused_inbox_and_categories` (a message
  without its Exchange data). With 8 copies of the binary in parallel 6 of
  16 runs failed.

They were reproduced with copies of the test binaries running in
parallel. Each failure was then traced: debug output was added to the
Exchange sync for this, and removed afterwards.

## Root causes

1. **Exchange: a moved message could be deleted and downloaded again (a
   product bug).** A pass syncs a mailbox's folders one after the other. A
   folder that reports a message removed only marks it, and the marks are
   deleted (`flush_removed`) after "every folder of the mailbox was synced"
   (ADR 0078), so that a message another client moved is found in its new
   folder and moved locally instead. That counted folders synced *earlier
   in the same pass*. When the other client moved the message after the
   target folder's listing but before the source folder's listing (here:
   `Inbox/Receipts` before `Inbox`), the pass marked the message, found no
   folder still to sync, and deleted it. The next pass then downloaded it
   again from the target folder as a new message. Its local state (Nail
   tags, a pending Focused Inbox override) was lost, an EWS message got a
   new id, and outside a first listing the copy counts as arrived mail
   (rules, `NewMail`).
2. **EWS streaming: mail delivered before the subscription existed was
   missed until the next poll (a product bug).** The streaming subscription
   is created by the first `wait_for_changes` *after* a pass. A
   subscription reports only what happens after it exists. So mail
   delivered between a folder's listing and the subscription (a long first
   pass, a slow server) waited for the next pass: up to the 60 s stream
   wait, or the poll interval.
3. **Tests that waited on the wrong signal.** Messages are stored as they
   are listed, so they show up while the pass still runs. Their Exchange
   extras are written right after them. The account turns `Online` only
   when the whole pass has succeeded. A shared mailbox turns `Synced` only
   when all of its folders are synced. Three tests waited for a message
   count and then checked state that the rest of the pass sets:
   - the Gmail test checked `Online`;
   - the shared-mailbox test checked `Synced`;
   - the Graph test checked the messages' Exchange data.

## Decision

1. `sync_mailbox` tracks the folders synced *since the last folder that
   marked messages removed* (`SinceRemoval`). `sync_folder` now reports
   whether it marked messages removed, from a delta or from merging a
   full listing. Before marked messages are deleted, every other folder
   of the mailbox is synced again, read fresh from the store so a folder
   synced in this pass goes on from its new cursor. If such a sync marks
   messages removed itself, the folders synced before it go again. After
   three rounds the marks stay and the next pass decides; leaving a
   message marked is safe, deleting it is not. A pass without removals
   costs nothing extra. A pass with a removal costs one delta call per
   folder synced before the removing one.
2. The Exchange loop remembers the folder set its streaming subscription
   was made for. When there is none yet, or the own folders changed, the
   loop first waits with a timeout of 0, which only creates the
   subscription. It then wakes every folder and runs a pass before it
   waits on the stream. A failed wait forgets the subscription, so the
   next one is treated as new. The result is one extra delta pass after
   start-up, and after each change to the folder list. Back-off for
   failing subscriptions is unchanged (ADR 0078).
3. The tests wait on the signal that the state they check depends on.
   Gmail's `synced` and the Graph test wait for `Online` (plus the message
   count). The shared-mailbox test waits for `Synced`. No sleeps or longer
   timeouts were added. `a_message_moved_by_another_client_keeps_its_local_state`
   stays strict: it still tags and moves while the first pass may be
   running, and that window is exactly what fix 1 covers.

## Tests

- Unit: `exchange::sync::tests::folders_listed_before_a_removal_are_synced_again`
  covers the bookkeeping: which folders are stale after a removal, and
  the re-runs when a re-synced folder reports a removal of its own.
- Integration: `a_new_streaming_subscription_is_followed_by_a_sync` (the
  EWS mock's operation log must show `Subscribe`, then `SyncFolderItems`,
  then `GetStreamingEvents`, with exactly one subscription). It times out
  without fix 2, which was checked by switching the fix off.
- Stability (debug build, Windows, 32 threads, with 3 + 3 copies of the
  gmail / exchange binaries looping as background load):
  - each of the three named tests passed 50 of 50 runs sequentially, and
    50 of 50 with 5 copies in parallel;
  - the full gmail and exchange binaries passed 10 of 10 runs each;
  - with 6 copies of each binary in parallel, 24 of 24 runs per binary
    passed, against 6 of 16 exchange runs failing before the fix.

## Not fixed here (follow-ups)

- With about 20 copies of the exchange binary in parallel, the
  520-message listing tests run out of their 30 s budget from CPU
  starvation alone. That is throughput, not a race.
- The Exchange ingest writes the message, then `set_message_remote_id`,
  then `set_exchange_item`, each locking the store connection on its own.
  A reader in between briefly sees a message without its Exchange data
  (Focused Inbox, conversation). Writes are not affected: the engine
  replays queued writes only between passes, after that store call has
  finished. Making it atomic needs a `nail-store` API (an ingest that
  takes the remote id and the item record), so it is a request to the
  integrator.
