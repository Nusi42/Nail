//! Copies of the caller's attachments.
//!
//! Programs often delete the files they pass to `MAPISendMail` as soon as
//! the call returns (Office writes a temporary copy of the document, Explorer
//! passes the original). The DLL therefore copies every file into a private
//! folder under `%TEMP%\nail-mapi\` before it talks to the app, hands the
//! app the copies, and removes the folder once the app has answered (the
//! core reads every attachment before answering, ADR 0087). A folder left
//! behind by a killed process is swept away by a later call after a day.
//!
//! The root may have been prepared by someone else (M4b review): it is
//! refused unless it is a real directory owned by the caller (or the
//! Administrators group / SYSTEM), and it stays pinned while the call works
//! below it (`windows::staging_root`). The sweep removes only folders named
//! the way this DLL names them (`<pid>-<hex nanos>-<n>`) and never follows a
//! junction or symlink.

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
    /// The pinned root (released after the folder is gone).
    #[cfg(windows)]
    _root: Option<crate::windows::staging_root::RootGuard>,
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
        return Ok(StagedFiles {
            dir: None,
            #[cfg(windows)]
            _root: None,
        });
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
    fs::create_dir_all(root).map_err(|_| MapiResult::Failure)?;
    #[cfg(windows)]
    let guard =
        crate::windows::staging_root::RootGuard::open(root).map_err(|_| MapiResult::Failure)?;
    sweep_stale(root, STALE_AFTER);
    let dir = create_unique_dir(root).map_err(|_| MapiResult::Failure)?;
    let staged = StagedFiles {
        dir: Some(dir.clone()),
        #[cfg(windows)]
        _root: Some(guard),
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

/// Whether `name` is one of this DLL's staging folders
/// (`<pid>-<hex nanos>-<n>`, [`create_unique_dir`]).
pub fn is_staging_dir_name(name: &str) -> bool {
    let mut parts = name.split('-');
    let (Some(pid), Some(nanos), Some(n), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let digits = |s: &str, hex: bool| {
        !s.is_empty()
            && s.len() <= 32
            && s.chars().all(|c| {
                if hex {
                    c.is_ascii_digit() || ('a'..='f').contains(&c)
                } else {
                    c.is_ascii_digit()
                }
            })
    };
    digits(pid, false) && digits(nanos, true) && digits(n, false)
}

/// Whether a directory entry is a junction, symlink or other reparse point
/// (read from the listing itself, never by following it).
fn is_link(entry: &fs::DirEntry) -> bool {
    !entry.file_type().is_ok_and(|t| !t.is_symlink()) || is_reparse_point(entry)
}

#[cfg(windows)]
fn is_reparse_point(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    // DirEntry::metadata does not traverse links on Windows.
    !entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0)
}

#[cfg(not(windows))]
fn is_reparse_point(_entry: &fs::DirEntry) -> bool {
    false
}

/// Removes this DLL's staging folders under `root` last modified more than
/// `age` ago. Anything else (other names, junctions, symlinks, files) is
/// left alone; `remove_dir_all` does not follow links inside a folder.
pub fn sweep_stale(root: &Path, age: Duration) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let Some(cutoff) = SystemTime::now().checked_sub(age) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(is_staging_dir_name) || is_link(&entry) {
            continue;
        }
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
        let old = create_unique_dir(root.path()).unwrap();
        fs::write(old.join("f"), b"x").unwrap();
        sweep_stale(root.path(), Duration::from_secs(3600));
        assert!(old.exists());
        sweep_stale(root.path(), Duration::ZERO);
        assert!(!old.exists());
        sweep_stale(&root.path().join("missing"), Duration::ZERO);
    }

    #[test]
    fn only_own_folders_are_swept() {
        let root = tempfile::tempdir().unwrap();
        let other = root.path().join("SomeAppData");
        fs::create_dir(&other).unwrap();
        let file = root.path().join("12-ab-0");
        fs::write(&file, b"x").unwrap();
        sweep_stale(root.path(), Duration::ZERO);
        assert!(other.exists());
        assert!(file.exists());
        assert!(is_staging_dir_name("1234-18a2b3c4d5e6f-0"));
        assert!(is_staging_dir_name(
            create_unique_dir(root.path())
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        ));
        for name in [
            "old", "1-2", "1-2-3-4", "a-2-3", "1-g-3", "1--3", "1-AB-3", "",
        ] {
            assert!(!is_staging_dir_name(name), "{name}");
        }
    }

    /// A junction named like a staging folder: the sweep must not delete
    /// what it points to (an elevated caller would delete with its rights).
    #[cfg(windows)]
    #[test]
    fn junctions_are_never_followed() {
        use crate::windows::staging_root::tests::junction;
        let root = tempfile::tempdir().unwrap();
        let protected = tempfile::tempdir().unwrap();
        let precious = protected.path().join("precious");
        fs::create_dir(&precious).unwrap();
        fs::write(precious.join("data"), b"keep").unwrap();
        junction(&root.path().join("1-ab-0"), protected.path());
        sweep_stale(root.path(), Duration::ZERO);
        assert_eq!(fs::read(precious.join("data")).unwrap(), b"keep");
    }

    /// The root itself is a junction (pre-created by another process):
    /// nothing is swept or copied through it.
    #[cfg(windows)]
    #[test]
    fn a_junction_root_is_refused() {
        use crate::windows::staging_root::tests::junction;
        let base = tempfile::tempdir().unwrap();
        let protected = tempfile::tempdir().unwrap();
        let victim = protected.path().join("1-ab-0");
        fs::create_dir(&victim).unwrap();
        let root = base.path().join("nail-mapi");
        junction(&root, protected.path());
        let src = base.path().join("a.txt");
        fs::write(&src, b"a").unwrap();
        let mut m = message(&[(&src, None)]);
        assert_eq!(
            stage_attachments(&mut m, &root).unwrap_err(),
            MapiResult::Failure
        );
        assert!(victim.exists(), "nothing deleted through the junction");
        assert_eq!(fs::read_dir(protected.path()).unwrap().count(), 1);
        assert_eq!(m.attachments[0].path, src.to_str().unwrap());
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
