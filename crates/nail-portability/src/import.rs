//! Import previews and import jobs for every [`ImportFormat`].
//!
//! Each source is scanned into mail folders ([`MailFolderSource`]) and
//! PIM collections ([`PimCollectionSource`]); the preview reports them,
//! the import reads the chosen ones and pushes their items into the
//! core's sinks.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use nail_api::Flags;
use nail_api::LocalizedMessage;
use nail_api::domain::FolderRole;
use nail_api::portability::{
    ImportCollectionPreview, ImportFolderPreview, ImportFormat, ImportOptions, ImportPreview,
    ImportSource, IssueSeverity, ItemIssue, ItemRef, JobCounts, JobPhase, MAX_IMPORT_MESSAGE_BYTES,
    PortabilityIssueCode,
};
use nail_ical::import::ImportData;

use crate::error::{PortabilityError, Result};
use crate::folder::{folder_selected, source_folder};
use crate::job::{JobContext, PreviewIssues};
use crate::maildir::MaildirFolder;
use crate::mbox::{MboxReadOptions, MboxReader};
use crate::message::{
    duplicate_key, header_facts, header_fields, header_flags, issue_label, looks_like_message,
};
use crate::sink::{MailImportSink, PimImportSink, SourceCollection, SourceFolder};
use crate::{eml, maildir, pim, thunderbird};

/// mbox files above this are counted from their first part and
/// extrapolated in previews.
pub const PREVIEW_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// The Fluent id of the issue of a message deleted in the source.
pub const DELETED_IN_SOURCE_ID: &str = "portability-issue-deleted-in-source";

/// Where a source folder's messages are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderLocation {
    /// An mbox file.
    Mbox(PathBuf),
    /// A directory of `.eml` files.
    Eml(PathBuf),
    /// A Maildir folder.
    Maildir(MaildirFolder),
    /// A folder without messages (a parent).
    Empty,
}

/// A mail folder of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFolderSource {
    /// The folder as the sink sees it.
    pub folder: SourceFolder,
    /// The original path when the name had to change.
    pub renamed_from: Option<String>,
    /// Where its messages are.
    pub location: FolderLocation,
    /// Messages (estimated for large mbox files).
    pub messages: u64,
    /// Bytes of mail.
    pub size_bytes: u64,
    /// Whether `messages` is an estimate.
    pub estimated: bool,
}

/// Where a calendar / address book of a source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PimLocation {
    /// An `.ics` / `.vcf` file.
    File(PathBuf),
    /// A Thunderbird `abook.sqlite` / `history.sqlite`.
    AddressBookSqlite(PathBuf),
    /// A Thunderbird Mork `.mab` address book.
    Mork(PathBuf),
}

impl PimLocation {
    /// The file.
    pub fn path(&self) -> &Path {
        match self {
            Self::File(p) | Self::AddressBookSqlite(p) | Self::Mork(p) => p,
        }
    }

    /// Reads it.
    pub fn read(&self) -> Result<ImportData> {
        match self {
            Self::File(path) => pim::read_file(path),
            Self::AddressBookSqlite(path) => {
                thunderbird::abook::read_sqlite(path).map(ImportData::Cards)
            }
            Self::Mork(path) => thunderbird::mork::read_address_book(path).map(ImportData::Cards),
        }
    }
}

/// A calendar / address book of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimCollectionSource {
    /// The collection as the sink sees it.
    pub collection: SourceCollection,
    /// Items found.
    pub items: u64,
    /// Where it is.
    pub location: PimLocation,
}

/// One message as a reader found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessage {
    /// The raw message.
    pub raw: Vec<u8>,
    /// Flags from the source.
    pub flags: Flags,
    /// Keywords from the source.
    pub keywords: Vec<String>,
    /// Delivery time from the source.
    pub received_at: Option<i64>,
    /// Where it is.
    pub origin: ItemRef,
    /// Deleted in the source (not imported).
    pub deleted: bool,
    /// Whether the source stored flags in the message's own headers.
    pub header_flags: bool,
    /// Why it cannot be imported.
    pub problem: Option<(PortabilityIssueCode, String)>,
    /// Changes worth a warning (unknown flags).
    pub warnings: Vec<(PortabilityIssueCode, String)>,
}

