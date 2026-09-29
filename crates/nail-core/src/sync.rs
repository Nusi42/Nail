//! The read-only IMAP sync engine (M0).
//!
//! One pass over an account ([`SyncEngine::sync_account`]):
//!
//! 1. connect and log in, `LIST` the folders;
//! 2. mirror the folder tree into the store (new folders are added, folders
//!    gone from the server are deleted with their messages);
//! 3. for every selectable folder, fetch what is new since the stored
//!    `UIDVALIDITY`/`UIDNEXT` ([`ImapClient::fetch_new`]); a changed
//!    `UIDVALIDITY` drops the folder's messages and fetches it again;
//! 4. store raw messages in the blob store, parsed summaries in the store.
//!
//! Progress is broadcast as [`Event::SyncProgress`] (one unit for the folder
//! list plus one per folder), changed folders as [`Event::MailChanged`], and
//! a failed pass as [`Event::SyncFailed`].
//!
//! Not in M0 (see `docs/adr/0009-core-service.md`): flag changes and
//! expunges of already-synced messages, IDLE, CONDSTORE/QRESYNC.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use nail_api::{AccountId, ApiError, ErrorCode, Event, Folder, FolderId};
use nail_proto_imap::{
    Credentials, FetchedMessage, ImapClient, ImapConfig, RemoteFolder, SyncState,
};
use nail_store::{BlobStore, FolderSyncState, Store};
use tokio::sync::broadcast;

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};

/// How to reach an IMAP account. Credentials are held in memory only; the
/// OS keychain integration arrives with account setup in M1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapAccount {
    /// Server, port, security and timeouts.
    pub config: ImapConfig,
    /// User name and password.
    pub credentials: Credentials,
}

/// What one successful sync pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Folders on the server (selectable or not).
    pub folders: u32,
    /// Messages fetched and stored.
    pub new_messages: u32,
    /// Local folders deleted because they are gone from the server.
    pub removed_folders: u32,
    /// Folders whose `UIDVALIDITY` changed and that were fetched again.
    pub reset_folders: u32,
}

/// Runs sync passes and remembers which accounts can be synced.
#[derive(Debug, Clone)]
pub(crate) struct SyncEngine {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    store: Store,
    blobs: BlobStore,
    events: broadcast::Sender<Event>,
    accounts: Mutex<HashMap<AccountId, ImapAccount>>,
    running: Mutex<HashSet<AccountId>>,
}

