//! Replaying queued writes (ADR 0007 / 0015): the store's per-account
//! queue drives an executor per protocol, and every step is broadcast as
//! [`Event::OpStatus`].
//!
//! * IMAP ([`ImapExecutor`]): flags with `UID STORE`, moves with `MOVE` (or
//!   `COPY` + `\Deleted` + expunge), deletes to Trash as moves, permanent
//!   deletes as `\Deleted` + expunge. The new UID from `COPYUID` is stored
//!   so the target folder's next sync finds the moved row. Moves into
//!   another account append the raw message there and remove it from the
//!   source.
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

/// Where a message is now: its folder and UID (`None` = gone locally).
async fn location(
    service: &Service,
    message: &MessageId,
) -> Result<Option<(Folder, Option<u32>)>, EngineError> {
    let id = message.clone();
    service
        .store()
        .call(move |s| match s.get_message(&id) {
            Ok(m) => Ok(Some((s.get_folder(&m.summary.folder_id)?, m.uid))),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(err),
        })
        .await
        .map_err(|e| engine_error(e.into()))
}

async fn folder(service: &Service, id: &nail_api::FolderId) -> Result<Folder, EngineError> {
    let id = id.clone();
    service
        .store()
        .call(move |s| s.get_folder(&id))
        .await
        .map_err(|e| engine_error(e.into()))
}

/// The UID a queued move / delete acts on: the one recorded at enqueue
/// time, or (for a message moved again before the first move reached the
/// server) the one the first replay learned.
async fn source_uid(service: &Service, message: &MessageId, source: &MessageSource) -> Option<u32> {
    if source.uid.is_some() {
        return source.uid;
    }
    location(service, message)
        .await
        .ok()
        .flatten()
        .and_then(|(_, uid)| uid)
}

/// Records the UID a moved message got in its new folder, when the row is
/// still there.
async fn learn_uid(service: &Service, message: &MessageId, target: &Folder, uid: Option<u32>) {
    let Some(uid) = uid else { return };
    let id = message.clone();
    let target = target.id.clone();
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
            Ok(())
        })
        .await;
    if let Err(err) = result {
        tracing::debug!(%err, "cannot record the moved message's UID");
    }
}

/// Appends a message's raw bytes to a folder of another account (a
/// cross-account move). Accounts without an IMAP server keep the local
/// copy only. Returns the new UID when known.
async fn append_to_account(
    service: &Service,
    message: &MessageId,
    target: &Folder,
) -> Result<Option<u32>, EngineError> {
    let settings = match service.imap_settings(&target.account_id) {
        Some(settings) => settings,
        None => return Ok(None),
    };
    let id = message.clone();
    let blobs = service.blobs().clone();
    let (raw, flags) = service
        .store()
        .call(move |s| {
            let m = s.get_message(&id)?;
            let raw = match &m.blob_hash {
                Some(hash) => blobs.get(hash)?,
                None => Vec::new(),
            };
            Ok((raw, m.summary.flags))
        })
        .await
        .map_err(|e| engine_error(e.into()))?;
    if raw.is_empty() {
        return Err(EngineError::permanent(
            ErrorCode::Unavailable,
            format!("message {message} is not downloaded; it cannot be copied"),
        ));
    }
    let mut client = connect(&settings).await?;
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
    let result = client.append(&target.path, &raw, &imap_flags, None).await?;
    let _ = client.logout().await;
    Ok(result.uid)
}

/// The IMAP executor, on an engine's connection.
pub(crate) struct ImapExecutor<'a> {
    pub service: &'a Service,
    pub client: &'a tokio::sync::Mutex<ImapClient>,
}

impl ImapExecutor<'_> {
    async fn update_flags(
        &self,
        message: &MessageId,
        change: &nail_store::FlagChange,
    ) -> Result<(), EngineError> {
        let Some((folder, uid)) = location(self.service, message).await? else {
            return Ok(());
        };
        let Some(uid) = uid else {
            return Err(EngineError::temporary(
                ErrorCode::Unavailable,
                format!("the UID of message {message} is not known yet"),
            ));
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
        message: &MessageId,
        source: &MessageSource,
        target_id: &nail_api::FolderId,
    ) -> Result<(), EngineError> {
        let Some(uid) = source_uid(self.service, message, source).await else {
            // Never reached the server (or a local-only message).
            return Ok(());
        };
        let from = folder(self.service, &source.folder_id).await?;
        let target = folder(self.service, target_id).await?;
        if target.account_id == from.account_id {
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
            learn_uid(self.service, message, &target, new_uid).await;
            return Ok(());
        }
        let new_uid = append_to_account(self.service, message, &target).await?;
        learn_uid(self.service, message, &target, new_uid).await;
        self.client
            .lock()
            .await
            .expunge_messages(&from.path, &[uid])
            .await?;
        Ok(())
    }

    async fn delete_message(
        &self,
        message: &MessageId,
        source: &MessageSource,
        trash: Option<&nail_api::FolderId>,
    ) -> Result<(), EngineError> {
        if let Some(trash) = trash {
            return self.move_message(message, source, trash).await;
        }
        let Some(uid) = source_uid(self.service, message, source).await else {
            return Ok(());
        };
        let from = folder(self.service, &source.folder_id).await?;
        self.client
            .lock()
            .await
            .expunge_messages(&from.path, &[uid])
            .await?;
        Ok(())
    }
}

impl OpExecutor for ImapExecutor<'_> {
    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            match &op.operation {
                Operation::UpdateFlags { message_id, change } => {
                    self.update_flags(message_id, change).await
                }
                Operation::MoveMessage {
                    message_id,
                    target_folder_id,
                    source: Some(source),
                } => {
                    self.move_message(message_id, source, target_folder_id)
                        .await
                }
                Operation::DeleteMessage {
                    message_id,
                    source: Some(source),
                    trash_folder_id,
                    ..
                } => {
                    self.delete_message(message_id, source, trash_folder_id.as_ref())
                        .await
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
                        .map_err(|e| engine_error(e.into()))?;
                    Ok(())
                }
                Operation::MoveMessage {
                    message_id,
                    target_folder_id,
                    source: Some(source),
                } => {
                    let target = folder(self.service, target_folder_id).await?;
                    if target.account_id != source.account_id {
                        let new_uid = append_to_account(self.service, message_id, &target).await?;
                        learn_uid(self.service, message_id, &target, new_uid).await;
                        // The source copy left the account: for POP3 like
                        // a local delete.
                        let id = message_id.clone();
                        let _ = self
                            .service
                            .store()
                            .call(move |s| s.pop3_mark_deleted(&id))
                            .await;
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
            IncomingServer::Pop3 { .. } => None,
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
