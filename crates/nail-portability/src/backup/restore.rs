//! Reading `.nailbackup` archives: inspecting (the manifest), verifying
//! (every entry against its SHA-256), the dry-run restore plan and the
//! restore itself, both through a [`RestoreTarget`] the core implements.
//!
//! Configuration files and settings are replaced (the core keeps the
//! current files as configuration backups); local mail, PIM data, keys
//! and plug-ins are merged (nothing is deleted; mail already there is
//! skipped by `Message-ID` + date + size, PIM items with a known `UID`
//! are updated).

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use nail_api::LocalizedMessage;
use nail_api::backup::{
    BACKUP_FORMAT_VERSION, BACKUP_MANIFEST_ENTRY, BackupContents, BackupEntry, BackupItem,
    BackupManifest, RestoreAction, RestoreEffect, RestorePlan, RestoreReport,
};
use nail_api::ids::{AccountId, ConfigBackupId};
use nail_api::portability::{
    ImportCollectionKind, ImportOptions, IssueSeverity, ItemIssue, ItemRef, JobKind, JobPhase,
    JobReport, PortabilityIssueCode,
};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use super::crypto::DecryptingReader;
use super::{MailIndex, PimIndex, item_prefix, valid_entry_name};
use crate::error::{PortabilityError, Result, SinkError};
use crate::folder::{guess_role, sanitize_folder_name};
use crate::import::{RawMessage, import_message};
use crate::job::JobContext;
use crate::mbox::{MboxReadOptions, MboxReader};
use crate::sink::{DuplicateKey, MailImportSink, PimImportSink, SourceCollection, SourceFolder};

/// The note of restored accounts (argument `count`).
pub const NOTE_ACCOUNTS_NEED_PASSWORDS: &str = "restore-note-accounts-need-passwords";

/// The note of restored plug-ins.
pub const NOTE_PLUGINS_REVIEW: &str = "restore-note-plugins-review";

/// What merging a file did (or would do).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeOutcome {
    /// Records added.
    pub added: u64,
    /// Records replaced.
    pub replaced: u64,
    /// Records already there.
    pub unchanged: u64,
}

/// Where a restore writes; the core implements it over its configuration
/// manager, store and queues. Names are relative to the item's archive
/// folder (`theme.kdl`, `keyring.json`).
pub trait RestoreTarget: MailImportSink + PimImportSink {
    /// The current content of a configuration-like file (`None` = none).
    fn current_file(&mut self, item: BackupItem, name: &str) -> Result<Option<Vec<u8>>, SinkError>;

    /// Replaces a configuration-like file (settings: key by key); returns
    /// the configuration backup of the replaced file, if one was made.
    fn replace_file(
        &mut self,
        item: BackupItem,
        name: &str,
        content: &[u8],
    ) -> Result<Option<ConfigBackupId>, SinkError>;

    /// Merges a keys / plug-ins file (`dry_run` = only count).
    fn merge_file(
        &mut self,
        item: BackupItem,
        name: &str,
        content: &[u8],
        dry_run: bool,
    ) -> Result<MergeOutcome, SinkError>;

    /// The following mail goes into this local account (created when
    /// missing, unless `dry_run`).
    fn begin_mail_account(&mut self, account: &AccountId, dry_run: bool) -> Result<(), SinkError>;
}

/// Largest configuration, settings or keyring entry read into memory.
pub const MAX_CONFIG_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
/// Largest PIM entry (`.ics` / `.vcf` / the PIM index) read into memory.
pub const MAX_PIM_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// How much of an entry may be read into memory: the manifest's sizes
/// come from the archive itself, so a crafted archive must not make a
/// restore (or its dry run) allocate gigabytes.
pub fn max_in_memory_bytes(name: &str) -> u64 {
    if name.starts_with("plugins/") {
        nail_api::plugins::admin::MAX_PACKAGE_BYTES
    } else if name.starts_with("pim/") {
        MAX_PIM_ENTRY_BYTES
    } else {
        MAX_CONFIG_ENTRY_BYTES
    }
}

/// An opened archive.
pub struct BackupArchive {
    archive: ZipArchive<DecryptingReader<BufReader<File>>>,
    manifest: BackupManifest,
    path: String,
}

impl std::fmt::Debug for BackupArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupArchive")
            .field("path", &self.path)
            .field("entries", &self.archive.len())
            .finish_non_exhaustive()
    }
}

