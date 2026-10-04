# ADR 0016: Search — Tantivy index, query language, execution

Status: accepted · 2026-09-29 · M1a WP13 (`crates/nail-search`)

(Numbered with a package suffix because four packages ran in parallel; the
integrator may renumber.)

## Context

The design asks for one query language used everywhere (search bar, saved
searches, rules, MCP): `from: to: subject: has:attachment is:unread tag:
account: folder: before: after: size>`, free text, quotes, boolean
operators and parentheses, with precise error positions for the search
bar. Tantivy was chosen in ADR 0014. Some criteria (flags, tags, folder)
change all the time and live in the store; text lives in the index.

## Decisions

### Index (`SearchIndex`)

- Tantivy 0.26 (ADR 0014), on disk in `<data dir>/search/` (constant
  `INDEX_DIR_NAME`) or in RAM for tests and demos.
- One document per message: `id` (raw, stored), `account` (raw, for
  dropping an account), and text fields `from`, `to`, `cc` (`Name <addr>`),
  `subject`, `body` (the decoded text part, or the HTML part's text),
  `filename` (attachment names). `IndexDocument::from_parsed` /
  `from_raw` build it with `nail-mime`.
- Tokenizer `nail`: simple tokenizer (splits on non-alphanumerics, so
  `from:ada` finds `ada@example.org`), long-token removal, lowercase and
  ASCII folding (`Müller` ↔ `muller`). No stemming: mail is multilingual and
  stemming the wrong language hurts more than it helps.
- **Commit batching:** writes are staged and committed after
  `max_pending` changes (default 1000) or `max_delay` (default 2 s) since
  the first staged change, checked on each write and by `commit_if_due`
  (for a timer in the core). Every search first commits staged changes, so
  results are never stale. Replacing a document is delete-by-id + add in
  the same batch.
- A version file in the index directory; another version or an unreadable
  index is deleted and recreated, and `was_rebuilt()` tells the core to
  reindex from the blob store (the index is derived data).

### Query language (`parse_query`)

Hand-written lexer + recursive-descent parser (no parser generator: the
grammar is small, and hand-written code gives the exact error spans the UI
underlines). Grammar and field list are documented in
`nail_search::query`. Choices:

- `AND` / `OR` / `NOT` only in upper case (lower-case words are text, as
  in Gmail); juxtaposition is AND; AND binds tighter than OR; `-x` is
  `NOT x` (a lone `-` is text).
- Fields are case-insensitive. Beyond the design's list: `cc:`, `body:`,
  `filename:`, and `is:` values `read`, `unflagged`, `answered`,
  `unanswered`, `draft` (aliases `seen`, `starred`, `replied`).
- `before:D` is strictly before local midnight of D; `after:D` is on or
  after it (so `after:2026-09-01 before:2026-10-01` is September).
  Relative ages: `h`, `d`, `w`, `m` (30 days), `y` (365 days); `after:7d`
  = the last 7 days.
- `size>` / `>=` / `<` / `<=` with `k` / `m` / `g` (1024-based, optional
  `b`).
- A `word:` whose prefix is letters only must be a known field
  (`unknown-field` otherwise — quote to search for such text); prefixes with
  digits or symbols are text (`10:30`).
- Errors use the contract's `SearchError` kinds with UTF-16 offsets. The
  AST keeps byte spans. `Display` prints a canonical form that parses back
  to the same AST (property-tested with `proptest`, together with "never
  panics", "errors are in bounds" and "term spans point at the term").

### Execution

`compile` turns the AST into a store `MessageFilter`
(`nail_store::MessageFilter`, ADR 0015): each full-text term — or all
full-text terms directly under one AND / OR, merged into one Tantivy
query — runs against the index and becomes an id set; everything else maps
to store predicates. The store evaluates the whole boolean expression in
one SQL query, so `NOT`, `OR` and mixed terms mean exactly what they say
and flags are always current. Terms with several tokens are phrase queries
per field; free text searches all fields (subject boosted ×2, from ×1.5).
Relevance = sum of the positive full-text scores; date order is the
default (newest first). `account:` accepts an account id, name or address,
a virtual account passed in the `SearchContext`, or `all`. The search
scope (selected account / folder, virtual ones resolved by the core) is
combined with AND.

Alternatives considered: indexing flags / folders in Tantivy (fast, but
every flag change would rewrite documents and the index would lag the
store), and translating everything to Tantivy's own query parser (no
control over syntax or error positions).

## Consequences

- The core resolves `SearchQuery.account_id` / `folder_id` (virtual ones
  included) into a `SearchScope`, fills `SearchContext` (now, UTC offset,
  virtual accounts), and answers parse errors with
  `Response::InvalidQuery` (`run_request` separates them from engine
  errors). `CheckQuery` is `parse_query` alone.
- Messages must be indexed when their body is stored and removed when they
  are deleted; the index lags the store only until the next commit or
  search.
- Headers-only messages match only on what was indexed; the server-side
  IMAP SEARCH fallback from the design is not part of M1a.
- Id sets of very common words are materialized; fine for personal mail
  volumes (100k messages), revisit with a custom collector if needed.
