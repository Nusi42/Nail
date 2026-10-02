# ADR 0094: M4c contract — import / export, backup / restore, recovery key, smartcards, command palette, printing, diagnostics, benchmark, Exchange / Gmail leftovers, slots

Status: accepted · 2026-10-02 · M4c WP120

## Context

Wave M4c (plan section 9) finishes v1.0: data portability and backups
(WP121, `crates/nail-portability`), data-protection completion and
smartcards (WP122: `crates/nail-vault`, `nail-store`, `nail-pgp`,
`nail-smime`, the new `crates/nail-pkcs11`), performance and the M3
leftovers (WP123: `nail-proto-graph`, `nail-proto-gmail`, `nail-proto-ews`,
`nail-exchange`, the new `crates/nail-bench` + `e2e/perf/`) and the UI
(WP124: `ui/src/features/{portability,palette,print}` and the security
additions), built in parallel and integrated by WP125. This ADR fixes what
they build against and the slots that keep them apart.

Constraints (plan "Scope decisions for M4", design "Storage, sync &
search", "Security & encryption", standard-feature checklist): the core
owns data, every write goes through the operation queue, writes by
programs are confirmed; no secrets in plain files; strings in Fluent
catalogs; no GPL / LGPL / AGPL, no OpenSSL / aws-lc; no non-loopback
sockets; paths on the desktop are desktop-only for remote sessions.

## Decisions

### Import, export and jobs (`nail_api::portability`)

- **Sources** (`ImportFormat`): `mbox` (mboxrd and mboxo, streamed),
  `eml-folder`, `maildir` (Courier / Dovecot `.Sub.Folder` trees and `:2,`
  flags), `thunderbird-profile` (local folders, address books from
  `abook.sqlite` / Mork `.mab` where readable, `prefs.js` accounts and
  `msgFilterRules.dat` filters as **proposals**), `ics`, `vcard`. A source
  is `{format, path}`; `FindImportSources` lists Thunderbird profiles from
  `profiles.ini`.
- **Targets** (`ExportFormat`): `mbox` (`MboxVariant` mboxrd default /
  mboxo), `eml`, `maildir`, `ics`, `vcard`; `ExportFormat::accepts` pairs
  formats with `ExportSource` (folders with subfolders, messages,
  calendars, address books). Existing targets are a `conflict` unless
  `overwrite`.
- **Flow**: `PreviewImport` reads without writing (`ImportPreview`: folders
  with counts and sizes, collections, `JobCounts`, `AccountProposal`s with
  the proposed `AccountConfig` and a duplicate hint, `RuleProposal`s with
  `complete` and the dropped parts, the first `MAX_PREVIEW_ISSUES` issues);
  `StartImport` / `StartExport` start a **job** and answer `PortabilityJob`
  (`JobStatus`). Progress: `Event::PortabilityJobProgress {job_id,
  progress}` (phase `queued` / `scanning` / `reading` / `writing` /
  `verifying` / `finishing`, items and bytes, current folder; at most a
  few per second); the end: `Event::PortabilityJobFinished {report}`.
  `ListPortabilityJobs`, `GetPortabilityJob`, `CancelPortabilityJob` (what
  was written stays). Jobs live in memory; finished ones are listed for
  `FINISHED_JOB_RETENTION_SECS`.
- **Reports** (`JobReport`): state `completed` / `completed-with-issues` /
  `failed` / `cancelled`, `JobCounts`, per-item `ItemIssue`s (`ItemRef`
  file / folder / index / offset / label, severity `warning` (imported,
  changed) or `skipped`, a `PortabilityIssueCode`, English detail and a
  `LocalizedMessage`), capped at `MAX_JOB_ISSUES` with
  `issues_truncated`, created folders, proposals, the backup manifest /
  restore report. `push_issue` counts skips and duplicates;
  `finished_state` ignores duplicates.
- **Where data goes**: mail into the local **"Local Folders"** account
  (`LOCAL_FOLDERS_ACCOUNT_ID`, made on first use like "Opened files", never
  synced) or under a folder of a real account — then each message is an
  `AppendMessage` through that account's operation queue; `skip_duplicates`
  (Message-ID + date + size) makes a repeated import harmless; flags from
  Maildir info / mbox `Status`, `X-Status`, `X-Mozilla-Status` when
  `keep_flags`. Events, tasks and contacts go through the PIM queue (same
  `UID` = update). Accounts and filters are only proposed: the UI saves
  the chosen ones with `AddAccount` / `SaveRule`; passwords are never
  imported (`secret-not-imported`).

### Backup and restore (`nail_api::backup`)

- **Items** (`BackupItem`): `settings` (app settings; never tokens,
  remote-UI users, MCP clients or pairing codes), `theme`, `layout`,
  `rules` (+ saved searches), `accounts` (`accounts.kdl`, `keychain:`
  references only), `local-mail`, `keys` (keyring incl. secret keys —
  **off by default**), `pim-local`, `plugins`. `BackupContents::default()`
  = all but keys.
- **Archive** (`.nailbackup`, `BACKUP_FORMAT_VERSION` 1): magic
  `NAILBAK1`, a length-prefixed JSON header (format, Argon2id parameters
  ≥ 64 MiB / 3 passes with a 16-byte salt, cipher, STREAM nonce prefix;
  authenticated as associated data), then XChaCha20-Poly1305 STREAM in
  64 KiB chunks over a zip whose first entry is `manifest.json`
  (`BackupManifest`: format, app and schema versions, contents, every
  entry with size and SHA-256, counts, accounts, plug-ins). A wrong
  passphrase fails on the first chunk: `auth_failed`
  (`backup-wrong-passphrase`). New passphrases need
  `MIN_BACKUP_PASSPHRASE_CHARS` (12) characters (`passphrase_issues`).
- **Requests**: `CreateBackup` (job), `InspectBackup` (manifest),
  `VerifyBackup` (job reading every entry; `checksum-mismatch` /
  `entry-missing`), `PreviewRestore` = the **dry run** (`RestorePlan`: per
  item `replace` / `merge` / `add` / `skip` with counts, issues, accounts
  needing passwords, restart needed), `StartRestore` (job; `RestoreReport`
  with the configuration backups of replaced files, revertible with
  `RevertConfig`). Configuration files and settings are replaced; mail,
  PIM data, keys and plug-ins are merged; nothing is deleted.

### Vault recovery key (`nail_api::recovery`)

- 32 random bytes; text form = Crockford base32 (52 characters) + 4 check
  characters (top 20 bits of CRC-32) in 14 groups of 4
  (`format_recovery_key` / `parse_recovery_key`: case, spaces, dashes,
  `O`→`0`, `I`/`L`→`1` forgiven; `RecoveryKeyIssue` empty /
  invalid-character / wrong-length / checksum-mismatch). `nail-vault`
  derives a key from it that wraps the data key; the wrapped key is stored
  next to the data (WP122 decides where so it survives full-database
  encryption), never the recovery key.
- `VaultStatus` gains `needs_recovery` (encrypted data, no key in the
  keychain) and `recovery: RecoveryKeyStatus` (configured, created,
  verified). Requests (desktop only): `CreateRecoveryKey {save_to}` →
  `RecoveryKeyCreated` (the key shown once; optional recovery-kit file;
  `conflict` + `vault-not-encrypted` on a plain vault), `VerifyRecoveryKey`
  → `RecoveryKeyVerified {valid}`, `RestoreVaultKey` → `VaultStatus`
  (`auth_failed` + `recovery-key-wrong`), `RemoveRecoveryKey`.

### Smartcards (`nail_api::smartcard`)

- `SmartcardSettings` (PKCS#11 module paths, absolute; PIN cache 0–480
  min) stored by the core (`security.smartcards`). `ListSmartcards` →
  tokens (`SmartcardToken`: module, slot, label, serial, PIN state) with
  objects (`SmartcardObject`: kind, `CKA_ID`, label, algorithm, usages,
  certificate facts with the SHA-256 fingerprint, OpenPGP fingerprint, the
  keyring entry using it) plus per-module problems (`SmartcardError`).
  `UseSmartcardKey {key: SmartcardKeyRef, technology}` adds a keyring entry
  whose secret stays on the card: `SecretStorage::Smartcard`,
  `KeySource::Smartcard`, `CryptoKey.smartcard`.
- **PIN prompts**: backends ask `PinSource::request_pin(PinRequest)`; the
  core sends `Event::PinRequested {prompt: PinPrompt}` (token, purpose
  login / sign / decrypt, key label, attempts left, `retry`, expiry =
  `PIN_PROMPT_TIMEOUT_SECS`) and waits for `AnswerPinPrompt {prompt_id,
  pin}` (`null` = cancel); `Event::PinPromptClosed {reason}` closes stale
  dialogs; `Event::SmartcardsChanged` on insert / remove. PINs are never
  stored, logged or sent to remote sessions; all smartcard requests and
  events are desktop-only.
- The PKCS#11 code lives in the new crate **`nail-pkcs11`** (owned by
  WP122) on `cryptoki`; `nail-pgp` and `nail-smime` depend on it.

