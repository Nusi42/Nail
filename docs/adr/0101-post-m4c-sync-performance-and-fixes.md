# ADR 0101: Post-M4c: overlapping sync pipeline, lean sync lookups, plug-in reload notices, demo-multi keychain, confirmed overwrite

Status: accepted · 2026-10-02 · post-M4c (verify findings)

## Context

The independent M4c verify failed the M4 automated gate on four points:

- **Performance.** Two strict runs of `node e2e/perf/run-perf.mjs --corpus
  standard` on release builds went over budget: initial sync 348.8 s and
  354.6 s (budget 300 s), incremental sync of 200 new messages 3,343 ms and
  3,339 ms (budget 3,000 ms). The integrator had measured 286.4 s /
  2,732 ms (ADR 0097): there was no real headroom.
- **Flaky plug-in reload tests** under full-workspace load:
  `nail-core --test plugins dev_folders_load_and_reload` and
  `nail-plugins --test dev dev_folders_load_log_api_calls_and_hot_reload`.
- **demo-multi secrets.** `nail serve --seed demo-multi` replaced the file
  keychain (`<data-dir>/demo-keychain.json`, ADR 0099) with DemoMulti's
  in-memory store, so encryption at rest did not survive a restart there.
- **Save dialog vs. core.** The desktop Save dialog asks "Replace?"
  (`FOS_OVERWRITEPROMPT`), but the core refused existing files (recovery
  kit, backups, diagnostics, PDF) with `conflict` (plan WP125 follow-up).

## Profiling (release build, phase timers, 20,000-message corpus)

Temporary timers around every step of a pass (not committed) showed that
the initial sync was one serial chain on the store's blocking call:

| Step (20k messages) | Time | Note |
| --- | --- | --- |
| blob writes | 40.0 s | zstd + temp file + `fsync` + rename, ~2 ms each, one at a time |
| store merge | 8.7 s | ~10 statements per message, most prepared anew each time |
| IMAP fetch | 6.3 s | the connection idles while the batch is stored |
| MIME parsing | 2.8 s | on the store thread |
| id resolution | 2.0 s | `get_message` (an `IMMEDIATE` transaction + full row + tags) per message |
| index add | 1.1 s | Tantivy, batched commits — not a problem |
| blob re-check | 0.7 s | one `stat` per message after the merge |
| **total** | **62.9 s** | ≈ 318 messages/s |

The incremental sync (200 new Inbox messages) spent most of its time not
on the new mail but in the flag resync: it loaded **every stored summary**
of every folder (`message_summaries`: four queries per message incl. Gmail
/ Exchange extras) to build the UID → flags map — 0.37 s at 20k, ~2 s at
100k, growing with the mailbox — plus 0.4–0.6 s of serial blob writes.

A first version of the cheaper moved-row lookup (below) made things worse:
SQLite walked the folder's date index (`messages_folder_date`) to avoid
the `ORDER BY` sort, so every message scanned its folder again (15 s at
20k, 4.7 ms per message on a 6k folder). The final query keeps the
planner on the `Message-ID` index, and a unit test pins the plan.

## Decisions

### Sync performance (budgets unchanged)

