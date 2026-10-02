//! `.nailbackup` archives (contract `nail_api::backup`): writing
//! ([`write_backup`]), inspecting, verifying and restoring
//! ([`restore::BackupArchive`]).
//!
//! # Inside the zip
//!
//! * `manifest.json` first ([`BackupManifest`]: every entry with size and
//!   SHA-256).
//! * Configuration-like items as the core hands them over
//!   ([`BackupSource::files`]): `config/<name>` for settings, theme,
//!   layout, rules and accounts, `keys/<name>`, `plugins/<name>`.
//! * Local mail: per account `mail/<account>/index.json`
//!   ([`MailIndex`]: folder paths and per-message flags, keywords and
//!   delivery times) and one mboxrd file per folder
//!   (`mail/<account>/<n>.mbox`).
//! * Local calendars and address books: `pim/index.json` ([`PimIndex`])
//!   and `pim/<n>.ics` / `pim/<n>.vcf`.
//!
//! Every entry is produced twice: once to hash it (the manifest comes
//! first), once to write it; an entry that changes in between fails the
//! backup ([`PortabilityError::SourceChanged`]) rather than producing an
//! archive that does not verify.

pub mod crypto;
pub mod restore;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nail_api::Flags;
use nail_api::backup::{
    BACKUP_FORMAT_VERSION, BACKUP_MANIFEST_ENTRY, BackupContents, BackupEntry, BackupItem,
    BackupManifest, passphrase_issues,
};
use nail_api::ids::{AccountId, PluginId};
use nail_api::portability::{ImportCollectionKind, JobCounts, JobPhase};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::{PortabilityError, Result, SinkError};
use crate::job::JobContext;
use crate::mbox::MboxWriter;
use crate::pim::{IcsBuilder, VcfBuilder};
use crate::sink::{ExportFolder, MailExportSource, PimExportSource, Visit};
use nail_api::portability::MboxVariant;

pub use crypto::KdfParams;
pub use restore::{BackupArchive, MergeOutcome, RestoreTarget};

/// A file of a configuration-like item, as the core lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    /// Its name inside the item (`theme.kdl`, `keyring.json`,
    /// `my-plugin/manifest.json`): relative, `/`-separated.
    pub name: String,
}

/// Facts the manifest records.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackupInfo {
    /// The writing Nail's version.
    pub app_version: String,
    /// The store schema version.
    pub schema_version: u32,
    /// Accounts in `accounts.kdl` (their passwords must be entered after a
    /// restore).
    pub accounts: Vec<AccountId>,
    /// Installed plug-ins (with [`BackupItem::Plugins`]).
    pub plugins: Vec<PluginId>,
}

/// What the core hands to a backup. Secrets never pass through here:
/// `accounts.kdl` only has `keychain:` references, and settings must not
/// hold tokens, remote-UI users, MCP clients or pairing codes.
pub trait BackupSource {
    /// Facts for the manifest.
    fn info(&mut self) -> BackupInfo;

    /// The files of a configuration-like item ([`BackupItem::Settings`],
    /// `Theme`, `Layout`, `Rules`, `Accounts`, `Keys`, `Plugins`).
    fn files(&mut self, item: BackupItem) -> Result<Vec<BackupFile>, SinkError>;

    /// Writes one of those files. Called twice per file (hashing, then
    /// writing); it must produce the same bytes both times.
    fn write_file(
        &mut self,
        item: BackupItem,
        name: &str,
        out: &mut dyn Write,
    ) -> Result<(), SinkError>;

    /// The local accounts whose mail is backed up ("Local Folders",
    /// opened files).
    fn local_mail_accounts(&mut self) -> Result<Vec<AccountId>, SinkError>;

    /// The mail of one local account.
    fn local_mail(
        &mut self,
        account: &AccountId,
    ) -> Result<Box<dyn MailExportSource + '_>, SinkError>;

    /// The local calendars and task lists.
    fn local_calendars(&mut self) -> Result<Box<dyn PimExportSource + '_>, SinkError>;