### Command palette (`nail_api::palette` + `ui/src/actions/palette.ts`)

- The core keeps the **recent actions** (`RecentActions`, most recent
  first, counted, at most 20, `app_settings` `palette.recent`):
  `GetRecentActions`, `RecordActionUsed {action_id}` (`invalid_request` +
  `palette-invalid-action` for malformed ids), `ClearRecentActions` — served
  now. `ActionAvailability` (`global`, `list`, `reader`, `composer`,
  `calendar`, `contacts`, `tasks`, `desktop`; the layout's key-binding
  contexts where they exist).
- The action list is assembled in the UI: `Feature.actionInfo` (optional;
  `ActionInfo {title, keywords, availability, hidden}`, merged by the
  registry with duplicate checks) and `buildPalette` (titles from layout
  labels → descriptions → plug-in titles, keywords, `shortcutsFor` the
  layout's argument-less bindings, availability, layout aliases, recent
  first). WP124 adds descriptions of built-in / other features' actions in
  its own folder, the fuzzy search and the overlay.

### Printing (`nail_api::print`)

- A **print job** (`CreatePrintJob {target, options}` → `PrintJob` with id,
  route `#/print/<id>`, `ready`) records what (`PrintTarget` message /
  thread / event occurrence / agenda `start`..`end` with calendars and an
  `AgendaLayout`; agendas checked: valid dates, end ≥ start, ≤ 366 days)
  and how (`PrintOptions`: page size A4 / A5 / Letter / Legal,
  orientation, basic / full headers, quoted text, attachment list, remote
  images, backgrounds). The UI's print view reads it (`GetPrintJob`),
  renders it with the theme's **`print.*` parts** (the "print variant" is
  a part namespace; no new theme syntax) and reports `PrintViewReady`.
  Printing = the system dialog (`window.print()`, also "Save as PDF" in
  web mode). **PDF files** (desktop): `SavePrintPdf {job_id, path}` →
  `DesktopHost::print_to_pdf` (hidden window on the route, waits for
  `print_view_ready`, WebView2 `PrintToPdf`) → `PdfSaved`. Jobs live in
  memory for 10 minutes. Served now (PDF through the host; `unsupported` +
  `desktop-unavailable` without one).

