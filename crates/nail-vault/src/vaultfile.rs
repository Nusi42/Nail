//! The vault file (`vault.json`) next to the database (M4c WP122, ADR
//! 0095).
//!
//! Once the database file itself is encrypted, the `vault_state` row
//! cannot be read before the key is known, so what opening (and recovering)
//! needs lives in a small plain file beside it: the keychain reference of
//! the data key, the key check value and the recovery-key-wrapped data key.
//! None of it is secret: the key check value is a sealed known plaintext,
//! the wrapped key is sealed with a key derived from the 256-bit recovery
//! key (which is never stored). The row stays the source of truth while
//! the database is readable; the vault keeps both in step (keychain first,
//! then this file, then the row; the reverse when forgetting).
//!
//! Written atomically (temporary file, sync, rename). In-memory stores keep
//! it in memory only.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// File name of the vault file inside the data directory.
pub const VAULT_FILE_NAME: &str = "vault.json";

/// Format version written into the file.
pub const VAULT_FILE_VERSION: u32 = 1;

/// The recovery key's part of the vault file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRecord {
    /// The data key sealed with the recovery wrapping key (hex).
    pub wrapped_key: String,
    /// When the recovery key was created (RFC 3339).
    pub created_at: String,
    /// When the user last proved to have it (RFC 3339).
    #[serde(default)]
    pub verified_at: Option<String>,
}

/// The contents of `vault.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultFile {
    /// [`VAULT_FILE_VERSION`].
    pub version: u32,
    /// Keychain reference of the data key.
    #[serde(default)]
    pub key_ref: Option<String>,
    /// The key check value (hex).
    #[serde(default)]
    pub key_check: Option<String>,
    /// Whether the database file was last written encrypted (informational:
    /// the file header is authoritative).
    #[serde(default)]
    pub database_encrypted: bool,
    /// The recovery key, when configured.
    #[serde(default)]
    pub recovery: Option<RecoveryRecord>,
}

impl Default for VaultFile {
    fn default() -> Self {
        Self {
            version: VAULT_FILE_VERSION,
            key_ref: None,
            key_check: None,
            database_encrypted: false,
            recovery: None,
        }
    }
}

impl VaultFile {
    /// The key check value as bytes.
    pub fn key_check_bytes(&self) -> Result<Option<Vec<u8>>, VaultError> {
        self.key_check
            .as_deref()
            .map(|h| {
                hex::decode(h)
                    .map_err(|_| VaultError::VaultFile("the key check value is malformed".into()))
            })
            .transpose()
    }

    /// Reads the file at `path`; a missing file is the default (nothing
    /// recorded).
    pub fn load(path: &Path) -> Result<Self, VaultError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err.into()),
        };
        let file: Self = serde_json::from_str(&text)
            .map_err(|err| VaultError::VaultFile(format!("{}: {err}", path.display())))?;
        if file.version > VAULT_FILE_VERSION {
            return Err(VaultError::VaultFile(format!(
                "{} was written by a newer Nail (version {})",
                path.display(),
                file.version
            )));
        }
        Ok(file)
    }

    /// Writes the file atomically.
    pub fn save(&self, path: &Path) -> Result<(), VaultError> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir)?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| VaultError::VaultFile(err.to_string()))?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".vault")
            .suffix(".tmp")
            .tempfile_in(dir)?;
        std::io::Write::write_all(&mut tmp, text.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|err| err.error)?;
        Ok(())
    }
}

/// The vault file of one data directory, cached in memory.
#[derive(Debug)]
pub(crate) struct VaultFileSlot {
    path: Option<PathBuf>,
    cached: Mutex<Option<VaultFile>>,
}

impl VaultFileSlot {
    /// A slot for the file at `path` (`None`: memory only).
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            cached: Mutex::new(None),
        }
    }

    /// The vault file beside a database file.
    pub(crate) fn beside(db: Option<&Path>) -> Self {
        Self::new(db.map(|db| db.with_file_name(VAULT_FILE_NAME)))
    }

    /// The current contents.
    pub(crate) fn get(&self) -> Result<VaultFile, VaultError> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(file) = cached.as_ref() {
            return Ok(file.clone());
        }
        let file = match &self.path {
            Some(path) => VaultFile::load(path)?,
            None => VaultFile::default(),
        };
        *cached = Some(file.clone());
        Ok(file)
    }

    /// Changes the contents (written through to disk).
    pub(crate) fn update(&self, f: impl FnOnce(&mut VaultFile)) -> Result<VaultFile, VaultError> {
        let mut file = self.get()?;
        let before = file.clone();
        f(&mut file);
        if file != before {
            if let Some(path) = &self.path {
                file.save(path)?;
            }
            *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = Some(file.clone());
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(VAULT_FILE_NAME);
        assert_eq!(VaultFile::load(&path).unwrap(), VaultFile::default());
        let file = VaultFile {
            key_ref: Some("keychain:nail/vault-1/data-key".into()),
            key_check: Some("00ff".into()),
            database_encrypted: true,
            recovery: Some(RecoveryRecord {
                wrapped_key: "abcd".into(),
                created_at: "2026-10-02T10:00:00Z".into(),
                verified_at: None,
            }),
            ..VaultFile::default()
        };
        file.save(&path).unwrap();
        assert_eq!(VaultFile::load(&path).unwrap(), file);
        assert_eq!(file.key_check_bytes().unwrap(), Some(vec![0, 255]));
        // No temporary files are left behind.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [VAULT_FILE_NAME]);
    }

    #[test]
    fn damaged_and_newer_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(VAULT_FILE_NAME);
        std::fs::write(&path, "{not json").unwrap();
        assert!(matches!(
            VaultFile::load(&path),
            Err(VaultError::VaultFile(_))
        ));
        std::fs::write(&path, r#"{"version": 99}"#).unwrap();
        assert!(matches!(
            VaultFile::load(&path),
            Err(VaultError::VaultFile(_))
        ));
        let bad = VaultFile {
            key_check: Some("zz".into()),
            ..VaultFile::default()
        };
        assert!(bad.key_check_bytes().is_err());
    }

    #[test]
    fn slot_caches_and_writes_through() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("nail.sqlite3");
        let slot = VaultFileSlot::beside(Some(&db));
        assert_eq!(slot.get().unwrap(), VaultFile::default());
        slot.update(|f| f.key_ref = Some("r".into())).unwrap();
        let on_disk = VaultFile::load(&dir.path().join(VAULT_FILE_NAME)).unwrap();
        assert_eq!(on_disk.key_ref.as_deref(), Some("r"));
        // Memory-only slots work without a file.
        let memory = VaultFileSlot::new(None);
        memory.update(|f| f.database_encrypted = true).unwrap();
        assert!(memory.get().unwrap().database_encrypted);
    }
}
