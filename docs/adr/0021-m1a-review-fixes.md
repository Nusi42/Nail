# ADR 0021: M1a review fixes: replay safety, Sent copies, discovery trust, limits

Status: accepted · 2026-09-29 · M1a review

## Context

The M1a review found writes that could hit the wrong message or get lost,
discovery results that anyone on the network could forge, a query that
crashes the core, and a few smaller issues. This ADR records how they were
fixed. Where a fix changes an earlier decision, the earlier ADR (0019,
0020) points here.

## Decisions

### Queued writes never act on a stale UID

- `MessageSource` (the location a queued move or delete starts from) now
  also records the folder's **UIDVALIDITY** and the message's
  **Message-ID** when the operation is queued. Both fields are optional in
  the stored JSON, so older queued operations still load.
- Before acting, the IMAP replayer selects the folder and compares its
  current UIDVALIDITY with the recorded one. It uses the UID only when they
  match. Otherwise, or when the UID was never known, it finds the message
  again with `UID SEARCH HEADER Message-ID "<id>"` (the angle brackets make
  the substring match exact). No match means that the message is not there
  (nothing to do). Several matches, or a renumbered folder with a message
  that has no Message-ID, fail the operation. A stale UID is never used.
  Operations queued before this change have no recorded UIDVALIDITY. They
  use the value stored for the folder at its last sync. That value belongs
  to the same UID, because replay runs before sync.
- Flag changes have no recorded source. They use the source of the next
  waiting move or delete of the same message, which says where the message
  is on the server at that point in the queue, else the local row.
- `Store::clear_folder` (after a UIDVALIDITY change) keeps rows that
  operations are still waiting for, with their UID cleared. The next sync
  adopts them again by Message-ID.
- New in `nail-proto-imap`: `ImapClient::select_for_write` (returns the
  mailbox status and reuses the current selection) and
  `ImapClient::find_by_message_id`. The mock IMAP server supports
  `SEARCH HEADER`.

### Later operations learn where earlier ones put the message

A move sets the local row's UID to NULL. A second move or delete queued
before the first one replayed therefore had no UID and was silently
treated as done. Now:

- When a move or append reports the new UID (`COPYUID` / `APPENDUID`), the
  replayer records it on the local row if the row is still in that folder
  without a UID. It also records it on the operations queued after it for
  the same message whose source is that folder
  (`Store::rebase_later_ops`). The walk stops at the next operation that
  takes the message away.
- Without UIDPLUS, the Message-ID search above finds the message.
- If the message is not found while an earlier operation of **another
  account** for it is still waiting (for example a cross-account move that
  has not filed its copy yet), the operation is deferred instead of
  finished.

### Cross-account moves and appends are idempotent

- Once the copy of a cross-account move is filed in the target, the queued
  operation is rewritten (`Store::update_op`) to what remains: a permanent
  delete from the source, with the exact UID and UIDVALIDITY. A retry after
  a failure of the source step only removes the message and does not file
  the copy again.
- An append (a cross-account copy or a Sent copy) that is retried after an
  earlier attempt first searches the target folder for the Message-ID. It
  does not append a copy that is already there. This also covers a crash
  between `APPEND` and recording the result.

### The Sent copy goes through the operation queue

This supersedes ADR 0020 ("The copy is part of sending, not a queued
operation"). After SMTP accepts a message, its copy is stored in the
account's local Sent folder at once. For POP3 accounts that was already so.
For IMAP accounts a new `Operation::AppendMessage { message_id, folder_id }`
is also queued. The IMAP replayer appends the row's raw bytes with its
flags and records the UID from `APPENDUID`. Without UIDPLUS, the next sync
adopts the row by Message-ID. So the copy survives being offline, like
every other write, and it is never lost because the IMAP server was
unreachable while sending. A copy deleted locally before it was filed is
not uploaded.

### Discovery: unauthenticated answers never win

- Autoconfig and `.well-known` answers fetched over plain `http` get
  `DiscoveryWarning::InsecureSource` (a new `warnings` field on
  `DiscoveredAccount`). They rank below every other source. They are
  dropped when an authenticated source answered: autoconfig, `.well-known`,
  ISPDB or Autodiscover over HTTPS.
- Answers from unauthenticated sources (plain HTTP, SRV) whose servers are
  not under the address's domain or the parent domain of one of its MX
  hosts get `DiscoveryWarning::ForeignHosts`. The parent of the MX host is
  used, not a guessed registrable domain, so `mx.example.co.uk` never
  admits all of `co.uk`. Answers from authenticated sources are the domain
  owner's (or Mozilla's) statement and get no host check.
- The setup form preselects the best proposal **without** warnings. It
  shows the warnings of each proposal, hides the untrusted provider name,
  opens the server fields for editing and shows an alert when the selected
  proposal has warnings.

### Search queries have limits

`nail-search` refuses queries longer than 4096 bytes and nesting deeper
than 64 levels (parentheses, `NOT`, `-`). Both raise a new error kind,
`SearchErrorKind::TooComplex` (`search-error-too-complex`). A pasted
`((((…` is now a query error. Before, it overflowed the stack, which
aborts the whole core. The UI's mock parser applies the same limits.

### Demo seeds never use the real directories

`nail serve --seed …` without `--data-dir` / `--config-dir` runs in a
temporary directory that is removed on exit. `--seed demo-multi` refuses a
config directory whose `accounts.kdl` holds real accounts: its in-memory
keychain would lock them out. An `accounts.kdl` written by an earlier
demo-multi run (marked by its first line) is replaced.

### Sender identity is chosen by the core

This supersedes ADR 0019 ("Identity rules in the UI (interim)") and the
virtual-accounts note in ADR 0020. The new request `ChooseIdentity {
account_id, recipients, reply_to }` answers `IdentityChosen {
identity_id }`. The core checks, in order:

1. the virtual account's identity rules (the first rule matching any
   recipient),
2. the identity the replied-to message was addressed to,
3. the default (a virtual account's `default-identity`, else the first
   identity).

The composer asks the core for the identity when it opens, and again
whenever the recipients change until the user picks an identity. The TS
copies of the rule matching are gone from the UI; the mock core has its
own.

### Smaller fixes

- The IMAP engine and the IDLE watcher reset their reconnect backoff only
  after a connection has been up for `EngineTiming::healthy_after`
  (`WatchConfig::healthy_after`, default 60 s). A server that accepts the
  login but drops the session right away no longer causes a reconnect and
  full sync every 2 seconds.
- `AccountManager::edit` compares `accounts.kdl` on disk with the text it
  last loaded or saved. A hand edit that the watcher has not picked up yet
  (or never will, with `--no-watch`) is loaded first, and the app's edit is
  applied on top. An invalid file refuses the edit.
- The sync policy `bodies` / `days` is marked "not applied yet" in the user
  docs and the JSON Schema.
- `ids.rs` writes its domain separators as `\0` escapes instead of raw NUL
  bytes. The ids do not change, and git no longer treats the file as
  binary.

## Consequences

- The replay path costs one `SELECT` per operation (reused while the
  folder stays selected). It costs one `UID SEARCH` only when a UID is
  unknown or stale.
- `DiscoveredAccount`, `SearchErrorKind`, `Request` and `Response` changed
  (bindings regenerated). Old clients ignore the new fields.