### Diagnostics bundle (`nail_api::diagnostics`)

- User-initiated: `PreviewDiagnostics {options}` returns **exactly** what
  `CreateDiagnosticsBundle {options, path}` writes (`DiagnosticsManifest`:
  every entry with item, size, redactions and a 4 KiB text preview) plus
  `manifest.json`; the bundle answer carries size and SHA-256.
  `DiagnosticsItem`: system info, logs, config, account status, store
  stats, operation queue, plug-ins — and, opt-in only, `mail-headers` /
  `mail-content` of at most 20 named messages (`DiagnosticsIssue` checks).
  Secrets are always removed; addresses / subjects / the user name in
  paths are redacted by default (`Redaction`). WP121 writes the zip
  (`nail-portability`); the core collects the content (WP125).

### File dialogs

- `PickPath {pick: PathPick}` (open file / open folder / save file, a
  Fluent title id, filters, default path) → `PathPicked {path}` through
  `DesktopHost::pick_path`; without a host `unsupported`
  (`desktop-unavailable`) and the UI asks for a typed path (local web UI).
  `DesktopHost` gains `pick_path`, `print_to_pdf`, `print_view_ready` with
  defaults, so the shell compiles unchanged until WP125 implements them.

### Benchmark harness (`crates/nail-bench`, `e2e/perf/`)