/// Reads an entry, hashing it.
struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(out)?;
        self.hasher.update(&out[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}

impl BackupArchive {
    /// Opens an archive (wrong passphrase: [`PortabilityError::WrongPassphrase`];
    /// not a backup: [`PortabilityError::NotABackup`]).
    pub fn open(path: &Path, passphrase: &str) -> Result<Self> {
        let file =
            File::open(path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
        let (reader, _header) = DecryptingReader::open(BufReader::new(file), passphrase)?;
        let mut archive =
            ZipArchive::new(reader).map_err(|e| PortabilityError::Corrupt(format!("zip: {e}")))?;
        let first = archive
            .by_index(0)
            .map_err(|e| PortabilityError::Corrupt(format!("zip: {e}")))?
            .name()
            .to_owned();
        if first != BACKUP_MANIFEST_ENTRY {
            return Err(PortabilityError::Corrupt(
                "the manifest is not the first entry".into(),
            ));
        }
        let manifest: BackupManifest = {
            let entry = archive
                .by_name(BACKUP_MANIFEST_ENTRY)
                .map_err(|e| PortabilityError::Corrupt(format!("manifest: {e}")))?;
            let mut text = Vec::new();
            entry
                .take(64 * 1024 * 1024)
                .read_to_end(&mut text)
                .map_err(|e| PortabilityError::Corrupt(format!("manifest: {e}")))?;
            serde_json::from_slice(&text)
                .map_err(|e| PortabilityError::Corrupt(format!("manifest: {e}")))?
        };
        if manifest.format_version > BACKUP_FORMAT_VERSION {
            return Err(PortabilityError::NewerFormat(manifest.format_version));
        }
        Ok(Self {
            archive,
            manifest,
            path: path.display().to_string(),
        })
    }

    /// The manifest ([`nail_api::Request::InspectBackup`]).
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }

    fn entry_ref(&self, entry: &str) -> ItemRef {
        ItemRef {
            file: Some(self.path.clone()),
            label: Some(entry.to_owned()),
            ..ItemRef::default()
        }
    }

    /// Reads one entry fully and checks it against the manifest.
    fn read_checked(
        &mut self,
        entry: &BackupEntry,
    ) -> std::result::Result<Vec<u8>, Box<ItemIssue>> {
        let item = self.entry_ref(&entry.name);
        let issue = |code, detail: String| {
            Box::new(ItemIssue::new(
                code,
                IssueSeverity::Skipped,
                item.clone(),
                detail,
            ))
        };
        if !valid_entry_name(&entry.name) {
            return Err(issue(
                PortabilityIssueCode::EntryMissing,
                "invalid entry name".into(),
            ));
        }
        let limit = max_in_memory_bytes(&entry.name);
        if entry.size_bytes > limit {
            return Err(issue(
                PortabilityIssueCode::TooLarge,
                format!("{} bytes (at most {limit})", entry.size_bytes),
            ));
        }
        let file = match self.archive.by_name(&entry.name) {
            Ok(file) => file,
            Err(e) => return Err(issue(PortabilityIssueCode::EntryMissing, e.to_string())),
        };
        let mut bytes = Vec::new();
        if let Err(e) = file.take(entry.size_bytes + 1).read_to_end(&mut bytes) {
            return Err(issue(PortabilityIssueCode::ChecksumMismatch, e.to_string()));
        }
        let sha = hex::encode(Sha256::digest(&bytes));
        if sha != entry.sha256 || bytes.len() as u64 != entry.size_bytes {
            return Err(issue(
                PortabilityIssueCode::ChecksumMismatch,
                format!(
                    "expected {} ({} bytes), found {sha} ({} bytes)",
                    entry.sha256,
                    entry.size_bytes,
                    bytes.len()
                ),
            ));
        }
        Ok(bytes)
    }

    /// Checks every entry, streaming; returns how many are intact.
    pub fn verify_entries(&mut self, ctx: &mut JobContext<'_>) -> Result<u64> {
        let entries = self.manifest.entries.clone();
        let mut good = 0;
        let names: HashSet<String> = self.archive.file_names().map(str::to_owned).collect();
        for entry in &entries {
            ctx.check_cancelled()?;
            ctx.set_current(Some(entry.name.clone()));
            let problem = if !valid_entry_name(&entry.name) || !names.contains(&entry.name) {
                Some((
                    PortabilityIssueCode::EntryMissing,
                    "not in the archive".to_owned(),
                ))
            } else {
                match self.archive.by_name(&entry.name) {
                    Err(e) => Some((PortabilityIssueCode::EntryMissing, e.to_string())),
                    Ok(file) => {
                        let mut reader = HashingReader {
                            inner: file,
                            hasher: Sha256::new(),
                            bytes: 0,
                        };
                        if let Err(e) = io::copy(&mut reader, &mut io::sink()) {
                            Some((PortabilityIssueCode::ChecksumMismatch, e.to_string()))
                        } else {
                            let sha = hex::encode(reader.hasher.finalize());
                            if sha == entry.sha256 && reader.bytes == entry.size_bytes {
                                None
                            } else {
                                Some((
                                    PortabilityIssueCode::ChecksumMismatch,
                                    format!("expected {}, found {sha}", entry.sha256),
                                ))
                            }
                        }
                    }
                }
            };
            match problem {
                None => good += 1,
                Some((code, detail)) => ctx.skipped(code, self.entry_ref(&entry.name), detail),
            }
            ctx.advance(1, entry.size_bytes);
        }
        Ok(good)
    }

    /// The verify job ([`nail_api::Request::VerifyBackup`]).
    pub fn verify(&mut self, ctx: &mut JobContext<'_>) -> Result<()> {
        ctx.report.source = Some(self.path.clone());
        ctx.report.backup = Some(self.manifest.clone());
        ctx.phase(
            JobPhase::Verifying,
            Some(self.manifest.entries.len() as u64),
            Some(self.manifest.total_bytes()),
        );
        let good = self.verify_entries(ctx)?;
        ctx.report.counts.files = good;
        ctx.phase(JobPhase::Finishing, None, None);
        Ok(())
    }

    /// The dry run ([`nail_api::Request::PreviewRestore`]): what restoring
    /// `contents` would do. Nothing is written.
    pub fn plan(
        &mut self,
        contents: &BackupContents,
        target: &mut dyn RestoreTarget,
    ) -> Result<RestorePlan> {
        let mut ctx = JobContext::silent(JobReport::started("plan".into(), JobKind::Restore, ""));
        let outcome = self.run(contents, target, true, &mut ctx)?;
        Ok(RestorePlan {
            manifest: self.manifest.clone(),
            actions: outcome.actions,
            issues: ctx.report.issues,
            accounts_needing_secrets: outcome.accounts_needing_secrets,
            restart_required: outcome.restart_required,
        })
    }

    /// Restores `contents` ([`nail_api::Request::StartRestore`]); the
    /// report gets the [`RestoreReport`] and the manifest.
    pub fn restore(
        &mut self,
        contents: &BackupContents,
        target: &mut dyn RestoreTarget,
        ctx: &mut JobContext<'_>,
    ) -> Result<RestoreReport> {
        ctx.report.source = Some(self.path.clone());
        ctx.report.backup = Some(self.manifest.clone());
        let outcome = self.run(contents, target, false, ctx)?;
        let report = RestoreReport {
            actions: outcome.actions,
            config_backups: outcome.config_backups,
            accounts_needing_secrets: outcome.accounts_needing_secrets,
            restart_required: outcome.restart_required,
        };
        ctx.report.restore = Some(report.clone());
        Ok(report)
    }

    fn run(
        &mut self,
        contents: &BackupContents,
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
    ) -> Result<RunOutcome> {
        let mut outcome = RunOutcome::default();
        let total: u64 = self
            .manifest
            .entries
            .iter()
            .filter(|e| contents.includes(e.item))
            .map(|e| e.size_bytes)
            .sum();
        ctx.phase(JobPhase::Writing, None, Some(total));
        // Items in the archive, in contract order.
        let items: Vec<BackupItem> = BackupItem::ALL
            .iter()
            .copied()
            .filter(|i| self.manifest.contents.includes(*i))
            .collect();
        for item in items {
            ctx.check_cancelled()?;
            ctx.set_current(Some(item.as_str().to_owned()));
            let mut action = RestoreAction {
                item,
                effect: RestoreEffect::Skip,
                added: 0,
                replaced: 0,
                unchanged: 0,
                note: None,
            };
            if contents.includes(item) {
                let entries: Vec<BackupEntry> = self.manifest.entries_of(item).cloned().collect();
                match item {
                    BackupItem::LocalMail => {
                        self.restore_mail(&entries, target, dry_run, ctx, &mut action)?;
                    }
                    BackupItem::PimLocal => {
                        self.restore_pim(&entries, target, dry_run, ctx, &mut action)?;
                    }
                    BackupItem::Keys | BackupItem::Plugins => {
                        self.restore_merged_files(
                            item,
                            &entries,
                            target,
                            dry_run,
                            ctx,
                            &mut action,
                        )?;
                    }
                    _ => self.restore_replaced_files(
                        item,
                        &entries,
                        target,
                        dry_run,
                        ctx,
                        &mut action,
                        &mut outcome,
                    )?,
                }
                action.effect = if action.added == 0 && action.replaced == 0 {
                    RestoreEffect::Skip
                } else if item.restore_replaces() && action.replaced > 0 {
                    RestoreEffect::Replace
                } else if action.unchanged == 0 && action.replaced == 0 {
                    RestoreEffect::Add
                } else {
                    RestoreEffect::Merge
                };
                if action.effect != RestoreEffect::Skip {
                    match item {
                        BackupItem::Accounts => {
                            outcome.accounts_needing_secrets = self.manifest.accounts.clone();
                            if !self.manifest.accounts.is_empty() {
                                action.note = Some(
                                    LocalizedMessage::new(NOTE_ACCOUNTS_NEED_PASSWORDS)
                                        .arg("count", self.manifest.accounts.len()),
                                );
                            }
                        }
                        BackupItem::Settings => outcome.restart_required = true,
                        BackupItem::Plugins => {
                            outcome.restart_required = true;
                            action.note = Some(LocalizedMessage::new(NOTE_PLUGINS_REVIEW));
                        }
                        _ => {}
                    }
                }
            }
            outcome.actions.push(action);
        }
        ctx.phase(JobPhase::Finishing, None, None);
        Ok(outcome)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the restore state of one item kind; grouping it would only move the list"
    )]
    fn restore_replaced_files(
        &mut self,
        item: BackupItem,
        entries: &[BackupEntry],
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
        action: &mut RestoreAction,
        outcome: &mut RunOutcome,
    ) -> Result<()> {
        for entry in entries {
            ctx.check_cancelled()?;
            let bytes = match self.read_checked(entry) {
                Ok(bytes) => bytes,
                Err(issue) => {
                    ctx.issue(*issue);
                    continue;
                }
            };
            ctx.advance(1, entry.size_bytes);
            if !dry_run {
                ctx.report.counts.files += 1;
            }
            let name = relative_name(item, &entry.name);
            let current = target
                .current_file(item, name)
                .map_err(PortabilityError::from)?;
            match current {
                Some(current) if current == bytes => {
                    action.unchanged += 1;
                    continue;
                }
                Some(_) => action.replaced += 1,
                None => action.added += 1,
            }
            if !dry_run {
                match target.replace_file(item, name, &bytes) {
                    Ok(Some(backup)) => outcome.config_backups.push(backup),
                    Ok(None) => {}
                    Err(SinkError::Item(detail)) => {
                        ctx.skipped(
                            PortabilityIssueCode::WriteFailed,
                            self.entry_ref(&entry.name),
                            detail,
                        );
                    }
                    Err(SinkError::Fatal(detail)) => return Err(PortabilityError::Sink(detail)),
                }
            }
        }
        Ok(())
    }

    fn restore_merged_files(
        &mut self,
        item: BackupItem,
        entries: &[BackupEntry],
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
        action: &mut RestoreAction,
    ) -> Result<()> {
        for entry in entries {
            ctx.check_cancelled()?;
            let bytes = match self.read_checked(entry) {
                Ok(bytes) => bytes,
                Err(issue) => {
                    ctx.issue(*issue);
                    continue;
                }
            };
            ctx.advance(1, entry.size_bytes);
            if !dry_run {
                ctx.report.counts.files += 1;
            }
            match target.merge_file(item, relative_name(item, &entry.name), &bytes, dry_run) {
                Ok(merged) => {
                    action.added += merged.added;
                    action.replaced += merged.replaced;
                    action.unchanged += merged.unchanged;
                }
                Err(SinkError::Item(detail)) => {
                    ctx.skipped(
                        PortabilityIssueCode::WriteFailed,
                        self.entry_ref(&entry.name),
                        detail,
                    );
                }
                Err(SinkError::Fatal(detail)) => return Err(PortabilityError::Sink(detail)),
            }
        }
        Ok(())
    }

    fn restore_mail(
        &mut self,
        entries: &[BackupEntry],
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
        action: &mut RestoreAction,
    ) -> Result<()> {
        let by_name: BTreeMap<String, BackupEntry> = entries
            .iter()
            .map(|e| (e.name.clone(), e.clone()))
            .collect();
        let indexes: Vec<&BackupEntry> = entries
            .iter()
            .filter(|e| e.name.ends_with("/index.json"))
            .collect();
        for index_entry in indexes {
            let index: MailIndex = match self.read_checked(index_entry) {
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(index) => index,
                    Err(e) => {
                        ctx.skipped(
                            PortabilityIssueCode::ChecksumMismatch,
                            self.entry_ref(&index_entry.name),
                            e.to_string(),
                        );
                        continue;
                    }
                },
                Err(issue) => {
                    ctx.issue(*issue);
                    continue;
                }
            };
            target
                .begin_mail_account(&index.account_id, dry_run)
                .map_err(PortabilityError::from)?;
            let mut ensured = HashSet::new();
            for folder in &index.folders {
                ctx.check_cancelled()?;
                let Some(entry) = by_name.get(&folder.entry) else {
                    ctx.skipped(
                        PortabilityIssueCode::EntryMissing,
                        self.entry_ref(&folder.entry),
                        "not in the manifest",
                    );
                    continue;
                };
                let source_folder = folder_of(&folder.path);
                if !dry_run {
                    for parent in parents_of(&source_folder) {
                        if ensured.insert(parent.path.clone()) {
                            target
                                .ensure_folder(&parent)
                                .map_err(PortabilityError::from)?;
                            ctx.report.counts.folders += 1;
                        }
                    }
                }
                ctx.set_current(Some(folder.path.clone()));
                self.restore_mbox(
                    entry,
                    &folder.messages,
                    &source_folder,
                    target,
                    dry_run,
                    ctx,
                    action,
                )?;
            }
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the restore state of one folder; grouping it would only move the list"
    )]
    fn restore_mbox(
        &mut self,
        entry: &BackupEntry,
        meta: &[super::MailIndexMessage],
        folder: &SourceFolder,
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
        action: &mut RestoreAction,
    ) -> Result<()> {
        let entry_ref = self.entry_ref(&entry.name);
        let file = match self.archive.by_name(&entry.name) {
            Ok(file) => file,
            Err(e) => {
                ctx.skipped(PortabilityIssueCode::EntryMissing, entry_ref, e.to_string());
                return Ok(());
            }
        };
        let mut hashing = HashingReader {
            inner: file,
            hasher: Sha256::new(),
            bytes: 0,
        };
        let options = ImportOptions {
            skip_duplicates: true,
            keep_flags: true,
            ..ImportOptions::default()
        };
        let mut failed = None;
        {
            let mut reader =
                MboxReader::new(BufReader::new(&mut hashing), MboxReadOptions::default());
            let mut index = 0usize;
            loop {
                ctx.check_cancelled()?;
                let next = match reader.next_entry() {
                    Ok(Some(next)) => next,
                    Ok(None) => break,
                    Err(e) => {
                        failed = Some(e.to_string());
                        break;
                    }
                };
                let m = meta.get(index).cloned().unwrap_or_default();
                index += 1;
                let origin = ItemRef {
                    file: Some(entry.name.clone()),
                    folder: Some(folder.path.clone()),
                    index: Some(next.index),
                    offset: Some(next.offset),
                    label: None,
                };
                let mut message = RawMessage::new(next.bytes, origin);
                message.flags = m.flags;
                message.keywords = m.keywords;
                message.received_at = m
                    .received_at
                    .or_else(|| crate::mbox::read::envelope_date(&next.envelope));
                if next.too_large {
                    message.problem = Some((
                        PortabilityIssueCode::TooLarge,
                        format!("{} bytes", next.size_on_disk),
                    ));
                }
                if dry_run {
                    let facts = crate::message::header_facts(&message.raw);
                    let key: DuplicateKey = crate::message::duplicate_key(&message.raw, &facts);
                    if target
                        .contains(folder, &key)
                        .map_err(PortabilityError::from)?
                    {
                        action.unchanged += 1;
                    } else {
                        action.added += 1;
                    }
                    ctx.advance(1, 0);
                } else {
                    let (before, duplicates) =
                        (ctx.report.counts.messages, ctx.report.counts.duplicates);
                    import_message(ctx, target, folder, &options, message)?;
                    if ctx.report.counts.messages > before {
                        action.added += 1;
                    } else if ctx.report.counts.duplicates > duplicates {
                        action.unchanged += 1;
                    }
                }
            }
        }
        // Whatever is left, then the checksum.
        drop(io::copy(&mut hashing, &mut io::sink()));
        let sha = hex::encode(hashing.hasher.finalize());
        if failed.is_some() || sha != entry.sha256 || hashing.bytes != entry.size_bytes {
            ctx.warning(
                PortabilityIssueCode::ChecksumMismatch,
                entry_ref,
                failed.unwrap_or_else(|| format!("expected {}, found {sha}", entry.sha256)),
            );
        }
        Ok(())
    }

    fn restore_pim(
        &mut self,
        entries: &[BackupEntry],
        target: &mut dyn RestoreTarget,
        dry_run: bool,
        ctx: &mut JobContext<'_>,
        action: &mut RestoreAction,
    ) -> Result<()> {
        let Some(index_entry) = entries.iter().find(|e| e.name == "pim/index.json") else {
            return Ok(());
        };
        let index: PimIndex = match self.read_checked(index_entry) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| PortabilityError::Corrupt(format!("pim index: {e}")))?,
            Err(issue) => {
                ctx.issue(*issue);
                return Ok(());
            }
        };
        for collection in index.collections {
            ctx.check_cancelled()?;
            let Some(entry) = entries.iter().find(|e| e.name == collection.entry) else {
                ctx.skipped(
                    PortabilityIssueCode::EntryMissing,
                    self.entry_ref(&collection.entry),
                    "not in the manifest",
                );
                continue;
            };
            let bytes = match self.read_checked(entry) {
                Ok(bytes) => bytes,
                Err(issue) => {
                    ctx.issue(*issue);
                    continue;
                }
            };
            if bytes.iter().all(u8::is_ascii_whitespace) {
                // An empty calendar / address book was backed up as an
                // empty file: nothing to restore (WP125).
                ctx.advance(1, entry.size_bytes);
                continue;
            }
            let source = SourceCollection {
                key: collection.key.clone(),
                name: collection.name.clone(),
                kind: collection.kind,
            };
            let file_name = if collection.kind == ImportCollectionKind::AddressBook {
                "x.vcf"
            } else {
                "x.ics"
            };
            let data = nail_ical::import::read_bytes(&bytes, Some(file_name));
            let uids: Vec<String> = match &data {
                nail_ical::import::ImportData::Calendar(c) => c
                    .events
                    .iter()
                    .map(|e| e.uid.clone())
                    .chain(c.tasks.iter().map(|t| t.uid.clone()))
                    .collect(),
                nail_ical::import::ImportData::Cards(c) => c
                    .contacts
                    .iter()
                    .map(|p| p.uid.clone())
                    .chain(c.groups.iter().map(|g| g.group.uid.clone()))
                    .collect(),
            };
            for uid in uids {
                if target.exists(&source, &uid) {
                    action.replaced += 1;
                } else {
                    action.added += 1;
                }
            }
            ctx.advance(1, entry.size_bytes);
            if !dry_run {
                crate::pim::import_data(ctx, target, &source, Some(&entry.name), data)?;
            }
        }
        Ok(())
    }
}

