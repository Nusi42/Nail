# ADR 0048: M2c integration — Sieve, triage, sync policies, localized core messages, M2b follow-ups

Status: accepted · 2026-09-30 · M2c WP55

## Context

WP51–WP54 delivered `nail-sieve` (ADR 0044), `nail-triage` (ADR 0045), the
engine polish of WP53 (ADR 0046) and the UI polish of WP54 (ADR 0047)
against the contract of ADR 0043. WP55 wires them into the core, the
desktop shell and the servers, adds the Playwright scenarios of plan
section 7, and closes four M2b follow-ups (plan "M2b follow-ups").

## Decisions

### Server-side rules (`nail-core/src/sieve.rs`)

- **One upload task** (`run_sieve`) per core. Accounts are queued at
  start-up, after every rule change (`SaveRule` …, hand edits of
  `rules.kdl`), vacation change, account change and after each full sync
  pass (the folders the script names are known then). A queued account is
  uploaded only when the **digest of its inputs** (rules, vacation, the
  stored folder names by role and path, virtual accounts containing it,
  identities, endpoint, script name) differs from the last successful
  upload, or when forced (`SyncSieve`), or on the day the compiler names
  for vacation dates the server cannot check (`recompile_on`, servers
  without `date`).
- The login is the account's IMAP login (`SieveAuth::from_config`), a
  rejected OAuth2 token is refreshed once; the core calls
  `SieveSession::login` itself to keep the typed `SieveError`, so a
  refused connection or a server without ManageSieve shows as
  `unavailable` (`SieveError::is_unavailable`), everything else as
  `failed` (recorded with `record_sieve_error`, retried after
  `timing.reconnect_max`).
- **Status**: `GetSieveStatus.rules` are the placements of the last
  successful upload while the account is in sync; otherwise the rules are
  compiled against the last seen extensions (`COMMON_EXTENSIONS` before
  the first connection) and `server` placements are demoted
  (`sieve-reason-pending`, `-unavailable`, `-sieve-off`). The in-memory
  upload state is lost on restart: until the start-up upload the account
  shows `pending`.
- **Arrival rules skip server rules** of the message's account (a filtered
  `RuleSet` per account); manual runs still run them.
- **Switching off** (`enabled=#false` or removing the node) removes Nail's
  script and re-activates the user's previous one (`nail_sieve::remove`),
  then clears the recorded upload (`Store::clear_sieve_upload`, a new
  accessor; no migration).
- `nail_sieve::CompileOptions::folders` became `&(dyn FolderResolver +
  Sync)` so an upload future can run on the multi-threaded runtime.
- **demo-multi** runs a `MockSieveServer` for "work" with the `sieve` node
  **switched off**: the mock stores scripts but does not execute them, and
  the older arrival-rule scenarios expect Nail to run their rules. The
  Playwright scenario turns it on in Settings → Server rules.

### Junk, snooze, notifications (`junk.rs`, `snooze.rs`, `notify.rs`)

- **Arrival order** in `process_rule_inbox`: security (M2a) → junk
  (`classify_arrivals`: the server's `$Junk` / `$NotJunk` / Junk folder when
  trusted, else the classifier when enabled and trained; verdicts stored,
  server verdicts trained) → arrival rules (returning their `notify`
  choices) → notifications. **Precedence** of the notification policy is
  ADR 0043's plus WP52's extra step: after a rule's `notify`, mail filed
  as junk does not notify.
- `StoreJunk` implements `nail_triage::JunkStore` on the store; the new
  `Store::untrain_junk` backs `classifier::undo`. `MarkJunk` trains, stores
  the user's verdict (`MessageBody.junk`, source `user`) and queues
  `plan_mark`'s keywords and moves. No planner can move to Trash.
