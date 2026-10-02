//! Copies of the caller's attachments.
//!
//! Programs often delete the files they pass to `MAPISendMail` as soon as
//! the call returns (Office writes a temporary copy of the document, Explorer
//! passes the original). The DLL therefore copies every file into a private
//! folder under `%TEMP%\nail-mapi\` before it talks to the app, hands the
//! app the copies, and removes the folder once the app has answered (the
//! core reads every attachment before answering, ADR 0087). A folder left
//! behind by a killed process is swept away by a later call after a day.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nail_api::mapi::{MapiMessage, MapiResult};

/// Largest file the DLL copies: the composer's attachment limit (25 MiB,
/// `nail_api::compose::MAX_ATTACHMENT_BYTES`); larger files are refused
/// with `MAPI_E_ATTACHMENT_TOO_LARGE` before anything is copied.
pub const MAX_STAGED_FILE_BYTES: u64 = nail_api::compose::MAX_ATTACHMENT_BYTES;

/// Staging folders older than this are removed by later calls.
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// The folder all staging folders live in: `%TEMP%\nail-mapi`.
pub fn default_staging_root() -> PathBuf {
    std::env::temp_dir().join("nail-mapi")
}

/// The copies of one call; dropping it removes them.
#[derive(Debug)]
pub struct StagedFiles {
    dir: Option<PathBuf>,
}

impl StagedFiles {
    /// The call's folder (`None` without attachments).
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }
}

impl Drop for StagedFiles {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            // Best effort: a file the app still holds open is swept later.
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn io_result(err: &io::Error) -> MapiResult {
    match err.kind() {
        io::ErrorKind::NotFound => MapiResult::AttachmentNotFound,
        _ => MapiResult::AttachmentOpenFailure,
    }
}

/// Copies every attachment of `message` into a new folder under `root` and
/// points the attachments at the copies (`file_name` keeps the name the
/// program meant). Nothing is copied when one file is missing, not a file
/// or larger than [`MAX_STAGED_FILE_BYTES`].
pub fn stage_attachments(
    message: &mut MapiMessage,
    root: &Path,
) -> Result<StagedFiles, MapiResult> {
    if message.attachments.is_empty() {
        return Ok(StagedFiles { dir: None });
    }
    for attachment in &message.attachments {
        let meta = fs::metadata(&attachment.path).map_err(|e| io_result(&e))?;
        if !meta.is_file() {
            return Err(MapiResult::AttachmentOpenFailure);
        }
        if meta.len() > MAX_STAGED_FILE_BYTES {
            return Err(MapiResult::AttachmentTooLarge);
        }
    }
    sweep_stale(root, STALE_AFTER);
    let dir = create_unique_dir(root).map_err(|_| MapiResult::Failure)?;
    let staged = StagedFiles {
        dir: Some(dir.clone()),
    };
    for (index, attachment) in message.attachments.iter_mut().enumerate() {
        let name = attachment.display_name();
        // One sub-folder per file: two attachments may share a name.
        let folder = dir.join(index.to_string());
        fs::create_dir(&folder).map_err(|_| MapiResult::Failure)?;
        let target = folder.join(safe_file_name(&name));
        fs::copy(&attachment.path, &target).map_err(|e| io_result(&e))?;
        attachment.path = target.to_str().ok_or(MapiResult::Failure)?.to_owned();
        attachment.file_name = Some(name);
    }
    Ok(staged)
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn create_unique_dir(root: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for _ in 0..64 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(format!("{}-{nanos:x}-{n}", std::process::id()));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

/// Removes staging folders under `root` last modified more than `age` ago.
pub fn sweep_stale(root: &Path, age: Duration) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let Some(cutoff) = SystemTime::now().checked_sub(age) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() && meta.modified().is_ok_and(|m| m < cutoff) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// A name Windows accepts for a file: reserved characters and control
/// characters become `_`, trailing dots and spaces go, device names
/// (`CON`, `COM1`, …) get a `_` prefix, at most 200 characters (keeping a
/// short extension), never empty.
pub fn safe_file_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    out = out.trim_start().trim_end_matches(['.', ' ']).to_owned();
    if out.is_empty() {
        return "attachment".to_owned();
    }
    let stem = out.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.len() == 4
        && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.insert(0, '_');
    }
    const MAX_CHARS: usize = 200;
    if out.chars().count() > MAX_CHARS {
        let ext = out
            .rfind('.')
            .map(|i| out[i..].to_owned())
            .filter(|e| e.chars().count() <= 16)
            .unwrap_or_default();
        let keep = MAX_CHARS - ext.chars().count();
        let head: String = out.chars().take(keep).collect();
        out = format!("{}{ext}", head.trim_end_matches(['.', ' ']));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::mapi::{MapiAttachment, MapiUi};
    use pretty_assertions::assert_eq;

    fn message(paths: &[(&Path, Option<&str>)]) -> MapiMessage {
        MapiMessage {
            subject: None,
            body: None,
            recipients: Vec::new(),
            attachments: paths
                .iter()
                .map(|(p, name)| MapiAttachment {
                    path: p.to_str().unwrap().to_owned(),
                    file_name: name.map(str::to_owned),
                })
                .collect(),
            ui: MapiUi::Dialog,
            program: None,
        }
    }

    #[test]
    fn copies_files_and_removes_them_on_drop() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let a = src.path().join("a.txt");
        let b = src.path().join("b.tmp");
        fs::write(&a, b"alpha").unwrap();
        fs::write(&b, b"beta").unwrap();
        let mut m = message(&[(&a, None), (&b, Some("Report.docx"))]);
        let staged = stage_attachments(&mut m, root.path()).unwrap();
        let dir = staged.dir().unwrap().to_owned();
        assert!(dir.starts_with(root.path()));
        assert_eq!(m.attachments[0].file_name.as_deref(), Some("a.txt"));
        assert_eq!(m.attachments[1].file_name.as_deref(), Some("Report.docx"));
        assert!(m.attachments[1].path.ends_with("Report.docx"));
        // The caller may delete its files now.
        fs::remove_file(&a).unwrap();
        fs::remove_file(&b).unwrap();
        assert_eq!(fs::read(&m.attachments[0].path).unwrap(), b"alpha");
        assert_eq!(fs::read(&m.attachments[1].path).unwrap(), b"beta");
        drop(staged);
        assert!(!dir.exists());
    }

    #[test]
    fn same_names_do_not_collide() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let a = src.path().join("x.txt");
        fs::write(&a, b"1").unwrap();
        let mut m = message(&[(&a, Some("same.txt")), (&a, Some("same.txt"))]);
        let _staged = stage_attachments(&mut m, root.path()).unwrap();
        assert_ne!(m.attachments[0].path, m.attachments[1].path);
    }

