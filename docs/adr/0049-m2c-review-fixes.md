# ADR 0049: M2c review fixes: tracked Sieve scripts, atomic snooze wake, marked Snoozed folder, per-folder wake notifications, paging with server hits, localized texts, ManageSieve interop

Status: accepted · 2026-09-30 · M2c review

## Context

The M2c reviews confirmed nine defects and four smaller gaps in the wave
integrated by ADR 0048:

- **Sieve.** `nail_sieve::upload` recorded any active script with another
  name as the user's "previous" script, including Nail's own script under
  an older name: after a rename the new script included the stale one
  (rules ran twice) and switching off re-activated it. Removal used the
  *current* settings (after the `sieve` node was removed: the IMAP host,
  port 4190, the default name), so a custom host, port or name left Nail's
  script running; a host change or a deleted account left it too. Arrival
  rules skipped server rules only through the in-memory upload state,
  which is empty after every restart and cleared by every failed upload,
  while the old script stays active on the server: `redirect :copy` and
  moves ran twice.
- **Snooze.** The wake removed the snooze records before the writes that
  bring the messages back were queued (separately, errors only logged): a
  crash or failure left mail in the hidden folder with no record, and
  nothing found it again. A resync without `Message-ID` did the same.
  Any top-level folder named `Snoozed` was treated as Nail's and hidden,
  including a user's own.
- **Notifications.** One wake notification covered all accounts; its
  `account_id` was the first message's, `folder_id` none, so a remote
  session granted one account got sender, subject and ids of another's.
- **Search.** Server hits were appended to page 1 without a cap and
  counted into `offset + hits.length`, so page 2 skipped that many local
  hits (UI, remote restricted scan, MCP). Restricted remote sessions made
  the core log in to servers of ungranted accounts.
- **Localized texts.** The MCP audit log ignored the core's `localized`;
  message-file import errors and OpenPGP key-import issues were English
  Nail texts in the UI.
- **Interop.** The ManageSieve interop tests moved to M2c were not written.
- Smaller: `nail mcp` trusted a possibly stale endpoint file before the
  data-dir lock; junk training storage grew without bound and was keyed by
  the sender-chosen `Message-ID`; the a11y Playwright scan skipped nine
  settings tabs, the move window, the message menu, the login page and the
  high-contrast mode; no core test reopened a Service after snoozing.

## Decisions

### Store schema v8 (`nail-store/src/schema.rs`)

- `sieve_state.location` (JSON `SieveLocation`: host, port, TLS, script
  name) and `sieve_state.server_rules` (JSON rule ids), written by
  `record_sieve_upload(&SieveUpload)`, cleared only by
  `clear_sieve_upload` (a failed upload keeps them).
- `folder_marks (folder_id → folders ON DELETE CASCADE, account_id, kind
  'snoozed', UNIQUE (account_id, kind))`; the migration marks existing
  top-level `Snoozed` folders that hold snoozed mail.
- `Store::wake_snoozed(ids, operations)` removes snooze records and
  applies + queues operations in **one** transaction;
  `orphaned_in_folder` lists messages with neither a snooze record nor a
  pending operation; `mark_snoozed_folder`, `snoozed_folder_of`,
  `is_snoozed_folder`.

### Sieve (`nail-sieve/src/sync.rs`, `nail-core/src/sieve.rs`)

- Only a script the **user** wrote counts as previous. An active script
  that starts with Nail's marker is reported as `replaced_managed` and its
  own recorded previous is taken over; a recorded previous that is a Nail
  script is followed (at most eight hops, loop-safe) to the user's script.
  `remove` re-activates only a user script.
- The core records where it uploaded. Before an upload to a different
  location (host, port, TLS, name) it removes the script from the recorded
  one (best effort: a server gone for good must not block the new one).
  Switching off uses the recorded location (the settings for pre-v8
  records); a failed removal is retried like a failed upload.
  `RemoveAccount` removes the script first (bounded to 20 s, while the
  login still exists).
- `server_rules()` reads the store: rules of the script known to be active
  stay skipped on arrival after restarts and failed uploads. Accepted
  consequence: a rule edited while uploads fail still runs in its old form
  on the server, not locally.

