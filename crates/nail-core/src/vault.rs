//! Encryption at rest in the core (M2a WP35; the vault itself is
//! `nail-vault`, ADR 0032).
//!
//! * At start ([`VaultCtl::open`]) the vault loads the data key from the
//!   keychain and installs the cipher in the store and blob store before
//!   anything reads messages or drafts. When the key is missing or wrong,
//!   the vault stays locked: message and draft reads answer `locked` /
//!   `crypto` ([`VaultCtl::check`]) instead of serving empty data, and the
//!   search index lives in memory so the sealed files on disk stay as they
//!   are.
//! * `SetVaultEncryption` records the target state and runs the migration
//!   in the background ([`Service::run_vault_migration`]): database and
//!   blobs first while the search index stays open, then the index is
//!   closed (a stand-in answers meanwhile, searches report
//!   `unavailable`), converted and opened again with the new directory
//!   wrapper. Progress goes out as [`Event::VaultProgress`]. An interrupted
//!   migration resumes when the service starts ([`resume_migration`]).

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nail_api::engine::SecretStore;
use nail_api::security::{VaultPhase, VaultStatus};
use nail_api::{ApiError, Event, LocalizedMessage};
use nail_search::{CommitPolicy, SearchIndex};
use nail_store::{BlobStore, MessageFilter, Store};
use nail_vault::{LockedVault, Vault, VaultError};

use crate::error::Result;
use crate::service::{Service, lock};

/// Shortest time between two progress events of a running migration.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

/// How long the index phase waits for readers of the old index to finish.
const INDEX_RELEASE_WAIT: Duration = Duration::from_secs(30);

/// The service's handle on encryption at rest.
pub(crate) struct VaultCtl {
    vault: Arc<Vault>,
    index_dir: Option<PathBuf>,
    /// Why opening failed (missing / wrong data key); reads answer it.
    open_error: Mutex<Option<ApiError>>,
    /// Held while a migration runs.
    running: tokio::sync::Mutex<()>,
    /// M4c: the data directory's encrypted database could not be opened
    /// (its key is gone from the keychain). The service then runs on an
    /// empty in-memory store; only the recovery key works
    /// ([`LockedVault::restore_key`]), and a restart opens the data.
    locked: Option<LockedVault>,
    /// The key was put back while locked: restarting Nail opens the data.
    restart_needed: AtomicBool,
}

impl VaultCtl {
    /// Opens the vault of a data directory (`index_dir` = its search index
    /// directory, `None` for an in-memory index). Column sealing only (the
    /// M2a vault): tests and embedders building a service from parts.
    pub(crate) fn open(
        store: Store,
        blobs: BlobStore,
        index_dir: Option<PathBuf>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        let vault = Vault::new(store, blobs, index_dir.clone(), secrets);
        let open_error = vault.open().err();
        Self::from_vault(vault, open_error, index_dir)
    }

    /// A vault [`nail_vault::open_data_dir`] opened (full-file encryption
    /// on; `open_error` = why the data key is not available).
    pub(crate) fn from_vault(
        vault: Vault,
        open_error: Option<VaultError>,
        index_dir: Option<PathBuf>,
    ) -> Self {
        let open_error = match open_error {
            None => {
                if let Ok(status) = vault.status()
                    && status.state != nail_api::security::VaultState::Plain
                {
                    tracing::info!(state = status.state.as_str(), "encryption at rest is on");
                }
                None
            }
            Some(err) => {
                tracing::error!(%err, "the data key of the encrypted data is not available");
                Some(ApiError::from(err))
            }
        };
        Self {
            vault: Arc::new(vault),
            index_dir,
            open_error: Mutex::new(open_error),
            running: tokio::sync::Mutex::new(()),
            locked: None,
            restart_needed: AtomicBool::new(false),
        }
    }

    /// A locked data directory: `stand_in` is a vault over the in-memory
    /// stand-in store (so the rest of the service works on empty data).
    pub(crate) fn locked(locked: LockedVault, stand_in: Vault) -> Self {
        let error = ApiError::from(VaultError::Locked(locked.error().to_string()));
        Self {
            vault: Arc::new(stand_in),
            index_dir: None,
            open_error: Mutex::new(Some(error)),
            running: tokio::sync::Mutex::new(()),
            locked: Some(locked),
            restart_needed: AtomicBool::new(false),
        }
    }

