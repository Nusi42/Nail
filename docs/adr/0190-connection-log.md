# ADR 0190: Connection log, error details and the application log — safe by default

Status: accepted · 2026-10-08 · finding from real-account testing (a real IMAP account: "The folder INBOX could not be synchronized", no
log anywhere)

## Context

The installed app logged only to stderr, so a failing sync left nothing to look at: the status bar said "The folder
INBOX could not be synchronized." and neither the user nor a developer could see which command failed or what the
server answered. Requirement: a place that shows the communication Nail did, without any mail content and
without passwords, safe by default.

## Decisions

### One redactor (SSOT): `crates/nail-connlog`

- A new small crate `nail-connlog` owns every redaction rule. `redact.rs` holds the text rules that the diagnostics
  bundle used (`redact_secrets`, `redact_subjects`, `redact_user_paths`, address scanning, `REDACTED`); they moved
  there from `nail-portability::diagnostics`, whose `Redactor` now only keeps its per-bundle placeholder mapping on top
  of the shared scanner (`for_each_address`). The protocol rules (`imap.rs`, `smtp.rs`, `pop3.rs`, `sieve.rs`,
  `http.rs`) turn one command or response line into its logged form. Nothing else in the workspace redacts.
- What a logged line keeps: commands and their keywords, tags, UID sets, sequence numbers, flags, response status
  lines and their response codes and server texts, capabilities, folder names, sizes, `INTERNALDATE`, durations, byte
  counts, HTTP methods, hosts, paths and status codes, TLS version / cipher / certificate summary.
- What a logged line never keeps:
  - **Credentials** → `<redacted>`: `LOGIN` passwords, every `AUTHENTICATE` / `AUTH` payload and every client line
    of a running SASL exchange (also `AUTH LOGIN`'s base64 user / password lines), POP3 `PASS` / `APOP`,
    `Authorization` / `Cookie` / `Set-Cookie` / `X-AnchorMailbox` header values, OAuth token request and response
    fields (`access_token`, `refresh_token`, `id_token`, `code`, `code_verifier`, `client_secret`, `password`,
    `assertion`), and every occurrence of a secret the connection registered (`ConnTap::add_secret`: the password,
    app password or access token it logged in with; at least 4 bytes, shorter ones are covered by the protocol rules
    only). Secret-looking `key=value` pairs and `Bearer` / `Basic` values anywhere (the diagnostics rules).
  - **Content** → placeholders: literals and bodies become `{4211 bytes}`; quoted strings inside `FETCH` responses
    (envelope fields, subjects, names, addresses) become `<header>` (`INTERNALDATE` stays); search criteria values
    become `<search term>` (IMAP `SEARCH`, Graph `$search` / `$filter`, Gmail `q`); `DATA` / `APPEND` / `PUTSCRIPT`
    / `RETR` / `TOP` / `GETSCRIPT` payloads become their byte count; HTTP bodies are never logged, only their size —
    for 4xx / 5xx answers the error code and message of a JSON (`error.code`, `error.message`, `error`,
    `error_description`) or SOAP (`faultstring`, `ResponseCode`, `MessageText`) body are kept, cut to 300 characters.
  - **Addresses** are masked everywhere: `sam@example.net` → `s***@example.net` (SMTP `MAIL FROM` / `RCPT TO`, IMAP / POP3
    user names, server texts, URL paths). A user name that is not an address keeps its first character (`n***`).
- Lines are cut to `CONNECTION_LOG_LINE_BYTES` (1,000) with the remaining byte count.
- Unit tests per protocol use real-looking transcripts (LOGIN with literals, AUTHENTICATE PLAIN / XOAUTH2, SMTP AUTH
  LOGIN, HTTP Authorization headers, FETCH literals, ENVELOPE / BODYSTRUCTURE address lists); a proptest checks that
  no logged line contains a registered secret or one of the generated subjects.

### Recording: `WireTap` → per-scope rings in the core

- Protocol clients take an optional `WireTap` (cheap clone; `WireTap::off()` records nothing): IMAP, SMTP, POP3 and
  ManageSieve log at their own command / response layer (they know literals and SASL state, so no byte-stream
  sniffing); the HTTP clients (CalDAV / CardDAV, Gmail, Graph, EWS, the OAuth token endpoint, Autodiscover /
  autoconfig) log one request line and one response line through `HttpTap`. A connection gets an id (`#12`) and logs
  its TLS parameters, capabilities, duration and byte counts when it closes.
- Log scope: an account (`ConnectionLogScope::Account`), a PIM source without an account (`PimSource`), or `Setup`
  (discovery and test logins before an account exists). The core keeps one ring per scope of the last
  `CONNECTION_LOG_CAPACITY` (2,000) entries in memory. Nothing is written to disk by default.
- **Keep connection log on disk** (`ConnectionLogSettings.keep_on_disk`, off by default, app setting
  `connlog.settings`): the same redacted lines are appended to `<data-dir>/logs/connections/<scope>.log`, rotated at
  1 MiB with 2 older files (`.1`, `.2`). Turning it off deletes the folder. The rings are memory only either way.
- Contract: `GetConnectionLog { scope, after, limit }` → `ConnectionLog`, `ClearConnectionLog`,
  `ExportConnectionLog { scope, path }` (with a path: writes a `.txt` there, desktop only; without: returns the text
  for a browser download), `Get` / `SetConnectionLogSettings`, event `ConnectionLogChanged { scope, last_seq }`
  (no content; at most four per second per scope). One formatter (`ConnectionLogEntry::line`) makes copy, export and
  disk lines.
- Access: remote UI sessions need the `admin` grant for all of it (`required_access`); MCP has no tool, resource or
  prompt for it, and plug-ins no API.

### Error details

- `ErrorDetails` (`code`, `protocol`, `step`, `folder`, `command`, `server_text`, `detail`, `at`, `log_seq`) is
  attached to `SyncFailed`, `AccountStatus` and failed sends (`SendStatus`). `command` and `server_text` pass through
  the redactor; `server_text` is the server's tagged response text verbatim otherwise. `log_seq` is the connection log
  position at the failure ("Show in connection log").
- The last error per account and per folder is stored (`sync_errors`, schema v19) and survives restarts; a
  successful pass clears the account's, a successful folder sync the folder's. `ListSyncErrors` returns them with the
  messages a folder skipped (ADR 0191).
- The UI shows "Show details" next to every sync, account and send error; the details window shows the localized
  wrapper text, the fields and the raw server text (verbatim, monospace) and links to the connection log.

### The application log

- The binaries tee `tracing` (info by default, `NAIL_LOG` overrides) through `nail_core::logs`; every line now passes
  the same redactor (secrets, addresses, subjects) before it reaches the in-memory ring, the diagnostics bundle or the
  file. The installed app and `nail serve` with a real data directory write `<data-dir>/logs/app.log` (rotated at
  5 MiB, 2 older files); seeded / temporary runs too, in their temporary directory. `GetAppLog` returns the path and
  the recent lines; Settings → Diagnostics links it.

## Consequences

- A failing sync can be diagnosed after the fact: the error's details, the connection log and `app.log`.
- No mail content, credential or full address reaches the log, the export, the disk files or the bundle; the rules
  live in one crate with tests per protocol.
- Protocol crates depend on `nail-connlog`; it depends only on `nail-api`, `x509-cert` and `chrono`.
