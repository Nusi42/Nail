//! Export jobs: mail as mbox / `.eml` / Maildir, calendars as `.ics`,
//! address books as `.vcf`.
//!
//! Layout:
//! * **mbox**: one folder → the target is that file; several → the
//!   target is a folder with `A.mbox`, `A/B.mbox`, ….
//! * **`.eml`** and **Maildir**: when every folder is below one top folder,
//!   it becomes the target itself and its subfolders subfolders (Maildir++
//!   `.Sub`); otherwise each top folder becomes a subfolder.
//! * **`.ics` / `.vcf`**: one collection → the target is that file;
//!   several → a folder with one file per collection.
//!
//! Single files are written next to the target and renamed into place
//! when complete; without `overwrite` an existing target (a non-empty
//! folder for folder targets) is a `conflict`.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nail_api::portability::{ExportFormat, ExportOptions, ItemRef, JobPhase, PortabilityIssueCode};

use crate::eml::EmlWriter;
use crate::error::{PortabilityError, Result, SinkError};
use crate::folder::safe_file_name;
use crate::job::JobContext;
use crate::maildir::MaildirWriter;
use crate::mbox::{MboxReadOptions, MboxReader, MboxWriter};
use crate::pim::{IcsBuilder, VcfBuilder};
use crate::sink::{
    ExportCollection, ExportFolder, ExportMessage, MailExportSource, PimExportSource, Visit,
};

/// Most characters of a file name written for a folder / collection.
const NAME_CHARS: usize = 120;

/// Checks the target: a file target must not exist, a folder target must
/// be missing or empty (unless `overwrite`).
pub fn check_target(path: &Path, directory: bool, overwrite: bool) -> Result<()> {
    if overwrite || !path.exists() {
        return Ok(());
    }
    let occupied = if directory && path.is_dir() {
        fs::read_dir(path)
            .map_err(|e| PortabilityError::io(path.display().to_string(), e))?
            .next()
            .is_some()
    } else {
        true
    };
    if occupied {
        Err(PortabilityError::TargetExists(path.display().to_string()))
    } else {
        Ok(())
    }
}

/// A file written under a temporary name and renamed into place.
struct PendingFile {
    temp: PathBuf,
    target: PathBuf,
}

impl PendingFile {
    fn create(target: &Path) -> Result<(Self, BufWriter<File>)> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| PortabilityError::io(parent.display().to_string(), e))?;
        }
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(".nail-part");
        let temp = target.with_file_name(name);
        let file =
            File::create(&temp).map_err(|e| PortabilityError::io(temp.display().to_string(), e))?;
        Ok((
            Self {
                temp,
                target: target.to_path_buf(),
            },
            BufWriter::new(file),
        ))
    }

    fn commit(self, writer: BufWriter<File>) -> Result<()> {
        let file = writer
            .into_inner()
            .map_err(|e| PortabilityError::io(self.temp.display().to_string(), e.into_error()))?;
        file.sync_all()
            .map_err(|e| PortabilityError::io(self.temp.display().to_string(), e))?;
        drop(file);
        fs::rename(&self.temp, &self.target)
            .map_err(|e| PortabilityError::io(self.target.display().to_string(), e))
    }

    fn discard(self) {
        drop(fs::remove_file(&self.temp));
    }
}

/// Where each folder goes, relative to a folder target: the single common
/// top folder is the root.
fn relative_paths(folders: &[ExportFolder]) -> Vec<Option<String>> {
    let top = folders
        .first()
        .map(|f| f.path.split('/').next().unwrap_or("").to_owned());
    let single_top = top.as_ref().is_some_and(|top| {
        folders
            .iter()
            .all(|f| f.path == *top || f.path.starts_with(&format!("{top}/")))
    });
    folders
        .iter()
        .map(|f| {
            if single_top {
                let top = top.as_deref().unwrap_or("");
                f.path
                    .strip_prefix(top)
                    .and_then(|r| r.strip_prefix('/'))
                    .map(str::to_owned)
            } else {
                Some(f.path.clone())
            }
        })
        .collect()
}

