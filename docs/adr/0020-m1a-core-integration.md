# ADR 0020: M1a core integration — engines, replay, outbox, virtual accounts

Status: accepted · 2026-09-29 · M1a WP14-core

## Context

WP11–WP13 and WP14-UI delivered the IMAP engine, SMTP / POP3 / OAuth2 /
discovery, store v2 with threading, drafts, the outbox table and search,
and the UI, each against the WP10 contract (ADR 0012). WP14-core wires them
into `nail-core` so every M1a request is answered and the Playwright
scenarios of plan WP14 run against the real core. This ADR records how, and
where the result deviates from the plan or the design.

## Decisions

### Accounts and secrets

- `accounts.kdl` is loaded by the core's account manager
  (`nail-core/src/accounts.rs`), edited only through
  `nail_config::AccountsDocument` (format-preserving, ADR 0013) and saved
  atomically. A watcher applies hand edits; an invalid file keeps the last
  valid accounts, is reported as `ConfigError` (also to clients that connect
  later) and **blocks edits through requests** until it is fixed, so a
  half-written file is never overwritten.
- Before saving, the core fills in what the reader would (display name =
  id, one identity from the address, identities owned by the account), so a
  saved configuration reads back unchanged.
- Secrets go to `SecretStore` (`nail-auth`'s keychain by default,
  `ServiceOptions::secrets` overrides it; tests, demos and e2e use the
  in-memory store and never touch the OS keychain). OAuth2 sign-ins
  (`StartOAuth`) keep their tokens in memory under the flow id for 10
  minutes; `AddAccount` / `UpdateAccount` with `SecretValue::OAuth2` store
  the refresh token through `TokenManager::store`. The token manager is
  rebuilt only when the `oauth-provider` entries change (a rebuild drops
  cached access tokens). The browser is opened only when
  `ServiceOptions::open_browser` is set (desktop app, plain `nail serve`).
- The OAuth2 callback pages are the only user-visible text the core
  renders; they come from `locales/<lang>/app.ftl` (embedded, simple
  `key = text` lookup, per-key English fallback).

### Engines

One task per enabled account (`engine.rs`), started, restarted (changed
configuration) and stopped by reconciliation after every account change.

- **IMAP** (`imap_engine.rs`): one long-lived connection for sync and
  replay, plus a `MailboxWatcher` (own connection, IDLE or NOOP polling)
  per folder whose sync policy asks for IDLE (INBOX by default). Watcher
  events sync that folder; queued writes replay at once; "sync now" and the
  poll interval (default 5 minutes) run a full pass. A failed connection
  sets the account `offline` (or `auth-error`), emits `SyncFailed` and
  reconnects with a doubling backoff (`EngineTiming`). When a watcher loses
  its connection, the engine probes its own with `NOOP`, so an outage shows
  at once.
- **Passes** (`sync.rs`, extended from M0): fetch new mail per folder
  (batches, checkpoints), then `resync_flags` (QRESYNC / CONDSTORE / full
  scan) for flag changes and expunges of stored messages; `HIGHESTMODSEQ`
  is kept in the store's folder state. Messages marked `\Deleted` on the
  server are not shown. `NewMail` is emitted for new mail in folders that
  were synced before (not on the first sync).
- **POP3** (`pop3_engine.rs`): a scheduled fetch (default 10 minutes, and
  "sync now") that replays local writes first, downloads unknown `UIDL`s
  into a local Inbox, applies the leave-on-server policy
  (`nail_proto_pop3::plan_deletions`) and forgets vanished `UIDL`s. POP3
  accounts get local Inbox, Sent, Archive and Trash folders.
- Accounts registered with `Service::add_imap_account` (M0 API: tests,
  `--seed demo-imap`) keep the on-demand pass with a fresh connection, and
  replay after it.

### Message identity across moves

Message ids stay derived from account / folder / `UIDVALIDITY` / UID
(ADR 0009), and a queued move keeps the row's id while its UID becomes
unknown. The replayer stores the new UID from `COPYUID` when the server
reports it. Otherwise (or if the target folder syncs first) ingest adopts
the moved row: a fetched message whose UID is not stored yet but whose
Message-ID matches a row in that folder without UID takes over that row.
So a moved message is never shown twice, and its id survives the move.

### Replaying queued writes

`replay.rs` adapts executors to `nail_store::Replayer` and broadcasts
`OpStatus` (`in-flight`, `done`, `failed`, or `pending` with the error of a
retryable failure):

- IMAP: flags and tags with `UID STORE` (`apply_flag_change`), moves with
  `move_messages` (MOVE or COPY + `\Deleted` + expunge), deletes to Trash as
  moves, permanent deletes as `\Deleted` + expunge. A message moved again
  before its first move reached the server uses the UID the first replay
  learned. *ADR 0021 adds UIDVALIDITY checks, Message-ID lookups and
  idempotent cross-account moves.*