/// An archive name without its item's folder.
fn relative_name(item: BackupItem, name: &str) -> &str {
    name.strip_prefix(item_prefix(item)).unwrap_or(name)
}

fn folder_of(path: &str) -> SourceFolder {
    let segments: Vec<String> = path.split('/').map(|s| sanitize_folder_name(s).0).collect();
    let path = segments.join("/");
    SourceFolder {
        role: guess_role(&path),
        name: segments.last().cloned().unwrap_or_default(),
        parent: (segments.len() > 1).then(|| segments[..segments.len() - 1].join("/")),
        path,
    }
}

/// The folder's ancestors and itself, outermost first.
fn parents_of(folder: &SourceFolder) -> Vec<SourceFolder> {
    let segments: Vec<&str> = folder.path.split('/').collect();
    (1..=segments.len())
        .map(|len| {
            if len == segments.len() {
                folder.clone()
            } else {
                folder_of(&segments[..len].join("/"))
            }
        })
        .collect()
}

#[derive(Debug, Default)]
struct RunOutcome {
    actions: Vec<RestoreAction>,
    config_backups: Vec<ConfigBackupId>,
    accounts_needing_secrets: Vec<AccountId>,
    restart_required: bool,
}

/// Reads only the manifest ([`nail_api::Request::InspectBackup`]).
pub fn inspect(path: &Path, passphrase: &str) -> Result<BackupManifest> {
    Ok(BackupArchive::open(path, passphrase)?.manifest)
}
