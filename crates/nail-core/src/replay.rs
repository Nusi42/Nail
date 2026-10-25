//! Replaying queued writes (ADR 0007 / 0015): the store's per-account
//! queue drives an executor per protocol, and every step is broadcast as
//! [`Event::OpStatus`].
//!
//! * IMAP ([`ImapExecutor`]): flags with `UID STORE`, moves with `MOVE` (or
//!   `COPY` + `\Deleted` + expunge), deletes to Trash as moves, permanent
//!   deletes as `\Deleted` + expunge, Sent copies with `APPEND`. Moves into
//!   another account append the raw message there and remove it from the
//!   source; once the copy is filed the operation is rewritten to the
//!   remaining expunge, so a retry never files it twice.
//! * A recorded UID is only used while the folder's UIDVALIDITY is the one
//!   it was recorded with; otherwise (or when the UID is unknown) the
//!   message is found again by its Message-ID (ADR 0021).
//! * The new UID a move or append reports (`COPYUID` / `APPENDUID`) goes to
//!   the local row when it is still in that folder, and to the operations
//!   queued after it for the same message ([`nail_store::Store::rebase_later_ops`]),
//!   so a message moved or deleted again before the first move reached the
//!   server is still found.
//! * POP3 and local accounts ([`LocalExecutor`]): everything is local; a
//!   permanent delete of a downloaded POP3 message is remembered for the
//!   "delete when deleted locally" policy.
//!
//! A retryable failure (offline, timeout) keeps the operation queued and
//! ends the account's pass; a permanent one fails it (the next sync then
//! shows the server's state again).

use std::sync::Arc;

use nail_api::engine::{BoxFuture, EngineError};
use nail_api::{AccountId, ErrorCode, Event, Folder, IncomingServer, MessageId, OpState, OpStatus};
use nail_proto_imap::{Flag, ImapClient};
use nail_store::{
    MessageSource, Operation, QueuedOp, ReplayOutcome, ReplayPolicy, ReplayReport, Replayer,
};

use crate::error::{CoreError, Result};
use crate::folders::{FolderApplied, FolderChange, server_path};
use crate::ids;
use crate::service::Service;
use crate::sync::{ImapSettings, connect};

/// Performs queued operations.
pub(crate) trait OpExecutor: Send + Sync {
    /// `Err` with `retryable` keeps the operation queued.
    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>>;

    /// Most operations [`Self::execute_group`] takes (P3, ADR 0164); `1`
    /// (the default) executes every operation on its own.
    fn max_group(&self) -> usize {
        1
    }

    /// Changes a folder on the server (ADR 0196). Default: `unsupported`
    /// (Gmail, whose folders are labels; nothing queues one there).
    fn folder_change<'a>(
        &'a self,
        change: &'a FolderChange,
    ) -> BoxFuture<'a, Result<FolderApplied, EngineError>> {
        let _ = change;
        Box::pin(async {
            Err(EngineError::permanent(
                ErrorCode::Unsupported,
                "this account cannot change folders",
            ))
        })
    }

    /// Executes consecutive operations of one account with the same
    /// `Operation::group_key` together (one server call where the protocol
    /// allows). One result per operation, in order; a shorter answer
    /// leaves the rest unattempted. The default executes them one by one
    /// and stops after the first retryable error.
    fn execute_group<'a>(
        &'a self,
        ops: &'a [QueuedOp],
    ) -> BoxFuture<'a, Vec<Result<(), EngineError>>> {
        Box::pin(async move {
            let mut out = Vec::with_capacity(ops.len());
            for op in ops {
                let result = self.execute(op).await;
                let stop = matches!(&result, Err(err) if err.retryable);
                out.push(result);
                if stop {
                    break;
                }
            }
            out
        })
    }
}

/// Executes `ops` one by one (an executor's fallback for groups it cannot
/// send as one call).
async fn one_by_one(
    executor: &(impl OpExecutor + ?Sized),
    ops: &[QueuedOp],
) -> Vec<Result<(), EngineError>> {
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(executor.execute(op).await);
    }
    out
}

/// Adapts an executor to the store's [`Replayer`], broadcasting progress.
struct EventReplayer<'a> {
    service: &'a Service,
    executor: &'a dyn OpExecutor,
    policy: ReplayPolicy,
}

impl EventReplayer<'_> {
    fn status(&self, op: &QueuedOp, state: OpState, error: Option<&EngineError>) {
        self.service.broadcast(Event::OpStatus {
            status: OpStatus {
                op_id: ids::op_id(op.id),
                account_id: op.account_id.clone(),
                state,
                error: error.map(|e| e.code),
                message: error.map(|e| e.message.clone()),
                localized: error.and_then(|e| e.localized.as_deref().cloned()),
            },
        });
    }
}

