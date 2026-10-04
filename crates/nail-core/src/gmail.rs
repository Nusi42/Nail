//! Gmail accounts (M3b, contract ADR 0065, integration ADR 0070): the label
//! requests and the account engine on `nail-proto-gmail` (ADR 0067).
//!
//! * **Sync loop** ([`run`]): the label queue and the mail operation queue
//!   are replayed first; then a full listing when no `historyId` is stored
//!   (`full_sync_page` until done, the first page's cursor stored once
//!   every page is in, an interrupted listing resumes), else
//!   `history.list` deltas until `more` is false. A cursor the server no
//!   longer knows (`CursorExpired`) means a full listing again, merged with
//!   the store (messages not listed any more are removed). The next poll
//!   follows [`PollTimer`] (adaptive interval, `Retry-After`, "Sync now").
//! * **Storage**: a Gmail message is stored once under an id derived from
//!   its Gmail id; its folder follows from its labels
//!   (`GmailFolder::for_labels`), user labels are tags, the Gmail data
//!   (thread, label ids, category) goes to `gmail_messages`. Gmail has no
//!   `\Answered`: a sync never clears the local flag.
//! * **Replay** ([`GmailExecutor`]): queued flag / tag changes, moves,
//!   deletes and appends become `RemoteWrite`s by the message's remote id;
//!   `Gone` drops the write; an append records the new remote id.
//! * **Labels**: the label queue (`gmail_label_ops`) through
//!   `create_label` / `update_label` / `delete_label`; the server's label
//!   list is applied with `replace_gmail_labels` on every pass.
//! * **Sending** goes through the engine as the account's
//!   `OutgoingTransport` (`crate::outbox`); Gmail files sent mail itself.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use nail_api::engine::{
    BoxFuture, DeltaOutcome, EngineError, FullSyncPage, MailApiEngine, RemoteMessage,
    RemoteMessageChange, RemoteWrite, SyncDelta, WriteOutcome,
};
use nail_api::gmail::{GmailFolder, GmailLabelSettings};
use nail_api::{
    AccountConfig, AccountId, AccountState, ApiError, ErrorCode, Event, Flags, Folder, FolderId,
    GmailLabelId, IncomingServer, LocalizedMessage, MessageId, Request, Response,
};
use nail_proto_gmail::{GmailEngine, GmailLabelInfo, PollTimer};
use nail_store::gmail::{GmailLabelOp, GmailLabelOpOutcome, GmailLabelRecord, GmailSyncRecord};
use nail_store::{Operation, QueuedOp, SyncedMessage};
use tokio::sync::watch;

use crate::engine::{Signals, state_for_error, stopped, unix_now};
use crate::error::{CoreError, Result};
use crate::ids;
use crate::ingest::{IncomingMessage, ingest_with_remote};
use crate::replay::{OpExecutor, replay_account};
use crate::service::Service;
use nail_store::exchange::RemoteExtras;

