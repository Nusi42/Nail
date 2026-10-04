//! The single-instance lock of a data directory (M2c WP53).
//!
//! One process at a time owns a data directory: its store, blob store,
//! search index and account engines. The owner holds an exclusive OS file
//! lock on `<data-dir>/nail.lock` for its lifetime ([`DataDirLock`]); the
//! OS releases it when the process ends, even after a crash, so a stale
//! lock cannot block the next start. Who holds it is written next to it
//! (`nail.lock.info`, JSON: process id, what runs, since when) for the
//! message a second instance shows ([`DataDirLock::holder`]); the lock
//! file itself stays empty because Windows locks are mandatory and nobody
//! could read it.
//!
//! `nail serve` and the headless `nail mcp` take it; the desktop app takes
//! it once the integration wires it in (WP55).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The lock file in the data directory.
pub const LOCK_FILE: &str = "nail.lock";
/// Who holds the lock, written by the holder.
pub const LOCK_INFO_FILE: &str = "nail.lock.info";

/// The holder of a data directory, as it described itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockHolder {
    /// Process id.
    pub pid: u32,
    /// What runs: `"serve"`, `"mcp"`, `"desktop"`.
    pub owner: String,
    /// When it took the lock (Unix seconds).
    pub since: i64,
}

/// Why a data directory could not be locked.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds it (`None` when it did not say who it is).
    #[error("the data directory is in use by another Nail process{}", holder_suffix(.0.as_ref()))]
    Held(Option<LockHolder>),
    /// The lock file could not be created or locked.
    #[error("cannot lock the data directory: {0}")]
    Io(#[from] std::io::Error),
}

fn holder_suffix(holder: Option<&LockHolder>) -> String {
    holder.map_or_else(String::new, |h| format!(" ({}, pid {})", h.owner, h.pid))
}

/// An exclusive lock on a data directory, released when dropped (or when
/// the process ends).
#[derive(Debug)]
pub struct DataDirLock {
    file: File,
    dir: PathBuf,
    holder: LockHolder,
}

impl DataDirLock {
    /// Takes the lock of `dir` (created when missing) for `owner`, without
    /// waiting. [`LockError::Held`] when another instance has it — also
    /// another lock in the same process.
    pub fn acquire(dir: &Path, owner: &str) -> Result<Self, LockError> {
        std::fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LockError::Held(Self::holder(dir))),
            Err(TryLockError::Error(err)) => return Err(LockError::Io(err)),
        }
        let holder = LockHolder {
            pid: std::process::id(),
            owner: owner.to_owned(),
            since: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
        };
        // Best effort: the lock works without the description.
        if let Err(err) = write_info(dir, &holder) {
            tracing::debug!(%err, "cannot describe the data directory lock");
        }
        Ok(Self {
            file,
            dir: dir.to_path_buf(),
            holder,
        })
    }

    /// Who holds the lock of `dir` as it described itself (`None` when
    /// nobody wrote a description; the description may be stale when the
    /// holder crashed — [`Self::acquire`] is the only reliable test).
    pub fn holder(dir: &Path) -> Option<LockHolder> {
        let bytes = std::fs::read(dir.join(LOCK_INFO_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// This lock's description.
    pub fn info(&self) -> &LockHolder {
        &self.holder
    }

    /// The locked directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn write_info(dir: &Path, holder: &LockHolder) -> std::io::Result<()> {
    let temp = dir.join(format!("{LOCK_INFO_FILE}.tmp"));
    std::fs::write(&temp, serde_json::to_vec(holder)?)?;
    std::fs::rename(&temp, dir.join(LOCK_INFO_FILE))
}

impl Drop for DataDirLock {
    fn drop(&mut self) {
        // Remove the description only while it is still ours.
        if Self::holder(&self.dir).is_some_and(|h| h == self.holder)
            && let Err(err) = std::fs::remove_file(self.dir.join(LOCK_INFO_FILE))
        {
            tracing::debug!(%err, "lock description not removed");
        }
        if let Err(err) = self.file.unlock() {
            tracing::debug!(%err, "data directory lock not released (the OS releases it on exit)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let first = DataDirLock::acquire(&data, "serve").unwrap();
        assert_eq!(first.info().pid, std::process::id());
        assert_eq!(first.dir(), data);
        let holder = DataDirLock::holder(&data).unwrap();
        assert_eq!(holder.owner, "serve");

        match DataDirLock::acquire(&data, "mcp") {
            Err(LockError::Held(Some(h))) => assert_eq!(h, holder),
            other => panic!("expected a held lock, got {other:?}"),
        }
        let err = DataDirLock::acquire(&data, "mcp").unwrap_err();
        assert!(err.to_string().contains("serve, pid"), "{err}");

        drop(first);
        assert_eq!(DataDirLock::holder(&data), None, "the description goes");
        let again = DataDirLock::acquire(&data, "mcp").unwrap();
        assert_eq!(DataDirLock::holder(&data).unwrap().owner, "mcp");
        drop(again);
    }

    /// A description left by a crashed holder does not block anyone.
    #[test]
    fn a_stale_description_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        write_info(
            dir.path(),
            &LockHolder {
                pid: 1,
                owner: "desktop".into(),
                since: 0,
            },
        )
        .unwrap();
        let lock = DataDirLock::acquire(dir.path(), "serve").unwrap();
        assert_eq!(DataDirLock::holder(dir.path()).unwrap(), *lock.info());
    }
}