impl EventReplayer<'_> {
    fn outcome(&self, op: &QueuedOp, result: Result<(), EngineError>) -> ReplayOutcome {
        match result {
            Ok(()) => {
                self.status(op, OpState::Done, None);
                ReplayOutcome::Done
            }
            Err(err) if err.retryable && op.attempts < self.policy.max_attempts => {
                tracing::debug!(op = %op.id, %err, "write deferred");
                self.status(op, OpState::Pending, Some(&err));
                ReplayOutcome::Retry(err.to_string())
            }
            Err(err) => {
                tracing::warn!(op = %op.id, %err, "write failed");
                self.status(op, OpState::Failed, Some(&err));
                ReplayOutcome::Fail(err.to_string())
            }
        }
    }
}

impl Replayer for EventReplayer<'_> {
    async fn replay(&self, op: &QueuedOp) -> ReplayOutcome {
        self.status(op, OpState::InFlight, None);
        let result = self.executor.execute(op).await;
        self.outcome(op, result)
    }

    fn max_group(&self) -> usize {
        self.executor.max_group()
    }

    async fn replay_group(&self, ops: &[QueuedOp]) -> Vec<ReplayOutcome> {
        for op in ops {
            self.status(op, OpState::InFlight, None);
        }
        let results = self.executor.execute_group(ops).await;
        let mut out = Vec::with_capacity(results.len());
        for (op, result) in ops.iter().zip(results) {
            out.push(self.outcome(op, result));
        }
        // Not attempted: back to pending.
        for op in &ops[out.len()..] {
            self.status(op, OpState::Pending, None);
        }
        out
    }
}

/// Replays `account`'s queue with `executor`.
pub(crate) async fn replay_account(
    service: &Service,
    account: &AccountId,
    executor: &dyn OpExecutor,
) -> Result<ReplayReport> {
    // Folder changes first: a move may go into a folder created just now
    // (ADR 0196).
    crate::folders::replay_folder_ops(service, account, executor).await?;
    let replayer = EventReplayer {
        service,
        executor,
        policy: ReplayPolicy::default(),
    };
    let report = service
        .store()
        .replay_account(account, &replayer, replayer.policy)
        .await?;
    if !report.failed.is_empty() {
        // Failed writes: clients show the store's state again.
        service.broadcast(Event::MailChanged {
            folder_ids: Vec::new(),
        });
    }
    Ok(report)
}

/// Replays a registered (manual) IMAP account's queue on a fresh
/// connection. Failures are logged; the writes stay queued.
pub(crate) async fn replay_with_connection(
    service: &Service,
    account: &AccountId,
    settings: &ImapSettings,
) {
    let pending = {
        let account = account.clone();
        service
            .store()
            .call(move |s| s.has_pending_writes(&account))
            .await
    };
    if !matches!(pending, Ok(true)) {
        return;
    }
    let client = match connect(settings).await {
        Ok(client) => client,
        Err(err) => {
            tracing::info!(%account, %err, "cannot connect to replay writes; keeping them queued");
            return;
        }
    };
    let client = tokio::sync::Mutex::new(client);
    let executor = ImapExecutor {
        service,
        client: &client,
    };
    if let Err(err) = replay_account(service, account, &executor).await {
        tracing::warn!(%account, %err, "replaying writes failed");
    }
    drop(client.into_inner().logout().await);
}

/// Replays an account without a server (local, POP3).
pub(crate) async fn replay_locally(service: &Service, account: &AccountId) {
    if let Err(err) = replay_account(service, account, &LocalExecutor { service }).await {
        tracing::warn!(%account, %err, "replaying local writes failed");
    }
}

fn engine_error(err: CoreError) -> EngineError {
    err.into()
}

fn store_error(err: nail_store::Error) -> EngineError {
    engine_error(err.into())
}

async fn folder(service: &Service, id: &nail_api::FolderId) -> Result<Folder, EngineError> {
    let id = id.clone();
    service
        .store()
        .call(move |s| s.get_folder(&id))
        .await
        .map_err(store_error)
}

/// Where a message is now according to its local row (folder, UID with
/// the folder's stored UIDVALIDITY, Message-ID); `None` = gone locally.
async fn row_location(
    service: &Service,
    message: &MessageId,
) -> Result<Option<(Folder, MessageSource)>, EngineError> {
    let id = message.clone();
    service
        .store()
        .call(move |s| {
            let Some(m) = s.find_message(&id)? else {
                return Ok(None);
            };
            let folder = s.get_folder(&m.summary.folder_id)?;
            let uid_validity = match m.uid {
                Some(_) => s.folder_sync_state(&folder.id)?.uidvalidity,
                None => None,
            };
            let header_message_id = s.message_meta(&id)?.message_id;
            let source = MessageSource {
                account_id: m.summary.account_id,
                folder_id: folder.id.clone(),
                uid: m.uid,
                uid_validity,
                header_message_id,
                remote_id: None,
            };
            Ok(Some((folder, source)))
        })
        .await
        .map_err(store_error)
}

/// Where the message of `op` is on the server when `op` runs: the source
/// recorded by the next waiting move / delete of the same message (the
/// local row may already be further along), else its local row.
async fn server_location(
    service: &Service,
    op: &QueuedOp,
) -> Result<Option<(Folder, MessageSource)>, EngineError> {
    let message = op.operation.message_id().clone();
    let (id, after) = (message.clone(), op.id);
    let next = service
        .store()
        .call(move |s| {
            Ok(s.pending_ops_for_message(&id)?
                .into_iter()
                .filter(|o| o.id > after)
                .find_map(|o| o.operation.source().cloned()))
        })
        .await
        .map_err(store_error)?;
    match next {
        Some(source) => Ok(Some((folder(service, &source.folder_id).await?, source))),
        None => row_location(service, &message).await,
    }
}

