# ADR 0018: The full IMAP engine (IDLE, CONDSTORE/QRESYNC, writes, OAuth2)

Status: accepted · 2026-09-29 · M1a WP11 (`crates/nail-proto-imap`,
`crates/nail-testkit`). Supersedes the `async-imap` choice of ADR 0005;
its TLS, pre-authentication and timeout decisions stay.

## Context

M0 built a read-only IMAP client on `async-imap` 0.11 with our own command
runner on top (ADR 0005). M1 needs IDLE with reconnects, CONDSTORE /
QRESYNC resynchronization with `VANISHED`, flag / move / append / folder
writes with UIDPLUS results, SASL `XOAUTH2` / `OAUTHBEARER`,
`COMPRESS=DEFLATE` and `ID`, plus a mock server that can exercise every
extension *and* every fallback without a network.

`async-imap` got in the way of almost each of these:

- `Session::compress` consumes the session and drops it when the server
  answers `NO`; the stream type changes, so the client type would have to
  become generic or boxed.
- Its `authenticate` cannot answer an OAuth error challenge the way
  RFC 7628 requires, and it has no hook for `LITERAL+` / synchronizing
  literals in `APPEND` or 8-bit `LOGIN` arguments.
- Its IDLE handle owns the session and uses its own stop-token type; the
  M0 runner already had to bypass its streaming helpers (ADR 0005).
- What we still used from it was framing and the `imap-proto` parser.

## Decision

### Protocol layer: in-house over `imap-proto`

`async-imap` is removed. `nail-proto-imap` depends on **`imap-proto`
0.16** (the parser `async-imap` used; it parses `VANISHED`,
`HIGHESTMODSEQ`, `MODSEQ`, `APPENDUID`, `COPYUID`, `ENABLED`, `ID`) and
frames responses itself (`conn.rs`, ~150 lines):

- Lines and `{n}` literals are assembled in a buffer owned by the
  connection, so `Connection::read` is **cancel-safe** — it can race a
  timer or a stop signal (IDLE) without losing bytes.
- Responses `imap-proto` rejects are handled leniently: tagged and `+`
  lines are recognized by a fallback parser, unknown untagged responses
  are logged and ignored. Unknown response codes (`[MODIFIED …]`,
  `[ALREADYEXISTS]`, `[TOOBIG]`) stay in the response text; error mapping
  reads them from there (`ImapError::response_code`).
- Commands are built from parts (text + literals): synchronizing literals
  wait for `+` (and accept a tagged `NO` instead, e.g. `[TOOBIG]`),
  `LITERAL+` / `LITERAL-` literals do not.
- After a transient failure (timeout, lost connection) the client marks
  itself broken and every later command fails fast with `ConnectionLost`:
  a timed-out command's answer may still arrive, so the connection is out
  of step.

### Connection setup

`ImapClient::connect`: transport and pre-auth exactly as ADR 0005, then
login, then (each only when offered, each refusal non-fatal):

1. **`COMPRESS=DEFLATE`** (`ImapConfig::compress`, default on) via
   **`async-compression` 0.4** (`tokio`, `deflate`; already in the lock
   file). A sync flush after every command; an end of stream in the middle
   of a DEFLATE block is reported as `ConnectionLost`, not as a
   decompression error.
2. **`ID`** (`ImapConfig::client_id`, default `name=Nail`,
   `version=<crate version>`; `None` sends nothing). The server's answer is
   kept (`server_id()`).
3. **`ENABLE QRESYNC`** (implies CONDSTORE), else `ENABLE CONDSTORE`.

### Authentication

