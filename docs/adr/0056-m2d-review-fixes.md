# ADR 0056: M2d review fixes: header-only plug-in search, stopped plug-ins stop at once, bounded plug-in logs, Nail's own OpenPGP texts, attributed OS notifications

Status: accepted · 2026-09-30 · M2d review

## Context

The M2d reviews confirmed four defects and three smaller gaps in the wave
integrated by ADR 0055:

- **Search probing.** `nail.mail.search` ran the plug-in's query on the
  local index, which holds body text and attachment names (and, for
  accounts opted in to decrypted indexing, the decrypted text of
  end-to-end encrypted mail). A plug-in with only `mail.read` ("not the
  message text") could confirm body words with `body:"…"` or free text,
  including words of encrypted mail, bypassing `mail.read.body` and ADR
  0055's "decrypted text never reaches a plug-in". Remote sessions and MCP
  already dropped encrypted hits in that case; plug-ins did not.
- **Stopping.** Disabling or uninstalling a plug-in set `alive = false`
  and queued `Stop` behind everything else on the thread's channel: up to
  64 resolved calls and due timers still ran, each with a full CPU budget
  and full API access (only `notify`, `showDialog`, `updatePanel` checked
  `alive`); the gate stayed in place. A reviewer measured 19 new fetches
  over 12 s after Disable.
- **Log flood.** Only the plug-in's own lines counted against the
  per-call cap; refused calls (`api`) and host lines did not. One turn of
  refused calls wrote 25,809 lines; the core's writer queue was unbounded
  (one SQLite transaction and one broadcast per line), so memory grew and
  the 2,000-line retention was overwritten.
- **English in the UI.** `nail-pgp` put English sentences it writes
  itself ("not an address: …", "not an Autocrypt setup message", …) into
  the `detail` argument of `pgp-error-invalid` / `pgp-error-crypto`, which
  is meant for library texts only; a German UI could not translate them
  and the text spoke of invalid OpenPGP data for a mistyped address.
  `LookupKeys` in the core had a plain English error without a
  `localized` text.
- Smaller: OS notifications of plug-ins put the "from <plug-in>"
  attribution after up to 2,000 characters of plug-in text (with line
  breaks), so a toast could look like Nail's own; the M2 gate was recorded
  as passed before M2d's reviews and verify; `nail plugin dev` printed
  English details ("not an http(s) URL", "no answer") and dropped the real
  connection error.

## Decision

- **Header-only search without `mail.read.body`.**
  `PluginServices::mail_search` gets a `body_scope` (the gate's
  `mail.read` ∩ `mail.read.body` accounts). The core searches the query
  as given only in those accounts. Elsewhere it rewrites the parsed query
  (`header_only`): free text becomes `from: OR to: OR cc: OR subject:`,
  and a query with `body:` or `filename:` terms skips those accounts. A
  query that names message text where no account allows it is refused
  with `forbidden` (`plugin-host-api-denied`, permission
  `mail.read.body`).
- **Encrypted mail hidden while the index holds decrypted text.** When
  any account opted in to decrypted indexing, plug-in searches drop every
  hit whose message is end-to-end encrypted (`message_is_encrypted`), with
  or without `mail.read.body`, and scan further pages (at most 5,000 hits
  per account) so hidden hits do not shorten the answer, like
  `remote_search`.
- **A stopped plug-in stops.** The interrupt handler also fires when
  `alive` is false (not counted as a CPU-budget crash), except while
  `deactivate` runs. The thread checks `alive` before and after every
  message: once stopped it runs `deactivate` (250 ms) and ends; queued
  resolutions, timers and commands never run. Every API op except `log`,
  `fatal` and `clearTimer` refuses with `unavailable` once stopped, and
  `stop` replaces the slot's gate with an empty one.
- **Bounded logs.** Every line a plug-in causes (its own, refused calls,
  host lines about its trees and rejections) counts against
  `max_log_lines_per_call` (200) and a new per-plug-in token bucket,
  `HostLimits::max_log_lines_per_minute` (600 a minute, burst 600).
  Dropped lines are counted and reported by one `plugin-host-log-dropped`
  line as soon as the rate allows, also when no further turn comes (the
  thread wakes for it). The core's writer queue is bounded (4,096 lines);
  a full queue drops and counts lines per plug-in, reported the same way
  when there is room.
- **`nail_pgp::Problem`.** What `nail-pgp` detects itself is an
  `Error::Problem(Problem)` with its own `pgp-problem-*` id and named
  arguments (address, domain, attribute, key, days) in
  `core-crypto.ftl`; `Crypto` / `Invalid` / `Unsupported` carry library
  texts only (the only `detail` of a problem is the base64 decoder's text
  for Autocrypt keydata). Import issues of a problem use its own text.
  `LookupKeys` refuses a typed non-address with
  `pgp-problem-not-an-address`.
- **OS notifications**: title `plugin-notification-os-title`
  ("{plugin}: {title}"), the plug-in's name first; line breaks in the
  plug-in's title and body are collapsed.
- **`nail plugin dev`**: `cli-plugin-dev-not-http` for a URL that is not
  http(s); a failed connection shows the WebSocket library's error as the
  `detail`.
- **Plan status**: the M2 automated gate reads "suites green at
  integration; gate pending the M2d verify and the CI jobs".

## Consequences

- Plug-ins with `mail.read` only can still find mail by sender,
  recipients, subject, flags, tags, dates, sizes and attachments present;
  searches that need message text need `mail.read.body` (documented in
  `userDocs/en/plugins/permissions.md` and the SDK's `search` doc; the
  SDK's `createTestHost` refuses `body:` / `filename:` without it).
- With decrypted indexing on, a plug-in never finds encrypted mail by
  search, not even by its headers (conservative, as for remote sessions).
- A plug-in that logs more than 600 lines a minute loses lines (counted);
  dev mode's per-call debug lines count too.
- `deactivate` can only log.
- Tests: `nail-plugins/tests/stop_and_logs.rs` (queued fetches after
  Disable, a busy turn interrupted by Uninstall, refused calls capped per
  call, the per-minute rate across timer turns), budget / log-rate unit
  tests; `nail-core` `plugin_host` unit tests (`header_only`, full log
  queue) and `tests/plugins.rs`
  `search_needs_mail_read_body_for_text_and_hides_decrypted_mail`, the OS
  notification texts; `nail-pgp` problem ids / catalog / address tests,
  core `LookupKeys` texts; `nail plugin dev` connect failures; SDK test
  host search.