impl RawMessage {
    /// A message without flags.
    pub fn new(raw: Vec<u8>, origin: ItemRef) -> Self {
        Self {
            raw,
            flags: Flags::default(),
            keywords: Vec::new(),
            received_at: None,
            origin,
            deleted: false,
            header_flags: false,
            problem: None,
            warnings: Vec::new(),
        }
    }

    /// A message that cannot be imported.
    pub fn problem(origin: ItemRef, code: PortabilityIssueCode, detail: impl Into<String>) -> Self {
        let mut message = Self::new(Vec::new(), origin);
        message.problem = Some((code, detail.into()));
        message
    }
}

/// What a source holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceScan {
    /// Mail folders, parents first.
    pub folders: Vec<MailFolderSource>,
    /// Calendars and address books.
    pub collections: Vec<PimCollectionSource>,
    /// Thunderbird proposals and problems found while scanning.
    pub profile: Option<thunderbird::ProfileScan>,
}

/// Scans a source (no message is read except mbox `From ` lines).
pub fn scan(source: &ImportSource) -> Result<SourceScan> {
    let path = Path::new(&source.path);
    let mut out = SourceScan::default();
    match source.format {
        ImportFormat::Mbox => out.folders.push(scan_mbox_file(path, &[file_stem(path)])?),
        ImportFormat::EmlFolder => out.folders = eml::scan(path)?,
        ImportFormat::Maildir => out.folders = maildir::scan(path)?,
        ImportFormat::ThunderbirdProfile => {
            let profile = thunderbird::scan_profile(path)?;
            out.folders = profile.folders.clone();
            out.collections = profile.collections.clone();
            out.profile = Some(profile);
        }
        ImportFormat::Ics | ImportFormat::Vcard => {
            if !path.is_file() {
                return Err(PortabilityError::InvalidSource(format!(
                    "{} is not a file",
                    path.display()
                )));
            }
            let data = pim::read_file(path)?;
            let (collection, items) = pim::collection_of(&source.path, &file_stem(path), &data);
            out.collections.push(PimCollectionSource {
                collection,
                items,
                location: PimLocation::File(path.to_path_buf()),
            });
        }
    }
    Ok(out)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mbox".to_owned())
}

/// Scans one mbox file as a folder named by `segments`: counts its
/// messages (from the first [`PREVIEW_SCAN_BYTES`] for larger files,
/// extrapolated).
pub fn scan_mbox_file(path: &Path, segments: &[String]) -> Result<MailFolderSource> {
    scan_mbox_file_within(path, segments, PREVIEW_SCAN_BYTES)
}

