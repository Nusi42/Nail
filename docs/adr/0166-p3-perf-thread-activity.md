# ADR 0166: P3 WP-perf — thread-activity maintenance and conversation pages

Status: accepted · 2026-10-05 · P3 WP-perf (contract ADR 0164; integration ADR 0169)

## Context

`ListThreads` grouped every message of the listed folders by thread for each
page (P2, ADR 0154 deferred the fix): 33–40 ms p95 per page on the standard
corpus, against 3–5 ms for a message page. The contract (ADR 0164) added the
per-folder table `thread_activity` (schema v17, backfilled), its page and
count queries, and set the budget `thread-page-p95-ms` ≤ 10 ms. This package
keeps the table current, switches the pages to it and meets the budget.

## Decisions

### 1. Triggers keep the table (schema v18)

`nail_store::thread::activity::MAINTENANCE_SQL` (migration 18) creates five
triggers and rebuilds the table once from the v17 backfill query
(`REBUILD_SELECT`):

- `thread_activity_insert` / `_delete` on `messages`, and
  `thread_activity_update` on `UPDATE OF folder_id, thread_id, date_sort`
  (only when one of them changes): recompute the (folder, thread) row of the
  old and the new values from the covering index `messages_folder_thread`
  (count and newest date; no row for a Drafts folder or without messages),
  and, when the folder is a Sent folder, the order key of the thread's rows in
  every folder of that account (the user's reply moves the Inbox row up).
- `thread_activity_folder_change` on `UPDATE OF role, account_id` of
  `folders`: rebuild the folder's rows, recompute the order keys of the old and
  the new account.
- `thread_activity_folder_delete` after a Sent folder is deleted: recompute
  its account's order keys (the folder's own rows go with `ON DELETE
  CASCADE`, its messages' delete triggers find no folder any more).

Why triggers and not code in the write paths: every path that touches
messages — ingest (insert with a NULL thread, then the thread update), JWZ
merges (`UPDATE … SET thread_id`), re-threading, moves and deletes through
the queue, expunges, folder and account removal (cascades fire the delete
trigger) — is covered without touching `mail.rs` / `queue.rs` (WP-actions-core
edits those), and a future write path cannot forget it. Flags, tags and read
state do not touch the table. A later migration that rebuilds `messages` or
`folders` must create the triggers again (noted in the module docs).

### 2. Pages read the table

- One real folder: `PAGE_SQL` (index range of `thread_activity_order`,
  `LIMIT / OFFSET`) and `COUNT_SQL`.
- Several folders (a virtual folder) or a Drafts member: each member's first
  `offset + limit` keys (`PAGE_SQL`; Drafts: `SINGLES_PAGE_SQL` over
  `messages_folder_date`), merged in Rust by `(last_date DESC, key DESC)`,
  a thread in two members once with its newer key; total =
  `COUNT(DISTINCT thread_id)` over the members plus the drafts. The merge is
  exact (a thread whose newest member row is not among a member's first
  `offset + limit` rows has that many threads ahead of it).
- The page's conversations are read in **one** query (`listed_page_sql`:
  `thread_id IN (page keys)` through `messages_thread_id`) instead of one per
  row, and only the message each row selects gets its tags / Gmail / Exchange
  data (`attach_tags`; the row's other messages only give counts, senders and
  dates). This was the larger half of the remaining cost: 83 messages × 3
  look-ups per page before.

### 3. Grants and one deviation

- The order key counts every Sent folder of the folder's account. A list
  whose joining Sent folders a remote grant narrowed (`activity::covers` is
  false) falls back to the P2 grouping query (`GROUPING_PAGE_SQL`, kept), so
  the order never reveals hidden replies. Such lists are as fast as before
  (P2), not faster.
- The grouping query joined the Sent folders of *every* listed account to
  every listed thread; the table (as the contract defines it) joins the
  folder's own account only. In a unified list over several accounts, a
  thread received in account A and answered from account B is ordered by A's
  activity; its row (messages, counts, newest date) is unchanged. Accepted:
  rare, and only the position differs.

### 4. Tests

- `thread/activity_tests.rs`: random raw-SQL writes (inserts in
  transactions, deletes, moves across accounts, date and thread changes, JWZ
  merges, role changes, folder and account removal, a folder changing account)
  — 4 seeds × 400 steps, the table equals `REBUILD_SELECT` after each; random
  store writes (ingest with Message-ID links, queued moves / deletes, expunges,
  date changes, flags, role changes, `rethread_all`, folder and account
  removal) — the table equals the backfill and every page (each folder, two
  unified lists, every offset, page sizes 3 and 50) equals the grouping query;
  the v18 rebuild of a stale table; the narrowed-Sent fallback; the documented
  cross-account deviation.
- `EXPLAIN QUERY PLAN` tests: the page, count, multi-folder count and Drafts
  page are index ranges without a temp B-tree; the trigger statements (with
  the row's values bound) never scan `messages` or `thread_activity` and read
  `messages_folder_thread`; the batched page read uses `messages_thread_id`;
  the grouping fallback keeps its covering index.

### 5. Budget

`e2e/perf/budgets.json`: `thread-page-p95-ms` 50 → 10. Measured with
`nail-bench run --corpus standard` (release build, built with `-j 4`) on the
same machine, which other work shared (search times swung by 2× between runs).
"Final" = table + one-query page + one store hop in the core
(`Service::list_conversations` checks the folder, builds the scope and lists
the page in one `Store::call`); the last column is the full perf job
(`node e2e/perf/run-perf.mjs --corpus standard`, Rust + Playwright) one step
before that last core change:

| Metric | Before | Table only | Final, run A | Final, run B | Full job |
| --- | --- | --- | --- | --- | --- |
| thread-page-p95-ms | 39.5 | 12.3 | 7.6 | 7.9 | 9.2 |
| list-page-p95-ms | 4.9 | 5.9 | 3.9 | 4.6 | 5.0 |
| initial-sync-secs | 106.7 | 128.2 | 117.9 | 91.3 | 98.2 |
| incremental-sync-ms | 753.9 | 1107.5 | 769.6 | 883.6 | 828.9 |
| search-p95-ms | 125.5 | 142.4 | 319.1 | 126.8 | 123.1 |
| cold-start-ms | 130.8 | 178.5 | 210.4 | 148.0 | 130.3 |
| peak-memory-mib | 115.2 | 118.9 | 115.2 | 115.8 | 118.6 |
| scroll-frame-p95-ms | – | – | – | – | 16.7 |
| scroll-long-frames | – | – | – | – | 0 |
| first-render-ms | – | – | – | – | 267.2 |

The store alone (`Store::list_conversations`, 40 pages spread over each
list): Inbox (17,829 conversations) p95 2.7 ms, Archive 1.2 ms, Sent 1.1 ms;
with the table but one query per row, Archive was 7.3 ms. The rest of the
benchmark's figure is the service around it while the engines still run after
the sync (`list-page-p95-ms` shows the same overhead). The triggers do not show
in the initial sync beyond the machine's noise. Run A's search spike did not
repeat; search does not touch this package's code.

## Consequences

- A conversation page costs a few index look-ups regardless of folder size.
- Every message write pays a few index look-ups more (two row recomputes, and
  for Sent folders an update of the thread's rows in the account).
- Narrowed remote lists keep P2's speed.
