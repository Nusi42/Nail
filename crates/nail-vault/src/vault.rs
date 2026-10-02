//! [`Vault`]: the encryption-at-rest switch and its crash-safe migration.
//!
//! # States
//!
//! `vault_state` (store v5) holds one row: `plain → encrypting → encrypted
//! → decrypting → plain`. While migrating, the row also holds the phase
//! (`database → blobs → index`) and the progress. The data key lives in the
//! OS keychain under a random reference kept in the row, next to a key
//! check value.
//!
//! # Crash safety
//!
//! Every unit of work is atomic and idempotent, and every unit tells by
//! itself whether it is done, so a crash (or [`ControlFlow::Break`] from
//! the progress callback, or an error) at any point loses nothing and the
//! next [`Vault::migrate`] continues where the row says:
//!
//! * database: a batch of column values converts in one SQLite
//!   transaction; a sealed value carries a prefix;
//! * blobs and index files: a file is rewritten to a temporary file,
//!   synced and renamed over the old one; a sealed file starts with a magic;
//! * the key is written to the keychain **before** the row names it, and
//!   deleted **after** the row forgets it (an orphaned key is harmless, a
//!   missing one would lose data);
//! * the state switches to `encrypted` / `plain` only after the last phase.
//!
//! While `encrypting`, new writes are already sealed; while `decrypting`,
//! they are plain; reads accept both. So the app keeps working during a
//! migration, except that the search index must be closed during the index
//! phase ([`Vault::migrate_until`] runs the first two phases with it open).

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nail_api::engine::SecretStore;
use nail_api::security::{VaultPhase, VaultState, VaultStatus};
use nail_api::{ErrorCode, SecretRef};
use nail_store::{BlobStore, DataCipher, SealMode, Store, VaultRecord};
use tantivy::directory::Directory;

use crate::directory::SealedDirectory;
use crate::error::VaultError;
use crate::format::{self, DataKey};
use crate::key::{KeyStore, VaultCipher, key_check, new_key_ref, verify_key_check};

/// Wraps the search index's directory (`nail_search::DirectoryWrapper`
/// has the same shape).
pub type IndexWrapper = Arc<dyn Fn(Box<dyn Directory>) -> Box<dyn Directory> + Send + Sync>;

/// Name of `nail-search`'s schema version file, which stays plain
/// (it holds a number).
pub const INDEX_VERSION_FILE: &str = "nail-search-version";

/// Column values converted per database transaction.
const DB_BATCH: usize = 200;
/// Files converted between two progress saves.
const SAVE_EVERY: u64 = 16;
/// Temporary blob files younger than this may belong to a running write.
const TEMP_FILE_MIN_AGE: Duration = Duration::from_secs(10 * 60);

const PHASES: [VaultPhase; 3] = [VaultPhase::Database, VaultPhase::Blobs, VaultPhase::Index];

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn encode_error(err: &VaultError) -> String {
    format!("{}|{err}", err.code().as_str())
}

fn decode_error(text: &str) -> (Option<ErrorCode>, String) {
    match text.split_once('|') {
        Some((code, message)) => (
            ErrorCode::ALL.into_iter().find(|c| c.as_str() == code),
            message.to_owned(),
        ),
        None => (Some(ErrorCode::Internal), text.to_owned()),
    }
}

/// Encryption at rest of one data directory.
#[derive(Debug)]
pub struct Vault {
    store: Store,
    blobs: BlobStore,
    index_dir: Option<PathBuf>,
    keys: KeyStore,
    key: Mutex<Option<DataKey>>,
}