/// Exports mail ([`nail_api::Request::StartExport`] with folders or
/// messages).
pub fn run_mail(
    format: ExportFormat,
    target: &Path,
    options: &ExportOptions,
    source: &mut dyn MailExportSource,
    ctx: &mut JobContext<'_>,
    now: DateTime<Utc>,
) -> Result<()> {
    if !matches!(
        format,
        ExportFormat::Mbox | ExportFormat::Eml | ExportFormat::Maildir
    ) {
        return Err(PortabilityError::InvalidOptions(format!(
            "{} cannot export mail",
            format.as_str()
        )));
    }
    ctx.report.target = Some(target.display().to_string());
    ctx.phase(JobPhase::Scanning, None, None);
    let folders = source.folders().map_err(PortabilityError::from)?;
    let single_file = format == ExportFormat::Mbox && folders.len() == 1;
    check_target(target, !single_file, options.overwrite)?;
    let total = folders.iter().map(|f| f.messages).sum::<Option<u64>>();
    let bytes = folders.iter().map(|f| f.size_bytes).sum::<Option<u64>>();
    ctx.phase(JobPhase::Writing, total, bytes);
    match format {
        ExportFormat::Mbox => export_mbox(target, single_file, options, &folders, source, ctx, now),
        ExportFormat::Eml => export_eml(target, &folders, source, ctx),
        _ => export_maildir(target, &folders, source, ctx),
    }?;
    ctx.phase(JobPhase::Finishing, None, None);
    Ok(())
}

