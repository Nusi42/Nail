//! Nail's data portability (plan M4c WP121; contract ADR 0094,
//! `nail_api::portability`, `nail_api::backup`, `nail_api::diagnostics`;
//! design ADR 0096-portability).
//!
//! Pure file work behind traits the core implements ([`sink`]): importers
//! push messages and PIM items into a [`sink::MailImportSink`] /
//! [`sink::PimImportSink`]; exporters and backups pull from a
//! [`sink::MailExportSource`] / [`sink::PimExportSource`] /
//! [`backup::BackupSource`]; restores write through a
//! [`backup::RestoreTarget`]. Every job reports progress and per-item
//! issues through a [`job::JobContext`].
//!
//! * **Importers** ([`import`]): mbox ([`mbox`]: mboxrd / mboxo / mboxcl /
//!   mboxcl2, streamed), folders of `.eml` files ([`eml`]), Maildir
//!   ([`maildir`]: `cur` / `new`, `:2,` flags, Dovecot / Courier keywords,
//!   Maildir++ and nested trees), Thunderbird profiles ([`thunderbird`]:
//!   local folders, address books from `abook.sqlite` / `history.sqlite` /
//!   Mork `.mab`, `prefs.js` accounts and `msgFilterRules.dat` filters as
//!   proposals), `.ics` / `.vcf` ([`pim`]).
//! * **Exporters** ([`export`]): mbox, `.eml`, Maildir, `.ics`, `.vcf`.
//! * **Backups** ([`backup`]): the encrypted `.nailbackup` archive
//!   (Argon2id + XChaCha20-Poly1305 STREAM over a zip with a manifest),
//!   verification, restore plans (dry run) and restores.
//! * **Diagnostics** ([`diagnostics`]): redaction and the bundle writer.

pub mod backup;
pub mod diagnostics;
pub mod eml;
pub mod error;
pub mod export;
pub mod files;
pub mod folder;
pub mod import;
pub mod job;
pub mod maildir;
pub mod mbox;
pub mod message;
pub mod pim;
pub mod sink;
pub mod thunderbird;

pub use error::{PortabilityError, Result, SinkError};
pub use job::{CancelFlag, JobContext};

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
