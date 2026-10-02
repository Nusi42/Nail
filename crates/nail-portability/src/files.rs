//! Putting a finished file in place (post-M4c): files are written under a
//! temporary name next to the target ([`temp_path`]) and then renamed onto
//! it ([`persist`]). Replacing an existing target happens only with
//! `overwrite` (the user confirmed it in the Save dialog) and is atomic —
//! readers see the old file or the new one, never a half-written one;
//! without `overwrite` an existing target is
//! [`PortabilityError::TargetExists`] and stays untouched, also when it
//! appeared while the file was written.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{PortabilityError, Result};

/// The temporary name a file is written under before [`persist`]
/// (`<name>.nail-part` in the target's folder, so the rename stays on one
/// volume).
pub fn temp_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".nail-part");
    target.with_file_name(name)
}

/// Whether `target` may be written: it does not exist, or `overwrite` is
/// set and it is not a folder.
pub fn check_target(target: &Path, overwrite: bool) -> Result<()> {
    if target.is_dir() || (target.exists() && !overwrite) {
        return Err(PortabilityError::TargetExists(target.display().to_string()));
    }
    Ok(())
}

/// Renames the written `temp` file onto `target`: replacing an existing
/// target only with `overwrite` (atomically), else
/// [`PortabilityError::TargetExists`]. `temp` is removed when the rename
/// fails.
pub fn persist(temp: &Path, target: &Path, overwrite: bool) -> Result<()> {
    let temp = tempfile::TempPath::try_from_path(temp)
        .map_err(|e| PortabilityError::io(temp.display().to_string(), e))?;
    let result = if overwrite {
        temp.persist(target)
    } else {
        temp.persist_noclobber(target)
    };
    // On failure the error owns the temporary path; dropping it deletes
    // the temporary file.
    result.map_err(|err| {
        if err.error.kind() == ErrorKind::AlreadyExists || (!overwrite && target.exists()) {
            PortabilityError::TargetExists(target.display().to_string())
        } else {
            PortabilityError::io(target.display().to_string(), err.error)
        }
    })
}

/// Writes `bytes` to `target` through a synced temporary file and
/// [`persist`].
pub fn write_file(target: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    use std::io::Write as _;
    check_target(target, overwrite)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| PortabilityError::io(parent.display().to_string(), e))?;
    }
    let temp = temp_path(target);
    let written = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(PortabilityError::io(temp.display().to_string(), err));
    }
    persist(&temp, target, overwrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_are_written_and_existing_ones_only_replaced_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub").join("kit.txt");
        write_file(&target, b"one", false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"one");

        // Without overwrite: refused, the file is unchanged, no temp left.
        let err = write_file(&target, b"two", false).unwrap_err();
        assert!(matches!(err, PortabilityError::TargetExists(_)), "{err:?}");
        assert_eq!(std::fs::read(&target).unwrap(), b"one");
        assert!(!temp_path(&target).exists());

        // Confirmed: replaced.
        write_file(&target, b"two", true).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two");
        assert!(!temp_path(&target).exists());
        let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["kit.txt"]);
    }

    #[test]
    fn a_target_that_appears_meanwhile_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.zip");
        let temp = temp_path(&target);
        std::fs::write(&temp, b"new").unwrap();
        std::fs::write(&target, b"theirs").unwrap();
        let err = persist(&temp, &target, false).unwrap_err();
        assert!(matches!(err, PortabilityError::TargetExists(_)), "{err:?}");
        assert_eq!(std::fs::read(&target).unwrap(), b"theirs");
        assert!(!temp.exists(), "the temporary file is removed");

        std::fs::write(&temp, b"new").unwrap();
        persist(&temp, &target, true).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn folders_are_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let err = write_file(dir.path(), b"x", true).unwrap_err();
        assert!(matches!(err, PortabilityError::TargetExists(_)), "{err:?}");
    }
}