/// Messages fetched (raw) per store transaction during a listing.
const INGEST_CHUNK: usize = 25;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::ListGmailLabels { account_id } => {
            gmail_account(service, &account_id)?;
            let id = account_id.clone();
            let labels = service
                .store()
                .call(move |s| s.list_gmail_labels(&id))
                .await?;
            Ok(Response::GmailLabels { account_id, labels })
        }
        Request::CreateGmailLabel {
            account_id,
            settings,
        } => {
            gmail_account(service, &account_id)?;
            check_label(&settings)?;
            duplicate_check(service, &account_id, &settings, None).await?;
            let (label_id, op_id) = queue_label_op(
                service,
                &account_id,
                &GmailLabelId::new("new"),
                GmailLabelOp::Create(settings),
            )
            .await?;
            Ok(Response::GmailLabelSaved {
                label: Box::new(label(service, &account_id, &label_id).await?),
                op_ids: vec![op_id],
            })
        }
        Request::UpdateGmailLabel {
            account_id,
            label_id,
            settings,
        } => {
            gmail_account(service, &account_id)?;
            check_label(&settings)?;
            duplicate_check(service, &account_id, &settings, Some(&label_id)).await?;
            let (label_id, op_id) = queue_label_op(
                service,
                &account_id,
                &label_id,
                GmailLabelOp::Update(settings),
            )
            .await?;
            Ok(Response::GmailLabelSaved {
                label: Box::new(label(service, &account_id, &label_id).await?),
                op_ids: vec![op_id],
            })
        }
        Request::DeleteGmailLabel {
            account_id,
            label_id,
        } => {
            gmail_account(service, &account_id)?;
            queue_label_op(service, &account_id, &label_id, GmailLabelOp::Delete).await?;
            Ok(Response::Done)
        }
        other => Err(ApiError::internal(format!(
            "{} routed to Gmail",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// The configuration of a Gmail account (`invalid_request` for others).
fn gmail_account(service: &Service, id: &AccountId) -> Result<AccountConfig> {
    let config = service.inner.accounts.config();
    let account = config
        .account(id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("account {id}")))?;
    if !matches!(account.incoming, IncomingServer::Gmail { .. }) {
        return Err(
            ApiError::invalid_request(format!("account {id} is not a Gmail account")).into(),
        );
    }
    Ok(account)
}

/// `invalid_request` with the first `gmail-label-issue-*` text.
fn check_label(settings: &GmailLabelSettings) -> Result<()> {
    let issues = settings.issues();
    if let Some(first) = issues.first() {
        let ids: Vec<&str> = issues.iter().map(|i| i.message_id()).collect();
        return Err(
            ApiError::invalid_request(format!("invalid label: {}", ids.join(", ")))
                .with_localized(LocalizedMessage::new(first.message_id()))
                .into(),
        );
    }
    Ok(())
}

/// A user label of the same name exists already (`conflict`).
async fn duplicate_check(
    service: &Service,
    account: &AccountId,
    settings: &GmailLabelSettings,
    except: Option<&GmailLabelId>,
) -> Result<()> {
    let id = account.clone();
    let labels = service.store().call(move |s| s.gmail_labels(&id)).await?;
    let name = settings.name.trim();
    if labels
        .iter()
        .any(|l| l.name.eq_ignore_ascii_case(name) && Some(&l.id) != except)
    {
        return Err(
            ApiError::invalid_request(format!("a label named {name:?} exists already"))
                .with_localized(LocalizedMessage::new(
                    nail_api::gmail::GmailLabelIssue::DuplicateName.message_id(),
                ))
                .into(),
        );
    }
    Ok(())
}

async fn queue_label_op(
    service: &Service,
    account: &AccountId,
    label_id: &GmailLabelId,
    op: GmailLabelOp,
) -> Result<(GmailLabelId, nail_api::OpId)> {
    let (id, label) = (account.clone(), label_id.clone());
    let queued = service
        .store()
        .call(move |s| s.enqueue_gmail_label_op(&id, &label, op))
        .await
        .map_err(|err| match err {
            nail_store::Error::InvalidData(message) => {
                CoreError::from(ApiError::invalid_request(message))
            }
            other => other.into(),
        })?;
    service.broadcast(Event::GmailLabelsChanged {
        account_id: account.clone(),
    });
    // Tags of messages may have changed with the label.
    service.broadcast(Event::MailChanged {
        folder_ids: Vec::new(),
    });
    service.inner.engines.request_replay(account);
    Ok(queued)
}

async fn label(
    service: &Service,
    account: &AccountId,
    label_id: &GmailLabelId,
) -> Result<nail_api::gmail::GmailLabel> {
    let id = account.clone();
    service
        .store()
        .call(move |s| s.list_gmail_labels(&id))
        .await?
        .into_iter()
        .find(|l| &l.id == label_id)
        .ok_or_else(|| ApiError::not_found(format!("label {label_id}")).into())
}

/// The Gmail engine of a configured account (OAuth2 tokens from the
/// core's token manager).
pub(crate) fn engine_for(service: &Service, account: &AccountConfig) -> Result<GmailEngine> {
    GmailEngine::for_account(account, service.inner.tokens.clone())
        .map_err(|e| CoreError::from(EngineError::from(e)))
}

/// The id of a Gmail message in the store.
pub(crate) fn gmail_message_id(account: &AccountId, remote_id: &str) -> MessageId {
    // The same derivation as a POP3 UIDL, in a namespace of its own.
    ids::pop3_message_id(account, &format!("\0gmail\0{remote_id}"))
}

/// The store rows of a Gmail account's folders.
pub(crate) fn gmail_folders(account: &AccountId) -> Vec<Folder> {
    GmailFolder::ALL
        .iter()
        .map(|f| Folder {
            id: ids::folder_id(account, f.path()),
            account_id: account.clone(),
            parent_id: None,
            name: f.path().to_owned(),
            path: f.path().to_owned(),
            role: f.role(),
            unread: 0,
            total: 0,
        })
        .collect()
}

fn core_error(err: EngineError) -> CoreError {
    err.into()
}

/// The engine of a Gmail account: see the module docs.
pub(crate) async fn run(
    service: Service,
    account: AccountConfig,
    signals: Arc<Signals>,
    mut stop: watch::Receiver<bool>,
) {
    let IncomingServer::Gmail { options, .. } = &account.incoming else {
        return;
    };
    service.set_status(&account.id, AccountState::Connecting, None);
    let engine = match engine_for(&service, &account) {
        Ok(engine) => Arc::new(engine),
        Err(err) => {
            service.set_status(
                &account.id,
                AccountState::Offline,
                Some((err.code(), err.to_string(), err.localized())),
            );
            stopped(&mut stop).await;
            return;
        }
    };
    let timing = service.inner.timing;
    let mut poll = PollTimer::new(options);
    let mut backoff = timing.reconnect_min;
    loop {
        if *stop.borrow() {
            break;
        }
        let result = tokio::select! {
            result = pass(&service, &account.id, &engine) => result,
            () = stopped(&mut stop) => break,
        };
        let delay = match result {
            Ok(outcome) => {
                backoff = timing.reconnect_min;
                service.set_status(&account.id, AccountState::Online, None);
                poll.observe(&Ok(outcome));
                timing.poll_override.unwrap_or_else(|| poll.next_delay())
            }
            Err(err) => {
                let code = err.code();
                tracing::info!(account = %account.id, %err, "Gmail sync failed");
                service.set_status(
                    &account.id,
                    state_for_error(code),
                    Some((code, err.to_string(), err.localized())),
                );
                service.broadcast(Event::SyncFailed {
                    account_id: account.id.clone(),
                    code,
                    message: err.to_string(),
                    localized: err.localized(),
                });
                let engine_err: EngineError = err.into();
                poll.observe(&Err(engine_err));
                let delay = poll.next_delay().max(backoff);
                backoff = (backoff * 2).min(timing.reconnect_max);
                delay
            }
        };
        // Writes are replayed at once; the next poll waits for the timer
        // or "Sync now".
        let deadline = tokio::time::Instant::now() + delay;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => break,
                () = signals.sync.notified() => {
                    poll.user_activity();
                    break;
                }
                () = signals.replay.notified() => {
                    poll.user_activity();
                    replay(&service, &account.id, &engine).await;
                }
                () = stopped(&mut stop) => return,
            }
        }
    }
    tracing::info!(account = %account.id, "engine stopped");
}