/// Removes an account from the running set when a pass ends (also on panic
/// or cancellation).
struct RunningGuard<'a> {
    running: &'a Mutex<HashSet<AccountId>>,
    account: AccountId,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        lock(self.running).remove(&self.account);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SyncEngine {
    pub(crate) fn new(store: Store, blobs: BlobStore, events: broadcast::Sender<Event>) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                blobs,
                events,
                accounts: Mutex::new(HashMap::new()),
                running: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Makes `account` syncable with `settings` (replacing earlier settings).
    pub(crate) fn register(&self, account: AccountId, settings: ImapAccount) {
        lock(&self.inner.accounts).insert(account, settings);
    }

    /// Whether `account` has server settings.
    pub(crate) fn is_syncable(&self, account: &AccountId) -> bool {
        lock(&self.inner.accounts).contains_key(account)
    }

    /// All syncable accounts, sorted.
    pub(crate) fn syncable_accounts(&self) -> Vec<AccountId> {
        let mut ids: Vec<_> = lock(&self.inner.accounts).keys().cloned().collect();
        ids.sort();
        ids
    }

    fn emit(&self, event: Event) {
        // An error only means nobody is listening right now.
        let _ = self.inner.events.send(event);
    }

    /// Runs one sync pass over `account`. Broadcasts progress, changes and,
    /// on failure, [`Event::SyncFailed`]. Fails with
    /// [`ErrorCode::Unavailable`] when a pass for the account is already
    /// running.
    pub(crate) async fn sync_account(&self, account: &AccountId) -> Result<SyncReport> {
        let settings = lock(&self.inner.accounts)
            .get(account)
            .cloned()
            .ok_or_else(|| {
                ApiError::not_found(format!("account {account} has no server to sync"))
            })?;
        if !lock(&self.inner.running).insert(account.clone()) {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("account {account} is already being synchronized"),
            )
            .into());
        }
        let _guard = RunningGuard {
            running: &self.inner.running,
            account: account.clone(),
        };

        let result = self.run_pass(account, &settings).await;
        match &result {
            Ok(report) => tracing::info!(%account, ?report, "sync finished"),
            Err(err) => {
                tracing::warn!(%account, %err, "sync failed");
                self.emit(Event::SyncFailed {
                    account_id: account.clone(),
                    code: err.code(),
                    message: err.to_string(),
                });
            }
        }
        result
    }

    async fn run_pass(&self, account: &AccountId, settings: &ImapAccount) -> Result<SyncReport> {
        let store = &self.inner.store;
        {
            let account = account.clone();
            store.call(move |s| s.get_account(&account)).await?;
        }

        let mut client = ImapClient::connect(&settings.config, &settings.credentials).await?;
        let mut remote = client.list_folders().await?;
        // A parent's path is a proper prefix of its children's paths, so
        // shorter paths first puts parents before children.
        remote.sort_by(|a, b| a.path.len().cmp(&b.path.len()).then(a.path.cmp(&b.path)));

        let mut report = SyncReport {
            folders: u32::try_from(remote.len()).unwrap_or(u32::MAX),
            ..SyncReport::default()
        };
        let selectable: Vec<&RemoteFolder> = remote.iter().filter(|f| f.selectable).collect();
        let total = u32::try_from(selectable.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);

        let (folders, tree_changes, removed) = self.mirror_folders(account, &remote).await?;
        report.removed_folders = removed;
        if !tree_changes.is_empty() {
            self.emit(Event::MailChanged {
                folder_ids: tree_changes,
            });
        }
        self.emit(Event::SyncProgress {
            account_id: account.clone(),
            done: 1,
            total,
        });

        for (index, remote_folder) in selectable.iter().enumerate() {
            let folder = folders.get(&remote_folder.path).cloned().ok_or_else(|| {
                ApiError::internal(format!("folder {} vanished", remote_folder.path))
            })?;
            let (new_messages, reset) = self.sync_folder(&mut client, &folder).await?;
            report.new_messages = report.new_messages.saturating_add(new_messages);
            if reset {
                report.reset_folders += 1;
            }
            if new_messages > 0 || reset {
                self.emit(Event::MailChanged {
                    folder_ids: vec![folder.id.clone()],
                });
            }
            self.emit(Event::SyncProgress {
                account_id: account.clone(),
                done: u32::try_from(index).unwrap_or(u32::MAX).saturating_add(2),
                total,
            });
        }

        if let Err(err) = client.logout().await {
            tracing::debug!(%err, "logout failed after a successful sync");
        }
        Ok(report)
    }

    /// Upserts the server's folder tree and deletes local folders that are
    /// gone. Returns the folders by path, the ids of added or removed
    /// folders, and the number removed.
    async fn mirror_folders(
        &self,
        account: &AccountId,
        remote: &[RemoteFolder],
    ) -> Result<(HashMap<String, Folder>, Vec<FolderId>, u32)> {
        let account = account.clone();
        let remote = remote.to_vec();
        let result = self
            .inner
            .store
            .call(move |s| {
                let paths: HashSet<&str> = remote.iter().map(|f| f.path.as_str()).collect();
                let existing = s.list_folders(&account)?;
                let known: HashSet<String> = existing.iter().map(|f| f.path.clone()).collect();
                let mut changed = Vec::new();
                let mut by_path = HashMap::new();
                for rf in &remote {
                    let parent_id = rf
                        .parent_path
                        .as_deref()
                        .filter(|p| paths.contains(p))
                        .map(|p| ids::folder_id(&account, p));
                    let folder = Folder {
                        id: ids::folder_id(&account, &rf.path),
                        account_id: account.clone(),
                        parent_id,
                        name: rf.name.clone(),
                        path: rf.path.clone(),
                        role: rf.role,
                        unread: 0,
                        total: 0,
                    };
                    s.upsert_folder(&folder)?;
                    if !known.contains(&rf.path) {
                        changed.push(folder.id.clone());
                    }
                    by_path.insert(rf.path.clone(), folder);
                }
                // Children before parents, so no parent is deleted under a
                // child (deletes cascade anyway; this keeps it explicit).
                let mut removed = 0;
                for folder in existing.iter().rev() {
                    if !paths.contains(folder.path.as_str()) && s.delete_folder(&folder.id)? {
                        changed.push(folder.id.clone());
                        removed += 1;
                    }
                }
                Ok((by_path, changed, removed))
            })
            .await?;
        Ok(result)
    }

    /// Fetches what is new in one folder. Returns the number of stored
    /// messages and whether the folder was reset (UIDVALIDITY change).
    async fn sync_folder(&self, client: &mut ImapClient, folder: &Folder) -> Result<(u32, bool)> {
        let store = &self.inner.store;
        let known = {
            let id = folder.id.clone();
            let state = store.call(move |s| s.folder_sync_state(&id)).await?;
            match (state.uidvalidity, state.uidnext) {
                (Some(uid_validity), Some(uid_next)) => Some(SyncState {
                    uid_validity,
                    uid_next,
                }),
                _ => None,
            }
        };

        let fetch = client.fetch_new(&folder.path, known).await?;
        let reset = fetch.uid_validity_changed;
        let uid_validity = fetch.next_state.uid_validity;
        let messages: Vec<IncomingMessage> = fetch.messages.into_iter().map(incoming).collect();
        let next = FolderSyncState {
            uidvalidity: Some(fetch.next_state.uid_validity),
            uidnext: Some(fetch.next_state.uid_next),
            highestmodseq: None,
        };

        let folder = folder.clone();
        let blobs = self.inner.blobs.clone();
        let stored = store
            .call(move |s| {
                if reset {
                    let dropped = s.clear_folder(&folder.id)?;
                    tracing::info!(folder = %folder.path, dropped, "UIDVALIDITY changed; refetching");
                }
                let stored = ingest(s, &blobs, &folder, uid_validity, &messages)?;
                s.set_folder_sync_state(&folder.id, &next)?;
                Ok(stored)
            })
            .await?;
        Ok((u32::try_from(stored).unwrap_or(u32::MAX), reset))
    }
}

fn incoming(message: FetchedMessage) -> IncomingMessage {
    IncomingMessage {
        uid: message.uid,
        flags: message.flags,
        internal_date: message.internal_date,
        raw: message.raw,
    }
}
