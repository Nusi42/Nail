# ADR 0099: M4c integration (WP125) — portability, recovery key and locked data, smartcards, PDF files, WP123 leftovers, release candidate

Status: accepted · 2026-10-02 · M4c WP125

## Context

The parallel packages of wave M4c (plan section 9) built against the
contract of ADR 0094: `nail-portability` (WP121, ADR 0096), full-file
database encryption, the recovery key and `nail-pkcs11` (WP122, ADR 0095),
the benchmark and the Exchange / Gmail engine work (WP123, ADR 0097) and the
UI (WP124, ADR 0098). WP125 wires them into the core, the desktop shell and
the layout, adds the Playwright scenarios of the plan and prepares
`1.0.0-rc.1`. This ADR records the integration decisions; the package ADRs
were renumbered 0095–0098 (they were written in parallel).

## Decisions

### Portability in the core (`nail-core/src/portability/`)

- **Jobs** (`jobs.rs`): in memory, ids `job-<n>`, the finished ones kept
  `FINISHED_JOB_RETENTION_SECS`. A job's file work runs on the blocking pool
  (`spawn_blocking`) with the crate's `JobContext` (progress throttled by the
  crate, mirrored into the job list and broadcast as
  `PortabilityJobProgress`); then an **async finish** runs in the runtime
  (announcing queued operations, `MailChanged` / `AccountsChanged`, writing
  PIM items through the core's async import path, reloading `accounts.kdl` /
  `rules.kdl`), and only then the report is finished
  (`job::finish_report`) and `PortabilityJobFinished` goes out. Cancelling
  sets the crate's `CancelFlag`.
- **Requests** are checked before a job starts: paths must be absolute
  (`portability-error-path-not-absolute`), sources must exist
  (`portability-error-missing-path`, `not_found`), the export format must fit
  the source, an existing export target is `conflict` (checked with the same
  single-file / folder rule the exporter uses), backup contents and
  passphrases give `backup-issue-*`, an existing backup file is `conflict`.
- **Where imported mail goes** (`mail.rs`, `ImportTarget`):
  - *Local Folders* (`LOCAL_FOLDERS_ACCOUNT_ID`, a `Local` account named by
    `local-folders-account`, made on first use) or another local account:
    the source's folder tree is created under the optional new folder
    (`folder_name`) below the optional parent. Imported folders get no
    special-use role (a second "Inbox" would join the unified inbox).
  - A **real account** (IMAP, Gmail, Exchange; POP3 keeps the copies local):
    there is no operation that creates a folder on a server, so the import
    files into **existing folders only** — the same-named folder below the
    chosen one (`parent_folder_id`, default the Inbox) when it exists, else
    the chosen folder itself, reported once per source folder as the new
    issue code **`folder-merged`** (warning). `folder_name` is refused
    there (`portability-error-account-folders`), virtual accounts too. Each
    stored message is queued as `AppendMessage` (one transaction per batch
    of 100 with the local rows); the queue's announcement wakes the engines.
    The UI hides the folder-name field for accounts. Creating folders on
    servers is a follow-up.
  - Message ids derive from the target folder and the content hash, so a
    repeated import updates the same rows; `skip_duplicates` compares
    Message-ID, date and size with the folder (and the pending batch).
- **Contract addition**: `PortabilityIssueCode::DeletedInSource`
  (`deleted-in-source`, WP121's request; Thunderbird messages not compacted
  away, Maildir `T`) and `FolderMerged` (`folder-merged`). The crate reports
  the former directly now.
- **PIM data** (`pim.rs`): the sinks buffer the parsed items; the async
  finish writes them with the existing `import_pim` (same `UID` = update,
  the PIM queue for server sources, reminders and the PIM index updated).
  A restore keeps the backed-up collection ids: a missing calendar or
  address book is created as a **local** one with that id and name. Items
  that cannot be saved become `write-failed` issues of the report.
