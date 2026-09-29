# ADR 0005: IMAP client library, TLS stack and IMAP test server

Status: accepted · 2026-09-29 · M0 WP3 · the `async-imap` choice is superseded by ADR 0018 (M1a WP11)

## Context

M0 needs a read-only IMAP client (`crates/nail-proto-imap`): implicit TLS,
STARTTLS, plaintext for tests, `LOGIN` and `AUTHENTICATE PLAIN`, `LIST` with
SPECIAL-USE, `EXAMINE`, `UID FETCH` of flags/envelope/`BODY.PEEK[]`, and
incremental fetch with UIDVALIDITY/UIDNEXT. M1 adds IDLE, CONDSTORE/QRESYNC,
MOVE, flag writes and COMPRESS. The design lists `imap-codec`/`imap-next` or
`async-imap` as candidates.

## Options

| | `async-imap` 0.11.3 | `imap-next` 0.3.4 (+ `imap-codec` 2.0.0-alpha.9) |
| --- | --- | --- |
| Model | High-level async session API (`login`, `examine`, `uid_fetch`, `idle`, ...) over any `AsyncRead + AsyncWrite` | Sans-I/O protocol flows; the application drives I/O and builds typed commands |
| Runtime | Tokio via `runtime-tokio` feature | Tokio adapter (`imap-next` "stream") |
| Parser | `imap-proto` 0.16 | `imap-codec` (complete, fuzzed, typed for many extensions) |
| Maturity | Used in production by Delta Chat for years; releases on 0.x but stable API | Codec is excellent, but 2.0 is still alpha; client-side flows are young; the higher-level `imap-client` crate is thin |
| Extensions | IDLE, ID, QUOTA, METADATA, COMPRESS (feature), CONDSTORE select; anything else via raw commands | Typed CONDSTORE/QRESYNC/ENABLE/... in the codec |
| Effort for M0 | Low | Medium-high (write our own client state machine) |

## Decision

Use **`async-imap` 0.11** (`default-features = false`, `runtime-tokio`) for
M0, wrapped behind our own API (`ImapClient`, `RemoteFolder`,
`FetchedMessage`, `SyncState`, `ImapError`) so callers never see
`async-imap` types. Re-evaluate `imap-next`/`imap-codec` when M1 starts
QRESYNC work and `imap-codec` 2.0 is stable; the wrapper keeps that switch
local to this crate.

Supporting choices:

- **TLS:** `tokio-rustls` 0.26 with the **ring** crypto provider set
  explicitly (`builder_with_provider`), so the build needs no CMake/NASM on
  Windows and does not depend on which provider other crates enable.
  Certificates are verified with **`rustls-platform-verifier`** (Windows
  certificate store, as the design requires) by default;
  `TlsTrust::WebPkiRoots` (`webpki-roots`) and `TlsTrust::CustomRoots` (DER
  roots, for tests and later TOFU pinning) are available. TLS 1.2+.
- **Pre-authentication is handled by us, not `async-imap`:** greeting,
  `CAPABILITY` and `STARTTLS` run on the raw stream before `async-imap`
  takes over. That gives us the capability list before choosing the auth
  mechanism (`AUTH=PLAIN` vs `LOGIN`, `LOGINDISABLED`) and guarantees that
  bytes buffered before `STARTTLS` are discarded (response injection).
  `Security::StartTls` never falls back to plaintext;
  `Security::InsecurePlaintext` must be chosen explicitly.
- **Command runner instead of `async-imap`'s streaming helpers.** We found
  that `async-imap`'s `uid_fetch`/`uid_search`/`list` streams end *silently*
  when the server closes the connection and ignore the tagged `NO`/`BAD`
  status — a sync would store a truncated result as complete. `ImapClient`
  therefore sends `EXAMINE`, `LIST`, `UID SEARCH`, `UID FETCH`,
  `CAPABILITY`, `NOOP` and `LOGOUT` itself (`Session::run_command` +
  `read_response`) and checks the tagged completion: EOF becomes
  `ImapError::ConnectionLost`, `NO` becomes `ImapError::No`. After each fetch
  batch, UIDs the server did not return are re-checked with `UID SEARCH`;
  if they still exist the fetch fails. `async-imap` is still used for
  framing, parsing and `LOGIN`/`AUTHENTICATE`.
- **Incremental fetch:** `plan_fetch(path, Option<SyncState>)` examines the
  mailbox; a changed UIDVALIDITY (or no stored state) plans everything and
  sets `uid_validity_changed`; otherwise it plans `UID <uidnext>:*` and
  filters the result, because `n:*` always matches the highest UID (RFC 3501
  6.4.8). The UIDs are grouped into batches by `RFC822.SIZE`
  (`ImapConfig::fetch_batch`, default at most 50 messages and 8 MiB), and
  `fetch_batch` downloads one batch; `FetchPlan::state_after(i)` is the
  checkpoint after batch `i`. `fetch_new` does the same but collects every
  message in memory (tests and small mailboxes).
- **Timeouts:** commands must finish within `ImapConfig::timeout`, except
  body downloads: the stream has an idle timeout (no data for `timeout`
  fails the read), so a large `FETCH` over a slow link is not cut off while
  data flows.

### Test server: in-house mock (`crates/nail-testkit`)

No docker is available, so integration tests run against an in-process
IMAP4rev1 mock server (tokio, `127.0.0.1:0`) serving an `.eml` corpus. It
supports plaintext, implicit TLS and STARTTLS with a throw-away CA from
**`rcgen`** 0.14 (ring), `LOGIN`/`AUTHENTICATE PLAIN` with configurable
credentials, `LIST` with SPECIAL-USE, `SELECT`/`EXAMINE`,
`FETCH`/`UID FETCH`, `SEARCH`/`UID SEARCH`, and failure injection
(`disconnect_on`, `fail_on`, `disconnect_after`, `deny_mailboxes`). It is
deliberately small; real-server
integration tests (Dovecot, Stalwart) come with CI containers later.

## Consequences

- IDLE and COMPRESS are available in `async-imap` when M1 needs them;
  CONDSTORE/QRESYNC will need raw commands plus our own response handling
  (the runner above already supports that pattern) or a switch to
  `imap-codec`.
- `async-imap` pulls `imap-proto` and `chrono`; acceptable.
- ENVELOPE strings are exposed raw (encoded words undecoded); display data
  comes from `nail-mime` parsing the full message.
- `mail-parser` builds nested `message/rfc822` parts as a tree of `Message`
  values whose compiler-generated `Drop` recurses once per level; a hostile
  mail with thousands of levels overflowed the stack (a process abort, not
  a panic). `nail-mime` wraps every parse result in a guard that tears the
  tree down iteratively. Parsing itself is iterative in `mail-parser`.
