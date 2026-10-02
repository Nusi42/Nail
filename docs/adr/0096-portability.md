# ADR 0096: import, export and backup in `nail-portability`

Status: accepted · 2026-10-02 · M4c WP121

## Context

Plan section 9, WP121, against the contract of ADR 0094: streaming
importers (mbox, `.eml` folders, Maildir, Thunderbird profiles, `.ics` /
`.vcf`), exporters (mbox, `.eml`, Maildir, `.ics`, `.vcf`), the encrypted
`.nailbackup` archive with verification, a dry-run restore and a restore
report, and the diagnostics bundle writer. The core owns the data and
every write goes through the operation queues, so the crate does file work
only and talks to the core through traits. No new dependencies: the
workspace's `zip`, `argon2`, `chacha20poly1305` (`stream`), `sha2`,
`rusqlite`, `mail-parser` and `nail-ical` (read-only use) are enough.

## Decisions

### 1. Traits the core implements (`nail_portability::sink`, `backup`)

| Trait | Direction | Used by |
| --- | --- | --- |
| `MailImportSink` (`ensure_folder`, `contains` = duplicate check, `append`) | crate → core | imports, restores |
| `PimImportSink` (`event`, `task`, `contact`, `group`, `exists`) | crate → core | imports, restores |
| `MailExportSource` (`folders`, `messages` visitor) | core → crate | mail exports, backups |
| `PimExportSource` (`collections`, `items` visitor; items as `VCALENDAR` / `VCARD` text) | core → crate | `.ics` / `.vcf` exports, backups |
| `BackupSource` (`info`, `files` / `write_file`, `local_mail_accounts` / `local_mail`, `local_calendars`, `local_address_books`) | core → crate | backups |
| `RestoreTarget: MailImportSink + PimImportSink` (`current_file`, `replace_file` → config backup id, `merge_file` (dry run), `begin_mail_account`) | crate → core | restore plans and restores |

Sinks answer `SinkError::Item` (the item becomes a `write-failed` issue
and the job goes on) or `SinkError::Fatal` (the job fails). Folder paths
are `/`-separated paths below the import target; the sink resolves them
against `MailImportTarget` (Local Folders by default, or a folder of an
account through `AppendMessage`) and records `created_folders` itself.
Items are pushed one by one: the crate never holds a folder in memory.

Every job takes a `JobContext` (the `JobReport` the core created, a
`CancelFlag`, a progress callback throttled to one call per 250 ms plus
every phase change) and the core ends it with `job::finish_report`.

### 2. mbox

* Streamed line by line through `BufRead::fill_buf`; lines longer than
  1 MiB are read in pieces, a message above `MAX_IMPORT_MESSAGE_BYTES` is
  skipped while reading (only its size is kept). Memory is bounded by the
  largest accepted message (test: 50,000 generated messages through a
  generating reader, a 3 MiB message skipped).
* Separators: `From ` after an empty line or at the start; without the
  empty line only when the line carries an `asctime` date.
* Dialects: `>+From ` lose one `>` (mboxrd; for `>From ` the same as
  mboxo, the only quoting mboxo writes). mboxo files whose original text
  had `>From ` lines cannot be told apart — the format is ambiguous;
  exports default to mboxrd, which is reversible. `Content-Length`
  (mboxcl / mboxcl2) is trusted only when the message then ends at an
  empty line + `From ` line or the end of the file; bodies with unquoted
  `From ` lines are mboxcl2 (no unquoting), a wrong length falls back to
  separator mode, a file ending before the length is `truncated` (kept,
  warning), a file ending inside the headers is `truncated` (skipped).
* Flags: `Status` / `X-Status` / `X-Mozilla-Status` / `X-Mozilla-Keys`.
  Messages Thunderbird marked deleted (expunged bit, not compacted yet)
  or Maildir `T` are **not imported**; each is a skipped `unknown-flag`
  issue with the specific text `portability-issue-deleted-in-source`
  (contract follow-up: a dedicated code).
* Previews count files above 64 MiB from their first 64 MiB and
  extrapolate by size (`estimated`).
* The writer adds no headers (flags are not written into messages), so a
  folder exported and imported again gives the same bytes; the only
  change is a final line end added to a message without one, which mbox
  cannot represent.

### 3. Maildir and `.eml`

