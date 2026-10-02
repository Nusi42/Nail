# Performance benchmark (M4c WP123)

`budgets.json` holds the budgets of the 100k-message benchmark (contract
ADR 0094). Two harnesses check them:

- **Rust** (`crates/nail-bench`, binary `nail-bench`): generates the corpus
  (`CorpusSpec::standard()` = 100,000 messages; `small()` for quick runs),
  serves it from the in-process mock IMAP server, and measures the initial
  sync, list paging, search latency, cold start of the core and peak
  memory: `cargo run -p nail-bench --release -- --corpus small`.
- **Playwright** (scenarios in this folder, WP123): list scrolling frame
  budget from performance traces and cold start to the first painted list.

Metric names are `nail_bench::MetricName` (kebab-case in the JSON); every
one has a budget here (a test in nail-bench checks it). Corpora and traces
go to the build directory, never into the repo.
