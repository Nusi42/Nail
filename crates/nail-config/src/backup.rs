//! Atomic writes and backups of configuration files.
//!
//! Before the merge dialog or a revert replaces a file, the current version
//! is copied to `<config dir>/backups/<stem>-<unix millis>.kdl`; a file that
//! did not exist is recorded as `<stem>-<millis>.absent`, so reverting to it
//! removes the file again. The newest [`KEEP`] backups per file are kept.

use std::io;
use std::path::{Path, PathBuf};

use nail_api::customize::ConfigBackup;
use nail_api::{ConfigBackupId, ConfigFile};

/// Directory of the backups inside the config directory.
pub const BACKUP_DIR: &str = "backups";

/// Backups kept per file.
pub const KEEP: usize = 20;

/// Writes `text` to `path` atomically (a temporary file next to it, then a
/// rename), creating the directory if needed.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&temp, text.as_bytes())?;
    std::fs::rename(&temp, path).inspect_err(|_| remove_temp(&temp))
}

/// Removes a temporary file after a failed write (best effort: a leftover
/// is only logged, and the next write replaces it).
pub(crate) fn remove_temp(temp: &Path) {
    if let Err(err) = std::fs::remove_file(temp)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(%err, temp = %temp.display(), "temporary file not removed");
    }
}

/// Reads a file; `None` when it does not exist.
pub(crate) fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn stem(file: ConfigFile) -> &'static str {
    file.file_name().trim_end_matches(".kdl")
}

/// A backup file name parsed: its id, time and whether the file was absent.
struct Entry {
    id: String,
    millis: u64,
    absent: bool,
}

fn parse_entry(file: ConfigFile, name: &str) -> Option<Entry> {
    let (id, absent) = if let Some(id) = name.strip_suffix(".kdl") {
        (id, false)
    } else {
        (name.strip_suffix(".absent")?, true)
    };
    let millis = id.strip_prefix(stem(file))?.strip_prefix('-')?;
    if millis.is_empty() || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Entry {
        id: id.to_owned(),
        millis: millis.parse().ok()?,
        absent,
    })
}

fn entries(dir: &Path, file: ConfigFile) -> io::Result<Vec<Entry>> {
    let backups = dir.join(BACKUP_DIR);
    let read = match std::fs::read_dir(&backups) {
        Ok(read) => read,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in read {
        let name = entry?.file_name();
        if let Some(parsed) = name.to_str().and_then(|n| parse_entry(file, n)) {
            out.push(parsed);
        }
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.millis));
    Ok(out)
}

fn backup_path(dir: &Path, entry: &Entry) -> PathBuf {
    let ext = if entry.absent { "absent" } else { "kdl" };
    dir.join(BACKUP_DIR).join(format!("{}.{ext}", entry.id))
}

fn to_backup(file: ConfigFile, entry: &Entry) -> ConfigBackup {
    ConfigBackup {
        id: ConfigBackupId::new(entry.id.clone()),
        file,
        created_at: rfc3339(entry.millis),
    }
}

/// Copies the current version of `file` (in config directory `dir`) into
/// the backups and prunes old ones.
pub(crate) fn take(dir: &Path, file: ConfigFile) -> io::Result<ConfigBackup> {
    let current = read_optional(&dir.join(file.file_name()))?;
    let existing = entries(dir, file)?;
    // Ids must be unique and increasing, also within one millisecond.
    let mut millis = nail_util::time::unix_now_millis();
    if let Some(newest) = existing.first()
        && newest.millis >= millis
    {
        millis = newest.millis + 1;
    }
    let entry = Entry {
        id: format!("{}-{millis}", stem(file)),
        millis,
        absent: current.is_none(),
    };
    write_atomic(
        &backup_path(dir, &entry),
        current.as_deref().unwrap_or_default(),
    )?;
    for old in existing.iter().skip(KEEP.saturating_sub(1)) {
        let old = backup_path(dir, old);
        if let Err(err) = std::fs::remove_file(&old) {
            tracing::debug!(%err, backup = %old.display(), "old backup not removed");
        }
    }
    Ok(to_backup(file, &entry))
}

/// Backups of `file`, newest first.
pub(crate) fn list(dir: &Path, file: ConfigFile) -> io::Result<Vec<ConfigBackup>> {
    Ok(entries(dir, file)?
        .iter()
        .map(|e| to_backup(file, e))
        .collect())
}

/// Restores a backup (`None` = the newest): backs the current file up
/// first, then writes the backup's text (or removes the file when the
/// backup records that it did not exist). `Ok(None)` when there is no such
/// backup.
pub(crate) fn restore(
    dir: &Path,
    file: ConfigFile,
    id: Option<&ConfigBackupId>,
) -> io::Result<Option<ConfigBackup>> {
    let all = entries(dir, file)?;
    let Some(target) = (match id {
        None => all.first(),
        Some(id) => all.iter().find(|e| e.id == id.as_str()),
    }) else {
        return Ok(None);
    };
    let text = if target.absent {
        None
    } else {
        Some(std::fs::read_to_string(backup_path(dir, target))?)
    };
    let saved = take(dir, file)?;
    let path = dir.join(file.file_name());
    match text {
        Some(text) => write_atomic(&path, &text)?,
        None => match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        },
    }
    Ok(Some(saved))
}

/// Milliseconds since the epoch as RFC 3339 UTC, e.g.
/// `2026-09-29T10:15:00.123Z`.
pub(crate) use nail_util::time::rfc3339_utc_millis as rfc3339;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(1_790_590_500_123), "2026-09-28T10:15:00.123Z");
    }

    #[test]
    fn names_are_parsed_strictly() {
        let e = parse_entry(ConfigFile::Theme, "theme-17.kdl").unwrap();
        assert_eq!((e.id.as_str(), e.millis, e.absent), ("theme-17", 17, false));
        assert!(
            parse_entry(ConfigFile::Theme, "theme-17.absent")
                .unwrap()
                .absent
        );
        for bad in [
            "layout-17.kdl",
            "theme-.kdl",
            "theme-1a.kdl",
            "theme-17.txt",
            "theme17.kdl",
        ] {
            assert!(parse_entry(ConfigFile::Theme, bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn backups_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.kdl");
        // No file yet: the backup records its absence.
        let first = take(dir.path(), ConfigFile::Theme).unwrap();
        write_atomic(&path, "one").unwrap();
        let second = take(dir.path(), ConfigFile::Theme).unwrap();
        write_atomic(&path, "two").unwrap();
        let listed = list(dir.path(), ConfigFile::Theme).unwrap();
        assert_eq!(listed, [second.clone(), first.clone()]);
        assert_eq!(list(dir.path(), ConfigFile::Layout).unwrap().len(), 0);

        // Newest: back to "one"; the current "two" is backed up first.
        let saved = restore(dir.path(), ConfigFile::Theme, None)
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one");
        assert_eq!(list(dir.path(), ConfigFile::Theme).unwrap()[0], saved);
        // The first backup: the file did not exist.
        restore(dir.path(), ConfigFile::Theme, Some(&first.id))
            .unwrap()
            .unwrap();
        assert!(!path.exists());
        assert_eq!(
            restore(dir.path(), ConfigFile::Theme, Some(&"theme-1".into())).unwrap(),
            None
        );
    }

    #[test]
    fn old_backups_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..KEEP + 5 {
            take(dir.path(), ConfigFile::Layout).unwrap();
        }
        assert_eq!(list(dir.path(), ConfigFile::Layout).unwrap().len(), KEEP);
    }
}
