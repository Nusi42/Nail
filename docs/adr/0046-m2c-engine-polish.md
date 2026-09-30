# ADR 0046 (M2c WP53): Engine polish — localized Rust messages, sync policies, server search, Markdown bodies, fixed MCP port, headless `nail mcp`

Status: accepted · 2026-09-30 · M2c WP53 (parallel package; the integrator
renumbers)

## Context

Plan section 7, WP53, on the contract of ADR 0043: sync policy `bodies`
(headers only / recent N days) with on-demand bodies, an IMAP `SEARCH`
fallback for messages without a local body, a separate replay connection,
`text/markdown` alternatives in `nail-mime`, every user-facing message
built in `nail-config`, `nail-mcp`, the CLI and `nail-rules` as a Fluent id
+ arguments, a fixed MCP port, a headless `nail mcp` with a single-instance
lock, and `RuleFromMessage` over the full header list. WP53 owns
`nail-proto-imap`, `nail-mime`, `nail-store`, `nail-config`, `nail-mcp`,
`nail-cli`, `nail-rules`, `nail-search` and `locales/{en,de}/core.ftl`; the
core is wired by WP55.

## Decisions

### Localized messages

- **Rendering in Rust** (`nail_config::l10n`): `Catalog` is a concurrent
  `fluent-bundle` over **all** `locales/<lang>/*.ftl` files, embedded at
  build time (`EN_FILES` / `DE_FILES`; a test fails when a catalog file is
  added without a line there). Lookups fall back per key: requested
  language → English → the id. `english()` renders diagnostics;
  `Catalog::for_locale` / `locale_from_env` (`NAIL_LOCALE`, `LC_ALL`,
  `LC_MESSAGES`, `LANG`) serve the CLI. `Catalog::text(id, &[(name,
  value)])` has the shape of the core's `strings::message_with`, so WP55
  can move `nail-core/src/strings.rs` over. `missing_message_ids(dir,
  prefixes)` is test support: every producer crate has a test that every
  id it spells out exists in English.
- **`nail-config`: the catalog is the single source.** Every
  `ConfigError` is built from a `LocalizedMessage` (`localized`, boxed to
  keep `Result<_, ConfigError>` small — clippy `result_large_err`); its
  English `message` is the English rendering. So the diagnostic and the
  UI text cannot drift, and English lives only in `core.ftl` (≈240
  `config-*` ids, grouped by file). `to_event` / `to_issue` / the
  `ApiError` conversion fill `localized` (the `ApiError` one adds `file`,
  `line`, `column`; an issue located in another file adds `in_file`).
  `EditError::Invalid` now holds a `LocalizedMessage` (public API change;
  nothing outside `nail-config` matched on it); merge conflicts and backup
  errors carry `localized` too.
- Arguments are language-neutral: `what` is a property (`` `key` ``) or a
  node's argument (`` `node` #1 ``), `found` is the value as written in
  KDL (`"text"`, `5`, `#true`), lists are joined with `, `. Consequences
  for the English wording (tests adapted): "found the string \"x\"" →
  "found \"x\""; "`x` needs the action as argument 2" → "`x` needs a value
  as argument 2"; choice lists read "one of a, b, c"; "is given twice" →
  "is set twice". The English texts are otherwise unchanged.
- **`nail-rules`** keeps its English diagnostics in Rust and adds
  `localized` to `RuleIssue` and `ConditionError` (`rule-issue-*`; the
  location is an argument: `trigger`, `action` 1-based, `condition` a path
  like `2.1` or `root`). Tests assert that the English rendering equals the
  diagnostic word for word.
- **`nail-mcp`**: agents keep reading English (tool descriptions, JSON-RPC
  errors, the refusal `hint`: MCP clients are programs and models, not the
  UI). User-facing texts are `mcp-core-*` (`messages`): admin errors
  (`ApiError::localized`), start errors of a fixed port, the bridge's
  errors (`BridgeError::localized`), and **audit details**: the store
  keeps the English detail (no `localized` column; ADR 0043 allows none in
  parallel packages), and `messages::audit_detail` maps every detail the
  server writes (token refusals, the rate-limit summary, `Denial`s, API
  errors as `code: message`) back to its message, round-trip tested
  against the `Display` forms.
- **CLI**: errors and notices go through `messages::render_error`
  (localized first message in the error chain — `CliError`, bridge or lock
  errors — then "Cause:" lines with the English detail); `main` renders to
  stderr and exits 1. Help and logs stay English (ADR 0009).

### Sync policies and on-demand bodies

- `nail-proto-imap`: `BodyFetch` (`Full`, `HeadersOnly`, `Since(day)`;
  `from_policy(&BodyPolicy, now)`), `ImapDate`,
  `ImapClient::fetch_batch_with(uids, BodyFetch)` (header blocks via
  `BODY.PEEK[HEADER]`, `FetchedMessage::headers_only`; for "recent N days"
  one `UID SEARCH UID <set> SINCE <day>` splits the batch), and
  `fetch_body(path, uid)` for one message on demand. Planning and
  checkpointing (`plan_fetch`, `state_after`) are unchanged, so the core's
  sync loop only swaps `fetch_batch` for `fetch_batch_with`. `RFC822.SIZE`
  stays the full size.
