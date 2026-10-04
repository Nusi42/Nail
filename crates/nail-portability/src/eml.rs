//! Folders of `.eml` files: the reader (subfolders become folders) and the
//! writer (one file per message).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use nail_api::portability::{ItemRef, MAX_IMPORT_MESSAGE_BYTES, PortabilityIssueCode};

use crate::error::{PortabilityError, Result};
use crate::folder::{safe_file_name, source_folder};
use crate::import::{FolderLocation, MailFolderSource, RawMessage};
use crate::job::JobContext;
use crate::message::{header_facts, issue_label};

/// Deepest folder tree read.
pub const MAX_DEPTH: usize = 32;

/// Whether a path is an `.eml` file name.
pub fn is_eml(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"))
}

/// Entries of a directory, sorted by name; symbolic links are not
/// followed.
pub(crate) fn sorted_entries(dir: &Path) -> Result<Vec<(PathBuf, fs::FileType)>> {
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|e| PortabilityError::io(dir.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| PortabilityError::io(dir.display().to_string(), e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| PortabilityError::io(entry.path().display().to_string(), e))?;
        if file_type.is_symlink() {
            continue;
        }
        entries.push((entry.path(), file_type));
    }
    entries.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    Ok(entries)
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The folders of an `.eml` folder tree: the root (named after its
/// directory) and every subfolder, parents first.
pub fn scan(root: &Path) -> Result<Vec<MailFolderSource>> {
    if !root.is_dir() {
        return Err(PortabilityError::InvalidSource(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    let mut out = Vec::new();
    scan_dir(root, &mut vec![display_name(root)], 0, &mut out)?;
    Ok(out)
}

fn scan_dir(
    dir: &Path,
    segments: &mut Vec<String>,
    depth: usize,
    out: &mut Vec<MailFolderSource>,
) -> Result<()> {
    let entries = sorted_entries(dir)?;
    let (messages, size_bytes) = entries
        .iter()
        .filter(|(path, t)| t.is_file() && is_eml(path))
        .fold((0u64, 0u64), |(n, size), (path, _)| {
            (n + 1, size + fs::metadata(path).map_or(0, |m| m.len()))
        });
    let (folder, renamed) = source_folder(segments);
    out.push(MailFolderSource {
        folder,
        renamed_from: renamed.then(|| segments.join("/")),
        location: FolderLocation::Eml(dir.to_path_buf()),
        messages,
        size_bytes,
        estimated: false,
    });
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    for (path, file_type) in entries {
        if file_type.is_dir() {
            segments.push(display_name(&path));
            scan_dir(&path, segments, depth + 1, out)?;
            segments.pop();
        }
    }
    Ok(())
}

/// Reads the `.eml` files directly in `dir`, in name order.
pub fn read_dir(
    dir: &Path,
    folder_path: &str,
    ctx: &mut JobContext<'_>,
    visit: &mut dyn FnMut(&mut JobContext<'_>, RawMessage) -> Result<()>,
) -> Result<()> {
    let entries = sorted_entries(dir)?;
    let mut index = 0u64;
    for (path, file_type) in entries {
        if !file_type.is_file() || !is_eml(&path) {
            continue;
        }
        ctx.check_cancelled()?;
        index += 1;
        let origin = ItemRef {
            file: Some(path.display().to_string()),
            folder: Some(folder_path.to_owned()),
            index: Some(index),
            ..ItemRef::default()
        };
        let size = fs::metadata(&path).map_or(0, |m| m.len());
        if size > MAX_IMPORT_MESSAGE_BYTES {
            ctx.advance(0, size);
            visit(
                ctx,
                RawMessage::problem(
                    origin,
                    PortabilityIssueCode::TooLarge,
                    format!("{size} bytes"),
                ),
            )?;
            continue;
        }
        match fs::read(&path) {
            Ok(raw) => {
                ctx.advance(0, raw.len() as u64);
                visit(ctx, RawMessage::new(raw, origin))?;
            }
            Err(e) => visit(
                ctx,
                RawMessage::problem(origin, PortabilityIssueCode::UnreadableFile, e.to_string()),
            )?,
        }
    }
    Ok(())
}

/// Writes messages as `.eml` files into a directory: `00001 Subject.eml`.
#[derive(Debug)]
pub struct EmlWriter {
    dir: PathBuf,
    next: u64,
}

impl EmlWriter {
    /// A writer into `dir` (created).
    pub fn create(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).map_err(|e| PortabilityError::io(dir.display().to_string(), e))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            next: 1,
        })
    }

    /// Writes one message; returns its path.
    pub fn write(&mut self, raw: &[u8]) -> std::io::Result<PathBuf> {
        let label = issue_label(&header_facts(raw)).unwrap_or_default();
        let name = if label.trim().is_empty() {
            format!("{:05}.eml", self.next)
        } else {
            format!("{:05} {}.eml", self.next, safe_file_name(&label, 80))
        };
        self.next += 1;
        let path = self.dir.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(raw)?;
        file.flush()?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobContext;
    use nail_api::portability::{JobKind, JobReport};

    fn ctx() -> JobContext<'static> {
        JobContext::silent(JobReport::started("j".into(), JobKind::Import, "t"))
    }

    #[test]
    fn scans_trees_and_reads_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Export");
        fs::create_dir_all(root.join("Sub/Deeper")).unwrap();
        fs::write(root.join("b.eml"), "Subject: b\n\nB\n").unwrap();
        fs::write(root.join("a.EML"), "Subject: a\n\nA\n").unwrap();
        fs::write(root.join("notes.txt"), "not mail").unwrap();
        fs::write(root.join("Sub/Deeper/c.eml"), "Subject: c\n\nC\n").unwrap();
        let folders = scan(&root).unwrap();
        let paths: Vec<_> = folders.iter().map(|f| f.folder.path.as_str()).collect();
        assert_eq!(paths, vec!["Export", "Export/Sub", "Export/Sub/Deeper"]);
        assert_eq!(folders[0].messages, 2);
        assert_eq!(folders[1].messages, 0);
        assert_eq!(folders[2].folder.parent.as_deref(), Some("Export/Sub"));
        let mut seen = Vec::new();
        read_dir(&root, "Export", &mut ctx(), &mut |_, m| {
            seen.push((m.origin.index, String::from_utf8(m.raw).unwrap()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            seen,
            vec![
                (Some(1), "Subject: a\n\nA\n".to_owned()),
                (Some(2), "Subject: b\n\nB\n".to_owned())
            ]
        );
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.eml");
        fs::write(&file, "x").unwrap();
        assert!(matches!(
            scan(&file),
            Err(PortabilityError::InvalidSource(_))
        ));
    }

    #[test]
    fn writer_names_files_by_subject() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = EmlWriter::create(&dir.path().join("out")).unwrap();
        let a = writer.write(b"Subject: Re: Hello/World?\n\nx\n").unwrap();
        let b = writer.write(b"From: x\n\nno subject\n").unwrap();
        assert_eq!(a.file_name().unwrap(), "00001 Re_ Hello_World_.eml");
        assert_eq!(b.file_name().unwrap(), "00002.eml");
        assert_eq!(fs::read(&a).unwrap(), b"Subject: Re: Hello/World?\n\nx\n");
    }
}
