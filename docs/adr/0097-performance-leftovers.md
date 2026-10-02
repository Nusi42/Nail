# ADR 0097: 100k-message benchmark, budgets in CI, Exchange / Gmail leftovers

Status: accepted · 2026-10-02 · M4c WP123

## Context

Plan section 9, WP123: a 100k-message benchmark with budgets checked in CI
(initial sync from the mock IMAP server, list scrolling, search latency,
cold start, memory) and fixes for what fails; the M3 leftovers in the API
engines (Graph upload sessions over 4 MB, send as / on behalf, Gmail thread
ids, sync policies for Exchange folders, moves between mailboxes /
accounts). Contract: ADR 0094 (`nail-bench` interfaces, `budgets.json`,
`send_with_progress`, `SendingMailbox::of`, `RemoteMessage.thread_id`).
WP123 owns `crates/nail-{proto-graph,proto-gmail,proto-ews,exchange,bench}`
and `e2e/perf/`; core / UI / store changes are requests to WP125.

## Decisions

### Benchmark (`crates/nail-bench`, `e2e/perf/`)

- **Corpus** (`corpus::Corpus`): deterministic (rand `StdRng` from the
  spec's seed; same spec = same bytes), 40 folders (`INBOX`, `Sent`,
  `Archive`, `Junk`, `Projects/…`, `Lists/…`, `Clients/…`; Inbox 30 %,
  Zipf for the rest), a sender pool where few people write most mail, 40 %
  replies threaded to recent messages of the folder (`In-Reply-To`,
  `References`, `Re:`), 60 % `multipart/alternative` HTML, bodies
  exponential around 4 kB, 10 % with one or two base64 attachments
  (log-normal around 60 kB, `CorpusSpec::mean_attachment_bytes`, new with a
  serde default), list headers, `Received` / DKIM-like headers, dates over
  three years, realistic flags. Markers: a word in exactly one message, a
  phrase in ~2 %. 100k messages ≈ 1.67 GB of MIME.
- **Mock server in a child process** (`nail-bench mock-imap`, JSON ready
  line, `deliver <n>` on stdin): the corpus' memory does not count against
  the core's peak working set. `--in-process` for quick runs and tests.
- **Metrics** through `nail_core::Service` like any client: initial sync
  (`sync_account`, no poll interval), incremental sync of 200 new Inbox
  messages (new metric `incremental-sync-ms`), 60 `ListMessages` pages
  (top, second and random pages of the three largest folders), 12 typical
  queries × 3 rounds (`search-p95-ms`), cold start (shutdown, reopen with
  the same secret store, folders + first Inbox page; the reopened index
  must find the rare word, else the run fails), peak working set
  (`GetProcessMemoryInfo`, `VmHWM` on Linux; `#![allow(unsafe_code)]` in
  one function). Sanity checks fail the run (message counts, the rare word
  found once). Afterwards the account is handed over as a local account so
  `nail serve` shows the data without a server.
- **Playwright** (`e2e/perf/list.perf.ts`, own config, not in `npm run
  e2e`): new metric `first-render-ms` (navigation → first rows painted,
  median of three cold contexts), `scroll-frame-p95-ms` and
  `scroll-long-frames` from `requestAnimationFrame` intervals during 240
  wheel steps; a Chrome trace of the scroll is saved and its long renderer
  tasks reported. Full Chromium in new headless mode (`channel:
  "chromium"`), one worker, no retries.
- **Budgets as data** (`e2e/perf/budgets.json`): one budget per metric (a
  test checks it), `ci-tolerance` 1.5 applied with `--tolerant` /
  `NAIL_PERF_TOLERANT=1` in CI (shared runners). New budgets:
  `incremental-sync-ms` 3000, `first-render-ms` 3000. `nail-bench check`
  merges `report-rust.json` and `report-ui.json`; `run-perf.mjs` runs the
  whole job; everything goes to the build directory.
- `nail-bench` output is developer output (English, not localized), like
  the scripts.

### Results (developer machine, 32 threads, release builds)

| Metric | Before store fix | With store fix | Budget |
| --- | --- | --- | --- |
| initial-sync-secs | > 1500 (stopped at 66k messages after 15 min; 430 → 43 msg/s) | 292.9 | 300 |
| incremental-sync-ms (200) | — | 2908.7 | 3000 |
| search-p95-ms | — | 114.1 | 250 |
| list-page-p95-ms | — | 4.7 | 50 |
| cold-start-ms | — | 53.0 | 3000 |
| peak-memory-mib | — | 123.1 | 800 |
| first-render-ms | — | 406.9 | 3000 |
| scroll-frame-p95-ms | — | 18.3 | 20 |
| scroll-long-frames | — | 0 | 5 |

The "store fix" is the threading query below (measured with the patch
applied to a private build snapshot only; the repository is unchanged).
Initial and incremental sync are at the budget edge: further fixes are
requested from WP125 (see Consequences).

### Exchange / Gmail leftovers

- **Graph upload sessions** (`nail_proto_graph::upload`): a message whose
  MIME (with the envelope-only recipients as `Bcc`) exceeds 4 MB in base64
  is split with `mail-parser` into a JSON draft (subject, HTML or text
  body, from / sender / to / cc / bcc / reply-to, `internetMessageId`,
  importance, read receipt, `In-Reply-To` / `References` as
  `PidTagInReplyToId` / `PidTagInternetReferences`, up to five `X-`
  headers) plus attachments: up to 3 MB posted as `fileAttachment`, larger
  through `createUploadSession` in 320 KiB-multiple ranges
  (`EngineOptions::upload_chunk`, default 3.2 MB) to the upload URL **without
  the bearer token** (`GraphClient::put_upload_range`: https or loopback
  only, `nextExpectedRanges` honoured, transient errors retried), then
  `POST messages/{id}/send`. A failure deletes the draft. Progress per range
  through `send_with_progress`. Signed / encrypted messages over 4 MB are
  `TooLarge` (a rebuilt message would break the signature). Appends over
  4 MB use the same parts in the target folder with `PR_MESSAGE_FLAGS`
  (not shown as unsent).