- **Snooze**: one `SnoozeScheduler` (loaded lazily from the store) behind
  a mutex; the wake task sleeps until the next wake time, at most
  `timing.snooze_poll` (60 s; 1 s in `EngineTiming::fast`). `TriageOp`s map
  onto queue operations; `EnsureSnoozedFolder` creates the folder
  **locally** (path `Snoozed`, hidden from `ListFolders` and virtual
  folders), the IMAP executor `CREATE`s it on the server before the first
  move into it (`ALREADYEXISTS` is fine), and a sync pass never deletes a
  local Snoozed folder that still holds mail. Messages moved within an
  account keep their id, so snoozes need no rebinding for Nail's own
  moves; a snooze whose message vanished follows it by `Message-ID`
  (`rebind_by_message_id`) or is forgotten. The Snoozed folder never
  produces `NewMail`.
- **Notifications**: `notify::notifying` + one `NotificationBatcher`
  (3 s quiet period, 15 s at most; 1 s / 3 s in `fast`), flushed by a
  timer task into `Event::NotificationRequested`. Snooze wake-ups notify
  through `notify::snooze_woke`; `TestNotification` uses
  `notify::test_notification` (ids `test:<n>`). The desktop shell renders
  title and body with `Service::render_localized` (the core's Fluent
  catalogs) and shows them with `tauri-plugin-notification`; the browser UI
  shows Web Notifications (WP54).
- **Clock**: snooze, quiet hours and batching read `ServiceOptions::clock`
  (`nail_triage::Clock`; the system clock by default). `--seed demo-multi`
  passes an `AdjustableClock` (system time plus an offset) that the test
  controls `clock`, `clock-set {at}` and `clock-advance {seconds}` move.

### Sync policies and bodies (`sync.rs`, `ingest.rs`, `bodies.rs`, `search.rs`)

- `sync_folder` takes the folder's effective `BodyPolicy` and fetches with
  `fetch_batch_with(BodyFetch::from_policy(..))`; `ingest` merges full and
  headers-only messages with `merge_with_body_state` (a headers-only
  resync keeps a downloaded body **and its search entry**). With
  `recent N days`, bodies that aged out are detached after each pass (the
  header block becomes the blob; search entries keep the body text).