    /// The local address books.
    fn local_address_books(&mut self) -> Result<Box<dyn PimExportSource + '_>, SinkError>;
}

/// How to write a backup ([`nail_api::Request::CreateBackup`]).
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// What goes in.
    pub contents: BackupContents,
    /// The passphrase (checked with [`passphrase_issues`]).
    pub passphrase: String,
    /// The Argon2id cost ([`KdfParams::STANDARD`]).
    pub kdf: KdfParams,
    /// Replace an existing file.
    pub overwrite: bool,
    /// "Now" (the manifest's time; mbox `From ` lines of messages without
    /// a time).
    pub now: DateTime<Utc>,
}

/// The per-account index of backed-up mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailIndex {
    /// The account.
    pub account_id: AccountId,
    /// Its folders, parents first.
    pub folders: Vec<MailIndexFolder>,
}

/// One backed-up folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MailIndexFolder {
    /// The folder's path in the account.
    pub path: String,
    /// The mbox entry holding its messages.
    pub entry: String,
    /// Per message, in mbox order.
    pub messages: Vec<MailIndexMessage>,
}

/// What an mbox file does not keep of a message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MailIndexMessage {
    /// Flags.
    pub flags: Flags,
    /// Keywords.
    pub keywords: Vec<String>,
    /// Delivery time.
    pub received_at: Option<i64>,
}

/// The index of backed-up calendars and address books.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PimIndex {
    /// The collections.
    pub collections: Vec<PimIndexCollection>,
}

/// One backed-up calendar / address book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PimIndexCollection {
    /// Calendar or address book.
    pub kind: ImportCollectionKind,
    /// The core's key (calendar / address book id).
    pub key: String,
    /// Its name.
    pub name: String,
    /// The `.ics` / `.vcf` entry.
    pub entry: String,
}

/// The archive folder of an item's files.
pub fn item_prefix(item: BackupItem) -> &'static str {
    match item {
        BackupItem::Settings
        | BackupItem::Theme
        | BackupItem::Layout
        | BackupItem::Rules
        | BackupItem::Accounts => "config/",
        BackupItem::LocalMail => "mail/",
        BackupItem::Keys => "keys/",
        BackupItem::PimLocal => "pim/",
        BackupItem::Plugins => "plugins/",
    }
}

/// Whether an entry name is safe: relative, `/`-separated, no empty, `.`
/// or `..` segments, no `\` / `:` / control characters, at most 400 bytes.
pub fn valid_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 400
        && !name.starts_with('/')
        && !name.contains(['\\', ':'])
        && !name.chars().any(char::is_control)
        && name
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

/// What will be in the archive, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Producer {
    File {
        name: String,
    },
    MailFolder {
        account: AccountId,
        folder: ExportFolder,
        index: usize,
    },
    MailIndex {
        account: AccountId,
    },
    Collection {
        kind: ImportCollectionKind,
        collection: crate::sink::ExportCollection,
    },
    PimIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Planned {
    item: BackupItem,
    name: String,
    producer: Producer,
}

/// What producing entries collected (indexes, counts).
#[derive(Debug, Default)]
struct Production {
    mail: BTreeMap<AccountId, MailIndex>,
    pim: PimIndex,
    counts: JobCounts,
}

fn sink(e: SinkError) -> PortabilityError {
    e.into()
}

