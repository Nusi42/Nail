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
//!      batch. The steps overlap ([`crate::pipeline`]): while one batch is
//!      merged into the store, the next one's blobs are written and its
//!      MIME parsed on worker threads and the one after it is fetched;
//!      batches are still stored and checkpointed strictly in order;
//!    * find flag changes and expunges of the messages already stored
//!      ([`ImapClient::resync_flags`]: QRESYNC, CONDSTORE or a full flag
//!      scan) and apply them; the new `HIGHESTMODSEQ` is saved.
//!
//! A message that cannot be downloaded (the server refuses it, withholds
//! it, sends it unreadable or too large) does not fail its folder (ADR
//! 0191): [`ImapClient::fetch_batch_resilient`] isolates it, it is recorded
//! as skipped (`sync_skipped`, with its error details) in the same store
//! call as the batch's checkpoint, the checkpoint moves past it, and later
//! passes retry it ([`retry_skipped`]). A folder that fails (the server
//! refuses to open it, and so on) is logged, listed in
//! [`SyncReport::failed_folders`] with its [`ErrorDetails`] and skipped; the
//! other folders are still synced, and the pass ends with one
//! [`Event::SyncFailed`] naming the failed folders. Connection-level errors
//! (connection lost, timeout, TLS) and store failures end the pass at once.
//! The last error of the account and of each folder is stored until a
//! later pass succeeds (ADR 0190).
//!
//! Progress is broadcast as [`Event::SyncProgress`] (one unit for the folder
//! list plus one per folder), changed folders as [`Event::MailChanged`]
//! (while a folder's batches are stored, at most one per
//! [`MAIL_CHANGED_INTERVAL`] and one after its last batch), new mail in
//! folders synced before as [`Event::NewMail`], and a failed pass as
//! [`Event::SyncFailed`].
//!
//! Configured accounts run passes from their engine (`engine.rs`) on a
//! long-lived connection. Accounts registered with
//! [`crate::Service::add_imap_account`] (tests, embedding) are synced on
//! demand by [`SyncEngine`], one connection per pass.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use nail_api::accounts::{BodyPolicy, SyncPolicy};
use nail_api::connlog::{ConnectionLogScope, ErrorDetails, ErrorStep, WireProtocol};
use nail_api::{AccountId, ApiError, ErrorCode, Event, Folder, FolderId, FolderRole};
use nail_connlog::ConnectionLogs;
use nail_proto_imap::{
    BodyFetch, CredentialProvider, Credentials, FetchFailure, FetchFailureKind, FetchedMessage,
    ImapClient, ImapConfig, ImapError, MessageFlags, RemoteFolder, ResyncState, Secret, SyncState,
};
use nail_store::{BlobStore, FolderSyncState, Store};
use tokio::sync::broadcast;

use crate::connlog::{ErrorContext, error_details};
use crate::error::{CoreError, Result};
use crate::ids;
use crate::ingest::{IncomingMessage, PreparedBatch, ingest_prepared, prepare};
use crate::pipeline::{BatchSink, BatchSource, pipeline};

/// Shortest time between two [`Event::MailChanged`] for a folder whose
/// batches are being stored (a large initial sync stores thousands of
/// batches; clients reload the list on every event).
pub(crate) const MAIL_CHANGED_INTERVAL: Duration = Duration::from_millis(500);

/// Skipped messages a folder sync retries at most per pass.
const SKIPPED_RETRIES: usize = 50;

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
    /// Folders synced without an error in this pass.
    pub synced_folders: Vec<FolderId>,
    /// Messages skipped in this pass (could not be downloaded; retried
    /// later, ADR 0191).
    pub skipped: u32,
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
    /// The precise cause ("Show details", ADR 0190).
    pub details: ErrorDetails,
}

/// What syncing one folder did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FolderOutcome {
    pub new_messages: u32,
    pub reset: bool,
    pub flag_changes: u32,
    pub vanished: u32,
    /// Messages newly skipped (ADR 0191).
    pub skipped: u32,
}

/// Everything a pass writes to.
#[derive(Clone)]
pub(crate) struct SyncCtx {
    pub store: Store,
    pub blobs: BlobStore,
    pub events: broadcast::Sender<Event>,
    pub index: Arc<crate::search::SearchSlot>,
    /// The connection logs (error details name their position, ADR 0190).
    pub connlog: Arc<ConnectionLogs>,
}