/// Visits the messages of a folder, reporting unreadable ones.
fn each_message(
    source: &mut dyn MailExportSource,
    folder: &ExportFolder,
    ctx: &mut JobContext<'_>,
    write: &mut dyn FnMut(&mut JobContext<'_>, u64, ExportMessage) -> Result<()>,
) -> Result<()> {
    let mut index = 0u64;
    let mut failure: Option<PortabilityError> = None;
    source
        .messages(folder, &mut |message| {
            if ctx.cancel_flag().is_cancelled() {
                failure = Some(PortabilityError::Cancelled);
                return Visit::Stop;
            }
            index += 1;
            match message {
                Ok(message) => {
                    let size = message.raw.len() as u64;
                    if let Err(e) = write(ctx, index, message) {
                        failure = Some(e);
                        return Visit::Stop;
                    }
                    ctx.advance(1, size);
                }
                Err(SinkError::Item(detail)) => {
                    ctx.skipped(
                        PortabilityIssueCode::UnreadableFile,
                        ItemRef {
                            folder: Some(folder.path.clone()),
                            index: Some(index),
                            ..ItemRef::default()
                        },
                        detail,
                    );
                    ctx.advance(1, 0);
                }
                Err(SinkError::Fatal(detail)) => {
                    failure = Some(PortabilityError::Sink(detail));
                    return Visit::Stop;
                }
            }
            Visit::Continue
        })
        .map_err(PortabilityError::from)?;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn export_mbox(
    target: &Path,
    single_file: bool,
    options: &ExportOptions,
    folders: &[ExportFolder],
    source: &mut dyn MailExportSource,
    ctx: &mut JobContext<'_>,
    now: DateTime<Utc>,
) -> Result<()> {
    let mut written_files = Vec::new();
    for folder in folders {
        ctx.check_cancelled()?;
        ctx.set_current(Some(folder.path.clone()));
        let path = if single_file {
            target.to_path_buf()
        } else {
            let mut path = target.to_path_buf();
            let segments: Vec<&str> = folder.path.split('/').collect();
            for segment in &segments[..segments.len() - 1] {
                path.push(safe_file_name(segment, NAME_CHARS));
            }
            path.push(format!(
                "{}.mbox",
                safe_file_name(segments[segments.len() - 1], NAME_CHARS)
            ));
            path
        };
        if !single_file && !options.overwrite && path.exists() {
            return Err(PortabilityError::TargetExists(path.display().to_string()));
        }
        let (pending, file) = PendingFile::create(&path)?;
        let mut writer = MboxWriter::new(file, options.mbox_variant);
        let file_name = path.display().to_string();
        let result = each_message(source, folder, ctx, &mut |_, _, message| {
            writer
                .write_message(&message, now)
                .map_err(|e| PortabilityError::io(file_name.clone(), e))
        });
        let messages = writer.messages();
        let finished = result.and_then(|()| {
            writer
                .finish()
                .map_err(|e| PortabilityError::io(file_name.clone(), e))
        });
        match finished {
            Ok(file) => pending.commit(file)?,
            Err(e) => {
                pending.discard();
                return Err(e);
            }
        }
        ctx.report.counts.messages += messages;
        ctx.report.counts.folders += 1;
        written_files.push((path, messages, folder.path.clone()));
    }
    // Read the files back: every message must be there.
    ctx.phase(JobPhase::Verifying, Some(written_files.len() as u64), None);
    for (path, expected, folder) in written_files {
        ctx.check_cancelled()?;
        let file =
            File::open(&path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
        let options = MboxReadOptions {
            keep_bytes: false,
            max_message_bytes: u64::MAX,
            dialect: None,
        };
        let found = MboxReader::new(BufReader::new(file), options)
            .flatten()
            .count() as u64;
        if found != expected {
            ctx.warning(
                PortabilityIssueCode::WriteFailed,
                ItemRef {
                    file: Some(path.display().to_string()),
                    folder: Some(folder),
                    ..ItemRef::default()
                },
                format!("{expected} messages written, {found} read back"),
            );
        }
        ctx.advance(1, 0);
    }
    Ok(())
}

fn export_eml(
    target: &Path,
    folders: &[ExportFolder],
    source: &mut dyn MailExportSource,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    for (folder, relative) in folders.iter().zip(relative_paths(folders)) {
        ctx.check_cancelled()?;
        ctx.set_current(Some(folder.path.clone()));
        let mut dir = target.to_path_buf();
        for segment in relative.iter().flat_map(|r| r.split('/')) {
            dir.push(safe_file_name(segment, NAME_CHARS));
        }
        let mut writer = EmlWriter::create(&dir)?;
        let folder_path = folder.path.clone();
        each_message(source, folder, ctx, &mut |ctx, index, message| {
            match writer.write(&message.raw) {
                Ok(_) => ctx.report.counts.messages += 1,
                Err(e) => ctx.skipped(
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        PortabilityIssueCode::TargetExists
                    } else {
                        PortabilityIssueCode::WriteFailed
                    },
                    ItemRef {
                        folder: Some(folder_path.clone()),
                        index: Some(index),
                        ..ItemRef::default()
                    },
                    e.to_string(),
                ),
            }
            Ok(())
        })?;
        ctx.report.counts.folders += 1;
    }
    Ok(())
}

fn export_maildir(
    target: &Path,
    folders: &[ExportFolder],
    source: &mut dyn MailExportSource,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    let mut writer = MaildirWriter::create(target)?;
    for (folder, relative) in folders.iter().zip(relative_paths(folders)) {
        ctx.check_cancelled()?;
        ctx.set_current(Some(folder.path.clone()));
        let (dir, renamed) = writer.folder(relative.as_deref())?;
        if renamed {
            ctx.warning(
                PortabilityIssueCode::InvalidFolderName,
                ItemRef {
                    folder: Some(folder.path.clone()),
                    file: Some(dir.display().to_string()),
                    ..ItemRef::default()
                },
                "Maildir++ folder names cannot hold dots",
            );
        }
        let folder_path = folder.path.clone();
        each_message(source, folder, ctx, &mut |ctx, index, message| {
            let item = ItemRef {
                folder: Some(folder_path.clone()),
                index: Some(index),
                ..ItemRef::default()
            };
            match writer.write(
                &dir,
                &message.raw,
                message.flags,
                &message.keywords,
                message.received_at,
            ) {
                Ok((_, notes)) => {
                    ctx.report.counts.messages += 1;
                    if !notes.dropped_keywords.is_empty() {
                        ctx.warning(
                            PortabilityIssueCode::UnknownFlag,
                            item,
                            format!("keywords dropped: {}", notes.dropped_keywords.join(" ")),
                        );
                    }
                }
                Err(e) => ctx.skipped(PortabilityIssueCode::WriteFailed, item, e.to_string()),
            }
            Ok(())
        })?;
        ctx.report.counts.folders += 1;
    }
    writer.finish()
}

/// Exports calendars (`.ics`) or address books (`.vcf`).
pub fn run_pim(
    format: ExportFormat,
    target: &Path,
    options: &ExportOptions,
    source: &mut dyn PimExportSource,
    ctx: &mut JobContext<'_>,
) -> Result<()> {
    if !matches!(format, ExportFormat::Ics | ExportFormat::Vcard) {
        return Err(PortabilityError::InvalidOptions(format!(
            "{} cannot export calendars or contacts",
            format.as_str()
        )));
    }
    ctx.report.target = Some(target.display().to_string());
    ctx.phase(JobPhase::Scanning, None, None);
    let collections = source.collections().map_err(PortabilityError::from)?;
    let single = collections.len() == 1;
    check_target(target, !single, options.overwrite)?;
    let total = collections.iter().map(|c| c.items).sum::<Option<u64>>();
    ctx.phase(JobPhase::Writing, total, None);
    let extension = format.extension().unwrap_or("txt");
    let mut used_names: Vec<String> = Vec::new();
    for collection in &collections {
        ctx.check_cancelled()?;
        ctx.set_current(Some(collection.name.clone()));
        let path = if single {
            target.to_path_buf()
        } else {
            let mut name = safe_file_name(&collection.name, NAME_CHARS);
            let base = name.clone();
            let mut n = 2;
            while used_names.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
                name = format!("{base} ({n})");
                n += 1;
            }
            used_names.push(name.clone());
            let path = target.join(format!("{name}.{extension}"));
            if !options.overwrite && path.exists() {
                return Err(PortabilityError::TargetExists(path.display().to_string()));
            }
            path
        };
        let text = collect_collection(format, collection, source, ctx)?;
        let (pending, mut file) = PendingFile::create(&path)?;
        let written = file
            .write_all(text.as_bytes())
            .map_err(|e| PortabilityError::io(path.display().to_string(), e));
        match written {
            Ok(()) => pending.commit(file)?,
            Err(e) => {
                pending.discard();
                return Err(e);
            }
        }
        ctx.report.counts.files += 1;
    }
    ctx.phase(JobPhase::Finishing, None, None);
    Ok(())
}

fn collect_collection(
    format: ExportFormat,
    collection: &ExportCollection,
    source: &mut dyn PimExportSource,
    ctx: &mut JobContext<'_>,
) -> Result<String> {
    let mut ics = IcsBuilder::new(&collection.name);
    let mut vcf = VcfBuilder::new();
    let mut index = 0u64;
    let mut failure = None;
    source
        .items(collection, &mut |item| {
            if ctx.cancel_flag().is_cancelled() {
                failure = Some(PortabilityError::Cancelled);
                return Visit::Stop;
            }
            index += 1;
            let item_ref = ItemRef {
                folder: Some(collection.name.clone()),
                index: Some(index),
                ..ItemRef::default()
            };
            match item {
                Ok(text) => {
                    let added = if format == ExportFormat::Ics {
                        ics.add(&text)
                    } else {
                        vcf.add(&text)
                    };
                    if added == 0 {
                        ctx.skipped(
                            if format == ExportFormat::Ics {
                                PortabilityIssueCode::InvalidCalendarData
                            } else {
                                PortabilityIssueCode::InvalidContactData
                            },
                            item_ref,
                            "the item holds no calendar component / card",
                        );
                    }
                }
                Err(SinkError::Item(detail)) => {
                    ctx.skipped(PortabilityIssueCode::UnreadableFile, item_ref, detail);
                }
                Err(SinkError::Fatal(detail)) => {
                    failure = Some(PortabilityError::Sink(detail));
                    return Visit::Stop;
                }
            }
            ctx.advance(1, 0);
            Visit::Continue
        })
        .map_err(PortabilityError::from)?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(if format == ExportFormat::Ics {
        ctx.report.counts.events += ics.events();
        ctx.report.counts.tasks += ics.tasks();
        ics.finish()
    } else {
        ctx.report.counts.groups += vcf.groups();
        ctx.report.counts.contacts += vcf.cards() - vcf.groups();
        vcf.finish()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use nail_api::Flags;
    use nail_api::portability::{ImportFormat, ImportSource, JobKind, JobReport, MboxVariant};
    use pretty_assertions::assert_eq;

    /// Mail held in memory, like the core's store.
    #[derive(Default, Clone)]
    pub(crate) struct MemoryMail {
        pub folders: Vec<(String, Vec<ExportMessage>)>,
    }

    impl MailExportSource for MemoryMail {
        fn folders(&mut self) -> std::result::Result<Vec<ExportFolder>, SinkError> {
            Ok(self
                .folders
                .iter()
                .map(|(path, messages)| ExportFolder {
                    path: path.clone(),
                    messages: Some(messages.len() as u64),
                    size_bytes: None,
                })
                .collect())
        }

        fn messages(
            &mut self,
            folder: &ExportFolder,
            visit: &mut dyn FnMut(std::result::Result<ExportMessage, SinkError>) -> Visit,
        ) -> std::result::Result<(), SinkError> {
            let (_, messages) = self
                .folders
                .iter()
                .find(|(p, _)| *p == folder.path)
                .unwrap();
            for message in messages {
                let item = if message.raw.starts_with(b"UNREADABLE") {
                    Err(SinkError::Item("blob missing".into()))
                } else {
                    Ok(message.clone())
                };
                if visit(item) == Visit::Stop {
                    break;
                }
            }
            Ok(())
        }
    }

    pub(crate) fn message(subject: &str, body: &str, flags: Flags) -> ExportMessage {
        ExportMessage {
            raw: format!("Subject: {subject}\r\nMessage-ID: <{subject}@x>\r\nDate: Thu, 01 Oct 2026 10:00:00 +0000\r\n\r\n{body}").into_bytes(),
            flags,
            keywords: vec![],
            received_at: Some(1_790_848_800),
            envelope_from: None,
        }
    }

    pub(crate) fn sample() -> MemoryMail {
        let seen = Flags {
            seen: true,
            ..Flags::default()
        };
        MemoryMail {
            folders: vec![
                (
                    "Inbox".into(),
                    vec![
                        message("one", "From the start\r\n>From quoted\r\n", seen),
                        message("two", "no newline at the end", Flags::default()),
                    ],
                ),
                (
                    "Inbox/Lists".into(),
                    vec![message(
                        "three",
                        "list mail\r\n",
                        Flags {
                            flagged: true,
                            ..Flags::default()
                        },
                    )],
                ),
            ],
        }
    }

    fn ctx() -> JobContext<'static> {
        JobContext::silent(JobReport::started("j".into(), JobKind::Export, "t"))
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_848_800, 0).unwrap()
    }

    #[test]
    fn mbox_export_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("inbox.mbox");
        let mut one = MemoryMail {
            folders: vec![sample().folders[0].clone()],
        };
        let mut ctx = ctx();
        run_mail(
            ExportFormat::Mbox,
            &target,
            &ExportOptions::default(),
            &mut one,
            &mut ctx,
            now(),
        )
        .unwrap();
        assert_eq!(ctx.report.counts.messages, 2);
        assert!(ctx.report.issues.is_empty(), "{:?}", ctx.report.issues);
        assert!(!dir.path().join("inbox.mbox.nail-part").exists());
        // Re-import: identical messages (a final line end is added).
        let mut sink = crate::testing::MemorySink::default();
        let source = ImportSource {
            format: ImportFormat::Mbox,
            path: target.display().to_string(),
        };
        sink.import(&source).unwrap();
        let back = &sink.messages("inbox");
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].raw, one.folders[0].1[0].raw);
        let mut expected = one.folders[0].1[1].raw.clone();
        expected.extend_from_slice(b"\r\n");
        assert_eq!(back[1].raw, expected);
        assert_eq!(back[0].received_at, Some(1_790_848_800));
        // The target exists now.
        let again = run_mail(
            ExportFormat::Mbox,
            &target,
            &ExportOptions::default(),
            &mut one,
            &mut ctx_new(),
            now(),
        );
        assert!(matches!(again, Err(PortabilityError::TargetExists(_))));
        let overwrite = ExportOptions {
            overwrite: true,
            mbox_variant: MboxVariant::Mboxo,
            ..ExportOptions::default()
        };
        run_mail(
            ExportFormat::Mbox,
            &target,
            &overwrite,
            &mut one,
            &mut ctx_new(),
            now(),
        )
        .unwrap();
    }

    fn ctx_new() -> JobContext<'static> {
        ctx()
    }

    #[test]
    fn several_folders_make_a_folder_of_mbox_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out");
        let mut mail = sample();
        let mut ctx = ctx();
        run_mail(
            ExportFormat::Mbox,
            &target,
            &ExportOptions::default(),
            &mut mail,
            &mut ctx,
            now(),
        )
        .unwrap();
        assert!(target.join("Inbox.mbox").is_file());
        assert!(target.join("Inbox").join("Lists.mbox").is_file());
        assert_eq!(ctx.report.counts.folders, 2);
        assert_eq!(ctx.report.counts.messages, 3);
    }

    #[test]
    fn maildir_export_keeps_flags_and_tree() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Maildir");
        let mut mail = sample();
        mail.folders[0].1[0].keywords = vec!["work".into()];
        let mut ctx = ctx();
        run_mail(
            ExportFormat::Maildir,
            &target,
            &ExportOptions::default(),
            &mut mail,
            &mut ctx,
            now(),
        )
        .unwrap();
        let mut sink = crate::testing::MemorySink::default();
        let source = ImportSource {
            format: ImportFormat::Maildir,
            path: target.display().to_string(),
        };
        sink.import(&source).unwrap();
        let inbox = &sink.messages("Inbox");
        assert_eq!(inbox.len(), 2);
        let one = inbox
            .iter()
            .find(|m| m.raw == mail.folders[0].1[0].raw)
            .unwrap();
        assert!(one.flags.seen);
        assert_eq!(one.keywords, vec!["work"]);
        let lists = &sink.messages("Lists");
        assert!(lists[0].flags.flagged);
        assert_eq!(lists[0].raw, mail.folders[1].1[0].raw);
    }

    #[test]
    fn eml_export_and_unreadable_messages() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("eml");
        let mut mail = sample();
        mail.folders[0].1.push(ExportMessage {
            raw: b"UNREADABLE".to_vec(),
            ..ExportMessage::default()
        });
        mail.folders
            .push(("Other".into(), vec![message("four", "x", Flags::default())]));
        let mut ctx = ctx();
        run_mail(
            ExportFormat::Eml,
            &target,
            &ExportOptions::default(),
            &mut mail,
            &mut ctx,
            now(),
        )
        .unwrap();
        assert!(target.join("Inbox").join("00001 one.eml").is_file());
        assert!(
            target
                .join("Inbox")
                .join("Lists")
                .join("00001 three.eml")
                .is_file()
        );
        assert!(target.join("Other").join("00001 four.eml").is_file());
        assert_eq!(ctx.report.counts.messages, 4);
        assert_eq!(ctx.report.counts.skipped, 1);
        assert_eq!(
            ctx.report.issues[0].code,
            PortabilityIssueCode::UnreadableFile
        );
        // A non-empty folder target is a conflict.
        let again = run_mail(
            ExportFormat::Eml,
            &target,
            &ExportOptions::default(),
            &mut mail,
            &mut ctx_new(),
            now(),
        );
        assert!(matches!(again, Err(PortabilityError::TargetExists(_))));
    }

    #[test]
    fn wrong_formats_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_mail(
            ExportFormat::Ics,
            dir.path(),
            &ExportOptions::default(),
            &mut sample(),
            &mut ctx(),
            now(),
        );
        assert!(matches!(result, Err(PortabilityError::InvalidOptions(_))));
    }

    fn collection(key: &str, name: &str) -> ExportCollection {
        ExportCollection {
            key: key.into(),
            name: name.into(),
            items: None,
        }
    }

    #[test]
    fn calendars_and_address_books() {
        let dir = tempfile::tempdir().unwrap();
        let event = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20261001T100000Z\r\nSUMMARY:x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let task = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:t1\r\nSUMMARY:y\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let mut source = crate::testing::MemoryPim::new(vec![
            (
                collection("c1", "Home"),
                vec![event.into(), task.into(), "garbage".into()],
            ),
            (collection("c2", "Home"), vec![event.into()]),
        ]);
        let target = dir.path().join("calendars");
        let mut ctx = ctx();
        run_pim(
            ExportFormat::Ics,
            &target,
            &ExportOptions::default(),
            &mut source,
            &mut ctx,
        )
        .unwrap();
        assert!(target.join("Home.ics").is_file());
        assert!(target.join("Home (2).ics").is_file());
        assert_eq!(ctx.report.counts.events, 2);
        assert_eq!(ctx.report.counts.tasks, 1);
        assert_eq!(ctx.report.counts.files, 2);
        assert_eq!(ctx.report.counts.skipped, 1);
        let text = fs::read_to_string(target.join("Home.ics")).unwrap();
        assert!(text.contains("X-WR-CALNAME:Home"));

        let card = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada\r\nEND:VCARD\r\n";
        let mut books =
            crate::testing::MemoryPim::new(vec![(collection("b1", "People"), vec![card.into()])]);
        let file = dir.path().join("people.vcf");
        let mut ctx = ctx_new();
        run_pim(
            ExportFormat::Vcard,
            &file,
            &ExportOptions::default(),
            &mut books,
            &mut ctx,
        )
        .unwrap();
        assert_eq!(ctx.report.counts.contacts, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), card);
        let again = run_pim(
            ExportFormat::Vcard,
            &file,
            &ExportOptions::default(),
            &mut books,
            &mut ctx_new(),
        );
        assert!(matches!(again, Err(PortabilityError::TargetExists(_))));
    }

    #[test]
    fn cancelling_an_export_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.mbox");
        let mut one = MemoryMail {
            folders: vec![sample().folders[0].clone()],
        };
        let mut ctx = ctx();
        ctx.cancel_flag().cancel();
        let result = run_mail(
            ExportFormat::Mbox,
            &target,
            &ExportOptions::default(),
            &mut one,
            &mut ctx,
            now(),
        );
        assert!(matches!(result, Err(PortabilityError::Cancelled)));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn relative_layout() {
        let single = [ExportFolder::new("A"), ExportFolder::new("A/B")];
        assert_eq!(relative_paths(&single), vec![None, Some("B".to_owned())]);
        let several = [ExportFolder::new("A"), ExportFolder::new("C")];
        assert_eq!(
            relative_paths(&several),
            vec![Some("A".to_owned()), Some("C".to_owned())]
        );
        let prefix = [ExportFolder::new("A"), ExportFolder::new("AB")];
        assert_eq!(relative_paths(&prefix)[1], Some("AB".to_owned()));
    }
}