/// The stored UIDVALIDITY of a folder (from its last sync).
async fn stored_uid_validity(service: &Service, folder: &Folder) -> Option<u32> {
    let id = folder.id.clone();
    service
        .store()
        .call(move |s| s.folder_sync_state(&id))
        .await
        .ok()
        .and_then(|state| state.uidvalidity)
}

/// Records that `op` put its message into `folder` with `uid`: the local
/// row learns the UID when it is still there without one, and later
/// queued operations that start from `folder` learn it too (the row may
/// have moved on or be gone already).
async fn placed(
    service: &Service,
    op: &QueuedOp,
    folder: &Folder,
    uid: Option<u32>,
    uid_validity: Option<u32>,
) {
    let Some(uid) = uid else { return };
    let id = op.operation.message_id().clone();
    let target = folder.id.clone();
    let after = op.id;
    let result = service
        .store()
        .call(move |s| {
            if let Ok(m) = s.get_message(&id)
                && m.summary.folder_id == target
                && m.uid.is_none()
                && s.find_message_by_uid(&target, uid)?.is_none()
            {
                s.set_message_uid(&id, Some(uid))?;
            }
            s.rebase_later_ops(&id, after, &target, uid, uid_validity)?;
            Ok(())
        })
        .await;
    if let Err(err) = result {
        tracing::debug!(%err, "cannot record where the message went");
    }
}

/// When a message cannot be found on the server: fine (nothing to do)
/// unless an earlier operation of another account, which may still bring
/// it there, is waiting; then `op` has to wait too.
async fn not_on_server(
    service: &Service,
    op: &QueuedOp,
) -> Result<Option<(u32, u32)>, EngineError> {
    let id = op.operation.message_id().clone();
    let (before, account) = (op.id, op.account_id.clone());
    let waiting = service
        .store()
        .call(move |s| {
            Ok(s.pending_ops_for_message(&id)?
                .iter()
                .any(|o| o.id < before && o.account_id != account))
        })
        .await
        .map_err(store_error)?;
    if waiting {
        return Err(EngineError::temporary(
            ErrorCode::Unavailable,
            format!(
                "message {} waits for an earlier operation of another account",
                op.operation.message_id()
            ),
        ));
    }
    Ok(None)
}

/// The results of a group so far: one slot per operation.
pub(crate) type GroupSlots = Vec<Option<Result<(), EngineError>>>;

/// Empty slots for a group of `n` operations.
pub(crate) fn group_slots(n: usize) -> GroupSlots {
    vec![None; n]
}

/// A group's results: an operation without one had nothing to do.
pub(crate) fn group_results(slots: GroupSlots) -> Vec<Result<(), EngineError>> {
    slots.into_iter().map(|r| r.unwrap_or(Ok(()))).collect()
}

/// How an API engine (Gmail, Exchange) replays one operation of a group
/// (P3).
pub(crate) enum Grouped<W> {
    /// Its write goes with the others.
    Write(W),
    /// Nothing to do (the message never reached the server).
    Nothing,
    /// On its own (e.g. a move into another account).
    Alone,
}

/// Sorts a group in order: operations with nothing to do are done, the
/// ones that go alone run now (`run`), errors are recorded; returns the
/// writes to send together with their operations' indexes.
pub(crate) async fn sort_group<'a, W, G, R>(
    ops: &'a [QueuedOp],
    slots: &mut GroupSlots,
    group: impl Fn(&'a QueuedOp) -> G,
    run: impl Fn(&'a QueuedOp) -> R,
) -> Vec<(usize, W)>
where
    G: Future<Output = Result<Grouped<W>, EngineError>>,
    R: Future<Output = Result<(), EngineError>>,
{
    let mut writes = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        match group(op).await {
            Ok(Grouped::Write(write)) => writes.push((index, write)),
            Ok(Grouped::Nothing) => slots[index] = Some(Ok(())),
            Ok(Grouped::Alone) => slots[index] = Some(run(op).await),
            Err(err) => slots[index] = Some(Err(err)),
        }
    }
    writes
}

/// The remote id an operation recorded for its message, else what
/// `fallback` finds (the row's).
pub(crate) async fn recorded_or(
    source: Option<&MessageSource>,
    fallback: impl Future<Output = Result<Option<String>, EngineError>>,
) -> Result<Option<String>, EngineError> {
    match source.and_then(|s| s.remote_id.clone()) {
        Some(remote) => Ok(Some(remote)),
        None => fallback.await,
    }
}