impl SyncCtx {
    pub(crate) fn emit(&self, event: Event) {
        // An error only means nobody is listening right now.
        drop(self.events.send(event));
    }

    /// The details of an error of `account` in `context`.
    pub(crate) fn details(
        &self,
        account: &AccountId,
        err: &CoreError,
        context: ErrorContext,
    ) -> ErrorDetails {
        let scope = ConnectionLogScope::account(account);
        error_details(
            &self.connlog.tap(&scope),
            self.connlog.last_seq(&scope),
            err.code(),
            &err.to_string(),
            context,
        )
    }

    /// The context of an error on `client` (IMAP), else of the error alone.
    fn context(err: &CoreError, client: &ImapClient, folder: Option<&str>) -> ErrorContext {
        match err {
            CoreError::Imap(_) => ErrorContext::imap(client, folder),
            other => ErrorContext {
                folder: folder.map(str::to_owned),
                ..ErrorContext::of_error(WireProtocol::Imap, other)
            },
        }
    }

    /// The details of a message a batch skipped.
    fn skipped_details(
        &self,
        account: &AccountId,
        folder: &str,
        failure: &FetchFailure,
    ) -> ErrorDetails {
        let code = match failure.kind {
            FetchFailureKind::Refused => ErrorCode::Rejected,
            FetchFailureKind::Withheld => ErrorCode::Network,
            FetchFailureKind::TooLarge => ErrorCode::TooLarge,
        };
        let scope = ConnectionLogScope::account(account);
        error_details(
            &self.connlog.tap(&scope),
            self.connlog.last_seq(&scope),
            code,
            &failure.detail,
            ErrorContext {
                protocol: Some(WireProtocol::Imap),
                step: Some(ErrorStep::Fetch),
                folder: Some(folder.to_owned()),
                command: failure.command.clone(),
                server_text: failure.server_text.clone(),
            },
        )
    }

