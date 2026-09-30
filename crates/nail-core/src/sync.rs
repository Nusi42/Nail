//! IMAP synchronization: one pass over an account on a connected client.
//!
//! A pass ([`run_pass`]):
//!
//! 1. `LIST` the folders and mirror the tree into the store (new folders
//!    are added, folders gone from the server are deleted with their
//!    messages);
//! 2. for every selectable folder the sync policy enables
//!    ([`sync_folder`]):
//!    * plan what is new since the stored `UIDVALIDITY`/`UIDNEXT`
//!      ([`ImapClient::plan_fetch`]); a changed `UIDVALIDITY` drops the
//!      folder's messages and fetches it again;
//!    * fetch the plan batch by batch; each batch goes to the blob store,
//!      the store (threaded, merged with pending local changes) and the
//!      search index, and the folder's `UIDNEXT` checkpoint is saved right
//!      after it, so an interrupted pass resumes after the last stored
//!      batch;
//!    * find flag changes and expunges of the messages already stored
//!      ([`ImapClient::resync_flags`]: QRESYNC, CONDSTORE or a full flag
//!      scan) and apply them; the new `HIGHESTMODSEQ` is saved.
//!
//! A folder that fails (the server refuses to open it, withholds a message,
//! and so on) is logged, listed in [`SyncReport::failed_folders`] and
//! skipped; the other folders are still synced, and the pass ends with one
//! [`Event::SyncFailed`] naming the failed folders. Connection-level errors
//! (connection lost, timeout, TLS) and store failures end the pass at once.
//!
//! Progress is broadcast as [`Event::SyncProgress`] (one unit for the folder
//! list plus one per folder), changed folders as [`Event::MailChanged`],
//! new mail in folders synced before as [`Event::NewMail`], and a failed
//! pass as [`Event::SyncFailed`].
//!
//! Configured accounts run passes from their engine (`engine.rs`) on a
//! long-lived connection. Accounts registered with
//! [`crate::Service::add_imap_account`] (tests, embedding) are synced on
//! demand by [`SyncEngine`], one connection per pass.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use nail_api::accounts::{BodyPolicy, SyncPolicy};
use nail_api::{AccountId, ApiError, ErrorCode, Event, Folder, FolderId, FolderRole};
use nail_proto_imap::{
    BodyFetch, CredentialProvider, Credentials, FetchedMessage, ImapClient, ImapConfig, ImapError,
    MessageFlags, RemoteFolder, ResyncState, Secret, SyncState,
};
use nail_store::{BlobStore, FolderSyncState, Store};
use tokio::sync::broadcast;

use crate::error::{CoreError, Result};
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};

/// How to reach an IMAP account registered with
/// [`crate::Service::add_imap_account`]. Credentials are held in memory
/// only (configured accounts read theirs from the keychain).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapAccount {
    /// Server, port, security and timeouts.
    pub config: ImapConfig,
    /// User name and password.
    pub credentials: Credentials,
}

/// Server settings plus a credential source (engine-internal form).
#[derive(Clone)]
pub(crate) struct ImapSettings {
    pub config: ImapConfig,
    pub credentials: Arc<dyn CredentialProvider>,
}

impl std::fmt::Debug for ImapSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapSettings")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl From<ImapAccount> for ImapSettings {
    fn from(account: ImapAccount) -> Self {
        Self {
            config: account.config,
            credentials: Arc::new(account.credentials),
        }
    }
}

/// Connects and logs in. With an OAuth2 token the server rejects, a fresh
/// token is requested once.
pub(crate) async fn connect(settings: &ImapSettings) -> Result<ImapClient, ImapError> {
    let credentials = settings.credentials.credentials(false).await?;
    match ImapClient::connect(&settings.config, &credentials).await {
        Err(ImapError::AuthFailed(reason))
            if matches!(credentials.secret, Secret::AccessToken(_)) =>
        {
            tracing::info!(%reason, "IMAP rejected the access token; refreshing it once");
            let credentials = settings.credentials.credentials(true).await?;
            ImapClient::connect(&settings.config, &credentials).await
        }
        other => other,
    }
}

