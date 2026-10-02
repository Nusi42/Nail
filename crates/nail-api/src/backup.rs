//! Backup and restore (M4c, ADR 0094).
//!
//! A backup is one passphrase-encrypted archive (`.nailbackup`) holding the
//! chosen [`BackupItem`]s. It never holds secrets: account passwords and
//! tokens stay in the OS keychain (`accounts.kdl` only has `keychain:`
//! references), remote-UI users, MCP clients and pairing codes are not
//! backed up at all, and secret keys ([`BackupItem::Keys`], off by default)
//! are written only inside the encrypted archive.
//!
//! # Archive format (version [`BACKUP_FORMAT_VERSION`])
//!
//! ```text
//! magic "NAILBAK1" (8 bytes)
//! header length (u32 LE) + header JSON (BackupHeader: format version,
//!   KDF = Argon2id with its m / t / p and a 16-byte salt, the cipher and
//!   the 19-byte STREAM nonce prefix) — authenticated as associated data
//! ciphertext: XChaCha20-Poly1305 STREAM (64 KiB chunks, last-chunk flag)
//!   over a zip archive: `manifest.json` (BackupManifest) first, then one
//!   entry per file with the SHA-256 the manifest lists
//! ```
//!
//! The key is Argon2id(passphrase, salt) with at least 64 MiB memory, 3
//! passes. A wrong passphrase fails on the first chunk (`auth_failed`,
//! `backup-wrong-passphrase`) before anything is read. `nail-portability`
//! (WP121) implements the format; the constants live here so the core and
//! the tests agree.
//!
//! # Restore
//!
//! [`crate::Request::PreviewRestore`] is the dry run: it reads the archive
//! and says per item what would happen ([`RestorePlan`]) without writing.
//! [`crate::Request::StartRestore`] then restores the chosen items:
//! configuration files are replaced (the current ones are kept as
//! configuration backups, revertible like any edit), mail, PIM data and
//! keys are merged (nothing is deleted; duplicates are skipped), settings
//! are replaced key by key.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, ConfigBackupId, PluginId};
use crate::localized::LocalizedMessage;
use crate::portability::{ItemIssue, JobCounts};

/// File extension of backup archives (without the dot).
pub const BACKUP_FILE_EXTENSION: &str = "nailbackup";

/// The archive format version this build writes and reads.
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// The first bytes of every backup archive.
pub const BACKUP_MAGIC: &[u8; 8] = b"NAILBAK1";

/// Shortest passphrase accepted for a new backup (characters).
pub const MIN_BACKUP_PASSPHRASE_CHARS: usize = 12;

/// Longest passphrase accepted (characters).
pub const MAX_BACKUP_PASSPHRASE_CHARS: usize = 1024;

/// Name of the manifest entry inside the archive's zip.
pub const BACKUP_MANIFEST_ENTRY: &str = "manifest.json";

labeled_enum! {
    /// What a backup can hold (`backup-item-*`).
    pub enum BackupItem : "backup-item" {
        /// App settings from the store (security, notifications, sync
        /// policies, desktop, update and MCP settings, recent palette
        /// actions; never tokens or remote-UI users).
        Settings = "settings",
        /// `theme.kdl` (and the themes it extends from the config folder).
        Theme = "theme",
        /// `layout.kdl`.
        Layout = "layout",
        /// `rules.kdl` and saved searches.
        Rules = "rules",
        /// `accounts.kdl`: accounts, identities, signatures, PIM sources
        /// (`keychain:` references only — passwords must be entered again).
        Accounts = "accounts",
        /// Mail of the local accounts ("Local Folders", opened files).
        LocalMail = "local-mail",
        /// The OpenPGP / S/MIME keyring with trust decisions; secret keys
        /// included (off by default).
        Keys = "keys",
        /// Calendars, task lists and address books of the local PIM source.
        PimLocal = "pim-local",
        /// Installed plug-ins (packages, grants, settings, storage).
        Plugins = "plugins",
    }
}

impl BackupItem {
    /// Whether a new backup includes it unless the user changes it
    /// (everything but the keys).
    pub fn default_on(self) -> bool {
        self != Self::Keys
    }

    /// Whether restoring it replaces what is there (configuration files,
    /// settings); else it is merged.
    pub fn restore_replaces(self) -> bool {
        matches!(
            self,
            Self::Settings | Self::Theme | Self::Layout | Self::Rules | Self::Accounts
        )
    }
}

/// The items of a backup or restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupContents {
    /// The chosen items (no duplicates).
    pub items: Vec<BackupItem>,
}