    /// Stores the outcome of a pass: errors of failed folders, cleared
    /// errors of synced ones and of the account (ADR 0190). Announces a
    /// change with [`Event::SyncErrorsChanged`].
    async fn store_errors(
        &self,
        account: &AccountId,
        result: &Result<SyncReport>,
        failure: Option<&ErrorDetails>,
    ) {
        let id = account.clone();
        let (failed, synced): (Vec<(FolderId, ErrorDetails)>, Vec<FolderId>) = match result {
            Ok(report) => (
                report
                    .failed_folders
                    .iter()
                    .map(|f| (f.folder_id.clone(), f.details.clone()))
                    .collect(),
                report.synced_folders.clone(),
            ),
            Err(_) => (Vec::new(), Vec::new()),
        };
        let account_error = failure.cloned();
        let changed = self
            .store
            .call(move |s| {
                let mut changed = false;
                for (folder, details) in &failed {
                    s.set_sync_error(&id, Some(folder), details)?;
                    changed = true;
                }
                for folder in &synced {
                    changed |= s.clear_sync_error(&id, Some(folder))?;
                }
                match &account_error {
                    Some(details) => {
                        s.set_sync_error(&id, None, details)?;
                        changed = true;
                    }
                    None => changed |= s.clear_sync_error(&id, None)?,
                }
                Ok(changed)
            })
            .await;
        match changed {
            Ok(true) => self.emit(Event::SyncErrorsChanged {
                account_id: account.clone(),
            }),
            Ok(false) => {}
            Err(err) => tracing::warn!(%err, "cannot store the sync errors"),
        }
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
    let mut context = None;
    let result = async {
        let mut client = connect(settings).await.inspect_err(|err| {
            context = Some(ErrorContext::of_imap(err));
        })?;
        let report = run_pass(ctx, account, &mut client, policy).await;
        if let Err(err) = &report {
            context = Some(SyncCtx::context(err, &client, None));
        }
        let report = report?;
        if let Err(err) = client.logout().await {
            tracing::debug!(%err, "logout failed after a successful sync");
        }
        Ok(report)
    }
    .await;
    report_pass(ctx, account, &result, context).await;
    result
}

/// Logs a pass's outcome, stores its errors and broadcasts
/// [`Event::SyncFailed`] for a failed pass (`context`: where it failed) or
/// failed folders.
pub(crate) async fn report_pass(
    ctx: &SyncCtx,
    account: &AccountId,
    result: &Result<SyncReport>,
    context: Option<ErrorContext>,
) {
    let failure = result
        .as_ref()
        .err()
        .map(|err| ctx.details(account, err, context.unwrap_or_default()));
    ctx.store_errors(account, result, failure.as_ref()).await;
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
                details: Some(Box::new(first.details.clone())),
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
                details: failure.map(Box::new),
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
                report.skipped = report.skipped.saturating_add(outcome.skipped);
                if outcome.reset {
                    report.reset_folders += 1;
                }
                report.synced_folders.push(folder.id.clone());
            }
            Err(err) if is_fatal_for_pass(&err) => return Err(err),
            Err(err) => {
                tracing::warn!(%account, folder = %folder.path, %err, "folder sync failed; continuing");
                let details = ctx.details(
                    account,
                    &err,
                    SyncCtx::context(&err, client, Some(&folder.path)),
                );
                report.failed_folders.push(FolderSyncFailure {
                    folder_id: folder.id.clone(),
                    path: folder.path.clone(),
                    code: err.code(),
                    message: err.to_string(),
                    details,
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
            // A folder keeps its id while its path stays, also after Nail
            // renamed it (ADR 0196); a new path gets a free id.
            let mut ids_by_path: HashMap<String, FolderId> = existing
                .iter()
                .map(|f| (f.path.clone(), f.id.clone()))
                .collect();
            let mut changed = Vec::new();
            for rf in &remote {
                if !ids_by_path.contains_key(&rf.path) {
                    let id = ids::unique_folder_id(s, &account, &rf.path)?;
                    changed.push(id.clone());
                    ids_by_path.insert(rf.path.clone(), id);
                }
            }
            let mut by_path = HashMap::new();
            for rf in &remote {
                let parent_id = rf
                    .parent_path
                    .as_deref()
                    .filter(|p| paths.contains(p))
                    .and_then(|p| ids_by_path.get(p).cloned());
                let folder = Folder {
                    id: ids_by_path
                        .get(&rf.path)
                        .cloned()
                        .unwrap_or_else(|| ids::folder_id(&account, &rf.path)),
                    account_id: account.clone(),
                    parent_id,
                    name: rf.name.clone(),
                    path: rf.path.clone(),
                    role: rf.role,
                    unread: 0,
                    total: 0,
                };
                s.upsert_folder(&folder)?;
                s.set_folder_delimiter(&folder.id, rf.delimiter.as_deref())?;
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
                if crate::junk::is_snoozed_folder(s, folder) && folder.total > 0 {
                    continue;
                }
                if remove_local_folder(s, &blobs, &index, &folder.id)? {
                    changed.push(folder.id.clone());
                    removed += 1;
                }
            }
            Ok((by_path, changed, removed))
        })
        .await?;
    Ok(result)
}

/// Deletes a folder with its messages, their search-index entries and the
/// raw mail no other message uses (blocking). Subfolders become top-level;
/// delete them first. Returns whether the folder existed.
pub(crate) fn remove_local_folder(
    s: &Store,
    blobs: &BlobStore,
    index: &nail_search::SearchIndex,
    folder: &FolderId,
) -> nail_store::Result<bool> {
    let hashes = s.folder_blob_hashes(folder)?;
    let gone = s.folder_message_ids(folder)?;
    let existed = s.delete_folder(folder)?;
    if let Err(err) = index.remove(&gone) {
        tracing::warn!(%err, "cannot remove messages from the search index");
    }
    s.remove_unreferenced_blobs(blobs, &hashes)?;
    Ok(existed)
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
    let now = nail_util::time::unix_now();
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

    let checkpoint_modseq = if reset {
        None
    } else {
        stored_state.highestmodseq
    };
    let stored = store_batches(
        ctx,
        client,
        folder,
        &plan.batches,
        fetch,
        |index| FolderSyncState {
            uidvalidity: Some(uid_validity),
            uidnext: Some(plan.state_after(index).uid_next),
            highestmodseq: checkpoint_modseq,
        },
        uid_validity,
    )
    .await?;
    outcome.new_messages = stored.stored;
    outcome.skipped = stored.skipped;
    let mut new_ids = stored.new;
    if !reset {
        let retried = retry_skipped(ctx, client, folder, fetch, uid_validity).await?;
        outcome.new_messages = outcome.new_messages.saturating_add(retried.stored);
        new_ids.extend(retried.new);
    }
    let hidden = {
        let folder = folder.clone();
        store
            .call(move |s| Ok(crate::junk::is_snoozed_folder(s, &folder)))
            .await
            .unwrap_or(false)
    };
    if known.is_some()
        && !reset
        && !new_ids.is_empty()
        && runs_arrival_rules(folder.role)
        && !hidden
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

/// What [`store_batches`] stored.
#[derive(Debug, Default)]
struct StoredBatches {
    /// Messages written.
    stored: u32,
    /// Messages that did not exist before.
    new: Vec<nail_api::MessageId>,
    /// Messages skipped (recorded in `sync_skipped`).
    skipped: u32,
}

/// One fetched batch: the messages and the ones the server could not
/// deliver (ADR 0191).
struct FetchedBatch {
    messages: Vec<IncomingMessage>,
    skipped: Vec<(u32, ErrorDetails)>,
}

/// Coalesces [`Event::MailChanged`] for one folder while its batches are
/// stored: at most one per [`MAIL_CHANGED_INTERVAL`], and a final one for
/// anything not announced yet ([`Self::flush`]).
#[derive(Debug, Default)]
pub(crate) struct MailChangedThrottle {
    last: Option<Instant>,
    unannounced: bool,
}

impl MailChangedThrottle {
    /// A batch was stored at `now` (`changed`: it wrote rows). Returns
    /// whether to announce the change now.
    pub(crate) fn stored(&mut self, changed: bool, now: Instant) -> bool {
        self.unannounced |= changed;
        let due = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= MAIL_CHANGED_INTERVAL);
        if self.unannounced && due {
            self.last = Some(now);
            self.unannounced = false;
            return true;
        }
        false
    }

    /// Whether stored changes are still unannounced (announce them now).
    pub(crate) fn flush(&mut self) -> bool {
        std::mem::take(&mut self.unannounced)
    }
}

/// The fetching step of [`store_batches`]: one `UID FETCH` per batch, split
/// around messages the server cannot deliver (ADR 0191).
struct ImapBatches<'a> {
    ctx: &'a SyncCtx,
    folder: &'a Folder,
    client: &'a mut ImapClient,
    batches: &'a [Vec<u32>],
    fetch: BodyFetch,
}