/// The raw bytes and flags of a stored message for an API engine's
/// append; `None` when the row or its blob is gone.
pub(crate) async fn stored_raw(
    service: &Service,
    message: &MessageId,
) -> Result<Option<(Vec<u8>, nail_api::Flags)>, EngineError> {
    let id = message.clone();
    let blobs = service.blobs().clone();
    service
        .store()
        .call(move |s| {
            let Some(m) = s.find_message(&id)? else {
                return Ok(None);
            };
            let raw = match m.blob_hash {
                Some(hash) => blobs.get(&hash).ok(),
                None => None,
            };
            Ok(raw.map(|raw| (raw, m.summary.flags)))
        })
        .await
        .map_err(store_error)
}

/// The raw bytes and flags of a local message, for `APPEND`; `None` when
/// the row is gone.
async fn raw_message(
    service: &Service,
    message: &MessageId,
) -> Result<Option<(Vec<u8>, Vec<Flag>)>, EngineError> {
    let id = message.clone();
    let blobs = service.blobs().clone();
    let found = service
        .store()
        .call(move |s| {
            let Some(m) = s.find_message(&id)? else {
                return Ok(None);
            };
            let raw = match &m.blob_hash {
                Some(hash) if blobs.contains(hash) => blobs.get(hash)?,
                _ => Vec::new(),
            };
            Ok(Some((raw, m.summary.flags)))
        })
        .await
        .map_err(store_error)?;
    let Some((raw, flags)) = found else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Err(EngineError::permanent(
            ErrorCode::Unavailable,
            format!("message {message} is not downloaded; it cannot be copied"),
        ));
    }
    let mut imap_flags = Vec::new();
    for (on, flag) in [
        (flags.seen, Flag::Seen),
        (flags.flagged, Flag::Flagged),
        (flags.answered, Flag::Answered),
    ] {
        if on {
            imap_flags.push(flag);
        }
    }
    Ok(Some((raw, imap_flags)))
}

/// Files a local message in `target` on `client` (`APPEND`) and returns
/// the new UID and the folder's UIDVALIDITY when known.
///
/// Idempotent across retries: when an earlier attempt of `op` may have
/// filed it already (the process stopped, or a later step failed before
/// the operation was updated), the folder is searched for its Message-ID
/// first, and a copy that is there is not appended again.
async fn append_once(
    client: &mut ImapClient,
    op: &QueuedOp,
    target: &Folder,
    header_message_id: Option<&str>,
    raw: &[u8],
    flags: &[Flag],
) -> Result<(Option<u32>, Option<u32>), EngineError> {
    if op.attempts > 1
        && let Some(header) = header_message_id
    {
        let found = client.find_by_message_id(&target.path, header).await?;
        if let Some(uid) = found.first() {
            let status = client.select_for_write(&target.path).await?;
            tracing::debug!(op = %op.id, uid, "already filed by an earlier attempt");
            return Ok((Some(*uid), Some(status.uid_validity)));
        }
    }
    let result = client.append(&target.path, raw, flags, None).await?;
    Ok((result.uid, result.uid_validity))
}

/// Appends a message's raw bytes to a folder of another account (a
/// cross-account move) on a connection of its own. Accounts without an
/// IMAP server keep the local copy only. Returns the new UID and
/// UIDVALIDITY when known.
async fn append_to_account(
    service: &Service,
    op: &QueuedOp,
    target: &Folder,
    header_message_id: Option<&str>,
) -> Result<(Option<u32>, Option<u32>), EngineError> {
    let Some(settings) = service.imap_settings(&target.account_id) else {
        // M4c: into an Exchange or Gmail account through its engine.
        if let Some(api) = crate::api_moves::api_target(service, &target.id).await? {
            let message = op.operation.message_id();
            let Some((raw, _)) = raw_message(service, message).await? else {
                return Err(EngineError::permanent(
                    ErrorCode::NotFound,
                    format!("message {message} is gone locally; it cannot be copied"),
                ));
            };
            let id = message.clone();
            let flags = service
                .store()
                .call(move |s| Ok(s.get_message(&id)?.summary.flags))
                .await
                .map_err(store_error)?;
            if let Some(remote) = crate::api_moves::append_raw(&api, raw, flags).await? {
                let id = message.clone();
                drop(
                    service
                        .store()
                        .call(move |s| s.set_message_remote_id(&id, Some(&remote)))
                        .await,
                );
            }
        }
        return Ok((None, None));
    };
    let message = op.operation.message_id();
    let Some((raw, flags)) = raw_message(service, message).await? else {
        return Err(EngineError::permanent(
            ErrorCode::NotFound,
            format!("message {message} is gone locally; it cannot be copied"),
        ));
    };
    let mut client = connect(&settings).await?;
    let result = append_once(&mut client, op, target, header_message_id, &raw, &flags).await;
    drop(client.logout().await);
    result
}

/// What remains of a cross-account move once the copy is filed in the
/// target: removing the message from the source. Stored in place of the
/// move, so a retry does not file the copy again.
fn remaining_expunge(op: &QueuedOp, source: &MessageSource) -> Operation {
    Operation::DeleteMessage {
        message_id: op.operation.message_id().clone(),
        permanent: true,
        source: Some(source.clone()),
        trash_folder_id: None,
    }
}