/// What one sync pass did. A pass that reaches the end is `Ok`, even when
/// single folders failed (see [`Self::failed_folders`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Folders on the server (selectable or not).
    pub folders: u32,
    /// Messages fetched and stored.
    pub new_messages: u32,
    /// Local folders deleted because they are gone from the server.
    pub removed_folders: u32,
    /// Folders whose `UIDVALIDITY` changed and that were fetched again.
    pub reset_folders: u32,
    /// Stored messages whose flags changed on the server.
    pub flag_changes: u32,
    /// Stored messages that were expunged on the server (and removed).
    pub vanished: u32,
    /// Folders that could not be synced in this pass (the others were).
    pub failed_folders: Vec<FolderSyncFailure>,
}

/// A folder that failed during an otherwise completed pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSyncFailure {
    /// The folder.
    pub folder_id: FolderId,
    /// Its server path.
    pub path: String,
    /// Machine-readable error class.
    pub code: ErrorCode,
    /// Diagnostic detail (English).
    pub message: String,
}

/// What syncing one folder did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FolderOutcome {
    pub new_messages: u32,
    pub reset: bool,
    pub flag_changes: u32,
    pub vanished: u32,
}

/// Everything a pass writes to.
#[derive(Clone)]
pub(crate) struct SyncCtx {
    pub store: Store,
    pub blobs: BlobStore,
    pub events: broadcast::Sender<Event>,
    pub index: Arc<crate::search::SearchSlot>,
}

impl SyncCtx {
    pub(crate) fn emit(&self, event: Event) {
        // An error only means nobody is listening right now.
        let _ = self.events.send(event);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks an account as being synced; a second pass is refused.
#[derive(Debug, Default)]
pub(crate) struct RunningPasses(Mutex<HashSet<AccountId>>);

/// Removes an account from the running set when a pass ends (also on panic
/// or cancellation).
pub(crate) struct RunningGuard<'a> {
    running: &'a RunningPasses,
    account: AccountId,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        lock(&self.running.0).remove(&self.account);
    }
}

impl RunningPasses {
    /// Claims `account`, failing with `unavailable` while a pass runs.
    pub(crate) fn claim(&self, account: &AccountId) -> Result<RunningGuard<'_>> {
        if !lock(&self.0).insert(account.clone()) {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("account {account} is already being synchronized"),
            )
            .into());
        }
        Ok(RunningGuard {
            running: self,
            account: account.clone(),
        })
    }
}

/// Runs passes for accounts registered with
/// [`crate::Service::add_imap_account`].
#[derive(Debug, Default)]
pub(crate) struct SyncEngine {
    accounts: Mutex<HashMap<AccountId, ImapAccount>>,
}

impl SyncEngine {
    /// Makes `account` syncable with `settings` (replacing earlier settings).
    pub(crate) fn register(&self, account: AccountId, settings: ImapAccount) {
        lock(&self.accounts).insert(account, settings);
    }

    /// The settings of a registered account.
    pub(crate) fn settings(&self, account: &AccountId) -> Option<ImapAccount> {
        lock(&self.accounts).get(account).cloned()
    }

    /// Whether `account` is registered.
    pub(crate) fn is_registered(&self, account: &AccountId) -> bool {
        lock(&self.accounts).contains_key(account)
    }

    /// All registered accounts, sorted.
    pub(crate) fn accounts(&self) -> Vec<AccountId> {
        let mut ids: Vec<_> = lock(&self.accounts).keys().cloned().collect();
        ids.sort();
        ids
    }
}

/// Runs one pass over `account` on a fresh connection and reports the
/// outcome as events (see [`report_pass`]).
pub(crate) async fn sync_with_connection(
    ctx: &SyncCtx,
    account: &AccountId,
    settings: &ImapSettings,
    policy: &SyncPolicy,
) -> Result<SyncReport> {
    let result = async {
        let mut client = connect(settings).await?;
        let report = run_pass(ctx, account, &mut client, policy).await?;
        if let Err(err) = client.logout().await {
            tracing::debug!(%err, "logout failed after a successful sync");
        }
        Ok(report)
    }
    .await;
    report_pass(ctx, account, &result);
    result
}