    /// Whether the data directory could not be opened (see [`Self::locked`]).
    pub(crate) fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// Opens the search index in `dir` as the vault state needs it
    /// (sealed, or plain). A locked vault gets an in-memory index, so the
    /// sealed files are not mistaken for damage and rebuilt.
    pub(crate) fn open_index(&self, dir: &Path) -> Result<SearchIndex> {
        let in_ram =
            || -> Result<SearchIndex> { Ok(SearchIndex::open_in_ram(CommitPolicy::default())?) };
        if self.check().is_err() {
            return in_ram();
        }
        let wrapper = match self.vault.index_wrapper() {
            Ok(wrapper) => wrapper,
            Err(err) => {
                tracing::error!(%err, "cannot wrap the search index; searching in memory only");
                return in_ram();
            }
        };
        match SearchIndex::open_with(dir, CommitPolicy::default(), wrapper) {
            Ok(index) => Ok(index),
            Err(err) => {
                tracing::error!(%err, "cannot open the search index; searching in memory only");
                in_ram()
            }
        }
    }

    /// `Ok` when stored data can be read; the open error otherwise.
    pub(crate) fn check(&self) -> Result<()> {
        match lock(&self.open_error).clone() {
            None => Ok(()),
            Some(err) => Err(err.into()),
        }
    }

    /// The status; a failed open is reported as its error.
    pub(crate) fn status(&self) -> Result<VaultStatus> {
        if let Some(locked) = &self.locked {
            let mut status = locked.status();
            if self.restart_needed.load(Ordering::Acquire) {
                status.key_in_keychain = true;
                status.needs_recovery = false;
                status.error = None;
                status.message = Some("restart Nail to open the data".into());
                status.localized = Some(LocalizedMessage::new(RESTART_NEEDED));
            }
            return Ok(status);
        }
        let mut status = self.vault.status()?;
        if let Some(err) = lock(&self.open_error).clone() {
            status.error = Some(err.code);
            status.message = Some(err.message);
            status.localized = err.localized.map(|l| *l);
        }
        Ok(status)
    }

    /// The vault (blocking calls).
    pub(crate) fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }
}

/// The text of a vault whose key was restored while it was locked.
pub(crate) const RESTART_NEEDED: &str = "vault-restart-needed";

/// Emits [`Event::VaultProgress`] at most every [`PROGRESS_EVERY`] (and
/// on every phase change).
struct Throttle {
    service: Service,
    last: Option<(Instant, Option<VaultPhase>)>,
}

impl Throttle {
    fn report(&mut self, status: &VaultStatus) -> ControlFlow<()> {
        let due = self
            .last
            .is_none_or(|(at, phase)| phase != status.phase || at.elapsed() >= PROGRESS_EVERY);
        if due {
            self.last = Some((Instant::now(), status.phase));
            self.service.broadcast(Event::VaultProgress {
                status: status.clone(),
            });
        }
        ControlFlow::Continue(())
    }
}

