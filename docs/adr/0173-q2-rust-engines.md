# ADR 0173: Q2 WP-rust-engines — one definition for the engines' shared code

- Status: accepted (package of wave Q2; the integrator renumbers)
- Date: 2026-10-08
- Context: ADR 0171 (Q2 contract: SSOT / ODR / DRY, duplicate check, ratchet, ownership)

## Context

The Q2 contract recorded 404 findings owned by WP-rust-engines: copies in
the protocol crates, the mocks and their tests, and every copy shared by a
core crate and an engine crate (`scripts/lib/duplicates/owners.mjs`: a
cross-package clone is never split, so this package owns it). The protocol
clients had grown by copying each other: four TLS / stream / line-reading
layers (IMAP, POP3, SMTP, ManageSieve), three HTTP API clients with their
own retry, `Retry-After`, bounded-body and error-class code (Gmail, Graph,
EWS, plus DAV and Exchange's own HTTP), two XML trees, a dozen mock servers
with their own listener / journal / fault plumbing, eleven civil-date
copies, and test helpers pasted between test files.

## Decision

Every fact and every piece of logic has one definition; the other places
import it. Behaviour does not change except where noted below.

### Where shared code lives

| Home | What | Who uses it |
| --- | --- | --- |
| `nail-net` (features `http`, `io`, `deflate`, `tls`, `xml`; featureless `ports`, `status`, `trust`, `wire`) | streams that delegate to a plain / TLS variant (`delegate_stream_enum!`, `forward_async_*!`), bounded line reading (`read_line_limited`, `read_response_line`, `MAX_LINE`, `MAX_MESSAGE`), timeouts, disconnect detection, idle timeouts, DEFLATE streams, the rustls configuration and handshake (`TlsTrust`, `tls::connect`), endpoint / SASL builder macros, login `Debug` helpers, line-client error codes, IANA ports, HTTP status numbers, IMAP sequence sets; HTTP: URL policy, origins (`origin_url`), retry policy and `Retries`, `Retry-After`, bodies, bounded reads, transport-error classes and error macros (`client_error_classes!`, `engine_error_from!`, `transport_errors!`), bearer / Basic / OAuth2 sign-in (`ApiAuth`), `ApiClient` (one JSON API client for Gmail and Graph), header pairs, answer texts, the adaptive poll timer (`poll::RoundTimer`, `options_timer!`); XML: one namespace-aware tree with depth / element limits (DAV, EWS, mocks) | every protocol crate, the mocks, `nail-core` (redirect limit), `nail-sieve` |
| `nail-util` (no dependencies) | `time` (civil dates, RFC 3339, `DAYS_PER_YEAR`, weekdays, last weekday of a month, fixture instants), `ical_text` (UTC stamps, durations, signs), `percent`, `batch` (batched writes with a one-by-one fallback), `sieve_lex` (the Sieve lexer and the ManageSieve line tokenizer), `named` (case-insensitive pair lookups), `mime_text` (byte search, header sections and unfolding, line offsets, base64 wrapping, backslash escapes, `Bcc` headers), `markup` (HTML escaping), `redact`, `expiring` (the keyrings' unlock caches) | engines and core crates alike |
| `nail-api` (contract) | `AccountConfig::new`, `ScheduleAgent` (RFC 6638), rule placement reasons (`reason_at_condition` / `reason_at_action`), `authority_host`, `Weekday::from_monday`, `MAIN_WINDOW`, catalog checks (`catalog_ids`, `missing_ids`, `bad_ids`) | DAV + iTIP, Sieve + Exchange, the desktop shell + config, every catalog test |
| `nail-ical` | `attendee` (ATTENDEE parameter keywords, reading and writing), `alarm::all_alarms`, `time::value_instant` | `nail-itip`, the core's reminders |
| `nail-exchange` | `exchange_mail_engine!` / `exchange_pim_engine!` (the engine trait glue of Graph and EWS), `mapping::categories::merge`, shared HTTP header access | Graph, EWS |
| `nail-pkcs11` | `PublicKeyMaterial::same_public_key`, one `Module::private_op` (sign / decrypt / ECDH as `KeyOp`) | OpenPGP, S/MIME, the soft token |
| `nail-testkit` | `catalog` (error-text tests, `error_texts_test!`), `exchange` (the demo tenants' shared expectations), `tokens::FakeTokens`, `TEST_TIMEOUT` | engine and core tests |
| `nail-testkit-net` | `server` (`MockListener`, `Session`, `Journal`, `Faults`, control macros), `api_mock` (`MockApi`, `MockService`, `MockReply`, `Served`, `injected_error!`), shared SASL verification and answers | every mock server |

Test-only sharing inside one crate uses a `tests/common` module (DAV,
`nail-testkit`, iTIP — the iTIP English renderer is included into the unit
tests with `#[path]`) or a helper next to the tests.

### Before / after (duplicate check, `--owner WP-rust-engines`)

| Check | Before | After |
| --- | ---: | ---: |
| clone | 349 | 0 |
| const-value | 31 | 0 |
| magic-number | 24 | 0 |
| **total** | **404** | **0** |

`node scripts/check-duplicates.mjs --strict --owner WP-rust-engines` passes;
the whole repository reports 0 new findings (1,661 recorded ones left, all
owned by the other packages; WP-rust-core's own count fell from 584 to 573
because the civil-date copies in its crates went too). Net change: 4,161 lines added, 9,398 removed.

### Behaviour notes (deliberate, small)

- Gmail now also checks that paging / upload URLs stay on the API origin
  (the check Graph had; `ApiClient`).
- Malformed iCalendar durations / UTC stamps, percent escapes (`%+1`) and
  ManageSieve lines are read by one parser each; edge cases that differed
  between the copies now behave like the stricter one.
- The SMTP client accepts `TlsTrust::WebPkiRoots` like the other clients;
  ManageSieve's `TlsTrust` gained it too (it is the shared type now).
- Diagnostics (English, never shown without a `localized` text) are worded
  once: "response line too long", "`<op>` without the new item id",
  "literal marker not at the end".
- Graph and EWS merge categories the same way: blank tags and tags that are
  also removed are not added.
- `PRODID` constants are named per use (`FREEBUSY_PRODID`, `EXPORT_PRODID`);
  values unchanged.
- Mock servers only: the HTTP mock records an empty query as none and
  accepts the axum default body size; the ManageSieve mock tokenizes `(` /
  `)` as atoms.

## Consequences

- A new protocol client starts from `nail-net` (streams, TLS, HTTP, XML) and
  a new mock from `nail-testkit-net::server` / `api_mock`; copying a
  sibling crate's plumbing fails the duplicate check.
- Core crates now depend on `nail-util` (civil dates, percent-decoding,
  shared texts) and `nail-core` on `nail-net` (HTTP facts).
- Follow-ups for the integrator: see the package report (CLAUDE.md layout
  lines for `nail-net` / `nail-util`, the duplicate check's `//` truncation
  of string constants, merge order with WP-rust-core's edits of the same
  core files).