/// Replays the label queue and the mail queue (errors keep them queued).
async fn replay(service: &Service, account: &AccountId, engine: &Arc<GmailEngine>) {
    if let Err(err) = replay_label_ops(service, account, engine).await {
        tracing::info!(%account, %err, "Gmail label operations deferred");
    }
    let executor = GmailExecutor {
        service,
        account: account.clone(),
        engine: Arc::clone(engine),
    };
    if let Err(err) = replay_account(service, account, &executor).await {
        tracing::warn!(%account, %err, "replaying Gmail writes failed");
    }
}

/// One pass: replay, labels, then a full listing or the history since the
/// stored cursor. Returns what the poll timer needs.
async fn pass(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
) -> Result<DeltaOutcome> {
    replay(service, account, engine).await;
    {
        let folders = gmail_folders(account);
        service
            .store()
            .call(move |s| {
                for folder in &folders {
                    s.upsert_folder(folder)?;
                }
                Ok(())
            })
            .await?;
    }
    sync_labels(service, account, engine).await?;
    let id = account.clone();
    let state = service.store().call(move |s| s.gmail_sync(&id)).await?;
    let Some(cursor) = state.history_id else {
        full_sync(service, account, engine, state).await?;
        return Ok(DeltaOutcome::CursorExpired);
    };
    let mut cursor = cursor.to_string();
    let mut total = SyncDelta::default();
    loop {
        match engine.changes(&cursor).await.map_err(core_error)? {
            DeltaOutcome::CursorExpired => {
                tracing::info!(%account, "Gmail history expired; listing everything again");
                let state = GmailSyncRecord {
                    history_id: None,
                    ..state
                };
                let (id, record) = (account.clone(), state.clone());
                service
                    .store()
                    .call(move |s| s.set_gmail_sync(&id, &record))
                    .await?;
                full_sync(service, account, engine, state).await?;
                return Ok(DeltaOutcome::CursorExpired);
            }
            DeltaOutcome::Delta(delta) => {
                apply_delta(service, account, engine, &delta).await?;
                let next: Option<u64> = delta.cursor.parse().ok();
                let id = account.clone();
                let more = delta.more;
                service
                    .store()
                    .call(move |s| {
                        let mut record = s.gmail_sync(&id)?;
                        if next.is_some() {
                            record.history_id = next;
                        }
                        record.last_sync_at = Some(unix_now());
                        s.set_gmail_sync(&id, &record)
                    })
                    .await?;
                cursor.clone_from(&delta.cursor);
                total.added.extend(delta.added);
                total.changed.extend(delta.changed);
                total.removed.extend(delta.removed);
                if !more {
                    break;
                }
            }
        }
    }
    total.cursor = cursor;
    Ok(DeltaOutcome::Delta(total))
}