impl Service {
    /// `SetVaultEncryption`: records the target and starts the migration
    /// in the background. `conflict` while a migration runs (unless it
    /// stopped with an error: then this retries or reverses it).
    pub(crate) async fn set_vault_encryption(&self, enabled: bool) -> Result<VaultStatus> {
        self.inner.vault.check()?;
        if self.inner.vault.running.try_lock().is_err() {
            return Err(ApiError::new(
                nail_api::ErrorCode::Conflict,
                "encryption at rest is being switched already",
            )
            .into());
        }
        let vault = Arc::clone(self.inner.vault.vault());
        let status = tokio::task::spawn_blocking(move || vault.set_enabled(enabled)).await??;
        self.broadcast(Event::VaultProgress {
            status: status.clone(),
        });
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(err) = service.run_vault_migration().await {
                tracing::warn!(%err, "switching encryption at rest failed");
            }
        });
        Ok(status)
    }

    /// Runs a pending migration to the end (see the module docs).
    pub(crate) async fn run_vault_migration(&self) -> Result<VaultStatus> {
        let ctl = &self.inner.vault;
        let _running = ctl.running.lock().await;
        let vault = Arc::clone(ctl.vault());
        if !{
            let vault = Arc::clone(&vault);
            tokio::task::spawn_blocking(move || vault.needs_migration()).await??
        } {
            return ctl.status();
        }
        let result = self.migrate(&vault).await;
        let status = ctl.status()?;
        self.broadcast(Event::VaultProgress {
            status: status.clone(),
        });
        result.map(|()| status)
    }

    async fn migrate(&self, vault: &Arc<Vault>) -> Result<()> {
        // Database and blobs, with the index open.
        let first = {
            let vault = Arc::clone(vault);
            let mut throttle = Throttle {
                service: self.clone(),
                last: None,
            };
            let stop = self
                .inner
                .vault
                .index_dir
                .as_ref()
                .map(|_| VaultPhase::Index);
            tokio::task::spawn_blocking(move || {
                vault.migrate_until(stop, &mut |s| throttle.report(s))
            })
            .await?
        };
        first?;
        let Some(dir) = self.inner.vault.index_dir.clone() else {
            return Ok(());
        };
        // The index phase: close it, convert it, open it again.
        let slot = &self.inner.search;
        slot.set_closed(true);
        let old = slot.replace(SearchIndex::open_in_ram(CommitPolicy::default())?);
        let deadline = Instant::now() + INDEX_RELEASE_WAIT;
        while Arc::strong_count(&old) > 1 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        drop(old);
        let converted = {
            let vault = Arc::clone(vault);
            let mut throttle = Throttle {
                service: self.clone(),
                last: None,
            };
            tokio::task::spawn_blocking(move || vault.migrate(&mut |s| throttle.report(s))).await?
        };
        let reopened = {
            let service = self.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let index = service.inner.vault.open_index(&dir)?;
                service.inner.search.replace(index);
                service.inner.search.set_closed(false);
                // Mail that arrived meanwhile went to the stand-in.
                let index = service.inner.search.get();
                let stored = u64::from(service.store().count_messages(&MessageFilter::All)?);
                if index.was_rebuilt() || index.doc_count()? < stored {
                    crate::ingest::reindex_all(service.store(), service.blobs(), &index)?;
                    let settings = service
                        .store()
                        .setting::<nail_api::security::SecuritySettings>(
                            nail_store::SECURITY_SETTINGS_KEY,
                        )?
                        .unwrap_or_default();
                    let policy = nail_search::IndexPolicy::from_settings(&settings);
                    service.reindex_encrypted(&settings.index_decrypted, &policy)?;
                }
                Ok(())
            })
            .await?
        };
        converted?;
        reopened
    }
}

/// Resumes an interrupted migration when the service starts; afterwards
/// encrypts the database file of a vault encrypted before M4c (column
/// sealing only) once.
pub(crate) async fn resume_migration(service: Service) {
    let vault = Arc::clone(service.inner.vault.vault());
    let pending = tokio::task::spawn_blocking(move || vault.needs_migration()).await;
    if service.inner.vault.check().is_err() {
        return;
    }
    if !matches!(pending, Ok(Ok(true))) {
        let _running = service.inner.vault.running.lock().await;
        let vault = Arc::clone(service.inner.vault.vault());
        match tokio::task::spawn_blocking(move || vault.encrypt_database_file()).await {
            Ok(Ok(true)) => tracing::info!("the database file is encrypted now"),
            Ok(Ok(false)) => {}
            Ok(Err(err)) => tracing::warn!(%err, "cannot encrypt the database file"),
            Err(err) => tracing::warn!(%err, "encrypting the database file stopped"),
        }
        return;
    }
    tracing::info!("resuming the switch of encryption at rest");
    if let Err(err) = service.run_vault_migration().await {
        tracing::warn!(%err, "resuming the switch of encryption at rest failed");
    }
}