/// Logs a pass's outcome and broadcasts [`Event::SyncFailed`] for a failed
/// pass or failed folders.
pub(crate) fn report_pass(ctx: &SyncCtx, account: &AccountId, result: &Result<SyncReport>) {
    match result {
        Ok(report) if !report.failed_folders.is_empty() => {
            tracing::warn!(%account, ?report, "sync finished; some folders failed");
            let paths: Vec<&str> = report
                .failed_folders
                .iter()
                .map(|f| f.path.as_str())
                .collect();
            let first = &report.failed_folders[0];
            ctx.emit(Event::SyncFailed {
                account_id: account.clone(),
                code: first.code,
                message: format!(
                    "{} folder(s) could not be synchronized ({}); first error: {}",
                    paths.len(),
                    paths.join(", "),
                    first.message
                ),
                localized: Some(
                    nail_api::LocalizedMessage::new("sync-error-folders")
                        .arg("count", paths.len())
                        .arg("folders", paths.join(", ")),
                ),
            });
        }
        Ok(report) => tracing::info!(%account, ?report, "sync finished"),
        Err(err) => {
            tracing::warn!(%account, %err, "sync failed");
            ctx.emit(Event::SyncFailed {
                account_id: account.clone(),
                code: err.code(),
                message: err.to_string(),
                localized: err.localized(),
            });
        }
    }
}

/// One pass over every folder of `account` on `client`.
pub(crate) async fn run_pass(
    ctx: &SyncCtx,
    account: &AccountId,
    client: &mut ImapClient,
    policy: &SyncPolicy,
) -> Result<SyncReport> {
    {
        let account = account.clone();
        ctx.store.call(move |s| s.get_account(&account)).await?;
    }
    let mut remote = client.list_folders().await?;
    // A parent's path is a proper prefix of its children's paths, so
    // shorter paths first puts parents before children.
    remote.sort_by(|a, b| a.path.len().cmp(&b.path.len()).then(a.path.cmp(&b.path)));

    let mut report = SyncReport {
        folders: u32::try_from(remote.len()).unwrap_or(u32::MAX),
        ..SyncReport::default()
    };
    let selectable: Vec<&RemoteFolder> = remote
        .iter()
        .filter(|f| f.selectable && policy.effective(&f.path, f.role).enabled)
        .collect();
    let total = u32::try_from(selectable.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);

    let (folders, tree_changes, removed) = mirror_folders(ctx, account, &remote).await?;
    report.removed_folders = removed;
    if !tree_changes.is_empty() {
        ctx.emit(Event::MailChanged {
            folder_ids: tree_changes,
        });
    }
    ctx.emit(Event::SyncProgress {
        account_id: account.clone(),
        done: 1,
        total,
    });

    for (index, remote_folder) in selectable.iter().enumerate() {
        let folder = folders
            .get(&remote_folder.path)
            .cloned()
            .ok_or_else(|| ApiError::internal(format!("folder {} vanished", remote_folder.path)))?;
        let bodies = policy.effective(&folder.path, folder.role).bodies;
        match sync_folder(ctx, client, &folder, &bodies).await {
            Ok(outcome) => {
                report.new_messages = report.new_messages.saturating_add(outcome.new_messages);
                report.flag_changes = report.flag_changes.saturating_add(outcome.flag_changes);
                report.vanished = report.vanished.saturating_add(outcome.vanished);
                if outcome.reset {
                    report.reset_folders += 1;
                }
            }
            Err(err) if is_fatal_for_pass(&err) => return Err(err),
            Err(err) => {
                tracing::warn!(%account, folder = %folder.path, %err, "folder sync failed; continuing");
                report.failed_folders.push(FolderSyncFailure {
                    folder_id: folder.id.clone(),
                    path: folder.path.clone(),
                    code: err.code(),
                    message: err.to_string(),
                });
                // The command failed cleanly; make sure the connection
                // is still usable before going on.
                client.noop().await?;
            }
        }
        ctx.emit(Event::SyncProgress {
            account_id: account.clone(),
            done: u32::try_from(index).unwrap_or(u32::MAX).saturating_add(2),
            total,
        });
    }
    Ok(report)
}

/// Upserts the server's folder tree and deletes local folders that are
/// gone. Returns the folders by path, the ids of added or removed
/// folders, and the number removed.
async fn mirror_folders(
    ctx: &SyncCtx,
    account: &AccountId,
    remote: &[RemoteFolder],
) -> Result<(HashMap<String, Folder>, Vec<FolderId>, u32)> {
    let account = account.clone();
    let remote = remote.to_vec();
    let blobs = ctx.blobs.clone();
    let index = ctx.index.get();
    let result = ctx
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
                if paths.contains(folder.path.as_str()) {
                    continue;
                }
                // The hidden Snoozed folder exists locally before the
                // first queued move creates it on the server (M2c).
                if crate::junk::is_snoozed_folder(folder) && folder.total > 0 {
                    continue;
                }
                let hashes = s.folder_blob_hashes(&folder.id)?;
                let gone: Vec<_> = s
                    .folder_uids(&folder.id)?
                    .into_iter()
                    .map(|(_, id)| id)
                    .collect();
                if s.delete_folder(&folder.id)? {
                    changed.push(folder.id.clone());
                    removed += 1;
                }
                if let Err(err) = index.remove(&gone) {
                    tracing::warn!(%err, "cannot remove messages from the search index");
                }
                // The raw mail of the deleted messages goes too.
                s.remove_unreferenced_blobs(&blobs, &hashes)?;
            }
            Ok((by_path, changed, removed))
        })
        .await?;
    Ok(result)
}