impl BatchSource for ImapBatches<'_> {
    type Batch = FetchedBatch;

    async fn fetch(&mut self, index: usize) -> Result<FetchedBatch> {
        let fetched = self
            .client
            .fetch_batch_resilient(&self.batches[index], self.fetch)
            .await?;
        let skipped = fetched
            .failed
            .iter()
            .map(|failure| {
                tracing::warn!(folder = %self.folder.path, uid = failure.uid, detail = %failure.detail, "message skipped");
                (
                    failure.uid,
                    self.ctx
                        .skipped_details(&self.folder.account_id, &self.folder.path, failure),
                )
            })
            .collect();
        Ok(FetchedBatch {
            messages: fetched
                .messages
                .into_iter()
                // A message marked \Deleted is on its way out (e.g. a move
                // without UIDPLUS); do not show it.
                .filter(|m| !m.deleted)
                .map(incoming)
                .collect(),
            skipped,
        })
    }
}

/// The storing step of [`store_batches`]: [`ingest_prepared`] plus the
/// folder's checkpoint in one store call per batch.
struct FolderSink<'a, C> {
    ctx: &'a SyncCtx,
    folder: &'a Folder,
    uid_validity: u32,
    checkpoint: C,
    out: StoredBatches,
    events: MailChangedThrottle,
}