async fn record_remaining(service: &Service, op: &QueuedOp, rest: Operation) {
    let id = op.id;
    if let Err(err) = service.store().call(move |s| s.update_op(id, &rest)).await {
        tracing::warn!(op = %op.id, %err, "cannot record the filed copy of a move");
    }
}

/// The IMAP executor, on an engine's connection.
pub(crate) struct ImapExecutor<'a> {
    pub service: &'a Service,
    pub client: &'a tokio::sync::Mutex<ImapClient>,
}

impl ImapExecutor<'_> {
    /// The UID to act on in `folder` and the folder's current UIDVALIDITY;
    /// `None` when the message is not (or no longer) there.
    ///
    /// A recorded UID is used only while the folder's UIDVALIDITY is still
    /// the one it was recorded with. Otherwise (the server renumbered the
    /// folder), or when the UID was never known, the message is found
    /// again by its Message-ID; a stale UID is never acted on.
    async fn resolve(
        &self,
        op: &QueuedOp,
        folder: &Folder,
        source: &MessageSource,
    ) -> Result<Option<(u32, u32)>, EngineError> {
        let status = self
            .client
            .lock()
            .await
            .select_for_write(&folder.path)
            .await?;
        let current = status.uid_validity;
        let expected = match source.uid_validity {
            Some(v) => Some(v),
            None => stored_uid_validity(self.service, folder).await,
        };
        if let Some(uid) = source.uid {
            match expected {
                Some(v) if v == current => return Ok(Some((uid, current))),
                None if source.header_message_id.is_none() => return Ok(Some((uid, current))),
                _ => tracing::info!(
                    op = %op.id,
                    folder = %folder.path,
                    ?expected,
                    current,
                    "UIDVALIDITY changed; finding the message by its Message-ID"
                ),
            }
        }
        let Some(header) = source.header_message_id.as_deref() else {
            if source.uid.is_some() {
                return Err(EngineError::permanent(
                    ErrorCode::Conflict,
                    format!(
                        "{} was renumbered on the server and message {} has no Message-ID",
                        folder.path,
                        op.operation.message_id()
                    ),
                ));
            }
            return not_on_server(self.service, op).await;
        };
        let found = self
            .client
            .lock()
            .await
            .find_by_message_id(&folder.path, header)
            .await?;
        match found.as_slice() {
            [uid] => Ok(Some((*uid, current))),
            [] => not_on_server(self.service, op).await,
            _ => Err(EngineError::permanent(
                ErrorCode::Conflict,
                format!(
                    "{} holds {} messages with the Message-ID of message {}",
                    folder.path,
                    found.len(),
                    op.operation.message_id()
                ),
            )),
        }
    }

    /// Creates the hidden Snoozed folder (M2c) on the server when `target`
    /// is it: the first move into it creates it.
    async fn ensure_snoozed_folder(&self, target: &Folder) -> Result<(), EngineError> {
        let snoozed = {
            let target = target.clone();
            self.service
                .store()
                .call(move |s| Ok(crate::junk::is_snoozed_folder(s, &target)))
                .await
                .unwrap_or(false)
        };
        if !snoozed {
            return Ok(());
        }
        match self.client.lock().await.create_folder(&target.path).await {
            Ok(()) => tracing::info!(folder = %target.path, "created the Snoozed folder"),
            Err(err) if err.code() == ErrorCode::Conflict => {}
            Err(err) => return Err(err.into()),
        }
        Ok(())
    }

    async fn update_flags(
        &self,
        op: &QueuedOp,
        change: &nail_store::FlagChange,
    ) -> Result<(), EngineError> {
        let Some((folder, source)) = server_location(self.service, op).await? else {
            return Ok(());
        };
        if folder.account_id != op.account_id {
            // Moved into another account meanwhile: the copy filed there
            // carries the flags.
            return Ok(());
        }
        let Some((uid, _)) = self.resolve(op, &folder, &source).await? else {
            return Ok(());
        };
        self.client
            .lock()
            .await
            .apply_flag_change(&folder.path, &[uid], change)
            .await?;
        Ok(())
    }

    async fn move_message(
        &self,
        op: &QueuedOp,
        source: &MessageSource,
        target_id: &nail_api::FolderId,
    ) -> Result<(), EngineError> {
        let from = folder(self.service, &source.folder_id).await?;
        let Some((uid, uid_validity)) = self.resolve(op, &from, source).await? else {
            // Never reached the server (or already moved by an earlier
            // attempt).
            return Ok(());
        };
        let target = folder(self.service, target_id).await?;
        if target.account_id == from.account_id {
            self.ensure_snoozed_folder(&target).await?;
            let result = self
                .client
                .lock()
                .await
                .move_messages(&from.path, &[uid], &target.path)
                .await?;
            let new_uid = result
                .copy
                .mapping
                .iter()
                .find(|(old, _)| *old == uid)
                .map(|(_, new)| *new);
            placed(self.service, op, &target, new_uid, result.copy.uid_validity).await;
            return Ok(());
        }
        let (new_uid, new_validity) = append_to_account(
            self.service,
            op,
            &target,
            source.header_message_id.as_deref(),
        )
        .await?;
        placed(self.service, op, &target, new_uid, new_validity).await;
        let exact = MessageSource {
            uid: Some(uid),
            uid_validity: Some(uid_validity),
            ..source.clone()
        };
        record_remaining(self.service, op, remaining_expunge(op, &exact)).await;
        self.client
            .lock()
            .await
            .expunge_messages(&from.path, &[uid])
            .await?;
        Ok(())
    }

    async fn delete_message(
        &self,
        op: &QueuedOp,
        source: &MessageSource,
        trash: Option<&nail_api::FolderId>,
    ) -> Result<(), EngineError> {
        if let Some(trash) = trash {
            return self.move_message(op, source, trash).await;
        }
        let from = folder(self.service, &source.folder_id).await?;
        let Some((uid, _)) = self.resolve(op, &from, source).await? else {
            return Ok(());
        };
        self.client
            .lock()
            .await
            .expunge_messages(&from.path, &[uid])
            .await?;
        Ok(())
    }

    /// Files a local message (a Sent copy) in its folder on this account's
    /// server.
    async fn append_message(
        &self,
        op: &QueuedOp,
        folder_id: &nail_api::FolderId,
    ) -> Result<(), EngineError> {
        let message = op.operation.message_id();
        let Some((raw, flags)) = raw_message(self.service, message).await? else {
            // Deleted locally before it was filed: nothing to file.
            return Ok(());
        };
        let target = folder(self.service, folder_id).await?;
        let header = {
            let id = message.clone();
            self.service
                .store()
                .call(move |s| s.message_meta(&id))
                .await
                .map_err(store_error)?
                .message_id
        };
        let (uid, uid_validity) = {
            let mut client = self.client.lock().await;
            append_once(&mut client, op, &target, header.as_deref(), &raw, &flags).await?
        };
        placed(self.service, op, &target, uid, uid_validity).await;
        Ok(())
    }
}