- `CorpusSpec` (`standard()` = 100,000 messages in 40 folders, thread /
  attachment / HTML ratios, mean body size, fixed seed; `small()` = 2,000),
  `MetricName` (initial sync s, search p95 ms, list page p95 ms, cold start
  ms, peak memory MiB — Rust; scroll frame p95 ms, long frames — Playwright),
  `Metric`, `Budget`, `Budgets` (`e2e/perf/budgets.json`, every metric has
  one; `check`), `BenchReport`. Budgets are tightened, not loosened, without
  an ADR.

### Exchange / Gmail leftovers

- **Send as / on behalf** for shared mailboxes: `Identity.send_as:
  SendAs {mode: send-as | on-behalf, sender}` (`accounts.kdl`: `identity
  "team@x" send-as="on-behalf" sender="ada@x"`; `sender` only with
  on-behalf → `config-sender-needs-on-behalf`). `nail-compose` writes
  `Sender:` and uses the sender as the envelope `MAIL FROM` for on-behalf
  (done). API engines decide the mailbox from the built message with
  `accounts::SendingMailbox::of(from, sender, own)` (Graph `/users/{mailbox}
  /sendMail` or `from` / `sender`, EWS `From` / `Sender`) — WP123.
- **Upload sessions**: `OutgoingTransport::send_with_progress(envelope,
  message, progress)` (default: `send`, no progress); the core turns
  progress into `Event::SendProgress {outbox_id, sent_bytes, total_bytes}`
  (remote sessions see it like `SendStatus`). WP123 implements Graph upload
  sessions over 4 MB.
- **Server threads**: store v15 `messages.remote_thread_id`;
  `MessageMeta.remote_thread_id` (merge) and
  `Store::set_message_remote_thread_id` map `remote-thread:<account>:<id>`
  into `thread_refs`, so a Gmail `threadId` / Graph or EWS conversation id
  groups messages (still merging with Message-ID links; never by subject).
  Engines report it in `RemoteMessage.thread_id` (exists); the core passes
  it through (WP125).
- Exchange folder sync policies and moves between mailboxes / accounts for
  API engines stay within WP123's crates plus WP125's core wiring (no new
  contract).

### Store v15 (`nail-store`)

- `crypto_keys` rebuilt (foreign keys off) so `source` / `secret_storage`
  may be `smartcard` (`secret_ref` = the JSON of the `SmartcardKeyRef`, no
  `secret_data`); `messages.remote_thread_id` + index. Tests: schema
  upgrade from v14 keeps keys and their e-mail rows, server-thread
  grouping.

### Remote access

- Desktop-only (`forbidden` remotely): every portability / backup /
  restore / diagnostics request, `PickPath`, the recovery key, smartcards,
  `SavePrintPdf`. Session (no mail data): the recent-action requests.
  `Scope(read)`: `CreatePrintJob` (the core checks the target: a message
  must be readable, threads are narrowed when read, events / agendas need
  the full PIM grant), `GetPrintJob`, `PrintViewReady`. Events:
  portability, PIN and smartcard events never go to remote sessions.