- `nail-store` (no migration; v7's `body_state`): a body-less message
  stores its **header block as its blob** (threading, rules on headers and
  `header_fields` keep working). `merge_with_body_state(messages, state)`
  is the sync merge per policy — a headers-only resync never replaces a
  body downloaded before (blob, state and size are kept);
  `attach_body` (→ `on-demand`, returns the header blob for collection),
  `detach_body` (a policy that keeps fewer bodies), `body_fetch_targets`
  (folder path + UID of headers-only messages), `bodies_before` (bodies
  that aged out of "recent N days"), `headers_only_count`.
- **Separate replay connection**: needs nothing at the protocol level —
  it is a second `ImapClient::connect` with the same configuration (WP55
  decides when the core opens it).

### Server-side search

`nail_proto_imap::SearchKey` (a tree: text keys, flags, keywords, dates,
sizes, `Not` / `Or` / `And`) with `ImapClient::search_messages(path,
key)`: quoted ASCII strings, literals for anything else with `CHARSET
UTF-8`, keywords checked as atoms; a top-level `And` is sent as a plain
key sequence (servers that do not take a lone parenthesized list).
`nail_search::imap::server_search(query, now, utc_offset)` maps the query
language: text fields → `TEXT` / `FROM` / … ; `before:` / `after:` →
`SENTBEFORE` / `SENTSINCE` (days, like the local search on `Date:`);
`size` comparisons exactly; `tag:` → `KEYWORD`. Terms the server cannot
express **widen** their branch (dropped from `AND`, an `OR` or `NOT`
containing one becomes "all"), and `exact` says so, so the fallback never
misses a hit; `account:` / `folder:` are the caller's folder scope.
`needs_server(query)`: only free text, `body:` and `filename:` need the
server. `nail-search` now depends on `nail-proto-imap` (no cycle).

### Markdown alternatives (`nail-mime`)

`mail-parser` treats every text subtype but plain/html as an attachment.
A `text/markdown` / `text/x-markdown` part that is the message's own body
or a child of a `multipart/alternative` (and not `Content-Disposition:
attachment`) is a body alternative: never listed as an attachment (the
attachment numbering stays the parser's, so `attachment_content` indices
do not shift); its source is the text body when there is no `text/plain`;
without HTML it is rendered with `comrak` (already a workspace dependency,
BSD-2-Clause; GFM extensions, raw HTML not passed through) and sanitized
like any mail HTML. Attached `.md` files stay attachments.

### Fixed MCP port

`nail_mcp::configured_port(&McpSettings, override)`: `--mcp-port` wins,
else the settings' port, else 0 (free port). `ServerOptions` is unchanged
(the core builds it as a literal). A fixed port that is taken is an error
explained by `messages::start_error` (`mcp-core-port-in-use`), not a
silent move to another port: clients configured with the URL would
otherwise talk to nothing. `Store::mcp_settings()` reads the settings.
`nail serve` uses the settings now (`--mcp-port` became optional).

### Headless `nail mcp` and the single-instance lock

`nail_store::DataDirLock`: an exclusive OS lock (`std::fs::File::try_lock`,
no dependency) on `<data-dir>/nail.lock`, held for the process lifetime and
released by the OS on a crash; who holds it is written to
`nail.lock.info` (the locked file itself cannot be read on Windows). `nail
serve` takes it. `nail mcp`: `--url` → bridge; else a live endpoint file
(loopback TCP probe) → bridge; else (unless `--no-start`) take the lock,
open the core headless (engines on), start the MCP server on the
configured port with the endpoint file, bridge stdio to it, and stop
everything when the client closes stdin. A lock held by a process without
an MCP server is reported (`cli-mcp-instance-without-mcp`) instead of
opening the data twice. New flags: `--config-dir`, `--no-start`,
`--mcp-port`.

### Rules from messages

`rule_from_message` moved to `nail-rules/src/suggest.rs` and reads the
full header list: `List-Id` (named after the list's description, falling
back to the id; encoded words are not guessed), else the posting address
of `List-Post` / `X-Mailing-List` / `Mailing-List` as a `header …
contains` condition, else `from equals`, else the subject.

### Also

`border-collapse` joined the supported theme properties (the in-progress
default theme of WP54 uses it).

## Consequences

- WP55 wires: `fetch_batch_with` + `merge_with_body_state` per folder
  policy in the sync, `fetch_body` + `attach_body` in `GetMessage` /
  `FetchMessageBodies`, `bodies_before` + `detach_body` when "recent" mail
  ages out, the server search (`needs_server`, `server_search`,
  `search_messages` per folder in scope, `SearchResults.server`), the
  replay connection, `configured_port` in the desktop shell's MCP start,
  the `DataDirLock` in the desktop app, `McpAuditEntry.localized` through
  `messages::audit_detail`, `RuleIssue.localized` into `ConfigIssue` /
  `ApiError`, and `strings.rs` → `nail_config::l10n`.
- German catalogs stay empty; every message falls back to English per key.
- The accounts reference still marks `bodies` / `days` "not active yet"
  until WP55 wires the sync.