/// Most operations an IMAP replay sends as one UID set (the client splits
/// long sets into chunks itself).
const IMAP_GROUP: usize = 500;

impl ImapExecutor<'_> {
    /// The UIDs of `ops` in `from` (each op's own [`MessageSource`]):
    /// `(index, uid)` for the messages found; the others are done (not on
    /// the server) or answer their error.
    async fn resolve_all(
        &self,
        ops: &[QueuedOp],
        from: &Folder,
        out: &mut [Option<Result<(), EngineError>>],
    ) -> Vec<(usize, u32, u32)> {
        let mut found = Vec::new();
        for (index, op) in ops.iter().enumerate() {
            let Some(source) = op.operation.source() else {
                out[index] = Some(Err(EngineError::permanent(
                    ErrorCode::Internal,
                    format!("cannot replay {} without its source", op.operation.kind()),
                )));
                continue;
            };
            match self.resolve(op, from, source).await {
                Ok(Some((uid, validity))) => found.push((index, uid, validity)),
                Ok(None) => out[index] = Some(Ok(())),
                Err(err) => out[index] = Some(Err(err)),
            }
        }
        found
    }

    /// Flag changes of a group: one `UID STORE` per folder.
    async fn flags_group(
        &self,
        ops: &[QueuedOp],
        change: &nail_store::FlagChange,
    ) -> Vec<Result<(), EngineError>> {
        let mut out = group_slots(ops.len());
        let mut by_folder: Vec<(Folder, Vec<(usize, u32)>)> = Vec::new();
        for (index, op) in ops.iter().enumerate() {
            let located = match server_location(self.service, op).await {
                Ok(Some((folder, source))) if folder.account_id == op.account_id => self
                    .resolve(op, &folder, &source)
                    .await
                    .map(|uid| uid.map(|(uid, _)| (folder, uid))),
                Ok(_) => Ok(None),
                Err(err) => Err(err),
            };
            match located {
                Ok(Some((folder, uid))) => {
                    nail_util::group::push_by(&mut by_folder, folder, (index, uid), |a, b| {
                        a.id == b.id
                    });
                }
                Ok(None) => out[index] = Some(Ok(())),
                Err(err) => out[index] = Some(Err(err)),
            }
        }
        for (folder, members) in by_folder {
            let uids: Vec<u32> = members.iter().map(|(_, uid)| *uid).collect();
            let result = self
                .client
                .lock()
                .await
                .apply_flag_change(&folder.path, &uids, change)
                .await
                .map(drop)
                .map_err(EngineError::from);
            for (index, _) in members {
                out[index] = Some(result.clone());
            }
        }
        group_results(out)
    }

    /// Moves of a group within the account (deletes to Trash too): one
    /// `UID MOVE` (or COPY + expunge) with the UID set.
    async fn move_group(
        &self,
        ops: &[QueuedOp],
        from_id: &nail_api::FolderId,
        target_id: &nail_api::FolderId,
    ) -> Vec<Result<(), EngineError>> {
        let folders = async {
            Ok::<_, EngineError>((
                folder(self.service, from_id).await?,
                folder(self.service, target_id).await?,
            ))
        };
        let (from, target) = match folders.await {
            Ok(found) => found,
            Err(err) => return vec![Err(err); ops.len()],
        };
        if target.account_id != from.account_id {
            // Into another account: copy by copy.
            return one_by_one(self, ops).await;
        }
        let mut out = group_slots(ops.len());
        let found = self.resolve_all(ops, &from, &mut out).await;
        if !found.is_empty() {
            let mut result = self.ensure_snoozed_folder(&target).await;
            if result.is_ok() {
                let uids: Vec<u32> = found.iter().map(|(_, uid, _)| *uid).collect();
                let moved = self
                    .client
                    .lock()
                    .await
                    .move_messages(&from.path, &uids, &target.path)
                    .await;
                match moved {
                    Ok(moved) => {
                        for (index, uid, _) in &found {
                            let new_uid = moved
                                .copy
                                .mapping
                                .iter()
                                .find(|(old, _)| old == uid)
                                .map(|(_, new)| *new);
                            placed(
                                self.service,
                                &ops[*index],
                                &target,
                                new_uid,
                                moved.copy.uid_validity,
                            )
                            .await;
                        }
                    }
                    Err(err) => result = Err(err.into()),
                }
            }
            for (index, _, _) in &found {
                out[*index] = Some(result.clone());
            }
        }
        group_results(out)
    }

    /// Permanent deletes of a group: one `\Deleted` + expunge of the set.
    async fn expunge_group(
        &self,
        ops: &[QueuedOp],
        from_id: &nail_api::FolderId,
    ) -> Vec<Result<(), EngineError>> {
        let from = match folder(self.service, from_id).await {
            Ok(from) => from,
            Err(err) => return vec![Err(err); ops.len()],
        };
        let mut out = group_slots(ops.len());
        let found = self.resolve_all(ops, &from, &mut out).await;
        if !found.is_empty() {
            let uids: Vec<u32> = found.iter().map(|(_, uid, _)| *uid).collect();
            let result = self
                .client
                .lock()
                .await
                .expunge_messages(&from.path, &uids)
                .await
                .map(drop)
                .map_err(EngineError::from);
            for (index, _, _) in &found {
                out[*index] = Some(result.clone());
            }
        }
        group_results(out)
    }
}

