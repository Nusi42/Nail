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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nail_api::engine::SecretStore;
use nail_api::security::{VaultPhase, VaultStatus};
use nail_api::{ApiError, Event};
use nail_search::{CommitPolicy, SearchIndex};
use nail_store::{BlobStore, MessageFilter, Store};
use nail_vault::Vault;

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
}

impl VaultCtl {
    /// Opens the vault of a data directory (`index_dir` = its search index
    /// directory, `None` for an in-memory index).
    pub(crate) fn open(
        store: Store,
        blobs: BlobStore,
        index_dir: Option<PathBuf>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        let vault = Vault::new(store, blobs, index_dir.clone(), secrets);
        let open_error = match vault.open() {
            Ok(status) => {
                if status.state != nail_api::security::VaultState::Plain {
                    tracing::info!(state = status.state.as_str(), "encryption at rest is on");
                }
                None
            }
            Err(err) => {
                tracing::error!(%err, "the data key of the encrypted data is not available");
                Some(ApiError::new(err.code(), err.to_string()))
            }
        };
        Self {
            vault: Arc::new(vault),
            index_dir,
            open_error: Mutex::new(open_error),
            running: tokio::sync::Mutex::new(()),
        }
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
        let mut status = self.vault.status()?;
        if let Some(err) = lock(&self.open_error).clone() {
            status.error = Some(err.code);
            status.message = Some(err.message);
        }
        Ok(status)
    }

    /// The vault (blocking calls).
    pub(crate) fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }
}

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

/// Resumes an interrupted migration when the service starts.
pub(crate) async fn resume_migration(service: Service) {
    let vault = Arc::clone(service.inner.vault.vault());
    let pending = tokio::task::spawn_blocking(move || vault.needs_migration()).await;
    if !matches!(pending, Ok(Ok(true))) || service.inner.vault.check().is_err() {
        return;
    }
    tracing::info!("resuming the switch of encryption at rest");
    if let Err(err) = service.run_vault_migration().await {
        tracing::warn!(%err, "resuming the switch of encryption at rest failed");
    }
}
