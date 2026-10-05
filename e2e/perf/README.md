# Performance benchmark (M4c WP123)

`budgets.json` holds the budgets of the 100k-message benchmark (contract
ADR 0094, package ADR 0097). Two harnesses check them:

- **Rust** (`crates/nail-bench`, binary `nail-bench`): generates the corpus
  (`CorpusSpec::standard()` = 100,000 messages in 40 folders, threads,
  HTML, attachments, fixed seed; `small()` = 2,000 for quick runs), serves
  it from `nail-testkit`'s mock IMAP server in a child process (its memory
  does not count), and measures through `nail_core::Service`:
  `initial-sync-secs`, `incremental-sync-ms` (200 new Inbox messages),
  `list-page-p95-ms`, `thread-page-p95-ms` (a page of the conversation list,
  `ListThreads`, the default list since P2; 10 ms since P3, when pages became
  index ranges of the per-folder `thread_activity` table), `search-p95-ms` (typical queries: sender, subject,
  body word, phrase, attachments, unread, a word that occurs once …),
  `cold-start-ms` (reopen + first Inbox page) and `peak-memory-mib`.
  `cargo run -p nail-bench --release -- --corpus small` (add `--tolerant`
  for the CI factor, `--no-check` to only measure).
- **Playwright** (`list.perf.ts`, own config `playwright.perf.config.ts`,
  not part of `npm run e2e`): `nail serve` on the data the Rust run left
  behind, then `first-render-ms` (navigation until the first list rows are
  painted, median of three cold page loads), `scroll-frame-p95-ms` and
  `scroll-long-frames` (frames over 50 ms) while wheel-scrolling deep into
  the virtualized Inbox; a Chrome trace of the scroll is saved with its
  long renderer tasks.

`node e2e/perf/run-perf.mjs [--corpus standard|small] [--tolerant]` runs
both and `nail-bench check`, which merges `report-rust.json` and
`report-ui.json` into `report.json` and exits non-zero when a budget is
exceeded. Prerequisites: `cargo build --release -p nail-cli -p nail-bench`
and `npm run build -w ui`.

Metric names are `nail_bench::MetricName` (kebab-case in the JSON); every
one has exactly one budget (a test in nail-bench checks it). `ci-tolerance`
multiplies every budget in CI (shared runners). Corpora, data, reports and
traces go to `<checkout>/.nail-build/perf/<corpus>` (gitignored;
`NAIL_BUILD_DIR`, `NAIL_PERF_WORK`), never into tracked files.