impl ImapExecutor<'_> {
    /// The hierarchy separator below `parent`: the stored one, else the
    /// one the server lists.
    async fn delimiter(
        &self,
        account: &AccountId,
        parent: Option<&Folder>,
    ) -> Result<Option<String>, EngineError> {
        let stored = {
            let (account, parent) = (account.clone(), parent.map(|p| p.id.clone()));
            self.service
                .store()
                .call(move |s| s.folder_delimiter(&account, parent.as_ref()))
                .await
                .map_err(store_error)?
        };
        if stored.is_some() {
            return Ok(stored);
        }
        let listed = self.client.lock().await.list_folders().await?;
        Ok(listed.into_iter().find_map(|f| f.delimiter))
    }

    /// Creates, renames / moves or deletes a mailbox (ADR 0196).
    async fn change_folder(&self, change: &FolderChange) -> Result<FolderApplied, EngineError> {
        match change {
            FolderChange::Create {
                account,
                parent,
                name,
            } => {
                let delimiter = self.delimiter(account, parent.as_ref()).await?;
                let path = server_path(true, parent.as_ref(), delimiter.as_deref(), name)?;
                let mut client = self.client.lock().await;
                client.create_folder(&path).await?;
                // Other mail programs often list subscribed folders only.
                if let Err(err) = client.set_subscribed(&path, true).await {
                    tracing::debug!(%err, "cannot subscribe to a new folder");
                }
                Ok(FolderApplied::Path { path, delimiter })
            }
            FolderChange::Update {
                folder,
                parent,
                name,
            } => {
                let delimiter = self.delimiter(&folder.account_id, Some(folder)).await?;
                let path = server_path(true, parent.as_ref(), delimiter.as_deref(), name)?;
                if path != folder.path {
                    let mut client = self.client.lock().await;
                    client.rename_folder(&folder.path, &path).await?;
                    if let Err(err) = client.set_subscribed(&path, true).await {
                        tracing::debug!(%err, "cannot subscribe to a renamed folder");
                    }
                }
                Ok(FolderApplied::Path { path, delimiter })
            }
            FolderChange::Delete { folder, subfolders } => {
                let mut client = self.client.lock().await;
                for f in subfolders.iter().chain(std::iter::once(folder)) {
                    match client.delete_folder(&f.path).await {
                        Ok(()) => {}
                        // Gone already (deleted elsewhere, or with its parent).
                        Err(err) if err.code() == ErrorCode::NotFound => {}
                        Err(err) => return Err(err.into()),
                    }
                    if let Err(err) = client.set_subscribed(&f.path, false).await {
                        tracing::debug!(%err, "cannot unsubscribe from a deleted folder");
                    }
                }
                Ok(FolderApplied::Deleted)
            }
        }
    }
}