impl<C> BatchSink<(PreparedBatch, Vec<(u32, ErrorDetails)>)> for FolderSink<'_, C>
where
    C: Fn(usize) -> FolderSyncState + Send + Sync,
{
    async fn store(
        &mut self,
        index: usize,
        (batch, skipped): (PreparedBatch, Vec<(u32, ErrorDetails)>),
    ) -> Result<()> {
        let next = (self.checkpoint)(index);
        let target = self.folder.clone();
        let blobs = self.ctx.blobs.clone();
        let search = self.ctx.index.get();
        let uid_validity = self.uid_validity;
        let skipped_count = u32::try_from(skipped.len()).unwrap_or(u32::MAX);
        let stored = self
            .ctx
            .store
            .call(move |s| {
                let report =
                    ingest_prepared(s, &blobs, Some(&search), &target, uid_validity, &batch)?;
                // Skipped messages are recorded before the checkpoint moves
                // past them, so they are retried, never lost.
                for (uid, details) in &skipped {
                    s.record_skipped(&target.id, *uid, details)?;
                }
                s.set_folder_sync_state(&target.id, &next)?;
                Ok(report)
            })
            .await?;
        if skipped_count > 0 {
            self.out.skipped = self.out.skipped.saturating_add(skipped_count);
            self.ctx.emit(Event::SyncErrorsChanged {
                account_id: self.folder.account_id.clone(),
            });
        }
        self.out.stored = self
            .out
            .stored
            .saturating_add(u32::try_from(stored.stored.len()).unwrap_or(u32::MAX));
        self.out.new.extend(stored.new);
        if self
            .events
            .stored(!stored.stored.is_empty(), Instant::now())
        {
            self.ctx.emit(Event::MailChanged {
                folder_ids: vec![self.folder.id.clone()],
            });
        }
        Ok(())
    }
}

/// Fetches `batches` of `folder` and stores them in order, each with its
/// checkpoint (`checkpoint(index)`), through the overlapping [`pipeline`]:
/// fetching on this connection, preparing ([`prepare`]: blob writes and
/// MIME parsing on worker threads) and storing ([`FolderSink`]).
async fn store_batches(
    ctx: &SyncCtx,
    client: &mut ImapClient,
    folder: &Folder,
    batches: &[Vec<u32>],
    fetch: BodyFetch,
    checkpoint: impl Fn(usize) -> FolderSyncState + Send + Sync,
    uid_validity: u32,
) -> Result<StoredBatches> {
    let mut source = ImapBatches {
        ctx,
        folder,
        client,
        batches,
        fetch,
    };
    let mut sink = FolderSink {
        ctx,
        folder,
        uid_validity,
        checkpoint,
        out: StoredBatches::default(),
        events: MailChangedThrottle::default(),
    };
    let blobs = ctx.blobs.clone();
    let result = pipeline(
        batches.len(),
        &mut source,
        move |batch: FetchedBatch| Ok((prepare(&blobs, batch.messages)?, batch.skipped)),
        &mut sink,
    )
    .await;
    // Whatever was stored is announced, also when a step failed.
    if sink.events.flush() {
        ctx.emit(Event::MailChanged {
            folder_ids: vec![folder.id.clone()],
        });
    }
    result.map(|()| sink.out)
}