/// The server's label list into the store (renames carry their tags).
async fn sync_labels(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
) -> Result<()> {
    let labels = engine.labels().await.map_err(|e| core_error(e.into()))?;
    let records: Vec<GmailLabelRecord> = labels.iter().map(|l| record_of(account, l)).collect();
    let id = account.clone();
    let changed = service
        .store()
        .call(move |s| {
            let before = s.gmail_labels(&id)?;
            s.replace_gmail_labels(&id, &records)?;
            Ok(before != s.gmail_labels(&id)?)
        })
        .await?;
    if changed {
        service.broadcast(Event::GmailLabelsChanged {
            account_id: account.clone(),
        });
    }
    Ok(())
}

fn record_of(account: &AccountId, label: &GmailLabelInfo) -> GmailLabelRecord {
    match label.kind {
        nail_api::gmail::GmailLabelKind::User => {
            GmailLabelRecord::user(label.id.clone(), &label.settings)
        }
        nail_api::gmail::GmailLabelKind::System => GmailLabelRecord::system(
            &label.id,
            label.folder.map(|f| ids::folder_id(account, f.path())),
        ),
    }
}

/// Replays queued label operations; a failure that may pass keeps the
/// rest queued.
async fn replay_label_ops(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
) -> Result<()> {
    let id = account.clone();
    let ops = service
        .store()
        .call(move |s| s.pending_gmail_label_ops(&id))
        .await?;
    if ops.is_empty() {
        return Ok(());
    }
    for op in ops {
        let result = match &op.op {
            GmailLabelOp::Create(settings) => engine
                .create_label(settings)
                .await
                .map(|info| Some(info.id)),
            GmailLabelOp::Update(settings) => engine
                .update_label(op.label_id.as_str(), settings)
                .await
                .map(|_| None),
            GmailLabelOp::Delete => engine
                .delete_label(op.label_id.as_str())
                .await
                .map(|()| None),
        };
        let (outcome, stop) = match result {
            Ok(server_id) => (GmailLabelOpOutcome::Done { server_id }, false),
            Err(err) => {
                let err: EngineError = err.into();
                if err.retryable && op.attempts < 5 {
                    (GmailLabelOpOutcome::Retry(err.message.clone()), true)
                } else {
                    tracing::warn!(%account, error = %err, "a Gmail label operation failed");
                    (GmailLabelOpOutcome::Failed(err.message.clone()), false)
                }
            }
        };
        let row = op.id;
        service
            .store()
            .call(move |s| s.finish_gmail_label_op(row, &outcome))
            .await?;
        if stop {
            break;
        }
    }
    service.broadcast(Event::GmailLabelsChanged {
        account_id: account.clone(),
    });
    Ok(())
}

