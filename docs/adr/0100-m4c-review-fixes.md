# ADR 0100: M4c review fixes: recovery kit before the key, validated settings restore, own keychain entries only, linear Mork, send progress in the UI

Status: accepted · 2026-10-02 · M4c review

## Context

The M4c reviews of the wave integrated by ADR 0099 confirmed five defects
and three smaller gaps:

- **Recovery key lost on a failed kit write.** `CreateRecoveryKey` stored
  the new wrapped data key in `vault.json` and wrote the recovery kit
  afterwards. A kit that could not be written (the file exists — the Save
  dialog had just asked "Replace?" — or an I/O error) failed the request:
  the new key was never shown, and the old one no longer opened the data.
- **Settings restore without checks.** Every key of `settings.json` was
  written raw. A crafted backup could name any PKCS#11 module (loaded on
  the next card use, through the DLL search order for a relative path) or
  enable the remote UI on every address without TLS (the listener re-checked
  only internet mode).
- **Keys restore wrote any keychain entry.** A key's `reference` and
  `keychain_secret` from `keyring.json` were written unchecked, so a backup
  could overwrite an account password or the vault's data key.
- **Quadratic Mork reader.** Every transaction group cloned the whole
  parse state; a 1 MB `.mab` with many groups took about 30 s.
- **`SendProgress` unused.** The core sent it, the docs promised it, no UI
  showed it.
- Low: Maildir `cur` / `new` junctions were followed; restore entries were
  read into memory bounded only by the archive's own manifest; the
  recovery-key steps had no Playwright accessibility scan.

## Decisions

- **Deliver, then replace.** `nail_vault::Vault::create_recovery_key_with`
  takes a `deliver(key, created_at)` step that runs before `vault.json`
  changes; the core writes the kit there. A failing step leaves the old
  record. The core still never writes over an existing file (a remote or
  MCP caller must not overwrite files); the Save dialog's overwrite prompt
  is a follow-up.
- **Machine-bound security settings stay on their machine.**
  `EXCLUDED_SETTINGS` (backup and restore) now holds
  `security.smartcards`, `remote-ui` and `mcp` besides the certificate and
  the first-run state. Known keys (`security`, `scheduling`,
  `notifications`, `desktop.updates`, `desktop.settings`, `palette.recent`)
  must parse as their stored type and pass its checks
  (`nail_store::check_stored_notification_settings` for the notification
  subset); refused ones are skipped and reported as one `write-failed`
  issue of `settings.json` naming them, the valid ones are written. Unknown
  keys (sync state, caches) are restored as before.
- **Defense in depth.** Stored smartcard settings that fail
  `SmartcardSettings::issues()` load no module (`checked_settings`);
  `Running::start` refuses stored remote-UI settings with any
  `RemoteUiSettings::issues()` (status `failed`).
- **A restored key names only its own wrapping entry.** A key whose secret
  references a keychain entry, stores a keychain secret or uses keychain
  storage must reference exactly `keychain_ref(technology, fingerprint)`
  (`nail_pgp::keyring::keychain_ref`, `nail_smime::keyring::keychain_ref`,
  now public); otherwise the whole `keyring.json` is refused before
  anything is written, in the dry run too.
- **Mork undo log.** The parse state records changes only inside a group
  (`Undo` entries: dictionary values, cells, created and cut rows) and
  replays them backwards on `~abort~`. Rows carry a sequence number, so a
  cut is O(1) and the order is sorted once at the end. 20,000 rows in
  20,000 groups parse in milliseconds.
- **Send progress in the UI.** `UiState.sendProgress` (by outbox id) is set
  by `SendProgress` and cleared by any `SendStatus` other than `sending`;
  the outbox entry shows a `<progress>` (`outbox.item.progress`,
  `outbox-upload-progress`) and the status bar's send item says
  `status-sending-progress`. The mock core reports progress in 3.125 MiB
  pieces for messages over 4 MiB.
- **Low.** Maildir checks `cur` / `new` with `symlink_metadata` (links and
  junctions are not maildirs). `BackupArchive::read_checked` refuses
  entries larger than `max_in_memory_bytes` (config / keys 16 MiB, plug-ins
  `MAX_PACKAGE_BYTES`, PIM 256 MiB) as `too-large` before reading.
  `vault-recovery.spec.ts` scans each recovery-key step with axe; the
  smartcard PIN dialog stays Vitest-only (no token in `nail serve`).

## Consequences

- A backup restored on another computer no longer brings smartcard
  modules, remote access or MCP settings; the user sets them up there
  (userDocs `backup-restore.md`).
- Tests: `nail-vault` `a_failed_delivery_keeps_the_old_recovery_key`; core
  `m4c_vault.rs` (an existing kit file leaves the old key valid), backup
  unit tests (machine-bound / invalid settings, foreign keychain
  references, own reference restored), smartcard `checked_settings`;
  `nail-server` `stored_settings_are_checked_again_before_the_listener_starts`;
  Mork abort / cut-order / many-groups; Maildir junctions; oversized backup
  entries; Vitest status bar / outbox / App / mock progress.
