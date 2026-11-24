# 0252 — R8 P4: flaky tests fixed at their causes

Status: accepted (2026-11-24)

## Context

Several tests failed now and then while other suites kept the machine busy and passed when run alone (backlog
"Tests and flaky tests", the R7 load flakes, ADR 0213's three `nail-core` races). ADR 0251 asks for the causes
to be fixed: no longer time-outs, no retries, no skips; a race in product code is fixed there with a regression
test. The failures were reproduced by running copies of the test binaries in parallel while `cargo test
--workspace` ran in a second target directory, and the Playwright specs with `--repeat-each` beside it; each
failure's trace or output was then read.

## Decision: causes and fixes

1. **Log capture in `replay` tests (test).** `a_deferred_pass_logs_once_at_info_without_the_text` captured its log
   lines with a scoped subscriber (`set_default`). While it is the only scoped subscriber, `tracing-core` registers
   a callsite that *another* thread reaches first with that thread's subscriber (none) and caches
   `Interest::never`, so the test's own event from that callsite was dropped. The tests now install one
   process-wide recorder (`Interest::sometimes`, asks for every event) that writes only for threads that capture
   (`capture_on_this_thread`). Regression test: a callsite reached first by another thread is still captured.
2. **A forced Sieve upload could be lost (product, `nail-core` `sieve.rs`).** `SyncSieve` marked the account as
   uploading in the same set an upload in progress clears when it ends; an earlier upload ending first dropped the
   request, the status said `pending` / `in sync` and the forced upload never ran (ADR 0213's
   `rules_and_vacation_run_on_the_sieve_server`). A forced upload is now its own set (`SieveCtl::force`), taken by
   the next pass (`take_forced`) and shown as uploading until then. Regression test:
   `a_forced_upload_survives_an_earlier_upload_ending`.
3. **A removed shared mailbox's folders came back (product, `exchange/sync.rs`).** A sync pass that listed another
   mailbox's hierarchy before `RemoveExchangeMailbox` wrote its folders back after the removal had forgotten them
   (`shared_mailboxes_sending_and_free_busy`). `apply_hierarchy` now checks that the mailbox is still configured in
   the same store call as its writes and writes nothing otherwise. Regression test:
   `a_removed_mailbox_gets_no_folders_back` (fails without the fix).
4. **Interrupted Exchange listings (test).** The two 520-message listing tests need about a thousand mock
   requests; on a busy machine they took longer than the 30 s wait without being stuck. They now wait with
   `eventually_progressing`: the wait fails only when the mock server answered no request for 30 s. This settles
   the "Listing throughput budgets" row too.
5. **Bulk IMAP fixture (test).** The 25 000-message sync tests ran their IMAP commands with the 10 s test
   time-out; a whole-command deadline (`UID SEARCH`, chunked `UID FETCH` / `UID MOVE`) over that many messages
   took longer on a busy machine. The bulk fixture keeps the product's deadline (`nail_net::DEFAULT_TIMEOUT`).
6. **Benchmark reopen (bench).** `nail-bench` reopened the data directory 750 ms after stopping the service. The
   search index's writer lock and the store's connections go with the last clone of the service, which aborted
   tasks drop later (a store call already running finishes first); a service opened before that searches in memory
   only ("the search index did not reopen") or finds the database locked. `close` now waits until the index's
   writer lock file can be locked (the same OS file lock `tantivy` takes) and SQLite's write-ahead log is gone
   (deleted when the last connection closes), at most a minute.
7. **Benchmark budgets in the test (test).** `a_tiny_run_measures_every_rust_metric` asserted that one tiny run's
   wall times are inside the real budgets; a single cold start on a busy machine is not. It now checks that every
   Rust metric is measured and checked against its budget consistently, and that the load-independent peak memory
   is inside its budget; the performance job measures the wall times.
8. **CBC padding test (test).** Damage inside the last CBC block scrambles it at random and still ends in valid
   padding about once in 256 runs (CBC has no integrity check). The test flips a bit of the next-to-last block,
   which flips the padding byte of the last one deterministically.
9. **Layout edits right after another (product, UI `layout-editor`).** `UpdateLayout` answers `Done`; the write's
   `EditHistoryChanged` (with the new revision) can arrive after the answer. An edit sent in between (m5d's Reset
   to default right after a move) named the replaced revision and answered `conflict` ("layout.kdl was changed
   outside Nail"). `useLayoutEdit` now reads the history after a write before the session stops being busy, and
   sends the cached revision at the time of the write. Regression test in `editor.test.tsx` (history events held
   back; fails without the fix).

10. **A settings section lost its unsaved changes (product, UI `state/uiState.ts`).** Showing the Settings view
    writes the remembered view, choosing a section the remembered section (two `SetUiState` in a row). Each answer
    is the core's whole UI state, and it replaced the cache: the view's answer, older than the section's write,
    brought the previous section back for a moment, so the section shown was unmounted and lost what the test had
    just changed (`r2-rules-window`: Appearance's save bar vanished before Save; `security.spec`: the MCP form lost
    the typed client name). An answer now sets only the key it wrote, and only when no newer write of that key was
    sent; `UiStateChanged` keeps the other keys current. Regression tests in `uiState.test.tsx` (both fail without
    the fix).

11. **Plug-in dev reload test (test).** `dev_folders_load_and_reload` edited the plug-in's files with
    `std::fs::write`, which truncates first; a reload that ran between the truncate and the write read an empty
    locale file and logged the plug-in's name as its key (`name`). The test now saves the way editors do (a dot
    file renamed over the old one; the dev watcher ignores dot files).

12. **An answer overtook its request's events (product, `nail-server` `ws.rs`).** The WebSocket sent events
    from one task and answers from each request's task, so an answer could reach the UI before the events its
    request had broadcast. The UI then acted on what it had not seen yet: the layout editor's next edit named the
    replaced revision (`conflict`, item 9) or moved a pane in the layout from before a Reset (m5d). Answers now go
    out through the event task, after every event queued before them (broadcasting is synchronous, so a request's
    events are queued before its answer is made). Regression test:
    `answers_come_after_the_events_their_requests_broadcast` (100 writes at once; it fails without the fix).

## Not reproduced

`mail_actions` `the_lab_move_conversation_on_the_imap_server`, `security::autocrypt_enables_encryption_after_one_message`,
`pim_sync::conflicts_keep_the_server_version_and_resolve_both_ways` and `categories_automatic_replies_and_rooms`
passed every run under load (see the P4 report); the e2e specs `a11y-windows` (connection-log step), `m2c`,
`desktop-health` and `r4-status-bulk` passed in every repeated run.

**Not fixed: WebKit under load.** The WebKit specs (`p3-card-height`, `mail-links`, `remote-content`,
`app-chrome`) run out of their 30 s when the machine is busy: alone the remote-origin card-height test takes about
18 s, under `cargo test --workspace` about twice that, and the trace's screencast shows the page not painting for
up to 7 s. Fewer WebKit engines at once (a project `workers` limit) did not remove the failures; making these tests
do less per test (or giving them a budget that fits their work) is left to a later wave.

## Consequences

New e2e scenarios close backlog gaps: `r8-p4-rule-drag.spec.ts` (rule list drag, `rules.kdl` order) and
`r8-p4-dialogs.spec.ts` (Exchange mailbox editor and remove question, category delete, automatic replies,
classify, the Outbox discard question). The Exchange e2e helpers moved to `e2e/support/exchange.ts`.
Writing the classify scenario found a product bug, left to the UI owners: the reader card's "Move to Other"
(`FocusMoveButton` inside `CardScope`) opens an empty classify window, because feature stores are kept per UI store
and a card gets a scoped UI store of its own (`scopedUiStore`); the scenario uses the message menu's "Move to
Focused / Other…" meanwhile. The Exchange move race's deterministic hook and the auto-schedule e2e need test hooks
in other packages' mocks and stay in the backlog.

Proof under load (`cargo test --workspace` running in a second target directory): the fixed Rust tests passed
10 of 10 rounds with copies running in parallel; the fixed e2e specs (`m5d`, `r2-rules-window`, `security`, the
new `r8-p4-*`) passed 10 of 10 repeats at the default workers. The whole Playwright suite at the default workers
beside `cargo test --workspace` still lost 4 to 12 of 412 tests per run, almost all by running out of their 30 s
(WebKit above, `a11y-windows`, `p1-views`, `vault-recovery`): the suite's budgets assume a machine that is not
saturated by a second suite. Once, the desktop shell's `notification_texts_are_rendered` (`app/src-tauri`) hung
for twenty minutes under that load; it was not investigated further.