/// [`scan_mbox_file`] reading at most `scan_bytes`.
pub fn scan_mbox_file_within(
    path: &Path,
    segments: &[String],
    scan_bytes: u64,
) -> Result<MailFolderSource> {
    let size = std::fs::metadata(path)
        .map_err(|e| PortabilityError::io(path.display().to_string(), e))?
        .len();
    let file = File::open(path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
    let limited = BufReader::with_capacity(256 * 1024, file.take(scan_bytes));
    let options = MboxReadOptions {
        keep_bytes: false,
        max_message_bytes: u64::MAX,
        dialect: None,
    };
    let mut reader = MboxReader::new(limited, options);
    let mut messages = 0u64;
    loop {
        match reader.next_entry() {
            Ok(Some(_)) => messages += 1,
            Ok(None) => break,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData && size > 0 => {
                return Err(PortabilityError::InvalidSource(format!(
                    "{}: {e}",
                    path.display()
                )));
            }
            Err(e) => return Err(PortabilityError::io(path.display().to_string(), e)),
        }
    }
    let estimated = size > scan_bytes;
    if estimated && reader.bytes_read() > 0 {
        let scanned = reader.bytes_read();
        messages = ((u128::from(messages) * u128::from(size)) / u128::from(scanned)) as u64;
    }
    let (folder, renamed) = source_folder(segments);
    Ok(MailFolderSource {
        folder,
        renamed_from: renamed.then(|| segments.join("/")),
        location: FolderLocation::Mbox(path.to_path_buf()),
        messages,
        size_bytes: size,
        estimated,
    })
}

fn renamed_issue(folder: &MailFolderSource) -> Option<ItemIssue> {
    folder.renamed_from.as_ref().map(|original| {
        ItemIssue::new(
            PortabilityIssueCode::InvalidFolderName,
            IssueSeverity::Warning,
            ItemRef {
                folder: Some(original.clone()),
                label: Some(folder.folder.path.clone()),
                ..ItemRef::default()
            },
            format!("folder {original:?} imported as {:?}", folder.folder.path),
        )
    })
}

/// What an import would bring in ([`nail_api::Request::PreviewImport`]).
/// Account proposals' `duplicate_of` is left for the core
/// ([`thunderbird::mark_duplicates`]).
pub fn preview(source: &ImportSource) -> Result<ImportPreview> {
    let scan = scan(source)?;
    let mut counts = JobCounts::default();
    let mut issues = PreviewIssues::default();
    let folders = scan
        .folders
        .iter()
        .map(|f| {
            counts.messages += f.messages;
            counts.folders += 1;
            if let Some(issue) = renamed_issue(f) {
                issues.push(issue);
            }
            ImportFolderPreview {
                path: f.folder.path.clone(),
                name: f.folder.name.clone(),
                messages: f.messages,
                size_bytes: f.size_bytes,
            }
        })
        .collect();
    let mut collections = Vec::new();
    for c in &scan.collections {
        match c.collection.kind {
            nail_api::portability::ImportCollectionKind::AddressBook => counts.contacts += c.items,
            _ => counts.events += c.items,
        }
        collections.push(ImportCollectionPreview {
            key: c.collection.key.clone(),
            name: c.collection.name.clone(),
            kind: c.collection.kind,
            items: c.items,
        });
    }
    let (accounts, rules) = match scan.profile {
        Some(profile) => {
            for issue in profile.issues {
                issues.push(issue);
            }
            (profile.accounts, profile.rules)
        }
        None => (Vec::new(), Vec::new()),
    };
    counts.accounts = accounts.len() as u64;
    counts.rules = rules.len() as u64;
    Ok(ImportPreview {
        source: source.clone(),
        folders,
        collections,
        counts,
        accounts,
        rules,
        issues: issues.issues,
        issues_truncated: issues.truncated,
    })
}

/// Runs an import ([`nail_api::Request::StartImport`]): reads the chosen
/// folders and collections and pushes their items into the sinks; the
/// report gets counts, issues and the Thunderbird proposals. The caller
/// finishes the report ([`crate::job::finish_report`]).
pub fn run(
    source: &ImportSource,
    options: &ImportOptions,
    mail: &mut dyn MailImportSink,
    pim_sink: &mut dyn PimImportSink,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    ctx.report.source = Some(source.path.clone());
    ctx.phase(JobPhase::Scanning, None, None);
    let scan = scan(source)?;
    ctx.check_cancelled()?;
    let selection = options.folders.as_deref();
    let folders: Vec<&MailFolderSource> = scan
        .folders
        .iter()
        .filter(|f| folder_selected(selection, &f.folder.path))
        .collect();
    let collections: Vec<&PimCollectionSource> = scan
        .collections
        .iter()
        .filter(|c| {
            options
                .collections
                .as_ref()
                .is_none_or(|keys| keys.contains(&c.collection.key))
        })
        .collect();
    if let Some(profile) = &scan.profile {
        for issue in &profile.issues {
            ctx.issue(issue.clone());
        }
        ctx.report.accounts = profile.accounts.clone();
        ctx.report.rules = profile.rules.clone();
        ctx.report.counts.accounts = profile.accounts.len() as u64;
        ctx.report.counts.rules = profile.rules.len() as u64;
    }
    let total = folders.iter().map(|f| f.messages).sum::<u64>()
        + collections.iter().map(|c| c.items).sum::<u64>();
    let bytes = folders.iter().map(|f| f.size_bytes).sum::<u64>();
    ctx.phase(JobPhase::Reading, Some(total), Some(bytes));

    let mut ensured: HashSet<String> = HashSet::new();
    for folder in folders {
        ctx.check_cancelled()?;
        if let Some(issue) = renamed_issue(folder) {
            ctx.issue(issue);
        }
        ensure_with_parents(mail, &folder.folder, &mut ensured, ctx)?;
        ctx.set_current(Some(folder.folder.path.clone()));
        let target = folder.folder.clone();
        let mut visit = |ctx: &mut JobContext<'_>, message: RawMessage| -> Result<()> {
            import_message(ctx, mail, &target, options, message)
        };
        read_location(&folder.location, &folder.folder.path, ctx, &mut visit)?;
    }
    for collection in collections {
        ctx.check_cancelled()?;
        ctx.set_current(Some(collection.collection.name.clone()));
        let file = collection.location.path().display().to_string();
        match collection.location.read() {
            Ok(data) => pim::import_data(ctx, pim_sink, &collection.collection, Some(&file), data)?,
            Err(PortabilityError::Cancelled) => return Err(PortabilityError::Cancelled),
            Err(e) => ctx.skipped(
                match collection.location {
                    PimLocation::File(_) => PortabilityIssueCode::UnreadableFile,
                    _ => PortabilityIssueCode::UnsupportedFormat,
                },
                ItemRef {
                    file: Some(file),
                    folder: Some(collection.collection.name.clone()),
                    ..ItemRef::default()
                },
                e.to_string(),
            ),
        }
    }
    ctx.phase(JobPhase::Finishing, None, None);
    Ok(())
}

