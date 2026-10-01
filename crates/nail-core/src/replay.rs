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
use crate::ids;
use crate::service::Service;
use crate::sync::{ImapSettings, connect};

/// Performs one queued operation.
pub(crate) trait OpExecutor: Send + Sync {
    /// `Err` with `retryable` keeps the operation queued.
    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>>;
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

impl Replayer for EventReplayer<'_> {
    async fn replay(&self, op: &QueuedOp) -> ReplayOutcome {
        self.status(op, OpState::InFlight, None);
        match self.executor.execute(op).await {
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

/// Replays `account`'s queue with `executor`.
pub(crate) async fn replay_account(
    service: &Service,
    account: &AccountId,
    executor: &dyn OpExecutor,
) -> Result<ReplayReport> {
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
            .call(move |s| Ok(s.accounts_with_pending_ops()?.contains(&account)))
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
    let _ = client.into_inner().logout().await;
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
            let m = match s.get_message(&id) {
                Ok(m) => m,
                Err(err) if err.is_not_found() => return Ok(None),
                Err(err) => return Err(err),
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
            let m = match s.get_message(&id) {
                Ok(m) => m,
                Err(err) if err.is_not_found() => return Ok(None),
                Err(err) => return Err(err),
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
    let _ = client.logout().await;
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
        let api_change = nail_api::FlagChange {
            seen: change.seen,
            flagged: change.flagged,
            answered: change.answered,
            draft: change.draft,
            add_tags: change.add_tags.clone(),
            remove_tags: change.remove_tags.clone(),
        };
        self.client
            .lock()
            .await
            .apply_flag_change(&folder.path, &[uid], &api_change)
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
            let snoozed = {
                let target = target.clone();
                self.service
                    .store()
                    .call(move |s| Ok(crate::junk::is_snoozed_folder(s, &target)))
                    .await
                    .unwrap_or(false)
            };
            if snoozed {
                // The hidden Snoozed folder (M2c) is created by the first
                // move into it.
                let created = self.client.lock().await.create_folder(&target.path).await;
                match created {
                    Ok(()) => tracing::info!(folder = %target.path, "created the Snoozed folder"),
                    Err(err) if err.code() == ErrorCode::Conflict => {}
                    Err(err) => return Err(err.into()),
                }
            }
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

impl OpExecutor for ImapExecutor<'_> {
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
                config: crate::creds::imap_config(server, options),
                credentials: Arc::new(self.endpoint_credentials(&config.id, &server.auth)),
            }),
            IncomingServer::Pop3 { .. } | IncomingServer::Gmail { .. } => None,
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
                tokens: Arc::clone(&self.inner.tokens)
                    as Arc<dyn nail_api::engine::AccessTokenProvider>,
            },
        }
    }
}