- **On demand**: `GetMessage` of a headers-only message and
  `FetchMessageBodies` fetch bodies on a connection of their own
  (`fetch_body`), attach them, re-index and announce `MailChanged`.
  Offline, the reader shows the header block (and the "Download full
  message" bar stays).
- **Server search fallback** (first page only): when the query looks into
  text (`nail_search::imap::needs_server`) and folders in scope hold
  headers-only mail, each such account is asked with `UID SEARCH`; hits
  that are headers-only and not found locally are merged (sorted by date
  for date order), `total` grows by them, and `SearchResults.server` names
  the accounts asked and those that failed. Inexact keys (attachments, file
  names) are not filtered further locally (follow-up).
- **Separate replay connection**: an IMAP session opens a second
  connection lazily for replaying queued writes, so replay never changes
  the main connection's selected mailbox; without it, writes replay on the
  main connection as before. When the replay connection breaks, the step
  fails like a broken main connection (the session reconnects and the
  deferred writes retry). Replay and sync stay sequenced in the session
  loop (no concurrency change).
- The mock IMAP server (`nail-testkit`) now understands `TEXT`, `BODY`,
  `SUBJECT` / `FROM` / `TO` / `CC`, `OR`, `NOT`, lists and `CHARSET`.

### Localized core messages

- `strings.rs` renders through `nail_config::l10n::Catalog` (fluent-bundle,
  every catalog, per-key English fallback); `strings::render` renders a
  `LocalizedMessage`.
- `CoreError::localized` passes producer texts on (`ApiError`,
  `EngineError`, `ConfigError`); `SyncFailed` and `AccountStatus` carry
  them; a pass with failed folders says `sync-error-folders` (core.ftl).
  Rule issues fill `ConfigIssue.localized` / `ApiError.localized`; MCP audit
  entries get `nail_mcp::messages::audit_detail` when listed.
- The UI prefers `localized` in the status bar (sync failures), the config
  error list, the merge dialog's conflicts and the rule text view.

### MCP port, data-directory lock (desktop)

- The desktop shell starts the MCP server on `Service::mcp_port()` (the
  settings' fixed port, else any) and restarts it on
  `McpSettingsChanged`. It takes `nail_core::paths::lock_data_dir(dir,
  "desktop")` before opening the data: when a headless `nail mcp` or
  `nail serve` holds it, a message box shows `cli-data-dir-in-use-by` and
  the app exits.

### Contract additions

- `Request::GetDraftAttachment {draft_id, blob}` → `Response::AttachmentData
  {mime, data}` (WP54's inline images of reopened drafts): only blobs the
  draft lists are served; remote access like `GetDraft` (draft scope,
  identity account, `encrypted` gate).
- `RemoteContentScope::Once` and `RemoteUiSettings.audit_retention_days`
  (+ `RemoteUiSettingsIssue::InvalidAuditRetention`), see below.

### M2b follow-ups

- **Render tokens are bound to the session**: `Signer::sign_render_for`
  puts the remote session id into the render token; the sandbox origin
  serves a document only when the request carries that live session's
  cookies (`RemoteAuth::session_id_of`, which neither touches nor rotates
  the session; browsers send host cookies to the other port). Unbound
  tokens are refused.
- **"Load once"**: `RemoteContentScope::Once` allows one message's remote
  content in memory for `ONCE_ALLOW_SECS` (15 minutes) or until restart;
  it is never stored or listed, the proxy's check honours it, and
  `RemoveRemoteContentAllow {once}` ends it.
- **Proxy rate limit**: the proxy routes allow 300 fetches per minute per
  remote session, or per client address where no session cookie arrives
  (mail documents on the sandbox origin have an opaque origin and send
  none); more are answered 429.
- **Remote audit retention**: `audit_retention_days` (1–3650, default 90)
  and a purge at start and then daily (`HISTORY_PURGE_INTERVAL`) of the
  remote security log; MCP and rule-run history keep 90 days.

### Tests and harness

- Core: `tests/m2c_wired.rs` (Sieve upload / placement / vacation /
  removal, junk training and filing, snooze with the clock, notifications
  and quiet hours, headers-only + server search + fetch on open),
  updated `tests/m2c.rs`, remote tests for load once, retention and bound
  render tokens; unit tests for the clock, the lock helper, folder names,
  catalog ids, header blocks.
- Playwright `m2c.spec.ts`: server rule + vacation on the mock ManageSieve
  server, junk training and filing, snooze returning at the wake time,
  notification + quiet hours (recorded browser notifications), server
  search + fetch on open, right-click / Shift+F10 menu, rebinding a key;
  `a11y.spec.ts` covers the new tabs, windows, dialogs and the folder
  menu; `remote-content.spec.ts` covers "Load once".
- The e2e harness passes `--ui-dir <repo>/ui/dist`: `nail serve` prefers a
  `ui` folder beside its binary, and building `nail-app` copies Tauri's
  resources into `target/debug/ui`, which went stale after UI rebuilds.
- The context menu renders inside the app's `main` landmark (axe
  `region`).

## Consequences and follow-ups

- Real servers: run upload / remove against Dovecot Pigeonhole and Stalwart
  in the interop job; the mock does not execute scripts.
- The server search does not filter inexact hits locally; `has:attachment`
  and `filename:` may over-match on the server side.
- The Sieve upload state lives in memory: after a restart the account shows
  `pending` until the start-up upload; a store column would avoid that.
- Snoozed mail moved by another client to another folder is found again by
  `Message-ID` only in the same account.
- Unsure junk verdicts are stored as `junk: false` with a score.
- The proxy limit is per session only in the remote UI's app origin; the
  sandbox origin limits per client address.
- German catalogs stay empty.