/// A full listing (resuming an interrupted one), merged with the store.
async fn full_sync(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
    state: GmailSyncRecord,
) -> Result<()> {
    let resumed = state.full_sync_page.is_some();
    let mut page_token = state.full_sync_page.clone();
    let mut cursor = state.full_sync_history_id;
    let mut listed: HashSet<String> = HashSet::new();
    let mut pages = 0u32;
    loop {
        let page: FullSyncPage = engine
            .full_sync_page(page_token.as_deref(), None)
            .await
            .map_err(core_error)?;
        if let Some(first) = &page.cursor {
            cursor = first.parse().ok();
        }
        listed.extend(page.messages.iter().map(|m| m.remote_id.clone()));
        store_messages(service, account, engine, &page.messages, false).await?;
        pages += 1;
        service.broadcast(Event::SyncProgress {
            account_id: account.clone(),
            done: pages,
            total: pages + u32::from(page.next_page.is_some()),
        });
        if resumed && page.next_page.is_none() {
            // The pages listed before the interruption are not in `listed`:
            // the merge needs every id the server has now (ids only). Asked
            // before the cursor is stored, so a failure resumes here again.
            listed = engine
                .all_message_ids()
                .await
                .map_err(|e| core_error(e.into()))?;
        }
        let (id, next, at) = (account.clone(), page.next_page.clone(), cursor);
        service
            .store()
            .call(move |s| {
                s.set_gmail_sync(
                    &id,
                    &GmailSyncRecord {
                        history_id: if next.is_none() { at } else { None },
                        full_sync_page: next.clone(),
                        full_sync_history_id: if next.is_some() { at } else { None },
                        last_sync_at: Some(unix_now()),
                    },
                )
            })
            .await?;
        match page.next_page {
            Some(next) => page_token = Some(next),
            None => break,
        }
    }
    {
        // Merge: messages the server no longer lists are gone (also after
        // a resumed listing: `listed` then holds every id on the server).
        let id = account.clone();
        let gone: Vec<MessageId> = service
            .store()
            .call(move |s| {
                let mut gone = Vec::new();
                for folder in gmail_folders(&id) {
                    for m in s.list_messages(&folder.id, 0, u32::MAX)?.messages {
                        let remote = s.message_remote_id(&m.id)?;
                        if remote.as_ref().is_some_and(|r| !listed.contains(r)) {
                            gone.push(m.id);
                        }
                    }
                }
                Ok(gone)
            })
            .await?;
        remove_messages(service, gone).await?;
    }
    service.broadcast(Event::MailChanged {
        folder_ids: gmail_folders(account).into_iter().map(|f| f.id).collect(),
    });
    Ok(())
}

