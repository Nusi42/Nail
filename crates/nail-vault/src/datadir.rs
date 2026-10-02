//! Opening a data directory with encryption at rest (M4c WP122, ADR
//! 0095): what the core calls at start instead of
//! `nail_store::open_data_dir` + [`Vault::new`].
//!
//! The vault file says which keychain entry holds the data key; the
//! database file's header says whether the database itself is encrypted.
//! An encrypted database needs the key before anything can be read, so
//! when the key is missing (or does not match) the directory comes back
//! [`OpenedDataDir::Locked`]: no store, only what recovery needs
//! ([`LockedVault::restore_key`]); afterwards the caller opens the
//! directory again. A plain database always opens; a missing data key
//! then only locks the sealed columns, blobs and index as in M2a
//! ([`ReadyDataDir::open_error`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nail_api::SecretRef;
use nail_api::engine::SecretStore;
use nail_api::security::{VaultState, VaultStatus};
use nail_store::{BLOB_DIR_NAME, BlobStore, DB_FILE_NAME, FileEncryption, Store, file_encryption};

use crate::error::VaultError;
use crate::format::DataKey;
use crate::key::{KeyStore, database_key, verify_key_check};
use crate::recovery;
use crate::vault::Vault;
use crate::vaultfile::{VAULT_FILE_NAME, VaultFile, VaultFileSlot};

/// A data directory whose store is open.
#[derive(Debug)]
pub struct ReadyDataDir {
    /// The metadata database (opened with its key when encrypted).
    pub store: Store,
    /// The blob store.
    pub blobs: BlobStore,
    /// The vault, already [`Vault::open`]ed (cipher installed) unless
    /// `open_error` says why not; full-file encryption is on.
    pub vault: Vault,
    /// Why the vault could not be opened (missing / wrong data key of a
    /// plain database with sealed columns); reads of sealed data then
    /// answer `locked` / `crypto`.
    pub open_error: Option<VaultError>,
}

/// The result of [`open_data_dir`].
#[derive(Debug)]
pub enum OpenedDataDir {
    /// The store is open.
    Ready(ReadyDataDir),
    /// The database file is encrypted and its key is not available.
    Locked(LockedVault),
}

/// Opens the data directory `dir` (`nail.sqlite3`, `blobs/`, `vault.json`):
/// loads the data key from the keychain when the database file is
/// encrypted, opens the store with it and opens the vault. `index_dir` is
/// the search index directory the vault converts (`None` for an in-memory
/// index).
pub fn open_data_dir(
    dir: &Path,
    index_dir: Option<PathBuf>,
    secrets: Arc<dyn SecretStore>,
) -> Result<OpenedDataDir, VaultError> {
    std::fs::create_dir_all(dir)?;
    let db = dir.join(DB_FILE_NAME);
    let keys = KeyStore::new(Arc::clone(&secrets));
    let mut preloaded = None;
    let store = if file_encryption(&db)? == FileEncryption::Encrypted {
        let file = VaultFile::load(&dir.join(VAULT_FILE_NAME))?;
        match load_data_key(&file, &keys) {
            Ok(key) => match Store::open_with_key(&db, Some(&database_key(&key))) {
                Ok(store) => {
                    preloaded = Some(key);
                    store
                }
                Err(err @ (nail_store::Error::Crypto(_) | nail_store::Error::Locked(_))) => {
                    return Ok(OpenedDataDir::Locked(LockedVault::new(
                        dir,
                        keys,
                        err.into(),
                    )));
                }
                Err(err) => return Err(err.into()),
            },
            Err(err) => return Ok(OpenedDataDir::Locked(LockedVault::new(dir, keys, err))),
        }
    } else {
        Store::open(&db)?
    };
    let blobs = BlobStore::open(dir.join(BLOB_DIR_NAME))?;
    let vault = Vault::new(store.clone(), blobs.clone(), index_dir, secrets)
        .with_full_file_encryption(true);
    if let Some(key) = preloaded {
        vault.preload_key(key);
    }
    let open_error = vault.open().err();
    if let Some(err) = &open_error {
        tracing::error!(%err, "the data key of the encrypted data is not available");
    }
    Ok(OpenedDataDir::Ready(ReadyDataDir {
        store,
        blobs,
        vault,
        open_error,
    }))
}

/// The data key named by the vault file, checked against its key check
/// value.
fn load_data_key(file: &VaultFile, keys: &KeyStore) -> Result<DataKey, VaultError> {
    let key_ref = file.key_ref.as_deref().ok_or_else(|| {
        VaultError::Locked("the database is encrypted but the vault file names no data key".into())
    })?;
    let key = keys.load(&SecretRef(key_ref.to_owned()))?;
    if let Some(check) = file.key_check_bytes()? {
        verify_key_check(&key, &check)?;
    }
    Ok(key)
}

/// A data directory whose encrypted database cannot be opened: what is
/// left is checking and restoring the data key with the recovery key.
#[derive(Debug)]
pub struct LockedVault {
    file: VaultFileSlot,
    keys: KeyStore,
    error: VaultError,
}

impl LockedVault {
    fn new(dir: &Path, keys: KeyStore, error: VaultError) -> Self {
        tracing::error!(%error, "the encrypted database cannot be opened");
        Self {
            file: VaultFileSlot::new(Some(dir.join(VAULT_FILE_NAME))),
            keys,
            error,
        }
    }

    /// Why the database could not be opened.
    pub fn error(&self) -> &VaultError {
        &self.error
    }

    fn key_in_keychain(&self, file: &VaultFile) -> bool {
        file.key_ref
            .as_ref()
            .is_some_and(|r| self.keys.contains(&SecretRef(r.clone())))
    }

    /// The status to show: encrypted, recovery needed while the key is not
    /// in the keychain, the recovery key's state and the open error.
    pub fn status(&self) -> VaultStatus {
        let file = self.file.get().unwrap_or_default();
        let key_in_keychain = self.key_in_keychain(&file);
        VaultStatus {
            state: VaultState::Encrypted,
            phase: None,
            done: 0,
            total: 0,
            key_in_keychain,
            needs_recovery: !key_in_keychain,
            recovery: recovery::status_of(&file),
            error: Some(self.error.code()),
            message: Some(self.error.to_string()),
            localized: self.error.localized(),
        }
    }

    /// Checks a typed recovery key (see [`Vault::verify_recovery_key`]).
    pub fn verify_recovery_key(&self, text: &str) -> Result<bool, VaultError> {
        let file = self.file.get()?;
        match recovery::open_data_key(&file, text) {
            Ok(_) => Ok(true),
            Err(VaultError::RecoveryKeyWrong) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Puts the data key back into the keychain with the recovery key.
    /// The caller then opens the directory again with [`open_data_dir`].
    pub fn restore_key(&self, text: &str) -> Result<VaultStatus, VaultError> {
        let file = self.file.get()?;
        recovery::restore_into_keychain(&file, &self.keys, text)?;
        self.file.update(|file| {
            if let Some(entry) = file.recovery.as_mut() {
                entry.verified_at = Some(recovery::now_rfc3339());
            }
        })?;
        let mut status = self.status();
        status.error = None;
        status.message = None;
        status.localized = None;
        Ok(status)
    }
}