impl Default for BackupContents {
    fn default() -> Self {
        Self {
            items: BackupItem::ALL
                .iter()
                .copied()
                .filter(|i| i.default_on())
                .collect(),
        }
    }
}

impl BackupContents {
    /// Everything.
    pub fn all() -> Self {
        Self {
            items: BackupItem::ALL.to_vec(),
        }
    }

    /// Whether `item` is chosen.
    pub fn includes(&self, item: BackupItem) -> bool {
        self.items.contains(&item)
    }

    /// Problems: nothing chosen, or an item twice.
    pub fn issues(&self) -> Vec<BackupIssue> {
        let mut issues = Vec::new();
        if self.items.is_empty() {
            issues.push(BackupIssue::NothingChosen);
        }
        let mut seen = Vec::new();
        for item in &self.items {
            if seen.contains(item) {
                issues.push(BackupIssue::DuplicateItem);
                break;
            }
            seen.push(*item);
        }
        issues
    }
}

labeled_enum! {
    /// Why a backup request is refused (`backup-issue-*`).
    pub enum BackupIssue : "backup-issue" {
        /// No item chosen.
        NothingChosen = "nothing-chosen",
        /// An item listed twice.
        DuplicateItem = "duplicate-item",
        /// The passphrase is shorter than [`MIN_BACKUP_PASSPHRASE_CHARS`].
        PassphraseTooShort = "passphrase-too-short",
        /// The passphrase is longer than [`MAX_BACKUP_PASSPHRASE_CHARS`].
        PassphraseTooLong = "passphrase-too-long",
    }
}

/// Problems of a new backup's passphrase (empty = fine). Restores accept
/// any passphrase (the archive decides).
pub fn passphrase_issues(passphrase: &str) -> Vec<BackupIssue> {
    let chars = passphrase.chars().count();
    if chars < MIN_BACKUP_PASSPHRASE_CHARS {
        vec![BackupIssue::PassphraseTooShort]
    } else if chars > MAX_BACKUP_PASSPHRASE_CHARS {
        vec![BackupIssue::PassphraseTooLong]
    } else {
        Vec::new()
    }
}

/// One file inside the archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupEntry {
    /// The item it belongs to.
    pub item: BackupItem,
    /// Its path inside the archive (`config/theme.kdl`,
    /// `mail/local-folders/Inbox.mbox`, …).
    pub name: String,
    /// Uncompressed size.
    pub size_bytes: u64,
    /// SHA-256 of the uncompressed bytes (lowercase hex).
    pub sha256: String,
}

/// What an archive holds (its `manifest.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupManifest {
    /// [`BACKUP_FORMAT_VERSION`] of the writer.
    pub format_version: u32,
    /// The writing Nail's version.
    pub app_version: String,
    /// The store schema version at backup time.
    pub schema_version: u32,
    /// When it was written (RFC 3339).
    pub created_at: String,
    /// The items it holds.
    pub contents: BackupContents,
    /// Every file, in archive order.
    pub entries: Vec<BackupEntry>,
    /// What it holds, counted.
    pub counts: JobCounts,
    /// Accounts in its `accounts.kdl` (their passwords must be entered
    /// again after a restore).
    #[serde(default)]
    pub accounts: Vec<AccountId>,
    /// Plug-ins it holds.
    #[serde(default)]
    pub plugins: Vec<PluginId>,
}

impl BackupManifest {
    /// Total uncompressed bytes.
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.size_bytes).sum()
    }

    /// The entries of one item.
    pub fn entries_of(&self, item: BackupItem) -> impl Iterator<Item = &BackupEntry> {
        self.entries.iter().filter(move |e| e.item == item)
    }
}

labeled_enum! {
    /// What restoring an item does (`restore-effect-*`).
    pub enum RestoreEffect : "restore-effect" {
        /// Replaces the current file / settings (the current file is kept
        /// as a configuration backup).
        Replace = "replace",
        /// Adds what is missing; nothing is deleted.
        Merge = "merge",
        /// Nothing there yet: everything is added.
        Add = "add",
        /// Not restored (not chosen, not in the archive, or identical).
        Skip = "skip",
    }
}

/// What a restore does (plan) or did (report) with one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RestoreAction {
    /// The item.
    pub item: BackupItem,
    /// What happens.
    pub effect: RestoreEffect,
    /// Entries / records added.
    #[serde(default)]
    pub added: u64,
    /// Entries / records replaced.
    #[serde(default)]
    pub replaced: u64,
    /// Entries / records left out (identical or already there).
    #[serde(default)]
    pub unchanged: u64,
    /// A note for the user (e.g. "3 accounts need their password").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<LocalizedMessage>,
}

