//! Nail's data portability (plan M4c WP121; contract ADR 0094,
//! `nail_api::portability`, `nail_api::backup`, `nail_api::diagnostics`).
//!
//! * **Importers** read mbox (mboxrd / mboxo, streamed), folders of `.eml`
//!   files, Maildir (Courier / Dovecot flags and folder trees), Thunderbird
//!   profiles (local folders, address books, account and filter proposals)
//!   and `.ics` / `.vcf` files, yielding messages / PIM items with per-item
//!   [`nail_api::portability::ItemIssue`]s.
//! * **Exporters** write mbox, `.eml`, Maildir, `.ics` and `.vcf`.
//! * **Backups**: the encrypted `.nailbackup` archive
//!   ([`nail_api::backup::BACKUP_MAGIC`], Argon2id + XChaCha20-Poly1305
//!   STREAM over a zip with a manifest), verification and restore plans.
//! * **Diagnostics**: the bundle writer with redaction and an exact
//!   manifest.
//!
//! Slot of WP121: this crate is a stub until then.

/// The backup archive format version this crate writes.
pub fn backup_format_version() -> u32 {
    nail_api::backup::BACKUP_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_the_contract_format() {
        assert_eq!(super::backup_format_version(), 1);
        assert_eq!(nail_api::backup::BACKUP_MAGIC, b"NAILBAK1");
    }
}