impl Vault {
    /// A vault over the data directory's store, blob store and (optional)
    /// search index directory. Nothing is read yet; call [`Vault::open`].
    pub fn new(
        store: Store,
        blobs: BlobStore,
        index_dir: Option<PathBuf>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            store,
            blobs,
            index_dir,
            keys: KeyStore::new(secrets),
            key: Mutex::new(None),
        }
    }

    fn cached_key(&self) -> Option<DataKey> {
        self.key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_cached_key(&self, key: Option<DataKey>) {
        *self.key.lock().unwrap_or_else(PoisonError::into_inner) = key;
    }

    fn status_of(&self, record: &VaultRecord) -> VaultStatus {
        let (error, message) = record
            .error
            .as_deref()
            .map(decode_error)
            .map_or((None, None), |(code, message)| (code, Some(message)));
        VaultStatus {
            state: record.state,
            phase: record.phase,
            done: record.done,
            total: record.total,
            key_in_keychain: record
                .key_ref
                .as_ref()
                .is_some_and(|r| self.keys.contains(&SecretRef(r.clone()))),
            // M4c (ADR 0094): filled in by WP122 (recovery key).
            needs_recovery: false,
            recovery: nail_api::recovery::RecoveryKeyStatus::default(),
            error,
            message,
            localized: None,
        }
    }

    /// The current state and progress.
    pub fn status(&self) -> Result<VaultStatus, VaultError> {
        Ok(self.status_of(&self.store.vault_state()?))
    }

    /// Loads (once) and verifies the data key of an encrypted or migrating
    /// directory.
    fn require_key(&self, record: &VaultRecord) -> Result<DataKey, VaultError> {
        if let Some(key) = self.cached_key() {
            return Ok(key);
        }
        let key_ref = record
            .key_ref
            .as_deref()
            .ok_or_else(|| VaultError::Locked("the vault state names no data key".into()))?;
        let key = self.keys.load(&SecretRef(key_ref.to_owned()))?;
        let check = record
            .key_check
            .as_deref()
            .ok_or_else(|| VaultError::Crypto("the vault state has no key check value".into()))?;
        verify_key_check(&key, check)?;
        self.set_cached_key(Some(key.clone()));
        Ok(key)
    }

    fn install(&self, key: Option<&DataKey>, mode: SealMode) -> Result<(), VaultError> {
        let cipher: Option<Arc<dyn DataCipher>> =
            key.map(|k| Arc::new(VaultCipher::new(k.clone())) as Arc<dyn DataCipher>);
        self.store.set_cipher(cipher.clone(), mode)?;
        self.blobs.set_cipher(cipher, mode);
        Ok(())
    }

    fn mode_for(state: VaultState) -> SealMode {
        match state {
            VaultState::Encrypting | VaultState::Encrypted => SealMode::Seal,
            VaultState::Plain | VaultState::Decrypting => SealMode::Plain,
        }
    }

    /// Reads the state at start: for anything but `plain`, loads the key
    /// from the keychain, verifies it and installs the cipher in the store
    /// and blob store (sealing new writes while `encrypting` /
    /// `encrypted`). Fails with [`VaultError::Locked`] when the key is
    /// missing and [`VaultError::Crypto`] when it does not match.
    pub fn open(&self) -> Result<VaultStatus, VaultError> {
        let record = self.store.vault_state()?;
        if record.state == VaultState::Plain {
            self.set_cached_key(None);
            self.install(None, SealMode::Plain)?;
        } else {
            let key = self.require_key(&record)?;
            self.install(Some(&key), Self::mode_for(record.state))?;
        }
        Ok(self.status_of(&record))
    }

    /// Whether a migration is pending (after [`Vault::set_enabled`] or an
    /// interrupted run).
    pub fn needs_migration(&self) -> Result<bool, VaultError> {
        Ok(matches!(
            self.store.vault_state()?.state,
            VaultState::Encrypting | VaultState::Decrypting
        ))
    }

    /// Switches encryption at rest on or off. Records the target state
    /// (creating the data key on the way from `plain`) and installs the
    /// cipher; the data is converted by [`Vault::migrate`]. Switching
    /// during a migration reverses it. A no-op when already there.
    pub fn set_enabled(&self, enabled: bool) -> Result<VaultStatus, VaultError> {
        let mut record = self.store.vault_state()?;
        let target = match (record.state, enabled) {
            (VaultState::Plain, false) | (VaultState::Encrypted, true) => {
                return Ok(self.status_of(&record));
            }
            (VaultState::Encrypting, true) | (VaultState::Decrypting, false) => {
                return Ok(self.status_of(&record));
            }
            (VaultState::Plain, true) => {
                let key = DataKey::generate();
                let key_ref = new_key_ref();
                // Keychain first: the row must never name a key that is
                // not stored.
                self.keys.store(&key_ref, &key)?;
                record.key_ref = Some(key_ref.as_str().to_owned());
                record.key_check = Some(key_check(&key)?);
                self.set_cached_key(Some(key));
                VaultState::Encrypting
            }
            (VaultState::Encrypting | VaultState::Encrypted, false) => VaultState::Decrypting,
            (VaultState::Decrypting, true) => VaultState::Encrypting,
        };
        let key = self.require_key(&record)?;
        record.state = target;
        record.phase = Some(VaultPhase::Database);
        record.done = 0;
        record.total = 0;
        record.started_at = Some(now());
        record.finished_at = None;
        record.error = None;
        self.store.set_vault_state(&record)?;
        self.install(Some(&key), Self::mode_for(target))?;
        tracing::info!(state = target.as_str(), "encryption at rest switched");
        Ok(self.status_of(&record))
    }

    /// Runs (or resumes) a pending migration to the end. `progress` is
    /// called after every unit of work; [`ControlFlow::Break`] pauses (the
    /// next call resumes). The search index must not be open during the
    /// index phase. Returns the final status; on error the error is also
    /// recorded in the state row and the migration can be retried.
    pub fn migrate(
        &self,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> Result<VaultStatus, VaultError> {
        self.migrate_until(None, progress)
    }

    /// Like [`Vault::migrate`], but stops before `stop_before` (e.g. the
    /// index phase, which needs the search index closed). The state then
    /// stays `encrypting` / `decrypting` with that phase recorded.
    pub fn migrate_until(
        &self,
        stop_before: Option<VaultPhase>,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> Result<VaultStatus, VaultError> {
        let mut record = self.store.vault_state()?;
        let seal = match record.state {
            VaultState::Encrypting => true,
            VaultState::Decrypting => false,
            VaultState::Plain | VaultState::Encrypted => return Ok(self.status_of(&record)),
        };
        let key = match self.require_key(&record) {
            Ok(key) => key,
            Err(err) => return Err(self.record_error(&mut record, err)),
        };
        self.install(Some(&key), Self::mode_for(record.state))?;
        let start = record
            .phase
            .and_then(|p| PHASES.iter().position(|q| *q == p))
            .unwrap_or(0);
        for phase in &PHASES[start..] {
            if stop_before == Some(*phase) {
                record.phase = Some(*phase);
                self.store.set_vault_state(&record)?;
                return Ok(self.status_of(&record));
            }
            if record.phase != Some(*phase) {
                record.phase = Some(*phase);
                record.done = 0;
                record.total = 0;
            }
            record.error = None;
            self.store.set_vault_state(&record)?;
            let outcome = match phase {
                VaultPhase::Database => self.database_phase(seal, &mut record, progress),
                VaultPhase::Blobs => self.blob_phase(seal, &mut record, progress),
                VaultPhase::Index => self.index_phase(&key, seal, &mut record, progress),
            };
            match outcome {
                Ok(ControlFlow::Continue(())) => {}
                Ok(ControlFlow::Break(())) => {
                    self.store.set_vault_state(&record)?;
                    tracing::info!(phase = phase.as_str(), "vault migration paused");
                    return Ok(self.status_of(&record));
                }
                Err(err) => return Err(self.record_error(&mut record, err)),
            }
        }
        self.finish(record, seal)
    }

    fn record_error(&self, record: &mut VaultRecord, err: VaultError) -> VaultError {
        tracing::warn!(%err, "vault migration stopped");
        record.error = Some(encode_error(&err));
        if let Err(save) = self.store.set_vault_state(record) {
            tracing::warn!(%save, "could not record the vault error");
        }
        err
    }

    fn finish(&self, mut record: VaultRecord, seal: bool) -> Result<VaultStatus, VaultError> {
        record.phase = None;
        record.done = 0;
        record.total = 0;
        record.error = None;
        record.finished_at = Some(now());
        if seal {
            record.state = VaultState::Encrypted;
            self.store.set_vault_state(&record)?;
            tracing::info!("encryption at rest: encrypted");
            return Ok(self.status_of(&record));
        }
        record.state = VaultState::Plain;
        let key_ref = record.key_ref.take();
        record.key_check = None;
        // The row forgets the key before the keychain does.
        self.store.set_vault_state(&record)?;
        self.install(None, SealMode::Plain)?;
        self.set_cached_key(None);
        if let Some(key_ref) = key_ref
            && let Err(err) = self.keys.delete(&SecretRef(key_ref))
        {
            tracing::warn!(%err, "could not delete the old data key from the keychain");
        }
        tracing::info!("encryption at rest: plain");
        Ok(self.status_of(&record))
    }

    fn report(
        &self,
        record: &VaultRecord,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        progress(&self.status_of(record))
    }

    fn database_phase(
        &self,
        seal: bool,
        record: &mut VaultRecord,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> Result<ControlFlow<()>, VaultError> {
        let counts = self.store.sealed_field_counts()?;
        record.total = counts.total();
        record.done = if seal { counts.sealed } else { counts.plain };
        self.store.set_vault_state(record)?;
        if self.report(record, progress).is_break() {
            return Ok(ControlFlow::Break(()));
        }
        loop {
            let converted = self.store.convert_fields(seal, DB_BATCH)? as u64;
            if converted == 0 {
                break;
            }
            record.done += converted;
            // Rows written meanwhile (plain while decrypting) may add work.
            record.total = record.total.max(record.done);
            self.store.set_vault_state(record)?;
            if self.report(record, progress).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        // No old page may keep a value from before the conversion.
        self.store.compact()?;
        Ok(ControlFlow::Continue(()))
    }

    fn blob_phase(
        &self,
        seal: bool,
        record: &mut VaultRecord,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> Result<ControlFlow<()>, VaultError> {
        self.blobs.remove_temp_files(TEMP_FILE_MIN_AGE)?;
        let hashes = self.blobs.hashes()?;
        record.total = hashes.len() as u64;
        record.done = 0;
        self.store.set_vault_state(record)?;
        for hash in hashes {
            match self.blobs.convert(&hash, seal) {
                Ok(_) => {}
                // Deleted meanwhile (garbage collection).
                Err(err) if err.is_not_found() => {}
                Err(err) => return Err(err.into()),
            }
            record.done += 1;
            if record.done.is_multiple_of(SAVE_EVERY) {
                self.store.set_vault_state(record)?;
            }
            if self.report(record, progress).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        self.store.set_vault_state(record)?;
        Ok(ControlFlow::Continue(()))
    }

    fn index_phase(
        &self,
        key: &DataKey,
        seal: bool,
        record: &mut VaultRecord,
        progress: &mut dyn FnMut(&VaultStatus) -> ControlFlow<()>,
    ) -> Result<ControlFlow<()>, VaultError> {
        let Some(dir) = self.index_dir.as_deref().filter(|d| d.is_dir()) else {
            return Ok(ControlFlow::Continue(()));
        };
        let files = index_files(dir)?;
        record.total = files.len() as u64;
        record.done = 0;
        self.store.set_vault_state(record)?;
        for name in files {
            convert_index_file(key, dir, &name, seal)?;
            record.done += 1;
            if record.done.is_multiple_of(SAVE_EVERY) {
                self.store.set_vault_state(record)?;
            }
            if self.report(record, progress).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        self.store.set_vault_state(record)?;
        Ok(ControlFlow::Continue(()))
    }

    /// How the search index's directory must be wrapped now: `None` for a
    /// plain data directory; sealed (strict) when encrypted; sealed writes
    /// with plain reads allowed while encrypting; plain writes with sealed
    /// reads while decrypting. Needs the key ([`Vault::open`]).
    pub fn index_wrapper(&self) -> Result<Option<IndexWrapper>, VaultError> {
        let record = self.store.vault_state()?;
        if record.state == VaultState::Plain {
            return Ok(None);
        }
        let key = self.require_key(&record)?;
        let (seal, accept_plain) = match record.state {
            VaultState::Encrypted => (true, false),
            VaultState::Encrypting => (true, true),
            VaultState::Decrypting | VaultState::Plain => (false, true),
        };
        Ok(Some(Arc::new(move |inner: Box<dyn Directory>| {
            Box::new(
                SealedDirectory::new(inner, key.clone())
                    .seal_writes(seal)
                    .accept_plain(accept_plain),
            ) as Box<dyn Directory>
        })))
    }
}

/// The index files the migration converts: everything but lock files, the
/// version file and temporary files, sorted.
fn index_files(dir: &Path) -> Result<Vec<String>, VaultError> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".lock") || name == INDEX_VERSION_FILE || name.starts_with(".tmp") {
            continue;
        }
        out.push(name);
    }
    out.sort();
    Ok(out)
}

/// Rewrites one index file sealed or plain (temporary file + rename).
fn convert_index_file(
    key: &DataKey,
    dir: &Path,
    name: &str,
    seal: bool,
) -> Result<bool, VaultError> {
    let path = dir.join(name);
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    if format::is_sealed(&data) == seal {
        return Ok(false);
    }
    let context = format!("nail/index/{name}").into_bytes();
    let new = if seal {
        format::seal(key, &context, &data)?
    } else {
        format::open(key, &context, &data)?
    };
    let mut tmp = tempfile::Builder::new().prefix(".tmp").tempfile_in(dir)?;
    std::io::Write::write_all(&mut tmp, &new)?;
    tmp.as_file().sync_all()?;
    tmp.persist(&path).map_err(|err| err.error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_round_trip_through_the_state_row() {
        let text = encode_error(&VaultError::Locked("no key".into()));
        assert_eq!(
            decode_error(&text),
            (Some(ErrorCode::Locked), "locked: no key".into())
        );
        assert_eq!(
            decode_error("legacy text"),
            (Some(ErrorCode::Internal), "legacy text".into())
        );
    }

    #[test]
    fn index_file_selection_skips_locks_version_and_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "meta.json",
            ".managed.json",
            "abc.idx",
            ".tantivy-writer.lock",
            INDEX_VERSION_FILE,
            ".tmpXYZ",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert_eq!(
            index_files(dir.path()).unwrap(),
            [".managed.json", "abc.idx", "meta.json"]
        );
    }
}