- **Exports** read the store and blob store (`StoreMailSource`,
  `StorePimSource`): folders oldest first, chosen messages as one folder,
  headers-only messages fetched first (`fetch_bodies`) unless
  `allow_headers_only`; PIM items as their stored iCalendar / vCard text.
  An mbox export of a folder imports again byte-identically (Playwright).

### Backups and restores (`portability/backup.rs`)

- **Files per item**: settings = `settings.json` (every `app_settings` key
  except the machine-bound `remote-ui-certificate` and `desktop.first-run`;
  tokens, remote-UI users, MCP clients and pairing codes live in their own
  tables and never go in), theme / layout / accounts / rules = the
  configuration files that exist, rules also `saved-searches.json`, keys =
  `keyring.json` (every key: public part, trust decision, secret part as
  stored — passphrase-protected or wrapped — **plus the keychain secret of a
  wrapped key**, so the key works on another machine; smartcard keys keep
  their reference), plug-ins = `<id>/package.nailplugin` (the approved
  package; development plug-ins are left out). Local mail: every `Local`
  account (Local Folders, opened files, the demo account).
- **Restores**: configuration files are replaced through the configuration
  manager's new `ConfigManager::replace_file` (the current file — also "no
  file" — becomes a configuration backup, revertible with `RevertConfig`;
  `file_text` reads them for the dry run); settings key by key; saved
  searches upserted; keys merged by technology + fingerprint (an existing
  key without a secret gets the backed-up secret); **plug-ins that are not
  installed are installed through the normal `InstallPlugin` path, disabled
  and without permissions** (`restore-note-plugins-review` asks the user to
  review them); mail into the local account named in the backup (created
  when missing). The dry run never creates anything (the mail target
  resolves folder ids without writing).
- `nail-portability` fix: an empty calendar or address book is backed up
  as an empty entry; restoring it is no longer an `unsupported-format`
  issue.

### Diagnostics (`portability/diagnostics.rs`, `nail-core/src/logs.rs`)

- Collected entries: `system/info.json`, `logs/nail.log` + `logs/plugins.log`,
  `config/<file>`, `accounts/status.json`, `store/stats.json`,
  `store/operations.json`, `plugins/plugins.json`, and opt-in
  `mail/<n>-headers.txt` / `mail/<n>.eml`. The crate redacts and writes.
- **Logs**: Nail writes no log files, so the binaries (`nail`, the desktop
  app) tee their `tracing` output through `nail_core::logs::TeeWriter`
  (stderr + a ring of the last 2,000 lines, ANSI colours removed).
- **Exactly the preview**: the last preview's prepared entries are kept for
  30 minutes; `CreateDiagnosticsBundle` with the same options writes them,
  so new log lines between preview and save do not change the bundle.

### Encryption at rest: opening the data directory, locked data, recovery key

- `Service::open` opens the data directory with `nail_vault::open_data_dir`
  (full-file encryption on). `VaultCtl` takes the built vault
  (`from_vault`); `Service::from_parts` keeps the column-only vault.
- **Locked data directory**: when the encrypted database cannot be opened
  (its key is gone from the keychain or wrong), the service runs on an
  **empty in-memory stand-in store** (blobs in a temporary folder),
  `VaultCtl::locked` answers `GetVaultStatus` from `LockedVault::status`
  (`needs_recovery`), reads answer `locked`, and **no engines start**
  (nothing syncs into the stand-in). `RestoreVaultKey` puts the key back
  with `LockedVault::restore_key`; the data opens **after a restart**: the
  status then carries `localized = vault-restart-needed` and the UI says
  so. (Re-opening a live service onto another store is not worth the risk
  for a one-time recovery.) A vault whose database is plain but whose
  sealed columns were locked (the M2a case) opens at once: the open error is
  cleared and the search index opened again.
- After start, `encrypt_database_file` runs once in the background (under
  the migration lock), so vaults encrypted before M4c get their database
  file encrypted.