impl OpExecutor for ImapExecutor<'_> {
    fn max_group(&self) -> usize {
        IMAP_GROUP
    }

    fn folder_change<'a>(
        &'a self,
        change: &'a FolderChange,
    ) -> BoxFuture<'a, Result<FolderApplied, EngineError>> {
        Box::pin(self.change_folder(change))
    }

    fn execute_group<'a>(
        &'a self,
        ops: &'a [QueuedOp],
    ) -> BoxFuture<'a, Vec<Result<(), EngineError>>> {
        Box::pin(async move {
            let Some(first) = ops.first() else {
                return Vec::new();
            };
            match &first.operation {
                Operation::UpdateFlags { change, .. } => self.flags_group(ops, change).await,
                Operation::MoveMessage {
                    target_folder_id,
                    source: Some(source),
                    ..
                } => {
                    self.move_group(ops, &source.folder_id, target_folder_id)
                        .await
                }
                Operation::DeleteMessage {
                    source: Some(source),
                    trash_folder_id: Some(trash),
                    ..
                } => self.move_group(ops, &source.folder_id, trash).await,
                Operation::DeleteMessage {
                    source: Some(source),
                    trash_folder_id: None,
                    ..
                } => self.expunge_group(ops, &source.folder_id).await,
                _ => one_by_one(self, ops).await,
            }
        })
    }

    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            match &op.operation {
                Operation::UpdateFlags { change, .. } => self.update_flags(op, change).await,
                Operation::MoveMessage {
                    target_folder_id,
                    source: Some(source),
                    ..
                } => self.move_message(op, source, target_folder_id).await,
                Operation::DeleteMessage {
                    source: Some(source),
                    trash_folder_id,
                    ..
                } => {
                    self.delete_message(op, source, trash_folder_id.as_ref())
                        .await
                }
                Operation::AppendMessage { folder_id, .. } => {
                    self.append_message(op, folder_id).await
                }
                other => Err(EngineError::permanent(
                    ErrorCode::Internal,
                    format!("cannot replay {} without its source", other.kind()),
                )),
            }
        })
    }
}

/// The executor of accounts without an IMAP server.
pub(crate) struct LocalExecutor<'a> {
    pub service: &'a Service,
}

impl OpExecutor for LocalExecutor<'_> {
    fn folder_change<'a>(
        &'a self,
        change: &'a FolderChange,
    ) -> BoxFuture<'a, Result<FolderApplied, EngineError>> {
        Box::pin(async move {
            // Nothing on a server: the store follows at once.
            match change {
                FolderChange::Create { parent, name, .. }
                | FolderChange::Update { parent, name, .. } => Ok(FolderApplied::Path {
                    path: server_path(false, parent.as_ref(), None, name)?,
                    delimiter: Some(crate::folders::LOCAL_DELIMITER.to_owned()),
                }),
                FolderChange::Delete { .. } => Ok(FolderApplied::Deleted),
            }
        })
    }

    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            match &op.operation {
                Operation::DeleteMessage {
                    message_id,
                    trash_folder_id: None,
                    ..
                } => {
                    let id = message_id.clone();
                    self.service
                        .store()
                        .call(move |s| s.pop3_mark_deleted(&id))
                        .await
                        .map_err(store_error)?;
                    Ok(())
                }
                Operation::MoveMessage {
                    message_id,
                    target_folder_id,
                    source: Some(source),
                } => {
                    let target = folder(self.service, target_folder_id).await?;
                    if target.account_id != source.account_id {
                        let (uid, uid_validity) = append_to_account(
                            self.service,
                            op,
                            &target,
                            source.header_message_id.as_deref(),
                        )
                        .await?;
                        placed(self.service, op, &target, uid, uid_validity).await;
                        // The copy is filed; what remains is the local
                        // delete (for POP3 remembered for its policy).
                        record_remaining(self.service, op, remaining_expunge(op, source)).await;
                        let id = message_id.clone();
                        self.service
                            .store()
                            .call(move |s| s.pop3_mark_deleted(&id))
                            .await
                            .map_err(store_error)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        })
    }
}

impl Service {
    /// The IMAP settings of a configured or registered account.
    pub(crate) fn imap_settings(&self, account: &AccountId) -> Option<ImapSettings> {
        if let Some(settings) = self.inner.manual.settings(account) {
            return Some(settings.into());
        }
        let config = self.inner.accounts.config().account(account)?.clone();
        match &config.incoming {
            IncomingServer::Imap { server, options } => Some(ImapSettings {
                config: crate::creds::imap_config(server, options)
                    .with_tap(self.account_tap(&config.id)),
                credentials: Arc::new(self.endpoint_credentials(&config.id, &server.auth)),
            }),
            IncomingServer::Pop3 { .. }
            | IncomingServer::Gmail { .. }
            | IncomingServer::Exchange { .. } => None,
        }
    }

    /// Stored credentials of an endpoint of a configured account.
    pub(crate) fn endpoint_credentials(
        &self,
        account: &AccountId,
        auth: &nail_api::AuthConfig,
    ) -> crate::creds::EndpointCredentials {
        crate::creds::EndpointCredentials {
            account: account.clone(),
            auth: auth.clone(),
            source: crate::creds::SecretSource::Stored {
                secrets: Arc::clone(&self.inner.secrets),
                tokens: self.inner.tokens.clone(),
            },
        }
    }
}