async fn remove_messages(service: &Service, ids: Vec<MessageId>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let index = service.inner.search.get();
    service
        .store()
        .call(move |s| {
            for id in &ids {
                s.delete_message(id)?;
            }
            if let Err(err) = index.remove(&ids) {
                tracing::warn!(%err, "cannot remove deleted messages from the index");
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Applies one history delta.
async fn apply_delta(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
    delta: &SyncDelta,
) -> Result<()> {
    if delta.added.is_empty() && delta.changed.is_empty() && delta.removed.is_empty() {
        return Ok(());
    }
    store_messages(service, account, engine, &delta.added, true).await?;
    let mut unknown = Vec::new();
    for change in &delta.changed {
        if !apply_change(service, account, change).await? {
            unknown.push(change.remote_id.clone());
        }
    }
    // A change of a message Nail does not have yet: fetch it whole.
    let mut fetched = Vec::new();
    for remote in unknown {
        match engine.message(&remote).await {
            Ok(m) => fetched.push(m),
            Err(nail_proto_gmail::GmailError::NotFound(_)) => {}
            Err(e) => return Err(core_error(e.into())),
        }
    }
    store_messages(service, account, engine, &fetched, true).await?;
    let id = account.clone();
    let removed = delta.removed.clone();
    let gone: Vec<MessageId> = service
        .store()
        .call(move |s| {
            let mut out = Vec::new();
            for remote in &removed {
                if let Some(m) = s.message_by_remote_id(&id, remote)? {
                    out.push(m);
                }
            }
            Ok(out)
        })
        .await?;
    remove_messages(service, gone).await?;
    service.broadcast(Event::MailChanged {
        folder_ids: gmail_folders(account).into_iter().map(|f| f.id).collect(),
    });
    Ok(())
}

/// A metadata change of a stored message; `false` when it is unknown.
async fn apply_change(
    service: &Service,
    account: &AccountId,
    change: &RemoteMessageChange,
) -> Result<bool> {
    let (id, change) = (account.clone(), change.clone());
    Ok(service
        .store()
        .call(move |s| {
            let Some(message_id) = s.message_by_remote_id(&id, &change.remote_id)? else {
                return Ok(false);
            };
            let mut stored = s.get_message(&message_id)?;
            if let Some(folder) = &change.folder {
                stored.summary.folder_id = ids::folder_id(&id, folder);
            }
            if let Some(flags) = change.flags {
                stored.summary.flags = merged_flags(flags, stored.summary.flags);
            }
            if let Some(tags) = &change.tags {
                stored.summary.tags.clone_from(tags);
            }
            s.merge_messages(&[SyncedMessage {
                message: stored,
                meta: None,
            }])?;
            if let Some(info) = &change.gmail {
                s.set_gmail_message(&message_id, info)?;
            }
            Ok(true)
        })
        .await?)
}

/// The server's flags, keeping a local `\Answered` (Gmail has no such
/// label, so the server always says "not answered").
fn merged_flags(server: Flags, local: Flags) -> Flags {
    Flags {
        answered: server.answered || local.answered,
        ..server
    }
}

/// Stores listed / new messages: known ones get the server's metadata,
/// new ones are fetched raw and ingested. `arrivals`: announce new mail
/// (history deltas; not the first listing).
async fn store_messages(
    service: &Service,
    account: &AccountId,
    engine: &Arc<GmailEngine>,
    messages: &[RemoteMessage],
    arrivals: bool,
) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let mut new_ids: Vec<(MessageId, FolderId)> = Vec::new();
    for chunk in messages.chunks(INGEST_CHUNK) {
        // Known messages: metadata only.
        let mut fresh: Vec<RemoteMessage> = Vec::new();
        for m in chunk {
            let change = RemoteMessageChange {
                remote_id: m.remote_id.clone(),
                folder: Some(m.folder.clone()),
                flags: Some(m.flags),
                tags: Some(m.tags.clone()),
                gmail: m.gmail.clone(),
                change_key: m.change_key.clone(),
                exchange: m.exchange.clone(),
            };
            if !apply_change(service, account, &change).await? {
                fresh.push(m.clone());
            }
        }
        for m in fresh {
            let raw = match &m.raw {
                Some(raw) => raw.clone(),
                None => match engine.fetch_raw(&m.remote_id).await {
                    Ok(raw) => raw,
                    Err(err) if err.code == ErrorCode::NotFound => continue,
                    Err(err) => return Err(core_error(err)),
                },
            };
            let id = gmail_message_id(account, &m.remote_id);
            let folder = gmail_folders(account)
                .into_iter()
                .find(|f| f.path == m.folder)
                .unwrap_or_else(|| gmail_folders(account).remove(0));
            let incoming = IncomingMessage {
                uid: None,
                id: Some(id.clone()),
                flags: m.flags,
                keywords: m.tags.clone(),
                internal_date: m.internal_date.and_then(|t| {
                    chrono::DateTime::from_timestamp(t, 0)
                        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                }),
                raw,
                headers_only: false,
                size: m.size,
                remote_thread_id: m.thread_id.clone(),
            };
            let blobs = service.blobs().clone();
            let index = service.inner.search.get();
            let (target, info, remote) = (folder.clone(), m.gmail.clone(), m.remote_id.clone());
            let stored_id = id.clone();
            let report = service
                .store()
                .call(move |s| {
                    // The message and its Gmail data in one transaction:
                    // no reader sees it without them (ADR 0120).
                    let extras = BTreeMap::from([(
                        stored_id,
                        RemoteExtras {
                            remote_id: Some(remote),
                            exchange: None,
                            gmail: info,
                        },
                    )]);
                    ingest_with_remote(s, &blobs, Some(&index), &target, &[incoming], &extras)
                })
                .await?;
            // Only mail arriving in the inbox or a user folder counts (like
            // IMAP): never the user's own sent mail, Spam or Trash.
            if report.new.contains(&id) && crate::sync::runs_arrival_rules(folder.role) {
                new_ids.push((id, folder.id));
            }
        }
    }
    if arrivals && !new_ids.is_empty() {
        let queued: Vec<MessageId> = new_ids.iter().map(|(id, _)| id.clone()).collect();
        if let Err(err) = service
            .store()
            .call(move |s| crate::rules::queue_for_rules(s, &queued))
            .await
        {
            tracing::warn!(%err, "cannot queue new Gmail mail for the rules");
        }
        let inbox = ids::folder_id(account, GmailFolder::Inbox.path());
        let mut by_folder: Vec<(FolderId, Vec<MessageId>)> = Vec::new();
        for (id, folder) in new_ids {
            match by_folder.iter_mut().find(|(f, _)| *f == folder) {
                Some((_, list)) => list.push(id),
                None => by_folder.push((folder, vec![id])),
            }
        }
        for (folder_id, message_ids) in by_folder {
            if folder_id == inbox {
                service.broadcast(Event::NewMail {
                    account_id: account.clone(),
                    folder_id,
                    message_ids,
                });
            }
        }
    }
    Ok(())
}

/// Replays queued writes of a Gmail account through its engine.
pub(crate) struct GmailExecutor<'a> {
    pub service: &'a Service,
    pub account: AccountId,
    pub engine: Arc<GmailEngine>,
}

impl GmailExecutor<'_> {
    async fn remote_id(&self, message: &MessageId) -> Result<Option<String>, EngineError> {
        let id = message.clone();
        match self
            .service
            .store()
            .call(move |s| s.message_remote_id(&id))
            .await
        {
            Ok(remote) => Ok(remote),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(CoreError::from(err).into()),
        }
    }

    async fn folder_path(&self, id: &FolderId) -> Result<Folder, EngineError> {
        let id = id.clone();
        self.service
            .store()
            .call(move |s| s.get_folder(&id))
            .await
            .map_err(|e| CoreError::from(e).into())
    }

    async fn write(&self, write: RemoteWrite) -> Result<WriteOutcome, EngineError> {
        self.engine.write(&write).await
    }

    async fn run(&self, op: &QueuedOp) -> Result<(), EngineError> {
        match &op.operation {
            Operation::UpdateFlags { message_id, change } => {
                let Some(remote_id) = self.remote_id(message_id).await? else {
                    return Ok(());
                };
                self.write(RemoteWrite::SetFlags {
                    remote_id,
                    change: nail_api::FlagChange {
                        seen: change.seen,
                        flagged: change.flagged,
                        answered: change.answered,
                        draft: change.draft,
                        add_tags: change.add_tags.clone(),
                        remove_tags: change.remove_tags.clone(),
                    },
                })
                .await?;
                Ok(())
            }
            Operation::MoveMessage {
                message_id,
                target_folder_id,
                source,
            } => {
                let remote = match source.as_ref().and_then(|s| s.remote_id.clone()) {
                    Some(r) => Some(r),
                    None => self.remote_id(message_id).await?,
                };
                let Some(remote_id) = remote else {
                    return Ok(());
                };
                let target = self.folder_path(target_folder_id).await?;
                let from = match source {
                    Some(source) => self.folder_path(&source.folder_id).await?,
                    None => return Ok(()),
                };
                if target.account_id != self.account {
                    // M4c: into another Gmail or an Exchange account.
                    let Some(api) =
                        crate::api_moves::api_target(self.service, target_folder_id).await?
                    else {
                        return Err(crate::api_moves::unsupported_target());
                    };
                    let source = nail_exchange::transfer::ApiMailbox::Api(self.engine.as_ref());
                    return crate::api_moves::move_out(
                        self.service,
                        message_id,
                        source,
                        &remote_id,
                        &api,
                    )
                    .await;
                }
                self.write(RemoteWrite::Move {
                    remote_id,
                    from_folder: from.path,
                    to_folder: target.path,
                })
                .await?;
                Ok(())
            }
            Operation::DeleteMessage {
                message_id,
                source,
                trash_folder_id,
                ..
            } => {
                let remote = match source.as_ref().and_then(|s| s.remote_id.clone()) {
                    Some(r) => Some(r),
                    None => self.remote_id(message_id).await?,
                };
                let Some(remote_id) = remote else {
                    return Ok(());
                };
                let write = if trash_folder_id.is_some() {
                    RemoteWrite::Trash { remote_id }
                } else {
                    RemoteWrite::Delete { remote_id }
                };
                self.write(write).await?;
                Ok(())
            }
            Operation::AppendMessage {
                message_id,
                folder_id,
            } => {
                let folder = self.folder_path(folder_id).await?;
                let id = message_id.clone();
                let blobs = self.service.blobs().clone();
                let found = self
                    .service
                    .store()
                    .call(move |s| {
                        let m = match s.get_message(&id) {
                            Ok(m) => m,
                            Err(err) if err.is_not_found() => return Ok(None),
                            Err(err) => return Err(err),
                        };
                        let raw = match m.blob_hash {
                            Some(hash) => blobs.get(&hash).ok(),
                            None => None,
                        };
                        Ok(raw.map(|raw| (raw, m.summary.flags)))
                    })
                    .await
                    .map_err(|e| EngineError::from(CoreError::from(e)))?;
                let Some((raw, flags)) = found else {
                    return Ok(());
                };
                if let WriteOutcome::Done {
                    remote_id: Some(remote),
                } = self
                    .write(RemoteWrite::Append {
                        folder: folder.path,
                        raw,
                        flags,
                    })
                    .await?
                {
                    let id = message_id.clone();
                    drop(
                        self.service
                            .store()
                            .call(move |s| s.set_message_remote_id(&id, Some(&remote)))
                            .await,
                    );
                }
                Ok(())
            }
            other => Err(EngineError::permanent(
                ErrorCode::Internal,
                format!("cannot replay {} on a Gmail account", other.kind()),
            )),
        }
    }
}

impl OpExecutor for GmailExecutor<'_> {
    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(self.run(op))
    }
}