- Recovery requests: `CreateRecoveryKey` (the kit file from the
  `recovery-kit-*` texts in the user's language, never over an existing
  file), `VerifyRecoveryKey`, `RestoreVaultKey`, `RemoveRecoveryKey`.
  `CoreError` now converts `VaultError` (and `nail-smime` errors) with their
  localized texts (`vault-not-encrypted`, `recovery-key-wrong`, …).
- **Seeded runs** keep their secrets in `<data-dir>/demo-keychain.json`
  (`nail_core::demo::FileSecretStore`, demo data only, never the OS
  keychain), so a restart of a seeded `nail serve` finds them — the
  recovery-key scenario deletes the data key's entry from that file.

### Smartcards (`nail-core/src/smartcard.rs`)

- `SmartcardCtl` loads the modules of the settings on first use
  (`Smartcards::load`; `ServiceOptions::smartcard_modules` injects modules
  — the soft token in tests) and **is the `KeyResolver` of both keyrings**:
  `Service::pgp()` / `smime()` call `.with_smartcards(...)`, so signing and
  decrypting with card keys works wherever software keys do.
  `SetSmartcardSettings` logs out and reloads.
- **PIN prompts**: `CorePinSource` (the core's `PinSource`) broadcasts
  `PinRequested` with a `pin-<n>` id and the expiry, waits for
  `AnswerPinPrompt` up to `PIN_PROMPT_TIMEOUT_SECS`, and sends
  `PinPromptClosed` (answered / cancelled / timed out) so other windows
  close their dialog. Card work runs on blocking threads
  (`BlockingPinSource`); a wrong PIN comes back as a new prompt with
  `retry`. Unknown prompts: `not_found` + `smartcard-prompt-gone`.
- `ListSmartcards` fills `key_id` from the S/MIME certificate's SHA-256 and
  `openpgp_fingerprint` (and the key id) from `fingerprint_for_card_key`.
  A poller (every 5 s, only while modules are configured) broadcasts
  `SmartcardsChanged` when the token list changes and ends PIN-cache
  logins; shutdown logs out of every card.

### Printing and file dialogs in the desktop shell

- **PDF files** (`app/src-tauri/src/printing.rs`): `print_to_pdf` opens a
  hidden window `print-<job>` of the normal UI at `index.html#/print/<job>`
  (same WebView2 data folder as the main window), waits up to 60 s for the
  view's `PrintViewReady` (forwarded by the core to
  `DesktopHost::print_view_ready`), then calls WebView2's
  `ICoreWebView2_7::PrintToPdf` with the job's page size (inches),
  orientation and backgrounds, no headers / footers; the window is
  destroyed afterwards and the answer is the file size. New dependency:
  `webview2-com` 0.39.1 (MIT; the version wry uses, so the interfaces match
  Tauri's webview handle). The capability `default` covers `print-*`
  windows. `app/src-tauri/build.rs` delay-loads `comctl32.dll`:
  test executables have no Common Controls 6 manifest, and with the window
  code linked they failed to start (`TaskDialogIndirect`,
  `STATUS_ENTRYPOINT_NOT_FOUND`); Nail.exe keeps its manifest.
- **File dialogs** (`dialog.rs`): the Windows common item dialog
  (`IFileOpenDialog` / `IFileSaveDialog`, folders with `FOS_PICKFOLDERS`)
  on a thread of its own with COM in STA mode, owned by the main window;
  title and filter names localized by the core; start folder and file name
  from `default_path`.

### WP123 leftovers in the core

- **Server threads**: `IncomingMessage.remote_thread_id` →
  `MessageMeta.remote_thread_id` for Gmail `threadId` and Graph / EWS
  conversation ids.
- **Exchange folder sync policies**: per message
  `nail_exchange::policy::body_fetch(effective(sync, path, role).bodies, …)`;
  headers-only messages are fetched with the new
  `ExchangeMailEngine::fetch_headers` and stored `headers_only`; their
  bodies come on demand through `fetch_mime` (`fetch_exchange_bodies`).
- **Moves**: new `ExchangeMailEngine::move_to_mailbox` (default: copy +
  delete; Graph and EWS forward to their server-side moves) for moves
  between mailboxes of one account; `api_moves.rs` moves between accounts
  with `nail_exchange::transfer` (Exchange ↔ Exchange, Exchange ↔ Gmail,
  Gmail ↔ Gmail) and files messages moved from IMAP / POP3 accounts into API
  accounts through the target engine (before, they were only kept locally
  — a data-loss bug). Tags are not carried over.
- **Send progress**: the outbox sends with `send_with_progress` and
  broadcasts `SendProgress` at most every 250 ms (and at the end).
- Send as / on behalf needs no core change (`GraphEngine::for_account`).

### Performance

- The subject-fallback threading queries repeat the partial index's
  condition and use a date range (`SUBJECT_REPLY_SQL` /
  `SUBJECT_ORIGINAL_SQL`, EXPLAIN test): initial sync is no longer
  quadratic. `ingest` uses hash sets instead of `Vec::contains` in loops.
- The 100k benchmark (`node e2e/perf/run-perf.mjs --corpus standard`) and
  its CI job `perf` (not blocking the other jobs); every budget holds
  (initial sync 286.4 s of 300, incremental 2,732 ms of 3,000, search p95
  107.7 ms, list page p95 3.7 ms, cold start 69 ms, peak memory 125 MiB,
  first render 413 ms, scroll frame p95 18.1 ms, 0 long frames). The list
  loads its next page one rendered window before the end (scrolling stalled
  at every 100-row page before).

### UI, layout and theme

- Theme: a `print` colour mode (`css::MODES`), `print=` values in the default
  theme, styles for every `palette.*`, `print.*`, wizard, portability,
  diagnostics, recovery and smartcard part; the features' base CSS in
  `theme/compile.ts`.
- Layout: Ctrl+K / Ctrl+Shift+P open the palette (also over dialogs),
  Ctrl+P prints in the list and the reader, print actions in the reader
  toolbar, the list and reader menus and the calendar toolbar, Settings tabs
  Recovery key, Smartcards, Import and export, Backups, Diagnostics; the
  M4c widgets have typed options (`show-jobs`).
- Fixes found by the scenarios: layout aliases are enabled by their own id
  in the palette (`settings.open` showed "not available"); account names
  without an address no longer show "()"; the restore dialog says when a
  restart is needed.

### Release candidate

- Version `1.0.0-rc.1` in the Cargo workspace, `tauri.conf.json` and the npm
  packages (the MSI version is derived: `1.0.0.1`). `CHANGELOG.md`,
  `userDocs/en/release-notes.md`, `packaging/release-notes/1.0.0-rc.1.md`,
  `docs/release-checklist.md` (the human gates).

## Consequences

- Tests: `nail-core/tests/m4c_portability.rs` (imports with flags, account
  targets through the queue, Thunderbird, export / re-import, backup → new
  data directory → restore twice, diagnostics), `m4c_vault.rs` (recovery
  key, locked directory, restore, reopen), `m4c_leftovers.rs` (Exchange
  policies and bodies on demand, conversations, moves into a shared mailbox
  and another account, upload progress), `m4c.rs` (smartcards with the soft
  token), unit tests in `smartcard.rs`, `jobs.rs`, `logs.rs`, the store
  (index use, `all_settings`), nail-config (`replace_file`), the shell
  (`printing.rs`, `dialog.rs`). Playwright: `m4c.spec.ts`,
  `vault-recovery.spec.ts`, `desktop-m4c.spec.ts`, the M4c dialogs in
  `a11y-windows.spec.ts`; the benchmark `e2e/perf/`.
- Follow-ups (plan section 9): creating folders on servers during imports;
  hot re-open after a recovery-key restore; real PKCS#11 hardware;
  printing HTML mail; Thunderbird calendars; keys that need the PIN for
  every use and EdDSA card keys.