    #[test]
    fn errors_copy_nothing() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let a = src.path().join("a.txt");
        fs::write(&a, b"a").unwrap();
        let missing = src.path().join("missing.txt");
        let mut m = message(&[(&a, None), (&missing, None)]);
        assert_eq!(
            stage_attachments(&mut m, root.path()).unwrap_err(),
            MapiResult::AttachmentNotFound
        );
        let mut m = message(&[(src.path(), None)]);
        assert_eq!(
            stage_attachments(&mut m, root.path()).unwrap_err(),
            MapiResult::AttachmentOpenFailure
        );
        let big = src.path().join("big.bin");
        let file = fs::File::create(&big).unwrap();
        file.set_len(MAX_STAGED_FILE_BYTES + 1).unwrap();
        drop(file);
        let mut m = message(&[(&big, None)]);
        assert_eq!(
            stage_attachments(&mut m, root.path()).unwrap_err(),
            MapiResult::AttachmentTooLarge
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        // No attachments: no folder at all.
        let mut m = message(&[]);
        assert!(
            stage_attachments(&mut m, root.path())
                .unwrap()
                .dir()
                .is_none()
        );
    }

    #[test]
    fn stale_folders_are_swept() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        fs::create_dir(&old).unwrap();
        sweep_stale(root.path(), Duration::from_secs(3600));
        assert!(old.exists());
        sweep_stale(root.path(), Duration::ZERO);
        assert!(!old.exists());
        sweep_stale(&root.path().join("missing"), Duration::ZERO);
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("Q3 report.docx"), "Q3 report.docx");
        assert_eq!(
            safe_file_name(r#"a<b>c:d"e/f\g|h?i*j"#),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(safe_file_name("tab\there"), "tab_here");
        assert_eq!(safe_file_name("name. . "), "name");
        assert_eq!(safe_file_name("  "), "attachment");
        assert_eq!(safe_file_name("..."), "attachment");
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(safe_file_name("COM1"), "_COM1");
        assert_eq!(safe_file_name("COM10.txt"), "COM10.txt");
        assert_eq!(safe_file_name("Consent.pdf"), "Consent.pdf");
        let long = format!("{}.pdf", "x".repeat(300));
        let safe = safe_file_name(&long);
        assert_eq!(safe.chars().count(), 200);
        assert!(safe.ends_with(".pdf"));
        let umlauts = "ä".repeat(250);
        assert_eq!(safe_file_name(&umlauts).chars().count(), 200);
    }
}