### Libraries

- New: `cryptoki` 0.12.1 (Apache-2.0; `cryptoki-sys` with pre-generated
  bindings, `libloading` (ISC), `secrecy`, `bitflags`) — loads the vendor
  module at run time; builds on Windows MSVC without bindgen / libclang.
  `cargo deny check licenses bans sources` passes.
- `nail-portability` uses the workspace `zip`, `argon2`,
  `chacha20poly1305` (feature `stream`), `sha2`, `rusqlite`,
  `mail-parser`; `nail-bench` the workspace `rand`, `clap`, `windows-sys`
  (process memory). The lock is final for the parallel stage except
  WP122's `[patch.crates-io]` for SQLite3MultipleCiphers (MIT) if it
  builds on MSVC (else the documented fallback, ADR by WP122).

### Served now

`nail-core` serves the palette's recent actions (`palette.rs`), print jobs
and PDF through the host (`print.rs`), the smartcard settings
(`smartcard.rs`) and the file dialog through the host (`portability.rs`).
Everything else answers `unsupported` (`m4c-not-ready`) until WP125 wires
`nail-portability`, `nail-pkcs11` / `nail-vault` and the shell.
`VaultStatus` reports `needs_recovery: false` and an empty recovery status
until then. Tests: `nail-core/tests/m4c.rs`, nail-api unit tests (every
labeled enum's wire names and Fluent ids, recovery-key round trips, agenda
ranges, report caps), `nail-remote-auth` (`m4c_requests`), store v15 and
threads tests, nail-config (send-as), nail-compose (on-behalf), the UI
mocks and the palette model.

### Collision-free slots

| Package | Owns |
| --- | --- |
| WP121 | `crates/nail-portability/` (new; no dependency on `nail-core`), `locales/*/core-portability.ftl` below its "WP121" heading, `userDocs/en/` pages on import / export / backup (new files) |
| WP122 | `crates/nail-vault/`, `crates/nail-store/`, `crates/nail-pgp/`, `crates/nail-smime/`, `crates/nail-pkcs11/` (new), the root `Cargo.toml` `[patch.crates-io]` section only, `locales/*/core-vault.ftl` below its "WP122" heading |
| WP123 | `crates/nail-proto-graph/`, `crates/nail-proto-gmail/`, `crates/nail-proto-ews/`, `crates/nail-exchange/`, `crates/nail-bench/` (new), `e2e/perf/` (new) |
| WP124 | `ui/src/features/portability/`, `ui/src/features/palette/`, `ui/src/features/print/` (new, registered, mocks routed), the M4c additions in `ui/src/features/security/` (`vaultMock.ts` and new files), `locales/*/{portability,palette,print}.ftl` and the security texts in `security.ftl` |

Shared slots made now: `Feature.actionInfo` + `ui/src/actions/palette.ts`;
mock state `data.features.{portability,palette,print,vault}`;
`nail-config/src/widgets/portability.rs` (future widgets `import-export`,
`backup-restore`, `diagnostics`) and `security-recovery-key` /
`security-smartcards` in `widgets/security.rs`; core modules
`portability.rs`, `print.rs`, `palette.rs`, `smartcard.rs`. Contract changes
(nail-api, nail-core, nail-config, shared UI files, `defaults/*.kdl`) go to
WP125 as apiRequests.

## Consequences

- WP125 wires the jobs into the core (runtime tasks, progress events,
  Local Folders account, queue appends), the recovery key and smartcards
  (PIN prompts over events, `PinSource`), the diagnostics collection, the
  shell's `pick_path` / `print_to_pdf`, the layout (Settings tabs, palette
  key binding, print actions, `print.*` theme parts) and the Playwright
  scenarios of plan WP125.
- `VaultStatus`, `Identity`, `CryptoKey` and `MessageMeta` gained fields
  (TS: `VaultStatus` fields are required, others optional); code building
  them was updated.
- German texts of the new catalogs come later (empty `de` files).