impl Service {
    /// `CreateRecoveryKey`: a new recovery key (shown once), optionally
    /// with a recovery-kit text file.
    pub(crate) async fn create_recovery_key(
        &self,
        save_to: Option<String>,
    ) -> Result<nail_api::Response> {
        if self.inner.vault.is_locked() {
            self.inner.vault.check()?;
        }
        let path = match save_to.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => {
                let path = PathBuf::from(p);
                if !path.is_absolute() {
                    return Err(
                        ApiError::invalid_request("the recovery kit needs a full path")
                            .with_localized(LocalizedMessage::new(
                                "portability-error-path-not-absolute",
                            ))
                            .into(),
                    );
                }
                Some(path)
            }
            None => None,
        };
        let vault = Arc::clone(self.inner.vault.vault());
        let locale = self.inner.locale.clone();
        tokio::task::spawn_blocking(move || -> Result<nail_api::Response> {
            // The kit is written before the new key replaces the old one: a
            // kit that cannot be written leaves the old recovery key working.
            let (key, status) = vault.create_recovery_key_with(|key, created| -> Result<()> {
                match &path {
                    Some(path) => write_kit(path, &recovery_kit(locale.as_deref(), key, created)),
                    None => Ok(()),
                }
            })?;
            let saved_to = path.map(|path| path.display().to_string());
            Ok(nail_api::Response::RecoveryKeyCreated {
                key: key.as_str().to_owned(),
                saved_to,
                status,
            })
        })
        .await?
    }

    /// `VerifyRecoveryKey`.
    pub(crate) async fn verify_recovery_key(&self, key: String) -> Result<nail_api::Response> {
        let key = zeroize::Zeroizing::new(key);
        let ctl = &self.inner.vault;
        if let Some(locked) = &ctl.locked {
            let valid = locked.verify_recovery_key(&key)?;
            return Ok(nail_api::Response::RecoveryKeyVerified {
                valid,
                status: ctl.status()?.recovery,
            });
        }
        let vault = Arc::clone(ctl.vault());
        let (valid, status) =
            tokio::task::spawn_blocking(move || vault.verify_recovery_key(&key)).await??;
        Ok(nail_api::Response::RecoveryKeyVerified { valid, status })
    }

    /// `RestoreVaultKey`: puts the data key back into the keychain. A
    /// locked data directory opens after a restart; a vault whose sealed
    /// columns were locked opens at once (the search index is opened again).
    pub(crate) async fn restore_vault_key(&self, key: String) -> Result<VaultStatus> {
        let key = zeroize::Zeroizing::new(key);
        let ctl = &self.inner.vault;
        if let Some(locked) = &ctl.locked {
            locked.restore_key(&key)?;
            ctl.restart_needed.store(true, Ordering::Release);
            let status = ctl.status()?;
            self.broadcast(Event::VaultProgress {
                status: status.clone(),
            });
            return Ok(status);
        }
        let vault = Arc::clone(ctl.vault());
        tokio::task::spawn_blocking(move || vault.restore_key(&key)).await??;
        let was_locked = lock(&ctl.open_error).take().is_some();
        if was_locked && let Some(dir) = ctl.index_dir.clone() {
            let service = self.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let index = service.inner.vault.open_index(&dir)?;
                service.inner.search.replace(index);
                let index = service.inner.search.get();
                let stored = u64::from(service.store().count_messages(&MessageFilter::All)?);
                if index.was_rebuilt() || index.doc_count()? < stored {
                    crate::ingest::reindex_all(service.store(), service.blobs(), &index)?;
                }
                Ok(())
            })
            .await??;
        }
        let status = ctl.status()?;
        self.broadcast(Event::VaultProgress {
            status: status.clone(),
        });
        Ok(status)
    }

    /// `RemoveRecoveryKey`.
    pub(crate) async fn remove_recovery_key(&self) -> Result<VaultStatus> {
        self.inner.vault.check()?;
        let vault = Arc::clone(self.inner.vault.vault());
        Ok(tokio::task::spawn_blocking(move || vault.remove_recovery_key()).await??)
    }
}

/// The recovery kit's text (the user's language; English fallback).
pub(crate) fn recovery_kit(locale: Option<&str>, key: &str, created: &str) -> String {
    let line = |id: &str, args: &[(&str, &str)]| crate::strings::message_with(locale, id, args);
    [
        line("recovery-kit-title", &[]),
        String::new(),
        line("recovery-kit-intro", &[]),
        String::new(),
        line("recovery-kit-key", &[("key", key)]),
        line("recovery-kit-created", &[("created", created)]),
        String::new(),
        line("recovery-kit-advice", &[]),
        String::new(),
    ]
    .join("\r\n")
}

/// Writes the kit (never over an existing file).
fn write_kit(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                ApiError::new(
                    nail_api::ErrorCode::Conflict,
                    format!("{} exists", path.display()),
                )
                .with_localized(LocalizedMessage::new("portability-error-target-exists"))
            } else {
                ApiError::internal(format!("cannot write {}: {e}", path.display()))
            }
        })?;
    file.write_all(text.as_bytes())?;
    Ok(())
}