- Moves into another account append the raw message there (a short-lived
  connection to the target's IMAP server; accounts without one keep only
  the local copy) and then expunge it from the source. This is not atomic:
  a failure after the append can leave a copy in both accounts, never none.
- POP3 and local accounts: everything is local; a permanent delete of a
  downloaded POP3 message is recorded for "delete when deleted locally".
- `EngineError::retryable` keeps an operation queued (and ends the
  account's pass); anything else fails it, and clients refetch.

### POP3 bookkeeping: store schema v3

The leave-on-server policies need, per account, the downloaded `UIDL`s with
their download time, the local message and whether the user deleted it.
That is store state, so schema v3 adds a `pop3_uids` table
(`Store::pop3_state`, `pop3_record`, `pop3_mark_deleted`, `pop3_forget`) —
a small addition to WP13's store made by the integrator.

### Outbox and sending

- `SendDraft` schedules at `now + delay` (default `undo-send-delay`, 10 s)
  or `send_at`; `CancelSend` restores the draft (undo). A sender task
  sleeps until the next due entry (or a wake-up), submits through
  `nail_proto_smtp::SmtpTransport` (password from the keychain or OAuth2
  token with one forced refresh) and records the outcome: `sent`; retryable
  failures back to `scheduled` with a doubling delay (at most 5 attempts);
  permanent ones `failed`. Each change is a `SendStatus` event.
- M1a builds messages itself (`mime.rs`): one `text/plain; charset=utf-8`
  part in quoted-printable, RFC 2047 headers, `In-Reply-To` / `References`
  for replies, Bcc only in the envelope. `nail-compose` (M1b, WP21) replaces
  it.
- The Sent copy is appended to the account's Sent folder on a short-lived
  connection, unless `save-sent-copy=#false`, or, when unset, the provider
  files sent mail itself (Gmail, Outlook.com / Office 365 hosts). POP3
  accounts keep the copy in their local Sent folder. The copy is part of
  sending, not a queued operation. A reply marks the original answered
  through the operation queue. *Superseded by ADR 0021: the copy is stored
  locally at once and filed through the operation queue
  (`AppendMessage`).*

### Virtual accounts

- `ListAccounts`: "All accounts" (`all`, empty name) first, then the
  configured accounts in file order, local accounts (demo data) and
  registered accounts, then the user's virtual accounts. IMAP / POP3 rows
  in the store without configuration (an account removed by editing the
  file) are hidden; their mail stays until the account is removed through
  the UI.
- Virtual folders are computed (`virtuals.rs`): one per merged role, one
  per hand-made mapping, and every other member folder under a per-account
  node. Their ids are `<virtual account>/<key>` (real ids never contain
  `/`), so a folder id names its virtual account. Lists over a virtual
  folder collapse copies with the same Message-ID (`duplicates`).
- Moving into a virtual folder targets the member folder of the message's
  own account; searching and syncing a virtual account mean its members.
- Choosing the sender identity for new messages and replies stays in the
  UI (ADR 0019); the core only lists identities (a virtual account's
  default first). A request for it can move the logic later.
  *Superseded by ADR 0021: `ChooseIdentity` in the core.*

### Search

The index lives in `<data dir>/search`. Ingest adds documents, deletes and
expunges remove them, a task commits staged changes every second, and a
maintenance task at start-up backfills threading headers of pre-v2 rows
and rebuilds the index when it was recreated or is behind the store. The
local UTC offset for day boundaries comes from `chrono` (clock feature, new
dependency of `nail-core`).

### Demo seed and test controls

`nail serve --seed demo-multi` starts in-process mock servers (IMAP "work"
with the demo corpus, POP3 "home", SMTP, a discoverable third IMAP account
with autoconfig / DNS mocks), writes `accounts.kdl` if missing, keeps
secrets in memory and uses fast reconnects. Only in this mode the server
exposes `POST /api/test/{command}` (`nail_server::TestControl`, same host
checks as every route) so Playwright can deliver mail, change flags, take
the IMAP server offline (`MockImapServer::set_offline`, new) and read what
SMTP received.

## Not in M1a (follow-ups)

- Sync policy `bodies` (`headers-only`, `recent`) is not applied yet:
  bodies are always downloaded (`enabled` and `idle` are).
- Drafts are local only; the server Drafts copy arrives with draft
  autosave in M1b (WP21).
- The server-side IMAP `SEARCH` fallback for messages without a body.
- Retrying a failed write from the UI (`Store::retry_op` exists; there is
  no request yet).
- One engine connection per account: a long full pass delays replay of new
  writes until it ends.

## Consequences

- Every M1a request is implemented; `unsupported` remains only for rich
  drafts (M1b) and client certificates.
- The Playwright scenarios of plan WP14 run offline against mock servers
  (`e2e/tests/accounts.spec.ts`, `mailops.spec.ts`,
  `search-threads.spec.ts`, `send.spec.ts`).
- `ServiceOptions` gains secrets, discovery, browser and timing options and
  is no longer `PartialEq`; `ServerOptions` gains `test_control`.