* Maildir: the root is `Inbox`, Maildir++ `.A.B` folders are `A/B`
  (modified UTF-7 decoded), nested `A/cur` trees too; missing parents are
  added. `cur` then `new` (messages in `new` are unseen), `tmp` ignored.
  Info separators `:`, `;` and `!` (Windows cannot store `:`); the writer
  uses `;` on Windows. Flags `D F R S T` (+`P` ignored), keyword letters
  through `dovecot-keywords`, Courier keywords through
  `courierimapkeywords/:list` (a name Windows cannot create, so it only
  appears in maildirs read on other systems). Unknown letters are
  warnings. Delivery time from the file name, else the file time.
* `.eml` folders: the folder itself is a folder named after it,
  subfolders become subfolders; files are read in name order, `.eml`
  only, symbolic links are never followed, depth ≤ 32.
* Export layout: one top folder becomes the target itself, else each top
  folder a subfolder (`.Top.Sub` in Maildir++; dots in names become `_`
  with an `invalid-folder-name` warning; more than 26 keywords per folder
  are dropped with a warning). mbox: one folder → the target file,
  several → `A.mbox`, `A/B.mbox`. Single files are written as
  `<name>.nail-part` and renamed when complete (cancelled or failed
  exports leave nothing behind); mbox exports are read back and counted
  (`verifying`).

### 4. Thunderbird profiles

* `profiles.ini` + `installs.ini` (`[Install…] Default=`) → found
  profiles, default first.
* `prefs.js`: a small JavaScript-literal parser (strings with escapes,
  integers, booleans). Accounts in `mail.accountmanager.accounts` order.
  IMAP / POP3 become `AccountProposal`s: host, port (protocol default when
  missing), `socketType` 3 → implicit TLS, 2 → STARTTLS, 1 ("STARTTLS if
  available") and 0 (plain) → **STARTTLS with a warning** (Nail never
  falls back to plain text); `authMethod` 3 → password, 7 → client
  certificate, 10 → OAuth2 for Google / Microsoft hosts, other methods
  → password with a warning; identities (name, reply-to, Bcc, signature
  text / HTML; signature files are a warning), SMTP from the identity or
  the default server, POP3 leave-on-server / delete-after-days, poll
  interval. Every proposal carries `secret-not-imported`; `logins.json`
  is never read. Feeds, news and other types are skipped issues.
  `mark_duplicates` (for the core) matches host + user name or address.
* Local mail: Local Folders and every POP3 / movemail account's directory
  (`directory-rel` `[ProfD]…`, else `directory`), named
  `<account name>/<folder>`, mbox files plus `Name.sbd` subfolders,
  `.msf` and other non-mail files ignored, roles from the names below the
  account. IMAP caches are not imported (the server has the mail).