/// Fetches messages earlier passes skipped (ADR 0191), least tried first:
/// the ones that come now are stored and forgotten as skipped, the ones that
/// still fail count another attempt, the ones gone from the server are
/// forgotten.
async fn retry_skipped(
    ctx: &SyncCtx,
    client: &mut ImapClient,
    folder: &Folder,
    fetch: BodyFetch,
    uid_validity: u32,
) -> Result<StoredBatches> {
    let uids = {
        let id = folder.id.clone();
        ctx.store
            .call(move |s| s.skipped_uids(&id, SKIPPED_RETRIES))
            .await?
    };
    if uids.is_empty() {
        return Ok(StoredBatches::default());
    }
    let fetched = client.fetch_batch_resilient(&uids, fetch).await?;
    let failed: Vec<(u32, ErrorDetails)> = fetched
        .failed
        .iter()
        .map(|f| {
            (
                f.uid,
                ctx.skipped_details(&folder.account_id, &folder.path, f),
            )
        })
        .collect();
    let done: Vec<u32> = uids
        .iter()
        .copied()
        .filter(|uid| !failed.iter().any(|(f, _)| f == uid))
        .collect();
    let messages: Vec<IncomingMessage> = fetched
        .messages
        .into_iter()
        .filter(|m| !m.deleted)
        .map(incoming)
        .collect();
    let target = folder.clone();
    let blobs = ctx.blobs.clone();
    let search = ctx.index.get();
    let report = ctx
        .store
        .call(move |s| {
            let report =
                crate::ingest::ingest(s, &blobs, Some(&search), &target, uid_validity, &messages)?;
            s.remove_skipped(&target.id, &done)?;
            for (uid, details) in &failed {
                s.record_skipped(&target.id, *uid, details)?;
            }
            Ok(report)
        })
        .await?;
    tracing::info!(folder = %folder.path, retried = uids.len(), stored = report.stored.len(), "skipped messages retried");
    ctx.emit(Event::SyncErrorsChanged {
        account_id: folder.account_id.clone(),
    });
    if !report.stored.is_empty() {
        ctx.emit(Event::MailChanged {
            folder_ids: vec![folder.id.clone()],
        });
    }
    Ok(StoredBatches {
        stored: u32::try_from(report.stored.len()).unwrap_or(u32::MAX),
        new: report.new,
        skipped: 0,
    })
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

/// Whether new mail in a folder with `role` is "arriving" mail: queued
/// for arrival rules and iTIP processing. Only the inbox and the user's
/// own folders; never Sent (the user's own mail, also what Nail sent),
/// Drafts, Junk, Trash, Archive or All Mail. IMAP and Gmail share it.
pub(crate) fn runs_arrival_rules(role: FolderRole) -> bool {
    matches!(role, FolderRole::Inbox | FolderRole::Other)
}

/// The header block of a raw message (up to and including the empty
/// line), or everything when there is no body.
pub(crate) use nail_util::mime_text::header_section as header_block;

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
            // Skipped UIDs of the old UIDVALIDITY name nothing now.
            s.clear_skipped(&target.id)?;
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
    // UIDs, flags and tags in two queries (loading every summary made an
    // incremental sync of a large mailbox slow; post-M4c).
    let local: Vec<(u32, nail_api::MessageId, MessageFlags)> = ctx
        .store
        .call(move |s| {
            Ok(s.folder_uid_flags(&folder_id)?
                .into_iter()
                .map(|m| {
                    let flags = MessageFlags {
                        flags: m.flags,
                        deleted: false,
                        keywords: m.tags,
                    };
                    (m.uid, m.id, flags)
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
        remote_thread_id: None,
    }
}

#[cfg(test)]
mod arrival_tests {
    use super::*;

    #[test]
    fn mail_changed_is_coalesced_while_batches_are_stored() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut throttle = MailChangedThrottle::default();
        // The first stored batch is announced at once.
        assert!(throttle.stored(true, at(0)));
        // More batches within the interval wait.
        assert!(!throttle.stored(true, at(100)));
        assert!(!throttle.stored(true, at(499)));
        // Once the interval passed, the next batch announces them all.
        assert!(throttle.stored(false, at(500)));
        // Nothing new: nothing to announce, not even later.
        assert!(!throttle.stored(false, at(2_000)));
        assert!(!throttle.flush());
        // A last change inside the interval is announced by the flush.
        assert!(throttle.stored(true, at(2_100)));
        assert!(!throttle.stored(true, at(2_200)));
        assert!(throttle.flush());
        assert!(!throttle.flush());
        // Batches that wrote nothing announce nothing.
        let mut idle = MailChangedThrottle::default();
        assert!(!idle.stored(false, at(0)));
        assert!(!idle.flush());
    }

    #[test]
    fn only_the_inbox_and_user_folders_take_arrivals() {
        // Review M3b: Gmail ran arrival rules / iTIP processing on Sent,
        // Spam and Trash; IMAP and Gmail now share this filter.
        assert!(runs_arrival_rules(FolderRole::Inbox));
        assert!(runs_arrival_rules(FolderRole::Other));
        for role in [
            FolderRole::Sent,
            FolderRole::Drafts,
            FolderRole::Archive,
            FolderRole::Junk,
            FolderRole::Trash,
        ] {
            assert!(!runs_arrival_rules(role), "{role:?}");
        }
    }
}