/// `TestAccount` for a Gmail account: a token, then `users.getProfile`.
pub(crate) async fn test_gmail(
    server: &nail_api::ServerEndpoint,
    login: std::result::Result<crate::creds::Login, EngineError>,
) -> std::result::Result<Vec<String>, EngineError> {
    let crate::creds::Login::OAuth2 {
        access_token: token,
        ..
    } = login?
    else {
        return Err(EngineError::permanent(
            ErrorCode::InvalidRequest,
            "Gmail accounts sign in with OAuth2",
        ));
    };
    let client = nail_proto_gmail::GmailClient::new(
        &nail_proto_gmail::api_base_for(server),
        nail_proto_gmail::GmailAuth::Bearer(token),
    )
    .map_err(EngineError::from)?;
    let profile = GmailEngine::new(client)
        .profile()
        .await
        .map_err(EngineError::from)?;
    Ok(vec![format!("gmail:{}", profile.email_address)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_ids_are_stable() {
        let account = AccountId::new("gm");
        let folders = gmail_folders(&account);
        assert_eq!(folders.len(), 6);
        assert_eq!(folders[0].path, "INBOX");
        assert_eq!(folders[0].role, nail_api::FolderRole::Inbox);
        assert_eq!(
            gmail_message_id(&account, "18c1"),
            gmail_message_id(&account, "18c1")
        );
        assert_ne!(
            gmail_message_id(&account, "18c1"),
            gmail_message_id(&AccountId::new("other"), "18c1")
        );
        assert_ne!(
            gmail_message_id(&account, "x"),
            ids::pop3_message_id(&account, "x")
        );
    }

    #[test]
    fn a_sync_never_clears_answered() {
        let local = Flags {
            answered: true,
            seen: false,
            ..Flags::default()
        };
        let server = Flags {
            seen: true,
            ..Flags::default()
        };
        let merged = merged_flags(server, local);
        assert!(merged.answered && merged.seen);
        assert!(!merged_flags(server, Flags::default()).answered);
    }

    #[test]
    fn label_names_are_checked() {
        assert!(check_label(&GmailLabelSettings::named("Work")).is_ok());
        let err = check_label(&GmailLabelSettings::named("")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidRequest);
        assert!(err.localized().is_some());
    }
}