/// Fetches what is new in one folder (storing and checkpointing batch by
/// batch), then applies flag changes and expunges of the stored messages.
/// Emits [`Event::MailChanged`] for every change it stores and
/// [`Event::NewMail`] for new mail in a folder synced before.
pub(crate) async fn sync_folder(
    ctx: &SyncCtx,
    client: &mut ImapClient,
    folder: &Folder,
    bodies: &BodyPolicy,
) -> Result<FolderOutcome> {
    let now = crate::engine::unix_now();
    let fetch = BodyFetch::from_policy(bodies, now);
    let store = &ctx.store;
    let stored_state = {
        let id = folder.id.clone();
        store.call(move |s| s.folder_sync_state(&id)).await?
    };
    let known = match (stored_state.uidvalidity, stored_state.uidnext) {
        (Some(uid_validity), Some(uid_next)) => Some(SyncState {
            uid_validity,
            uid_next,
        }),
        _ => None,
    };

    let plan = client.plan_fetch(&folder.path, known).await?;
    let reset = plan.uid_validity_changed;
    let uid_validity = plan.status.uid_validity;
    let mut outcome = FolderOutcome {
        reset,
        ..FolderOutcome::default()
    };
    if reset {
        reset_folder(ctx, folder, uid_validity).await?;
    }

    let mut new_ids = Vec::new();
    for (index, batch) in plan.batches.iter().enumerate() {
        let messages: Vec<IncomingMessage> = client
            .fetch_batch_with(batch, fetch)
            .await?
            .into_iter()
            // A message marked \Deleted is on its way out (e.g. a move
            // without UIDPLUS); do not show it.
            .filter(|m| !m.deleted)
            .map(incoming)
            .collect();
        let next = FolderSyncState {
            uidvalidity: Some(uid_validity),
            uidnext: Some(plan.state_after(index).uid_next),
            highestmodseq: if reset {
                None
            } else {
                stored_state.highestmodseq
            },
        };
        let target = folder.clone();
        let blobs = ctx.blobs.clone();
        let search = ctx.index.get();
        let stored = store
            .call(move |s| {
                let report = ingest(s, &blobs, Some(&search), &target, uid_validity, &messages)?;
                s.set_folder_sync_state(&target.id, &next)?;
                Ok(report)
            })
            .await?;
        outcome.new_messages = outcome
            .new_messages
            .saturating_add(u32::try_from(stored.stored.len()).unwrap_or(u32::MAX));
        if !stored.stored.is_empty() {
            ctx.emit(Event::MailChanged {
                folder_ids: vec![folder.id.clone()],
            });
        }
        new_ids.extend(stored.new);
    }
    if known.is_some()
        && !reset
        && !new_ids.is_empty()
        && matches!(folder.role, FolderRole::Inbox | FolderRole::Other)
        && !crate::junk::is_snoozed_folder(folder)
    {
        // Arrival rules run on new mail (the rules task wakes on NewMail;
        // the inbox survives a restart).
        let queued = new_ids.clone();
        if let Err(err) = store
            .call(move |s| crate::rules::queue_for_rules(s, &queued))
            .await
        {
            tracing::warn!(%err, "cannot queue new mail for the rules");
        }
        ctx.emit(Event::NewMail {
            account_id: folder.account_id.clone(),
            folder_id: folder.id.clone(),
            message_ids: new_ids,
        });
    }

    let mut state = FolderSyncState {
        uidvalidity: Some(plan.final_state.uid_validity),
        uidnext: Some(plan.final_state.uid_next),
        highestmodseq: if reset {
            None
        } else {
            stored_state.highestmodseq
        },
    };
    if known.is_some() && !reset {
        let resync = resync_folder(ctx, client, folder, &state).await?;
        outcome.flag_changes = resync.flag_changes;
        outcome.vanished = resync.vanished;
        if resync.uid_validity_changed {
            // Everything stored is stale: drop it; the next pass refetches.
            reset_folder(ctx, folder, 0).await?;
            outcome.reset = true;
            return Ok(outcome);
        }
        state.highestmodseq = resync.highest_modseq;
    } else {
        state.highestmodseq = plan.status.highest_modseq;
    }
    if state != stored_state {
        let id = folder.id.clone();
        store
            .call(move |s| s.set_folder_sync_state(&id, &state))
            .await?;
    }
    if let BodyPolicy::Recent { days } = bodies {
        let cutoff = now - i64::from(*days) * 86_400;
        trim_bodies(ctx, folder, cutoff).await?;
    }
    Ok(outcome)
}