### Snooze and the Snoozed folder (`nail-triage/src/snooze.rs`, `nail-core/src/snooze.rs`, `junk.rs`)

- `poll` / `unsnooze` no longer touch the store; the caller removes the
  records together with queuing the writes (`wake_snoozed`), or
  `commit`s; on failure `restore` schedules them again. A crash in between
  leaves the records, so the messages are due again after the restart.
- The hidden folder is the account's **marked** folder; a new one takes
  the first free name of `Snoozed`, `Nail Snoozed`, `Nail Snoozed 2`, …
  A user folder called `Snoozed` stays visible, gets arrival rules and
  notifications, and is never used or swept. `is_snoozed_folder` asks the
  store (list, virtual folders, replay's CREATE, the mirror, arrivals).
- Sweep: at the first poll after start and after `rebind_moved` had to
  forget a snooze, messages in the marked folder without a record or a
  pending write return to the Inbox (keyword cleared). Limitation: with
  two Nail installations on one account whose hidden folders share a name,
  one returns the other's snoozed mail (snooze stays per installation).

### Notifications (`nail-core/src/notify.rs`, `remote_access.rs`)

- One wake notification per account **and** folder the mail returned to
  (`folder_id` set, tag `snooze-woke:<folder>`).
- Remote filter: a notification naming messages without an account or a
  folder goes only to sessions with a full grant.

### Search (`nail-api` `SearchHit.from_server`, `nail-core/src/search.rs`, UI `nextSearchOffset`)

- Server hits are marked `from_server`, at most `limit` of them, on top of
  page 1 and in `total`; `offset` / `limit` count local hits. The UI's next
  offset is `offset + local hits` while it is below `total − server hits`.
- `Service::search_with(query, ServerSearchMode)`: `All` (UI), `Off`
  (MCP via `CoreAccess::search_local`, later pages, the restricted scan),
  `Only(filter)`: a restricted remote session asks only servers of granted
  accounts and folders, once, then filters and hides encrypted hits.

### Localized texts

- `AuditLog` renders `entry.localized` (English `detail` as fallback).
- `ImportMessageFile` errors carry `launch-error-unreadable` / `-size` /
  `-not-mail` (`core.ftl`; the OS reason stays an English `detail`); the
  launch notice renders the core's reason through `errorMessageOf`.
- OpenPGP import issues carry `security-import-issue-*` (`security.ftl`;
  rPGP's reason stays a `detail`); `ImportResult` renders them.

### ManageSieve interop (`scripts/interop`)

- `sieve` feature per server (`base + 4190`), Dovecot with Pigeonhole
  (`protocols = … sieve`, `managesieve-login` on 4190, `+regex`), start
  script and CI wait for 14190 / 24190, CI enables `sieve` for both.
- Tests: STARTTLS login, a refused wrong password, `fileinto` capability,
  `CHECKSCRIPT` of a broken script; upload next to an active user script
  (recorded, marker), a renamed script (never includes Nail's old one),
  switching off (the user's script is active again). The same checks run
  offline against `MockSieveServer` with STARTTLS; against Dovecot and
  Stalwart they run only in CI (not yet run: no Docker locally).

### Smaller fixes

- `nail mcp` takes the data-dir lock **first**: a free lock means the
  endpoint file is stale (removed, never probed); with a held lock only
  the holder's endpoint (same pid) counts, waited for up to 10 s (start-up
  race).
- Junk: the training key is `Message-ID#<64-bit SHA-256 of the token
  set>` (a copy is the same message; a spam reusing a Message-ID is
  trained on its own). Only the newest 1,000 trainings keep tokens, 20,000
  rows are remembered, and above 250,000 distinct tokens the ones seen in
  a single message go. Pre-v8 training rows keyed by the bare Message-ID
  are not re-keyed (development data only).
- Playwright: axe on the nine missing settings tabs, the message menu, the
  move window, `?mode=high-contrast` (main window and reader) and the
  themed login page; core tests reopen a Service after snoozing.

## Consequences

Store v8; `SearchHit` gained an optional field (TypeScript regenerated).
M2c follow-ups "upload state in memory" and "ManageSieve interop" are
closed except running the interop job against real servers.