/// The dry run of a restore ([`crate::Request::PreviewRestore`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RestorePlan {
    /// The archive's manifest.
    pub manifest: BackupManifest,
    /// One action per item in the archive.
    pub actions: Vec<RestoreAction>,
    /// Problems found while reading (corrupt entries are listed here and
    /// left out of the restore).
    #[serde(default)]
    pub issues: Vec<ItemIssue>,
    /// Accounts whose passwords must be entered after the restore.
    #[serde(default)]
    pub accounts_needing_secrets: Vec<AccountId>,
    /// Whether Nail must restart afterwards (settings the running core
    /// read at start, plug-ins).
    #[serde(default)]
    pub restart_required: bool,
}

/// What a restore did (in [`crate::portability::JobReport::restore`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RestoreReport {
    /// What happened per item.
    pub actions: Vec<RestoreAction>,
    /// Configuration backups of the replaced files (revert with
    /// `RevertConfig`).
    #[serde(default)]
    pub config_backups: Vec<ConfigBackupId>,
    /// Accounts whose passwords must be entered.
    #[serde(default)]
    pub accounts_needing_secrets: Vec<AccountId>,
    /// Whether Nail must restart now.
    #[serde(default)]
    pub restart_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn labels_match_the_wire() {
        assert_labels(BackupItem::ALL, BackupItem::as_str, BackupItem::message_id);
        assert_labels(
            BackupIssue::ALL,
            BackupIssue::as_str,
            BackupIssue::message_id,
        );
        assert_labels(
            RestoreEffect::ALL,
            RestoreEffect::as_str,
            RestoreEffect::message_id,
        );
        assert_eq!(BackupItem::PimLocal.message_id(), "backup-item-pim-local");
    }

    #[test]
    fn default_contents_leave_out_the_keys() {
        let contents = BackupContents::default();
        assert!(!contents.includes(BackupItem::Keys));
        assert!(contents.includes(BackupItem::LocalMail));
        assert_eq!(contents.items.len(), BackupItem::ALL.len() - 1);
        assert!(contents.issues().is_empty());
        assert!(BackupContents::all().includes(BackupItem::Keys));
        assert_eq!(
            BackupContents { items: vec![] }.issues(),
            vec![BackupIssue::NothingChosen]
        );
        assert_eq!(
            BackupContents {
                items: vec![BackupItem::Theme, BackupItem::Theme]
            }
            .issues(),
            vec![BackupIssue::DuplicateItem]
        );
        assert!(BackupItem::Accounts.restore_replaces());
        assert!(!BackupItem::LocalMail.restore_replaces());
    }

    #[test]
    fn passphrases() {
        assert_eq!(
            passphrase_issues("short"),
            vec![BackupIssue::PassphraseTooShort]
        );
        // Characters, not bytes.
        assert_eq!(passphrase_issues("ääääääääääää"), Vec::<BackupIssue>::new());
        assert_eq!(
            passphrase_issues(&"x".repeat(MAX_BACKUP_PASSPHRASE_CHARS + 1)),
            vec![BackupIssue::PassphraseTooLong]
        );
    }

    #[test]
    fn manifest_wire_shape_and_helpers() {
        let manifest = BackupManifest {
            format_version: BACKUP_FORMAT_VERSION,
            app_version: "1.0.0-rc.1".into(),
            schema_version: 15,
            created_at: "2026-10-02T10:00:00Z".into(),
            contents: BackupContents {
                items: vec![BackupItem::Theme, BackupItem::LocalMail],
            },
            entries: vec![
                BackupEntry {
                    item: BackupItem::Theme,
                    name: "config/theme.kdl".into(),
                    size_bytes: 10,
                    sha256: "ab".into(),
                },
                BackupEntry {
                    item: BackupItem::LocalMail,
                    name: "mail/local-folders/Inbox.mbox".into(),
                    size_bytes: 90,
                    sha256: "cd".into(),
                },
            ],
            counts: JobCounts {
                messages: 3,
                ..JobCounts::default()
            },
            accounts: vec![],
            plugins: vec![],
        };
        assert_eq!(manifest.total_bytes(), 100);
        assert_eq!(manifest.entries_of(BackupItem::Theme).count(), 1);
        let wire = serde_json::to_value(&manifest).unwrap();
        assert_eq!(wire["contents"], json!({"items": ["theme", "local-mail"]}));
        assert_eq!(wire["entries"][1]["item"], json!("local-mail"));
        let back: BackupManifest = serde_json::from_value(wire).unwrap();
        assert_eq!(back, manifest);
        assert_eq!(BACKUP_MAGIC.len(), 8);
    }
}