/// Drops the stored bodies of messages that aged out of a "recent N days"
/// policy (dated before `cutoff`): they keep their header block and are
/// fetched again on demand. Their search entries keep the body text.
async fn trim_bodies(ctx: &SyncCtx, folder: &Folder, cutoff: i64) -> Result<()> {
    let folder_id = folder.id.clone();
    let blobs = ctx.blobs.clone();
    let trimmed = ctx
        .store
        .call(move |s| {
            let mut trimmed = 0usize;
            loop {
                let batch = s.bodies_before(&folder_id, cutoff, 200)?;
                if batch.is_empty() {
                    break;
                }
                let mut gone = Vec::new();
                for body in &batch {
                    let raw = blobs.get(&body.blob_hash)?;
                    let headers = blobs.put(header_block(&raw))?;
                    gone.extend(s.detach_body(&body.message_id, &headers)?);
                    trimmed += 1;
                }
                s.remove_unreferenced_blobs(&blobs, &gone)?;
            }
            Ok(trimmed)
        })
        .await?;
    if trimmed > 0 {
        tracing::info!(folder = %folder.path, trimmed, "bodies older than the sync policy dropped");
        ctx.emit(Event::MailChanged {
            folder_ids: vec![folder.id.clone()],
        });
    }
    Ok(())
}

/// The header block of a raw message (up to and including the empty
/// line), or everything when there is no body.
pub(crate) fn header_block(raw: &[u8]) -> &[u8] {
    if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        return &raw[..i + 4];
    }
    if let Some(i) = raw.windows(2).position(|w| w == b"\n\n") {
        return &raw[..i + 2];
    }
    raw
}

/// Drops a folder's messages and restarts its checkpoint under a new
/// `UIDVALIDITY` (0 = unknown: fetch everything next time).
async fn reset_folder(ctx: &SyncCtx, folder: &Folder, uid_validity: u32) -> Result<()> {
    let target = folder.clone();
    let blobs = ctx.blobs.clone();
    let index = ctx.index.get();
    let restart = FolderSyncState {
        uidvalidity: (uid_validity != 0).then_some(uid_validity),
        uidnext: (uid_validity != 0).then_some(1),
        highestmodseq: None,
    };
    ctx.store
        .call(move |s| {
            let hashes = s.folder_blob_hashes(&target.id)?;
            let gone: Vec<_> = s
                .folder_uids(&target.id)?
                .into_iter()
                .map(|(_, id)| id)
                .collect();
            let dropped = s.clear_folder(&target.id)?;
            // Messages with waiting operations stay (without their UID).
            let gone: Vec<_> = gone
                .into_iter()
                .filter(|id| s.get_message(id).is_err())
                .collect();
            tracing::info!(folder = %target.path, dropped, "UIDVALIDITY changed; refetching");
            s.set_folder_sync_state(&target.id, &restart)?;
            if let Err(err) = index.remove(&gone) {
                tracing::warn!(%err, "cannot remove messages from the search index");
            }
            // Refetched messages store their blobs again.
            s.remove_unreferenced_blobs(&blobs, &hashes)?;
            Ok(())
        })
        .await?;
    ctx.emit(Event::MailChanged {
        folder_ids: vec![folder.id.clone()],
    });
    Ok(())
}

struct ResyncOutcome {
    flag_changes: u32,
    vanished: u32,
    uid_validity_changed: bool,
    highest_modseq: Option<u64>,
}