- **Send as / on behalf**: `SendingMailbox::of(From, Sender, own)`. Graph:
  send as = `/users/{mailbox}/sendMail` (or the draft created there), on
  behalf = the user's mailbox with `from` + `sender`; the own address comes
  from the account (`with_own_address`) or one `GET /me`. EWS: `t:From`
  on `CreateItem`, the Sent Items copy in the shared mailbox for send as,
  in the user's for on behalf; `ErrorSendAsDenied` → `forbidden`.
- **Thread ids**: Graph and EWS report the conversation id, Gmail the
  `threadId` on every listed, history and appended message (tests pin it);
  WP125 passes them into `MessageMeta.remote_thread_id`.
- **Sync policies for Exchange folders** (`nail_exchange::policy`): the
  account's `sync` rules apply by path / role as for IMAP; per message
  `body_fetch(policy, internal_date, now)` → full MIME or headers only.
  Engines: `GraphEngine::fetch_headers` (`internetMessageHeaders`, sent mail
  falls back to the MIME's header section) and `EwsEngine::fetch_headers`
  (`PR_TRANSPORT_MESSAGE_HEADERS` `0x007D`, same fallback). Listings stay
  complete (the delta cursor covers the folder).
- **Moves between mailboxes / accounts**: `GraphEngine::move_to_mailbox`
  (Graph's `move` within a mailbox, else copy + permanent delete),
  `EwsEngine::move_to_mailbox` (cross-mailbox `MoveItem`, falling back on
  `ErrorCrossMailboxMoveCopy` to a MIME copy + hard delete), and
  `nail_exchange::transfer::transfer` for any pair of API mailboxes
  (Exchange mailbox ↔ Exchange mailbox ↔ Gmail account): fetch, idempotent
  append, delete; replay-safe.
- **Mocks**: Graph — JSON drafts, `fileAttachment` posts, upload sessions on
  `/upload/{id}` (refuses bearer tokens, checks ranges and 320 KiB
  multiples), `send`, the 4 MB request limit (`413`),
  `internetMessageHeaders`, `SentMail.via`; EWS — `t:From` checks
  (`ErrorSendAsDenied`), `SentMessage.{from, saved_in}`, `0x007D`,
  `allow_cross_mailbox_moves(false)`.

### Libraries

No new crates. New edges inside the workspace lock: `nail-bench` →
`nail-proto-imap`, `base64`, `tracing-subscriber`, `mail-parser` (dev);
`nail-proto-graph` → `mail-parser` (Apache-2.0 OR MIT).

## Consequences

- WP125 / owners: apply the store threading fix, investigate the
  incremental sync cost, wire the engine additions (trait methods
  `fetch_headers` / `move_to_mailbox` or the inherent ones), add the CI
  `perf` job, document large Exchange attachments and headers-only
  Exchange folders for users.
- The budgets for initial and incremental sync have little headroom; CI
  runs with the 1.5 tolerance. Budgets are tightened, not loosened,
  without an ADR.

## Results in the repository (WP125)

With the threading fix in `nail-store` (ADR 0099), `node e2e/perf/run-perf.mjs
--corpus standard` on the development machine (release build, 100,000
messages / 1.67 GB, sync at about 349 messages per second, data directory
1.13 GB) passes every budget:

| Metric | Value | Budget |
| --- | --- | --- |
| initial-sync-secs | 286.4 | 300 |
| incremental-sync-ms (200 new) | 2732.3 | 3000 |
| search-p95-ms | 107.7 | 250 |
| list-page-p95-ms | 3.7 | 50 |
| cold-start-ms | 69.4 | 3000 |
| peak-memory-mib | 125.1 | 800 |
| first-render-ms | 413.4 | 3000 |
| scroll-frame-p95-ms | 18.1 | 20 |
| scroll-long-frames | 0 | 5 |


## Post-M4c (ADR 0101)

The M4c verify measured 348.8 s / 354.6 s initial and 3,343 / 3,339 ms
incremental on the same benchmark (over budget; the numbers above had no
real headroom). ADR 0101 profiles the sync and overlaps fetch / blob writes
+ MIME parsing / store, makes the sync lookups and the flag resync lean and
caches the hot statements. Same machine, two strict runs each, budgets and
CI tolerance unchanged:

| Metric | Before (fc2695a) | After (ADR 0101) | Budget |
| --- | --- | --- | --- |
| initial-sync-secs | 352.6 / 357.9 | 108.4 / 93.1 | 300 |
| incremental-sync-ms (200 new) | 3,088.2 / 3,256.1 | 757.4 / 733.7 | 3,000 |
| search-p95-ms | 124.0 / 126.4 | 126.3 / 131.3 | 250 |
| list-page-p95-ms | 5.6 / 5.5 | 5.6 / 5.3 | 50 |
| cold-start-ms | 83.9 / 100.7 | 84.9 / 87.1 | 3,000 |
| peak-memory-mib | 129.6 / 124.1 | 105.0 / 105.7 | 800 |
| first-render-ms | 397.2 / 392.1 | 439.4 / 450.2 | 3,000 |
| scroll-frame-p95-ms | 16.7 / 16.7 | 16.7 / 16.7 | 20 |
| scroll-long-frames | 0 / 0 | 0 / 0 | 5 |