fn plan(contents: &BackupContents, source: &mut dyn BackupSource) -> Result<Vec<Planned>> {
    let mut out = Vec::new();
    for item in BackupItem::ALL {
        if !contents.includes(*item) {
            continue;
        }
        match item {
            BackupItem::LocalMail => {
                for account in source.local_mail_accounts().map_err(sink)? {
                    if !valid_entry_name(account.as_str()) {
                        return Err(PortabilityError::Sink(format!(
                            "invalid account id {account}"
                        )));
                    }
                    let folders = source
                        .local_mail(&account)
                        .map_err(sink)?
                        .folders()
                        .map_err(sink)?;
                    for (index, folder) in folders.into_iter().enumerate() {
                        out.push(Planned {
                            item: *item,
                            name: format!("mail/{account}/{}.mbox", index + 1),
                            producer: Producer::MailFolder {
                                account: account.clone(),
                                folder,
                                index,
                            },
                        });
                    }
                    out.push(Planned {
                        item: *item,
                        name: format!("mail/{account}/index.json"),
                        producer: Producer::MailIndex { account },
                    });
                }
            }
            BackupItem::PimLocal => {
                let mut n = 0;
                let calendars = source
                    .local_calendars()
                    .map_err(sink)?
                    .collections()
                    .map_err(sink)?;
                let books = source
                    .local_address_books()
                    .map_err(sink)?
                    .collections()
                    .map_err(sink)?;
                for (kind, list, ext) in [
                    (ImportCollectionKind::Calendar, calendars, "ics"),
                    (ImportCollectionKind::AddressBook, books, "vcf"),
                ] {
                    for collection in list {
                        n += 1;
                        out.push(Planned {
                            item: *item,
                            name: format!("pim/{n}.{ext}"),
                            producer: Producer::Collection { kind, collection },
                        });
                    }
                }
                out.push(Planned {
                    item: *item,
                    name: "pim/index.json".to_owned(),
                    producer: Producer::PimIndex,
                });
            }
            _ => {
                for file in source.files(*item).map_err(sink)? {
                    let name = format!("{}{}", item_prefix(*item), file.name);
                    if !valid_entry_name(&file.name) || !valid_entry_name(&name) {
                        return Err(PortabilityError::Sink(format!(
                            "invalid file name {:?}",
                            file.name
                        )));
                    }
                    if out.iter().any(|p: &Planned| p.name == name) {
                        return Err(PortabilityError::Sink(format!("file {name} listed twice")));
                    }
                    out.push(Planned {
                        item: *item,
                        name,
                        producer: Producer::File { name: file.name },
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Produces one entry into `out`.
fn produce(
    planned: &Planned,
    source: &mut dyn BackupSource,
    out: &mut dyn Write,
    production: &mut Production,
    now: DateTime<Utc>,
    cancel: &crate::job::CancelFlag,
) -> Result<()> {
    let io_error = |e: io::Error| PortabilityError::io(planned.name.clone(), e);
    match &planned.producer {
        Producer::File { name } => {
            source.write_file(planned.item, name, out).map_err(sink)?;
            production.counts.files += 1;
        }
        Producer::MailFolder {
            account, folder, ..
        } => {
            let mut writer = MboxWriter::new(out, MboxVariant::Mboxrd);
            let mut messages = Vec::new();
            let mut failure = None;
            source
                .local_mail(account)
                .map_err(sink)?
                .messages(folder, &mut |message| {
                    if cancel.is_cancelled() {
                        failure = Some(PortabilityError::Cancelled);
                        return Visit::Stop;
                    }
                    match message {
                        Ok(message) => {
                            if let Err(e) = writer.write_message(&message, now) {
                                failure = Some(io_error(e));
                                return Visit::Stop;
                            }
                            messages.push(MailIndexMessage {
                                flags: message.flags,
                                keywords: message.keywords,
                                received_at: message.received_at,
                            });
                            Visit::Continue
                        }
                        Err(e) => {
                            failure = Some(PortabilityError::Sink(format!("{}: {e}", folder.path)));
                            Visit::Stop
                        }
                    }
                })
                .map_err(sink)?;
            if let Some(e) = failure {
                return Err(e);
            }
            writer.finish().map_err(io_error)?;
            production.counts.messages += messages.len() as u64;
            production.counts.folders += 1;
            production
                .mail
                .entry(account.clone())
                .or_insert_with(|| MailIndex {
                    account_id: account.clone(),
                    folders: Vec::new(),
                })
                .folders
                .push(MailIndexFolder {
                    path: folder.path.clone(),
                    entry: planned.name.clone(),
                    messages,
                });
        }
        Producer::MailIndex { account } => {
            let index = production
                .mail
                .remove(account)
                .unwrap_or_else(|| MailIndex {
                    account_id: account.clone(),
                    folders: Vec::new(),
                });
            serde_json::to_writer_pretty(&mut *out, &index).map_err(|e| io_error(e.into()))?;
        }
        Producer::Collection { kind, collection } => {
            let text = {
                let mut pim = if *kind == ImportCollectionKind::AddressBook {
                    source.local_address_books().map_err(sink)?
                } else {
                    source.local_calendars().map_err(sink)?
                };
                let mut ics = IcsBuilder::new(&collection.name);
                let mut vcf = VcfBuilder::new();
                let mut failure = None;
                pim.items(collection, &mut |item| match item {
                    Ok(text) => {
                        if *kind == ImportCollectionKind::AddressBook {
                            vcf.add(&text);
                        } else {
                            ics.add(&text);
                        }
                        Visit::Continue
                    }
                    Err(e) => {
                        failure = Some(PortabilityError::Sink(format!("{}: {e}", collection.name)));
                        Visit::Stop
                    }
                })
                .map_err(sink)?;
                if let Some(e) = failure {
                    return Err(e);
                }
                if *kind == ImportCollectionKind::AddressBook {
                    production.counts.contacts += vcf.cards() - vcf.groups();
                    production.counts.groups += vcf.groups();
                    vcf.finish()
                } else {
                    production.counts.events += ics.events();
                    production.counts.tasks += ics.tasks();
                    ics.finish()
                }
            };
            out.write_all(text.as_bytes()).map_err(io_error)?;
            production.pim.collections.push(PimIndexCollection {
                kind: *kind,
                key: collection.key.clone(),
                name: collection.name.clone(),
                entry: planned.name.clone(),
            });
        }
        Producer::PimIndex => {
            let index = std::mem::take(&mut production.pim);
            serde_json::to_writer_pretty(&mut *out, &index).map_err(|e| io_error(e.into()))?;
        }
    }
    Ok(())
}

/// A writer that hashes and counts what passes through.
pub(crate) struct HashingWriter<W> {
    pub inner: W,
    pub hasher: Sha256,
    pub bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    pub fn digest(self) -> (String, u64) {
        (hex::encode(self.hasher.finalize()), self.bytes)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(data)?;
        self.hasher.update(&data[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Writes a backup archive ([`nail_api::Request::CreateBackup`]): plans
/// the entries, hashes them, writes manifest and entries encrypted, then
/// reads the archive back and checks every entry. Returns the manifest
/// (also in the report).
pub fn write_backup(
    path: &Path,
    options: &BackupOptions,
    source: &mut dyn BackupSource,
    ctx: &mut JobContext<'_>,
) -> Result<BackupManifest> {
    let mut problems: Vec<&str> = options
        .contents
        .issues()
        .iter()
        .map(|i| i.as_str())
        .collect();
    problems.extend(
        passphrase_issues(&options.passphrase)
            .iter()
            .map(|i| i.as_str()),
    );
    if !problems.is_empty() {
        return Err(PortabilityError::InvalidOptions(problems.join(", ")));
    }
    options.kdf.check_for_writing()?;
    if path.exists() && !options.overwrite {
        return Err(PortabilityError::TargetExists(path.display().to_string()));
    }
    ctx.report.target = Some(path.display().to_string());

    ctx.phase(JobPhase::Scanning, None, None);
    let planned = plan(&options.contents, source)?;

    // Pass 1: hash every entry.
    ctx.phase(JobPhase::Reading, Some(planned.len() as u64), None);
    let mut production = Production::default();
    let mut entries = Vec::with_capacity(planned.len());
    for p in &planned {
        ctx.check_cancelled()?;
        ctx.set_current(Some(p.name.clone()));
        let mut hashing = HashingWriter::new(io::sink());
        produce(
            p,
            source,
            &mut hashing,
            &mut production,
            options.now,
            &ctx.cancel_flag().clone(),
        )?;
        let (sha256, size_bytes) = hashing.digest();
        ctx.advance(1, size_bytes);
        entries.push(BackupEntry {
            item: p.item,
            name: p.name.clone(),
            size_bytes,
            sha256,
        });
    }
    let info = source.info();
    let mut counts = production.counts;
    if options.contents.includes(BackupItem::Accounts) {
        counts.accounts = info.accounts.len() as u64;
    }
    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION,
        app_version: info.app_version,
        schema_version: info.schema_version,
        created_at: options
            .now
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        contents: options.contents.clone(),
        entries,
        counts,
        accounts: if options.contents.includes(BackupItem::Accounts) {
            info.accounts
        } else {
            Vec::new()
        },
        plugins: if options.contents.includes(BackupItem::Plugins) {
            info.plugins
        } else {
            Vec::new()
        },
    };
    let total_bytes = manifest.total_bytes();

    // Pass 2: write.
    ctx.phase(
        JobPhase::Writing,
        Some(planned.len() as u64),
        Some(total_bytes),
    );
    let temp = temp_path(path);
    let result = write_archive(&temp, options, &manifest, &planned, source, ctx);
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    fs::rename(&temp, path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;

    // Read it back.
    ctx.phase(
        JobPhase::Verifying,
        Some(manifest.entries.len() as u64),
        Some(total_bytes),
    );
    let mut archive = BackupArchive::open(path, &options.passphrase)?;
    archive.verify_entries(ctx)?;
    let skipped = ctx.report.counts.skipped;
    ctx.report.counts = manifest.counts.clone();
    ctx.report.counts.skipped = skipped;
    ctx.report.backup = Some(manifest.clone());
    ctx.phase(JobPhase::Finishing, None, None);
    Ok(manifest)
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".nail-part");
    path.with_file_name(name)
}

fn write_archive(
    temp: &Path,
    options: &BackupOptions,
    manifest: &BackupManifest,
    planned: &[Planned],
    source: &mut dyn BackupSource,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| PortabilityError::io(parent.display().to_string(), e))?;
    }
    let file =
        File::create(temp).map_err(|e| PortabilityError::io(temp.display().to_string(), e))?;
    let encrypting =
        crypto::EncryptingWriter::new(BufWriter::new(file), &options.passphrase, options.kdf)?;
    let mut zip = ZipWriter::new_stream(encrypting);
    let zip_error = |e: zip::result::ZipError| {
        PortabilityError::io(temp.display().to_string(), io::Error::other(e))
    };
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(BACKUP_MANIFEST_ENTRY, base)
        .map_err(zip_error)?;
    serde_json::to_writer_pretty(&mut zip, manifest)
        .map_err(|e| PortabilityError::io(BACKUP_MANIFEST_ENTRY, e.into()))?;
    let mut production = Production::default();
    for (p, entry) in planned.iter().zip(&manifest.entries) {
        ctx.check_cancelled()?;
        ctx.set_current(Some(p.name.clone()));
        let large = entry.size_bytes >= u64::from(u32::MAX) / 2;
        zip.start_file(p.name.as_str(), base.large_file(large))
            .map_err(zip_error)?;
        let mut hashing = HashingWriter::new(&mut zip);
        produce(
            p,
            source,
            &mut hashing,
            &mut production,
            options.now,
            &ctx.cancel_flag().clone(),
        )?;
        let (sha256, size) = hashing.digest();
        if sha256 != entry.sha256 || size != entry.size_bytes {
            return Err(PortabilityError::SourceChanged(p.name.clone()));
        }
        ctx.advance(1, size);
    }
    let stream = zip.finish().map_err(zip_error)?;
    let buffered = stream
        .into_inner()
        .finish()
        .map_err(|e| PortabilityError::io(temp.display().to_string(), e))?;
    let file = buffered
        .into_inner()
        .map_err(|e| PortabilityError::io(temp.display().to_string(), e.into_error()))?;
    file.sync_all()
        .map_err(|e| PortabilityError::io(temp.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