fn ensure_with_parents(
    mail: &mut dyn MailImportSink,
    folder: &SourceFolder,
    ensured: &mut HashSet<String>,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    let segments: Vec<&str> = folder.path.split('/').collect();
    for len in 1..=segments.len() {
        let path = segments[..len].join("/");
        if ensured.contains(&path) {
            continue;
        }
        let current = if len == segments.len() {
            folder.clone()
        } else {
            SourceFolder {
                name: segments[len - 1].to_owned(),
                parent: (len > 1).then(|| segments[..len - 1].join("/")),
                role: if len == 1 {
                    crate::folder::guess_role(&path)
                } else {
                    FolderRole::Other
                },
                path: path.clone(),
            }
        };
        mail.ensure_folder(&current)
            .map_err(PortabilityError::from)?;
        ensured.insert(path);
        ctx.report.counts.folders += 1;
    }
    Ok(())
}

/// Reads the messages at a location.
pub fn read_location(
    location: &FolderLocation,
    folder_path: &str,
    ctx: &mut JobContext<'_>,
    visit: &mut dyn FnMut(&mut JobContext<'_>, RawMessage) -> Result<()>,
) -> Result<()> {
    match location {
        FolderLocation::Mbox(path) => read_mbox(path, folder_path, ctx, visit),
        FolderLocation::Eml(dir) => eml::read_dir(dir, folder_path, ctx, visit),
        FolderLocation::Maildir(folder) => maildir::read_folder(folder, folder_path, ctx, visit),
        FolderLocation::Empty => Ok(()),
    }
}

/// Reads an mbox file message by message.
pub fn read_mbox(
    path: &Path,
    folder_path: &str,
    ctx: &mut JobContext<'_>,
    visit: &mut dyn FnMut(&mut JobContext<'_>, RawMessage) -> Result<()>,
) -> Result<()> {
    let file = File::open(path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
    let options = MboxReadOptions::default();
    let mut reader = MboxReader::new(BufReader::with_capacity(256 * 1024, file), options);
    let file_name = path.display().to_string();
    let mut last_bytes = 0u64;
    loop {
        ctx.check_cancelled()?;
        let entry = match reader.next_entry() {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Err(PortabilityError::InvalidSource(format!("{file_name}: {e}")));
            }
            Err(e) => return Err(PortabilityError::io(file_name, e)),
        };
        let read = reader.bytes_read();
        ctx.advance(0, read - last_bytes);
        last_bytes = read;
        let origin = ItemRef {
            file: Some(file_name.clone()),
            folder: Some(folder_path.to_owned()),
            index: Some(entry.index),
            offset: Some(entry.offset),
            label: None,
        };
        let message = if entry.too_large {
            RawMessage::problem(
                origin,
                PortabilityIssueCode::TooLarge,
                format!("{} bytes", entry.size_on_disk),
            )
        } else if entry.headers_unfinished {
            RawMessage::problem(
                origin,
                PortabilityIssueCode::Truncated,
                "the file ends inside the headers",
            )
        } else {
            let received_at = entry.received_at();
            let mut message = RawMessage::new(entry.bytes, origin);
            message.received_at = received_at;
            message.header_flags = true;
            if entry.truncated {
                message.warnings.push((
                    PortabilityIssueCode::Truncated,
                    "the file ends before the message's Content-Length".to_owned(),
                ));
            }
            message
        };
        visit(ctx, message)?;
    }
    Ok(())
}

/// Checks, deduplicates and appends one message.
pub fn import_message(
    ctx: &mut JobContext<'_>,
    sink: &mut dyn MailImportSink,
    folder: &SourceFolder,
    options: &ImportOptions,
    mut message: RawMessage,
) -> Result<()> {
    ctx.advance(1, 0);
    if let Some((code, detail)) = message.problem.take() {
        ctx.skipped(code, message.origin, detail);
        return Ok(());
    }
    if message.raw.len() as u64 > MAX_IMPORT_MESSAGE_BYTES {
        ctx.skipped(
            PortabilityIssueCode::TooLarge,
            message.origin,
            format!("{} bytes", message.raw.len()),
        );
        return Ok(());
    }
    if !looks_like_message(&message.raw) {
        ctx.skipped(
            PortabilityIssueCode::MalformedMessage,
            message.origin,
            "no header fields",
        );
        return Ok(());
    }
    let facts = header_facts(&message.raw);
    message.origin.label = issue_label(&facts);
    if message.header_flags {
        let found = header_flags(&header_fields(&message.raw));
        if found.present {
            message.flags = found.flags;
            message.keywords = found.keywords;
            message.deleted |= found.deleted;
        }
    }
    if message.deleted {
        let mut issue = ItemIssue::new(
            PortabilityIssueCode::UnknownFlag,
            IssueSeverity::Skipped,
            message.origin,
            "deleted in the source (not compacted / expunged yet)",
        );
        issue.localized = LocalizedMessage::new(DELETED_IN_SOURCE_ID);
        ctx.issue(issue);
        return Ok(());
    }
    for (code, detail) in std::mem::take(&mut message.warnings) {
        ctx.warning(code, message.origin.clone(), detail);
    }
    if !options.keep_flags {
        message.flags = Flags::default();
        message.keywords.clear();
    }
    let key = duplicate_key(&message.raw, &facts);
    if options.skip_duplicates
        && sink
            .contains(folder, &key)
            .map_err(PortabilityError::from)?
    {
        ctx.skipped(
            PortabilityIssueCode::Duplicate,
            message.origin,
            "already in the folder",
        );
        return Ok(());
    }
    let imported = crate::sink::ImportedMessage {
        raw: message.raw,
        flags: message.flags,
        keywords: message.keywords,
        received_at: message.received_at,
        key,
        origin: message.origin.clone(),
    };
    match sink.append(folder, imported) {
        Ok(()) => ctx.report.counts.messages += 1,
        Err(e) => pim::write_failed(ctx, message.origin, e)?,
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::error::SinkError;
    use crate::sink::{DuplicateKey, ImportedMessage, NullSink};
    use nail_api::portability::{JobKind, JobReport, JobState};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::fs;

    /// A sink that keeps everything in memory, like the core's store.
    #[derive(Default)]
    pub(crate) struct MemorySink {
        pub folders: Vec<SourceFolder>,
        pub messages: HashMap<String, Vec<ImportedMessage>>,
    }

    impl MailImportSink for MemorySink {
        fn ensure_folder(&mut self, folder: &SourceFolder) -> std::result::Result<(), SinkError> {
            if !self.folders.iter().any(|f| f.path == folder.path) {
                self.folders.push(folder.clone());
            }
            Ok(())
        }
        fn contains(
            &mut self,
            folder: &SourceFolder,
            key: &DuplicateKey,
        ) -> std::result::Result<bool, SinkError> {
            Ok(self
                .messages
                .get(&folder.path)
                .is_some_and(|m| m.iter().any(|m| &m.key == key)))
        }
        fn append(
            &mut self,
            folder: &SourceFolder,
            message: ImportedMessage,
        ) -> std::result::Result<(), SinkError> {
            if message.raw.windows(4).any(|w| w == b"FAIL") {
                return Err(SinkError::Item("refused".into()));
            }
            self.messages
                .entry(folder.path.clone())
                .or_default()
                .push(message);
            Ok(())
        }
    }

    fn new_ctx() -> JobContext<'static> {
        JobContext::silent(JobReport::started("j".into(), JobKind::Import, "t"))
    }

    const MBOX: &str = "From a@b Thu Oct  1 10:00:00 2026\nSubject: one\nMessage-ID: <1@x>\nStatus: RO\nX-Status: F\n\nHello\n>From here\n\nFrom b@c Thu Oct  1 11:00:00 2026\nSubject: two\nMessage-ID: <2@x>\nX-Mozilla-Status: 0008\n\ndeleted\n\nFrom c@d Thu Oct  1 12:00:00 2026\nnot a header line\n\nFrom d@e Thu Oct  1 13:00:00 2026\nSubject: FAIL\n\nx\n\nFrom e@f Thu Oct  1 14:00:00 2026\nSubject: four\nMessage-ID: <4@x>\n\nbye\n";

    #[test]
    fn imports_an_mbox_with_flags_issues_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Saved.mbox");
        fs::write(&path, MBOX).unwrap();
        let source = ImportSource {
            format: ImportFormat::Mbox,
            path: path.display().to_string(),
        };
        let preview = preview(&source).unwrap();
        assert_eq!(preview.folders.len(), 1);
        assert_eq!(preview.folders[0].path, "Saved");
        assert_eq!(preview.folders[0].messages, 5);
        assert_eq!(preview.counts.messages, 5);

        let mut sink = MemorySink::default();
        let mut ctx = new_ctx();
        run(
            &source,
            &ImportOptions::default(),
            &mut sink,
            &mut NullSink,
            &mut ctx,
        )
        .unwrap();
        let report = ctx.into_report();
        let saved = &sink.messages["Saved"];
        assert_eq!(saved.len(), 2);
        assert_eq!(report.counts.messages, 2);
        assert_eq!(report.counts.folders, 1);
        assert!(saved[0].flags.seen && saved[0].flags.flagged);
        assert_eq!(saved[0].received_at, Some(1_790_848_800));
        assert!(String::from_utf8_lossy(&saved[0].raw).contains("\nFrom here\n"));
        assert_eq!(saved[0].key.message_id.as_deref(), Some("1@x"));
        let codes: Vec<_> = report
            .issues
            .iter()
            .map(|i| (i.code, i.item.index))
            .collect();
        assert_eq!(
            codes,
            vec![
                (PortabilityIssueCode::UnknownFlag, Some(2)),
                (PortabilityIssueCode::MalformedMessage, Some(3)),
                (PortabilityIssueCode::WriteFailed, Some(4)),
            ]
        );
        assert_eq!(report.issues[0].localized.id, DELETED_IN_SOURCE_ID);
        assert_eq!(report.issues[2].item.label.as_deref(), Some("FAIL"));
        assert_eq!(report.finished_state(), JobState::CompletedWithIssues);

        // Again: everything is a duplicate now.
        let mut ctx2 = new_ctx();
        run(
            &source,
            &ImportOptions::default(),
            &mut sink,
            &mut NullSink,
            &mut ctx2,
        )
        .unwrap();
        let report = ctx2.into_report();
        assert_eq!(report.counts.messages, 0);
        assert_eq!(report.counts.duplicates, 2);
        assert_eq!(sink.messages["Saved"].len(), 2);
    }

    #[test]
    fn flags_can_be_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mbox");
        fs::write(&path, MBOX).unwrap();
        let source = ImportSource {
            format: ImportFormat::Mbox,
            path: path.display().to_string(),
        };
        let mut sink = MemorySink::default();
        let options = ImportOptions {
            keep_flags: false,
            skip_duplicates: false,
            ..ImportOptions::default()
        };
        run(&source, &options, &mut sink, &mut NullSink, &mut new_ctx()).unwrap();
        assert_eq!(sink.messages["x"][0].flags, Flags::default());
    }

    #[test]
    fn not_an_mbox_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mbox");
        fs::write(&path, "Subject: x\n\nno from line").unwrap();
        let source = ImportSource {
            format: ImportFormat::Mbox,
            path: path.display().to_string(),
        };
        assert!(matches!(
            preview(&source),
            Err(PortabilityError::InvalidSource(_))
        ));
        let missing = ImportSource {
            format: ImportFormat::Mbox,
            path: dir.path().join("nope").display().to_string(),
        };
        assert!(matches!(
            preview(&missing),
            Err(PortabilityError::Io { .. })
        ));
    }

    #[test]
    fn large_mbox_previews_are_estimated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mbox");
        let one = "From a Thu Oct  1 10:00:00 2026\nSubject: x\n\nbody\n\n";
        fs::write(&path, one.repeat(1000)).unwrap();
        let exact = scan_mbox_file(&path, &["x".into()]).unwrap();
        assert!(!exact.estimated);
        assert_eq!(exact.messages, 1000);
        assert_eq!(exact.size_bytes, (one.len() * 1000) as u64);
        // Only the first tenth read: extrapolated from the bytes read.
        let estimate =
            scan_mbox_file_within(&path, &["x".into()], (one.len() * 100) as u64).unwrap();
        assert!(estimate.estimated);
        assert!(
            (950..=1050).contains(&estimate.messages),
            "{}",
            estimate.messages
        );
    }

    #[test]
    fn imports_a_maildir_selection_with_parents() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for sub in ["cur", "new", "tmp"] {
            fs::create_dir_all(root.join(sub)).unwrap();
            fs::create_dir_all(root.join(".Lists.Rust").join(sub)).unwrap();
        }
        fs::write(root.join("cur/1700000000.a;2,S"), "Subject: inbox\n\n1\n").unwrap();
        fs::write(
            root.join(".Lists.Rust/cur/1700000001.b;2,FRS"),
            "Subject: rust\n\n2\n",
        )
        .unwrap();
        let source = ImportSource {
            format: ImportFormat::Maildir,
            path: root.display().to_string(),
        };
        let preview = preview(&source).unwrap();
        let paths: Vec<_> = preview.folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["Inbox", "Lists", "Lists/Rust"]);
        let mut sink = MemorySink::default();
        let options = ImportOptions {
            folders: Some(vec!["Lists/Rust".into()]),
            ..ImportOptions::default()
        };
        let mut ctx = new_ctx();
        run(&source, &options, &mut sink, &mut NullSink, &mut ctx).unwrap();
        let created: Vec<_> = sink.folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(created, vec!["Lists", "Lists/Rust"]);
        assert_eq!(sink.folders[1].parent.as_deref(), Some("Lists"));
        let rust = &sink.messages["Lists/Rust"];
        assert_eq!(rust.len(), 1);
        assert!(rust[0].flags.answered && rust[0].flags.flagged && rust[0].flags.seen);
        assert!(!sink.messages.contains_key("Inbox"));
        assert_eq!(ctx.report.counts.folders, 2);
    }

    #[test]
    fn imports_eml_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Mails");
        fs::create_dir_all(root.join("Sub")).unwrap();
        fs::write(root.join("a.eml"), "Subject: a\n\nA\n").unwrap();
        fs::write(root.join("Sub/b.eml"), "garbage without headers").unwrap();
        let source = ImportSource {
            format: ImportFormat::EmlFolder,
            path: root.display().to_string(),
        };
        let mut sink = MemorySink::default();
        let mut ctx = new_ctx();
        run(
            &source,
            &ImportOptions::default(),
            &mut sink,
            &mut NullSink,
            &mut ctx,
        )
        .unwrap();
        assert_eq!(sink.messages["Mails"].len(), 1);
        assert_eq!(ctx.report.counts.skipped, 1);
        assert_eq!(
            ctx.report.issues[0].code,
            PortabilityIssueCode::MalformedMessage
        );
    }

    #[test]
    fn imports_ics_and_vcf_files() {
        let dir = tempfile::tempdir().unwrap();
        let ics = dir.path().join("cal.ics");
        fs::write(
            &ics,
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART:20261001T100000Z\r\nSUMMARY:x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let source = ImportSource {
            format: ImportFormat::Ics,
            path: ics.display().to_string(),
        };
        let shown = preview(&source).unwrap();
        assert_eq!(shown.collections.len(), 1);
        assert_eq!(shown.collections[0].name, "cal");
        assert_eq!(shown.counts.events, 1);
        let mut ctx = new_ctx();
        run(
            &source,
            &ImportOptions::default(),
            &mut NullSink,
            &mut NullSink,
            &mut ctx,
        )
        .unwrap();
        assert_eq!(ctx.report.counts.events, 1);
        // A collection not chosen is left alone.
        let mut ctx = new_ctx();
        let options = ImportOptions {
            collections: Some(vec![]),
            ..ImportOptions::default()
        };
        run(&source, &options, &mut NullSink, &mut NullSink, &mut ctx).unwrap();
        assert_eq!(ctx.report.counts.events, 0);
        let folder = ImportSource {
            format: ImportFormat::Vcard,
            path: dir.path().display().to_string(),
        };
        assert!(matches!(
            preview(&folder),
            Err(PortabilityError::InvalidSource(_))
        ));
    }

    #[test]
    fn cancelling_stops_between_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mbox");
        fs::write(&path, MBOX).unwrap();
        let source = ImportSource {
            format: ImportFormat::Mbox,
            path: path.display().to_string(),
        };
        let mut ctx = new_ctx();
        ctx.cancel_flag().cancel();
        let result = run(
            &source,
            &ImportOptions::default(),
            &mut MemorySink::default(),
            &mut NullSink,
            &mut ctx,
        );
        assert!(matches!(result, Err(PortabilityError::Cancelled)));
    }
}