`Credentials { username, secret: Secret::Password | Secret::AccessToken }`.
`AuthMechanism::Auto` picks `PLAIN` / `LOGIN` for passwords and
`OAUTHBEARER` (preferred, RFC 7628) or `XOAUTH2` for tokens; explicit
mechanisms must match the secret kind. SASL-IR is used when advertised,
otherwise the initial response follows the `+`. On an OAuth failure the
server's base64 JSON challenge is answered (`""` for XOAUTH2, `AQ==` for
OAUTHBEARER) and its content is kept in the `AuthFailed` message. The
client never logs command lines of `LOGIN` / `AUTHENTICATE`. Tokens come
from the caller (the core's `AccessTokenProvider`, ADR 0012); the crate
does no OAuth itself.

### Change tracking and IDLE

Untagged `EXISTS`, `EXPUNGE`, `VANISHED` and flag `FETCH`es that no
command consumed are recorded as `MailboxChange`s (`take_changes()`,
`poll()` = `NOOP` + take). They are *hints*: the reliable reaction is an
incremental sync (`plan_fetch` for new mail, `resync_flags` for flags and
expunges). Our own STORE / MOVE / EXPUNGE results are not recorded.

- `ImapClient::idle(max, stop)`: `IDLE` on the selected mailbox until a
  change, `max`, or `stop`; then `DONE`. The read idle timeout is switched
  off while waiting (the server may be silent for many minutes) and
  restored afterwards. `IDLE_REISSUE` = 29 minutes (RFC 2177).
- `MailboxWatcher::spawn(config, Arc<dyn CredentialProvider>,
  WatchConfig)`: a **dedicated connection** per watched mailbox (INBOX by
  default) that EXAMINEs it and stays in IDLE (re-issued every
  `idle_reissue`), or polls with `NOOP` every `poll_interval` (default
  2 minutes) when the server lacks IDLE or `use_idle` is off. It emits
  `WatchEvent::Connected` (sync everything now), `Changed(changes)`, and
  `Disconnected { error, retry_in }`, and reconnects with exponential
  backoff (`reconnect_min` 1 s doubling to `reconnect_max` 5 min; reset
  after a successful connection). The watcher implements
  `futures::Stream`. `CredentialProvider::credentials(refresh)` is asked
  for every connection, with `refresh = true` after `AuthFailed`, so
  expired OAuth tokens are renewed. Dropping or `stop()`ping the watcher
  ends IDLE and logs out.

### Flag and expunge resync

`resync_flags(path, ResyncState { uid_validity, highest_modseq }, &local)`
where `local` maps known UIDs to their local `MessageFlags` returns the
known messages whose flags differ and the known UIDs that vanished — the
same answer by three methods:

| Server | Method | Commands |
| --- | --- | --- |
| QRESYNC enabled | `Qresync` | one `EXAMINE … (QRESYNC (uidvalidity modseq known-uids))`: `VANISHED (EARLIER)` + changed `FETCH`es |
| CONDSTORE only | `Condstore` | `EXAMINE … (CONDSTORE)`, `UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE m)`, `UID SEARCH ALL` for expunges |
| neither, `NOMODSEQ`, or no stored mod-sequence | `FullScan` | `UID FETCH 1:* (UID FLAGS)` and a diff (carry-over from M0: sync picks up flag changes and deletions) |

A changed UIDVALIDITY is reported (`uid_validity_changed`) with no
changes; the caller refetches. `next_state` is stored for the next run.
`FolderStatus` gained `highest_modseq`; `FetchedMessage` gained `modseq`
and `deleted` (`\Deleted`), and fetches include `MODSEQ` on CONDSTORE
servers. `SyncState` / `plan_fetch` are unchanged.

### Writes

All writes select the mailbox read-write themselves (reusing the current
selection) and chunk long UID sets (500 per command).

- `store_flags(path, uids, Add|Remove|Replace, &[Flag], unchanged_since)`:
  `UID STORE` (non-silent, so the resulting flags and mod-sequences are
  returned). `unchanged_since` needs CONDSTORE (else `Unsupported`) and
  reports `[MODIFIED]` UIDs as `conflicts`. `Flag::keyword` validates IMAP
  atoms (`InvalidArgument` otherwise; nothing is sent).
  `apply_flag_change(path, uids, &nail_api::FlagChange)` maps the API
  change to at most one `+FLAGS` and one `-FLAGS`.
- `move_messages`: `UID MOVE` (COPYUID from the untagged `OK`), else
  `UID COPY` + `+FLAGS.SILENT (\Deleted)` + `UID EXPUNGE` (UIDPLUS). Without
  UIDPLUS a plain `EXPUNGE` is used **only if no other message is marked
  `\Deleted`** (checked with `UID SEARCH DELETED`); otherwise the moved
  messages stay marked (`MoveMethod::CopyAndMarkDeleted`) — expunging a
  user's other deleted mail would be data loss. `expunge_messages` follows
  the same rules.
- `copy_messages` (`UID COPY`, COPYUID mapping).
- `append(path, raw, flags, internal_date)`: checks `APPENDLIMIT` before
  uploading, maps `[TOOBIG]` to `ImapError::TooLarge`, returns
  `APPENDUID` (UIDVALIDITY + UID) when the server has UIDPLUS. The deadline
  grows with the message size (timeout + size / 16 KiB/s).
- `create_folder` / `rename_folder` / `delete_folder` / `set_subscribed`
  take server paths; `folder_path(parent, delimiter, display_name)` builds
  one with the new `encode_modified_utf7`. Renaming or deleting the
  selected mailbox leaves it first (`UNSELECT`, or `EXAMINE` + `CLOSE`,
  which never expunges).

### Errors

New variants `Unsupported`, `InvalidArgument`, `TooLarge`; `ImapError::Tls`
now maps to `ErrorCode::Tls` (ADR 0012). `NO` maps by its RFC 5530 code:
`ALREADYEXISTS` / `INUSE` → `conflict`, `TOOBIG` / `LIMIT` / `OVERQUOTA` →
`too_large`, `AUTHENTICATIONFAILED` / `EXPIRED` → `auth_failed`,
`UNAVAILABLE` / `SERVERBUG` → `unavailable`, `CANNOT` /
`PRIVACYREQUIRED` / `CONTACTADMIN` → `rejected`, everything else
(`NONEXISTENT`, `TRYCREATE`, `NOPERM`, plain) → `not_found`, as before.
`From<ImapError> for nail_api::engine::EngineError` keeps the code and
uses `is_transient()` as `retryable`.

### Mock server (`nail-testkit`)

- Each session keeps a **view** of its selected mailbox. When the
  `MailStore` changes (another session or a test helper), the session
  reports the difference like a real server — `EXISTS`, `EXPUNGE` (or
  `VANISHED` once QRESYNC is enabled) and flag `FETCH`es — before and after
  each command (never `EXPUNGE` during sequence-number `FETCH` / `STORE` /
  `SEARCH` / `COPY` / `MOVE`), and immediately while in **IDLE** (a
  `watch` channel in the store wakes idling sessions).
- Per-message **MODSEQ** and a per-mailbox `HIGHESTMODSEQ`, expunge log
  for `VANISHED (EARLIER)`; `ENABLE`, `SELECT/EXAMINE (CONDSTORE)` and
  `(QRESYNC (…))`, `FETCH` `MODSEQ` / `CHANGEDSINCE` / `VANISHED`,
  `STORE` `UNCHANGEDSINCE` → `[MODIFIED]`, `STATUS`.
- `STORE`, `COPY`, `MOVE`, `EXPUNGE` / `UID EXPUNGE`, `CLOSE` (expunges),
  `APPEND` (flags, date, `APPENDUID`, `TRYCREATE`, `APPENDLIMIT` with
  `[TOOBIG]` before a synchronizing literal), `CREATE` / `DELETE` /
  `RENAME` / `SUBSCRIBE`, `ID`, `COMPRESS DEFLATE`, `AUTHENTICATE XOAUTH2`
  / `OAUTHBEARER` with a configurable, runtime-replaceable token and the
  JSON error challenge, SASL-IR on/off.
- `MockCapabilities` switches every extension (default: all on;
  `imap4rev1()`: none), so tests cover each fallback.
- Test helpers: `MailStore::{add_message, remove_message, set_flags,
  add_flags, remove_flags, flags, highest_modseq, rename_mailbox,
  copy_messages, store_flags}`; `MockImapServer::{commands, command_count,
  clear_commands, client_ids, compressed_sessions, idle_sessions,
  wait_for_idle, set_oauth_token}`.
- Fault injection: existing `disconnect_on` / `fail_on` /
  `disconnect_after` / `deny_mailboxes`, plus `drop_idle_connections`
  (closes every connection in IDLE without `BYE`) and
  `delay_responses(command, delay)` (a slow server). Connections close
  gracefully (TLS `close_notify`, FIN) after `LOGOUT`, so clients read the
  final response instead of a reset.

## Consequences

- One parser (`imap-proto`) instead of a client library; `async-imap`,
  `async-channel`, `chrono`, `stop-token` and friends left the lock file.
  The IMAP state machine is ours to maintain; it is small and fully
  covered by tests against the mock (sessions, writes, resync, IDLE,
  watcher; about 105 client tests and 45 mock-server tests).
- `imap-proto` sees few releases. A switch to `imap-codec` 2.x (once
  stable) stays possible and is local to `conn.rs` and the response
  handlers.
- WP14 (core) should: run one `MailboxWatcher` per account on INBOX (and
  configured folders) and trigger `plan_fetch` + `resync_flags` on
  `Connected` / `Changed`; store `ResyncState` next to `SyncState` (store
  v2 has a per-folder `highestmodseq`); replay queued writes with
  `apply_flag_change`, `move_messages`, `expunge_messages` and `append`,
  treating `EngineError::retryable` as "keep queued"; skip the Sent copy
  for servers that save it themselves.
- The mock is still not a real server (no `BYE` on UIDVALIDITY change, no
  `\Recent`, simple SEARCH, INBOX cannot be renamed). Real-server tests
  (Dovecot, Stalwart) remain a CI follow-up (ADR 0005).