* Address books: `ldap_2.servers.*.filename` plus `abook.sqlite` /
  `history.sqlite`. SQLite (`properties` / `lists` / `list_cards`) is read
  read-only; cards with `_vCard` (Thunderbird 102+) are taken as they
  are, older cards are built as vCard 4 from the named properties; lists
  become group cards. Mork `.mab` (before Thunderbird 78) is read by a
  small parser (dictionaries, tables, rows, cuts, `\` / `$XX` escapes,
  transaction groups with aborts); a `.mab` migrated to SQLite is not read
  twice. Everything then goes through `nail_ical::vcard::parse_cards`, so
  contacts take the same path as `.vcf` imports. Names: the book's
  description, else the file stem (Thunderbird's default names are
  `chrome://` references, not text).
* Filters (`msgFilterRules.dat` per server): conditions evaluated left to
  right like Thunderbird (`AND` / `OR` folded into `all` / `any`), quoted
  values with escapes; subject, body, from / to / cc / to-or-cc / all
  addresses, custom headers (incl. empty / not empty), date, age, size
  (KB), status read / replied / flagged, tags, attachments. Actions move /
  copy (own account's folders, or Local Folders →
  `FolderRef::Path { "Local Folders/…", account: local-folders }`),
  delete, mark read / unread, flag, tags (old labels → `$labelN`),
  forward, stop. Triggers: new mail / after classification → arrival,
  after sending → on-send, periodic → schedule
  (`periodicFilterRateMinutes`, default 10), otherwise manual. Anything
  else (priority, junk, "is in address book", other accounts' folders,
  reply templates, archiving trigger, …) is dropped with a
  `filter-not-expressible` issue naming it, `complete = false`, and the
  rule starts disabled. Account filters get an empty scope plus
  `account_key`: the UI sets the scope to the account created from the
  proposal.

### 5. Backups (`.nailbackup`)

* Encryption layer (`backup::crypto`): magic, length-prefixed JSON header
  (format, `argon2id` m / t / p, 16-byte salt, cipher
  `xchacha20poly1305-stream-be32`, 19-byte nonce prefix, chunk size), then
  XChaCha20-Poly1305 over 64 KiB chunks with the nonce `prefix ‖ counter
  (u32 BE) ‖ last flag` — byte-identical to `aead::stream::StreamBE32`
  (tested against `EncryptorBE32`) — and the whole prefix as associated
  data of every chunk, so a changed header (e.g. the KDF cost) fails.
  Argon2id 64 MiB / 3 passes / 1 lane; the writer refuses less (except
  `KdfParams::insecure_for_tests`, test builds and the crate feature
  `testing` only), the reader refuses absurd costs from a crafted header
  (> 1 GiB, > 16 passes or lanes). Truncation is caught by the last-chunk
  flag.
* Reading is **random access**: `DecryptingReader` implements `Seek`,
  decrypting and authenticating the chunk it needs, so the zip is read
  through its central directory (`ZipArchive`) without decrypting to disk
  or holding the archive in memory; nothing unauthenticated is returned.
  A wrong passphrase fails on the first chunk (`WrongPassphrase` →
  `auth_failed` / `backup-wrong-passphrase`).
* Writing streams the zip (`ZipWriter::new_stream`, deflate, zip64 for
  large entries) into the encrypting writer. Because `manifest.json` must
  come first with every entry's SHA-256, each entry is **produced twice**
  (hashing pass, writing pass); an entry that differs between the passes
  fails the backup (`SourceChanged`) instead of producing an archive that
  does not verify. The file is written as `.nail-part`, renamed, then
  opened again and every entry verified (`verifying`).
* Layout: `config/<file>` (settings, theme, layout, rules, accounts —
  names chosen by the core), `keys/<file>`, `plugins/<file>`, local mail
  as `mail/<account>/<n>.mbox` (mboxrd) plus `mail/<account>/index.json`
  (folder paths and per-message flags, keywords and delivery times, so
  flags survive without touching the messages), local PIM data as
  `pim/<n>.ics` / `pim/<n>.vcf` plus `pim/index.json` (kind, key, name).
  Entry names are validated (relative, no `..`, `\`, `:`).
* Restore: `BackupArchive::plan` (the dry run, `PreviewRestore`) and
  `restore` share one code path with `dry_run`. Configuration items are
  compared with the current file: missing → add, equal → unchanged
  (skip), different → replace (the core returns the configuration backup
  id → `RestoreReport.config_backups`). Mail is merged through the import
  path (duplicates skipped by Message-ID + date + size, flags from the
  index); PIM items are counted as replaced when their `UID` exists, then
  pushed through the PIM sink (same `UID` = update); keys and plug-ins go
  to `merge_file`. Damaged or missing entries are issues and left out.
  Accounts restored → `accounts_needing_secrets` = the manifest's accounts
  and a plural note; settings or plug-ins restored → `restart_required`.

### 6. Diagnostics

`diagnostics::prepare` redacts what the core collected and builds the
manifest the preview shows; `write_bundle` writes exactly those entries
plus `manifest.json` and returns size and SHA-256. Secrets are always
removed (secret-looking keys in `key=value` / `key: value` / JSON,
`Bearer` / `Basic` credentials, `keychain:` references, PEM private
keys); with `redact_addresses`, addresses become stable
`userN@example.invalid` placeholders and subjects / display names are
removed; the user name in home folder paths becomes `<user>`. Chosen mail
items are kept unredacted (the user picked those messages); non-text
entries are refused outside mail items because they cannot be redacted.

## Consequences

* WP125 implements the traits in the core (store, queues, configuration
  manager, keyring, plug-in manager), runs the jobs on the runtime, maps
  `PortabilityError::to_api_error` / `finish_report`, and calls
  `thunderbird::find_profiles(default_data_dir())` for
  `FindImportSources` and `mark_duplicates` for previews.
* The Thunderbird fixture in `crates/nail-portability/tests/fixtures`
  (synthetic; `abook.sqlite` / `history.sqlite` regenerated with
  `NAIL_REGENERATE_FIXTURES=1`, checked on every run) is the one the
  WP125 Playwright scenario imports.
* Not covered: Thunderbird calendars (`calendar-data/local.sqlite`) — the
  plan lists accounts, local folders, address books and filters; `.ics`
  exports from Thunderbird import through the `.ics` path.