/// Applies the server's flag changes and expunges to the stored messages.
async fn resync_folder(
    ctx: &SyncCtx,
    client: &mut ImapClient,
    folder: &Folder,
    state: &FolderSyncState,
) -> Result<ResyncOutcome> {
    let Some(uid_validity) = state.uidvalidity else {
        return Ok(ResyncOutcome {
            flag_changes: 0,
            vanished: 0,
            uid_validity_changed: false,
            highest_modseq: None,
        });
    };
    let folder_id = folder.id.clone();
    let local: Vec<(u32, nail_api::MessageId, MessageFlags)> = ctx
        .store
        .call(move |s| {
            let uids = s.folder_uids(&folder_id)?;
            let ids: Vec<_> = uids.iter().map(|(_, id)| id.clone()).collect();
            let summaries: HashMap<_, _> = s
                .message_summaries(&ids)?
                .into_iter()
                .map(|m| (m.id.clone(), m))
                .collect();
            Ok(uids
                .into_iter()
                .filter_map(|(uid, id)| {
                    let summary = summaries.get(&id)?;
                    let flags = MessageFlags {
                        flags: summary.flags,
                        deleted: false,
                        keywords: summary.tags.clone(),
                    };
                    Some((uid, id, flags))
                })
                .collect())
        })
        .await?;
    let by_uid: HashMap<u32, nail_api::MessageId> = local
        .iter()
        .map(|(uid, id, _)| (*uid, id.clone()))
        .collect();
    let flags: BTreeMap<u32, MessageFlags> = local
        .into_iter()
        .map(|(uid, _, flags)| (uid, flags))
        .collect();
    let resync = client
        .resync_flags(
            &folder.path,
            ResyncState {
                uid_validity,
                highest_modseq: state.highestmodseq,
            },
            &flags,
        )
        .await?;
    if resync.uid_validity_changed {
        return Ok(ResyncOutcome {
            flag_changes: 0,
            vanished: 0,
            uid_validity_changed: true,
            highest_modseq: None,
        });
    }
    let mut gone: Vec<u32> = resync.vanished.clone();
    let mut updates = Vec::new();
    for update in &resync.changed {
        let Some(id) = by_uid.get(&update.uid) else {
            continue;
        };
        if update.flags.deleted {
            gone.push(update.uid);
        } else {
            updates.push((id.clone(), update.flags.clone()));
        }
    }
    let flag_changes = u32::try_from(updates.len()).unwrap_or(u32::MAX);
    let vanished = u32::try_from(gone.len()).unwrap_or(u32::MAX);
    if !updates.is_empty() || !gone.is_empty() {
        let folder_id = folder.id.clone();
        let blobs = ctx.blobs.clone();
        let index = ctx.index.get();
        let gone_ids: Vec<_> = gone.iter().filter_map(|u| by_uid.get(u).cloned()).collect();
        ctx.store
            .call(move |s| {
                for (id, flags) in &updates {
                    s.update_flags_and_tags(id, flags.flags, &flags.keywords)?;
                }
                let mut hashes = Vec::new();
                for id in &gone_ids {
                    if let Ok(m) = s.get_message(id) {
                        hashes.extend(m.blob_hash);
                    }
                }
                s.delete_messages_by_uid(&folder_id, &gone)?;
                if let Err(err) = index.remove(&gone_ids) {
                    tracing::warn!(%err, "cannot remove messages from the search index");
                }
                s.remove_unreferenced_blobs(&blobs, &hashes)?;
                Ok(())
            })
            .await?;
        ctx.emit(Event::MailChanged {
            folder_ids: vec![folder.id.clone()],
        });
    }
    Ok(ResyncOutcome {
        flag_changes,
        vanished,
        uid_validity_changed: false,
        highest_modseq: resync.next_state.highest_modseq,
    })
}

/// Whether an error while syncing one folder ends the whole pass. Clean
/// server refusals (`NO`, `BAD`, a withheld message) only affect that
/// folder; a broken connection or a failing local store affects them all.
fn is_fatal_for_pass(err: &CoreError) -> bool {
    match err {
        CoreError::Imap(err) => !matches!(err, ImapError::No(_) | ImapError::Protocol(_)),
        CoreError::Api(_) => false,
        _ => true,
    }
}

fn incoming(message: FetchedMessage) -> IncomingMessage {
    IncomingMessage {
        uid: Some(message.uid),
        id: None,
        flags: message.flags,
        keywords: message.keywords,
        internal_date: message.internal_date,
        raw: message.raw,
        headers_only: message.headers_only,
        size: message.size.map(u64::from),
    }
}