- **Overlapping pipeline** (`nail-core/src/pipeline.rs`, used by
  `sync::store_batches`): batches go through three steps that overlap —
  **fetch** (`BatchSource`, one `UID FETCH` on the connection), **prepare**
  (blocking pool: `ingest::prepare`) and **store** (`BatchSink`, one store
  call: `ingest_prepared` + the folder's checkpoint). Channels of depth 1
  between the steps bound memory (a few batches of ≤ 8 MB). Batches are
  stored and checkpointed strictly in order, so an interrupted pass still
  resumes after the last stored batch (existing regression test). A failing
  step stops the others; batches fetched before a fetch error are still
  stored; the error furthest down the pipeline is returned (it is what
  stopped the others). Tests: order + real overlap (storing batch 0 waits
  for the fetch of batch 1), fetch / prepare / store errors, a panic in
  prepare, no hang when a later step stops.
- **`ingest` in two steps** (`nail-core/src/ingest.rs`): `prepare` writes
  the blobs and parses the MIME of a batch on up to 8 scoped threads (≥ 4
  messages per thread; order kept) without touching the store;
  `ingest_prepared` does the rest. `ingest()` = both, so POP3, Gmail,
  Exchange, the outbox, imports and the demo seed get parallel blob writes
  too. Every blob keeps its `fsync` + atomic rename (durability unchanged);
  they now overlap instead of queueing.
- **Blob removal marks** (`BlobStore::removal_mark` / `removed_since`):
  counters of started / finished removals shared by every clone. The
  post-merge re-check of the blobs ("a garbage collection may have removed
  a blob between `put` and the row") runs only when a removal ran (or was
  running) since the batch was prepared — same safety, no `stat` per
  message. Test: a blob removed between prepare and merge is written again.
- **Lean sync lookups** (`nail-store`): `message_id_by_uid`,
  `message_exists`, `uidless_message_by_message_id` (the oldest UID-less row
  with that Message-ID in the folder — the moved-row adoption, same rule as
  before; `+folder_id` / `+date_sort` keep the planner on the Message-ID
  index, `EXPLAIN QUERY PLAN` test) and `folder_uid_flags` (UID, id, flags
  and tags of a folder in two queries) for the flag resync.
- **One cached upsert per message**: the message row, its subject key and
  (with `MessageMeta`) its threading headers / size are written by one
  `INSERT … ON CONFLICT` (two cached variants) instead of an upsert plus two
  `UPDATE`s; the remaining hot statements (`folder_account`, tags, body
  state, thread row / thread id, subject fallback, `body_state`) use
  `prepare_cached`. Semantics unchanged (store suite incl. upgrades green).
- **Coalesced `MailChanged`** (`sync::MailChangedThrottle`): while a
  folder's batches are stored, at most one event per 500 ms and one after
  the last batch (also when a step failed). A 100k initial sync sent ~2,000
  events before; clients reload the list on each. Unit test with synthetic
  times.
- Kept as they were: IMAP batch limits (50 messages / 8 MB — real servers),
  Tantivy commit policy, arrival processing (still only for mail in folders
  synced before), the at-rest encryption and index policies (sealing,
  encrypted index, decrypted-index opt-in all go through the same calls).

### Results (this machine: 32 threads, release builds, `run-perf.mjs --corpus standard`)

Background: the user's Vite dev servers, a `tauri dev` session, OneDrive
and a game client were running (idle); no builds in parallel. Before = the
binaries of commit fc2695a, measured in the same session.

| Metric | Before 1 | Before 2 | After 1 | After 2 | Budget |
| --- | --- | --- | --- | --- | --- |
| initial-sync-secs | 352.6 | 357.9 | 108.4 | 93.1 | 300 |
| incremental-sync-ms | 3,088.2 | 3,256.1 | 757.4 | 733.7 | 3,000 |
| search-p95-ms | 124.0 | 126.4 | 126.3 | 131.3 | 250 |
| list-page-p95-ms | 5.6 | 5.5 | 5.6 | 5.3 | 50 |
| cold-start-ms | 83.9 | 100.7 | 84.9 | 87.1 | 3,000 |
| peak-memory-mib | 129.6 | 124.1 | 105.0 | 105.7 | 800 |
| first-render-ms | 397.2 | 392.1 | 439.4 | 450.2 | 3,000 |
| scroll-frame-p95-ms | 16.7 | 16.7 | 16.7 | 16.7 | 20 |
| scroll-long-frames | 0 | 0 | 0 | 0 | 5 |

Initial sync ≈ 3.5× faster (~280 → ~1,000 messages/s; 64–69 % under
budget), incremental ≈ 4.2× faster (75 % under budget). At 100k the three
steps take about the same time each (fetch ≈ 73 s, prepare ≈ 65 s, store ≈
74 s of busy time, overlapped), so no single step dominates any more. The
CI tolerance (1.5) and every budget stay as they are.

### Deterministic plug-in reload tests

Two real races in `nail-plugins`' host caused the flakes: the dev-folder
watcher lived in the plug-in's slot and two overlapping reloads could drop
it (later changes were no longer seen), and a reload already in flight
could replace a slot the user had just disabled. Now watchers live in
their own map (removed only on uninstall), reloads of a host run one at a
time, the "disabled" check is repeated under the slot lock when the new
slot is inserted, and a reload's stop never turns `Disabled` into
`Stopped`. The host broadcasts a **reload notice** after every reload
(`PluginHost::subscribe_reloads` → `PluginReload { plugin_id, outcome:
Started | Failed | Invalid | SkippedDisabled, generation }`;
`Service::subscribe_plugin_reloads` in the core). The tests wait on these
notices plus the stored state with a 120 s bound instead of polling the
lossy event stream / log lines against 10–20 s deadlines; one test still
uses the real file watcher, a new one reloads through `PluginHost::reload`
(three concurrent reloads, an invalid manifest, the watcher surviving
concurrent restarts). Stress: 30/30 runs green while the core and plug-in
suites ran in parallel.

### demo-multi keeps the file keychain

`DemoMulti::service_options` keeps a secret store already in the options
(the `FileSecretStore` that `nail serve --seed demo-multi` passes) and
writes the demo account / DAV secrets into it (`multi_secrets()`); only
without one does it use the in-memory store (tests). Test: encryption at
rest under demo-multi, data key in the file keychain, restart on the same
keychain opens the data (not locked).

### Confirmed overwrite on save

`CreateBackup`, `CreateRecoveryKey`, `SavePrintPdf` and
`CreateDiagnosticsBundle` carry `overwrite: bool` (serde default false,
omitted when false, optional in TypeScript; `StartExport` already had
`options.overwrite`). The UI sets it only for a path the Save dialog
returned (the user confirmed "Replace?"); typing a path clears it. The
core writes such files under `<name>.nail-part` next to the target and
renames them into place (`nail_portability::files`: `check_target`,
`persist`, `write_file`): with `overwrite` the rename replaces atomically,
without it a no-replace rename keeps `conflict` (`portability-error-target-
exists`), also for a file that appeared while writing. PDF: the shell
writes the temp file, the core renames it. Tests in `nail-portability`,
the core (`m4c*.rs`), Vitest (wizards, print, recovery key) and
`desktop-m4c.spec.ts` (no `.nail-part` left, a typed existing path is
refused). `tempfile` moves from dev-dependencies to dependencies of
`nail-portability` (already in the workspace; no new crate).

## Consequences

- The M4 automated gate holds with headroom; plan section 9 records the
  numbers. Throughput is now bound by the mock server / fetch, preparation
  and the store about equally; further gains would need IMAP pipelining
  or fewer `fsync`s per blob, neither needed for the budgets.
- `ingest` callers outside the IMAP sync spawn up to 8 short-lived
  threads per batch (one message runs inline).
- A sync emits fewer `MailChanged` events; clients still get one per
  changed folder at the end of its batches.
- Playwright fixes found on the way: `desktop-m4c.spec.ts` clicks the
  footer's Close (the title bar has one too); `scheduling.spec.ts` matches
  delivered subjects case-sensitively ("Retro" also matched a demo
  message's "retrouve" whenever that row was rendered). Two other one-off
  failures under the full parallel run were blank pages with Chromium's
  `net::ERR_NO_BUFFER_SPACE` (the machine, not Nail); the next full run
  passed 117 / 117.
