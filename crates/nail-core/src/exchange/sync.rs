//! The engine of an Exchange account (ADR 0077).
//!
//! * **Loop** ([`run`]): one pass = replay (mail queue, Exchange queue),
//!   mail of the own mailbox and every shown shared / delegate mailbox,
//!   settings (categories, automatic replies, inbox rules and placement;
//!   every [`SETTINGS_EVERY`] passes, after local changes and on "Sync"),
//!   PIM. EWS accounts with streaming wait for notifications between
//!   passes (`wait_for_changes`); Graph accounts poll with
//!   [`PollTimer`] and ask quiet folders less often ([`FolderPoll`]).
//! * **Mail**: the folder hierarchy per mailbox (a cursor each; another
//!   mailbox's tree lives under `@<address>`), then the items of each mail
//!   folder (a cursor each; an expired one means a full listing merged
//!   with the store). New messages are fetched as MIME and ingested once,
//!   under an id derived from their first server id; the server id lives
//!   in `messages.remote_id` and changes when the message moves.
//! * **Replay** ([`ExchangeExecutor`]): flags / tags (= categories) /
//!   moves / deletes / appends of the mail queue as `RemoteWrite`s against
//!   the message's mailbox; a move stores the new id. The Exchange queue
//!   (categories, automatic replies, rules, Focused Inbox) through the
//!   settings / classify calls.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nail_api::engine::{
    BoxFuture, DeltaOutcome, EngineError, RemoteMessage, RemoteMessageChange, RemoteWrite,
    SyncDelta, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeEngine, ExchangeFolder, ExchangeFolderClass, HierarchyOutcome,
};
use nail_api::exchange::{
    ExchangeArea, ExchangeCategorySettings, ExchangeEngine as EngineKind, ExchangeMailboxState,
    ExchangePush, ExchangeWarning, ExchangeWellKnownFolder, mailbox_folder_path,
};
use nail_api::{
    AccountConfig, AccountId, AccountState, ApiError, ErrorCode, Event, Folder, FolderId,
    FolderRole, MessageId,
};
use nail_proto_graph::{FolderPoll, PollTimer};
use nail_store::exchange::{
    ExchangeFolderRecord, ExchangeItemRecord, ExchangeOp, ExchangeOpOutcome, ExchangeSyncRecord,
    RemoteExtras,
};
use nail_store::{Operation, QueuedOp, SyncedMessage};
use tokio::sync::watch;

use super::{MAX_PAGES, RunningEngine, changed, exchange_parts};
use crate::engine::{Signals, stopped};
use crate::error::{CoreError, Result};
use crate::ids;
use crate::ingest::{API_INGEST_CHUNK, IncomingMessage, ingest_api_messages};
use crate::replay::{
    Grouped, OpExecutor, group_results, group_slots, recorded_or, replay_account, sort_group,
};
use crate::service::{Service, lock};
use nail_util::time::unix_now;

/// Settings (categories, automatic replies, rules) are read every this
/// many passes (and after local changes or "Sync").
pub(crate) const SETTINGS_EVERY: u32 = 10;

/// Longest a streaming wait lasts before a pass runs anyway.
const STREAM_WAIT_SECS: u32 = 60;

/// How often a pass syncs the folders of a mailbox again because a folder
/// reported messages removed after they were synced (see
/// [`sync_mailbox`]). Past this the removed messages stay marked and the
/// next pass decides.
const MAX_REMOVAL_ROUNDS: u32 = 3;

/// The store id of an Exchange message (from its first server id).
pub(crate) fn exchange_message_id(account: &AccountId, remote_id: &str) -> MessageId {
    ids::pop3_message_id(account, &format!("\0exchange\0{remote_id}"))
}

/// The store id of an Exchange folder (stable across renames and moves).
pub(crate) fn exchange_folder_id(
    account: &AccountId,
    mailbox: Option<&str>,
    remote: &str,
) -> FolderId {
    ids::folder_id(
        account,
        &format!("\u{1}exchange\u{1}{}\u{1}{remote}", mailbox.unwrap_or("")),
    )
}

/// The root folder of another mailbox (`@<address>`).
pub(crate) fn mailbox_root_id(account: &AccountId, mailbox: &str) -> FolderId {
    ids::folder_id(account, &format!("\u{1}exchange-root\u{1}{mailbox}"))
}

/// What the loop keeps between passes.
pub(crate) struct LoopState {
    poll: PollTimer,
    folders: FolderPoll,
    passes: u32,
    settings_due: bool,
    /// Folders a notification named (EWS): synced in the next pass even if
    /// the schedule would skip them.
    woken: HashSet<FolderId>,
    /// Whether the server classifies mail (Focused Inbox).
    focused_inbox: Option<bool>,
}

/// The engine of an Exchange account: see the module docs.
pub(crate) async fn run(
    service: Service,
    account: AccountConfig,
    signals: Arc<Signals>,
    mut stop: watch::Receiver<bool>,
) {
    let Some((_, options)) = exchange_parts(&account) else {
        return;
    };
    let options = options.clone();
    service.set_status(&account.id, AccountState::Connecting, None);
    let (engine, batch) = match super::setup::engines_for(&service, &account) {
        Ok(engines) => engines,
        Err(err) => {
            service
                .protocol_failed(&account.id, &err, super::wire_protocol(&account), false)
                .await;
            stopped(&mut stop).await;
            return;
        }
    };
    let streaming = engine.kind() == EngineKind::Ews && options.streaming();
    let running = Arc::new(RunningEngine {
        engine,
        batch,
        push: Mutex::new(if streaming {
            ExchangePush::Streaming
        } else {
            ExchangePush::Polling
        }),
        warnings: Mutex::new(Vec::new()),
        settings_wake: tokio::sync::Notify::new(),
    });
    service
        .inner
        .exchange
        .register(&account.id, Arc::clone(&running));
    changed(&service, &account.id, &[ExchangeArea::Status]);
    let timing = service.inner.timing;
    let mut state = LoopState {
        poll: PollTimer::new(&options),
        folders: FolderPoll::new(),
        passes: 0,
        settings_due: true,
        woken: HashSet::new(),
        focused_inbox: None,
    };
    let mut backoff = timing.reconnect_min;
    let mut stream_backoff = timing.reconnect_min;
    // The own folders the streaming subscription was made for (sorted);
    // `None`: none yet, or it may be gone.
    let mut subscribed: Option<Vec<String>> = None;
    loop {
        if *stop.borrow() {
            break;
        }
        let result = tokio::select! {
            result = pass(&service, &account, &running, &mut state) => result,
            () = stopped(&mut stop) => break,
        };
        let ok = result.is_ok();
        let delay = match result {
            Ok(()) => {
                backoff = timing.reconnect_min;
                service.engine_online(&account.id).await;
                timing
                    .poll_override
                    .unwrap_or_else(|| state.poll.next_delay())
            }
            Err(err) => {
                tracing::info!(account = %account.id, %err, "Exchange sync failed");
                service
                    .protocol_failed(&account.id, &err, super::wire_protocol(&account), true)
                    .await;
                let engine_err: EngineError = err.into();
                if let Some(secs) = engine_err.retry_after_secs {
                    record_throttle(&service, &account.id, secs).await;
                }
                state.poll.observe(&Err(engine_err));
                state.poll.end_round();
                let delay = state.poll.next_delay().max(backoff);
                backoff = (backoff * 2).min(timing.reconnect_max);
                delay
            }
        };
        let mut deadline = tokio::time::Instant::now() + delay;
        let mut push_ok = streaming && ok;
        loop {
            if push_ok {
                // EWS streaming: wait for the server (or the user).
                let own = own_folder_ids(&service, &account.id).await;
                let mut asked = own.clone();
                asked.sort();
                asked.dedup();
                // A new subscription reports only what changes after it
                // exists: it is made first (a wait that ends at once), then
                // every folder is synced once more, so mail that arrived
                // between a folder's listing and the subscription is not
                // missed until the next poll (WP175).
                let fresh = subscribed.as_ref() != Some(&asked);
                let wait = running.engine.wait_for_changes(
                    None,
                    &own,
                    if fresh { 0 } else { STREAM_WAIT_SECS },
                );
                tokio::select! {
                    result = wait => match result {
                        Ok(Some(_)) if fresh => {
                            stream_backoff = timing.reconnect_min;
                            subscribed = Some(asked);
                            state.folders.wake_all();
                            break;
                        }
                        Ok(Some(folders)) if !folders.is_empty() => {
                            stream_backoff = timing.reconnect_min;
                            for remote in folders {
                                state.woken.insert(exchange_folder_id(
                                    &account.id, None, &remote,
                                ));
                            }
                            break;
                        }
                        Ok(Some(_)) => {
                            stream_backoff = timing.reconnect_min;
                            if tokio::time::Instant::now() >= deadline {
                                break;
                            }
                        }
                        Ok(None) => {
                            *lock(&running.push) = ExchangePush::Polling;
                            push_ok = false;
                        }
                        Err(err) => {
                            subscribed = None;
                            // Poll until the next pass, which waits for the
                            // back-off and the server's Retry-After: a
                            // subscription that keeps failing must not turn
                            // into back-to-back passes (ADR 0078).
                            tracing::info!(account = %account.id, %err, "streaming notifications failed");
                            if let Some(secs) = err.retry_after_secs {
                                record_throttle(&service, &account.id, secs).await;
                            }
                            let wait = stream_retry_delay(
                                &mut state.poll,
                                &mut stream_backoff,
                                timing.reconnect_max,
                                &err,
                            );
                            deadline = deadline.max(tokio::time::Instant::now() + wait);
                            push_ok = false;
                        }
                    },
                    wake = next_wake(&signals, &running, &mut stop) => {
                        match woken(wake, &service, &account, &running, &mut state).await {
                            Next::Pass => break,
                            Next::Wait => {}
                            Next::Stop => return,
                        }
                    }
                }
                continue;
            }
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => break,
                wake = next_wake(&signals, &running, &mut stop) => {
                    if matches!(wake, Wake::Replay) {
                        // Polling: a write is the user at work.
                        state.poll.user_activity();
                    }
                    match woken(wake, &service, &account, &running, &mut state).await {
                        Next::Pass => break,
                        Next::Wait => {}
                        Next::Stop => return,
                    }
                }
            }
        }
    }
    service.inner.exchange.unregister(&account.id, &running);
    tracing::info!(account = %account.id, "engine stopped");
}

/// What woke a waiting engine.
enum Wake {
    /// "Sync now".
    Sync,
    /// Writes to replay.
    Replay,
    /// A settings pass is due.
    Settings,
    /// The engine stops.
    Stop,
}

/// What the engine does after a wake-up.
enum Next {
    /// A sync pass now.
    Pass,
    /// Wait on.
    Wait,
    /// End the engine.
    Stop,
}

/// Waits for the next wake-up of a waiting engine.
async fn next_wake(
    signals: &Signals,
    running: &RunningEngine,
    stop: &mut watch::Receiver<bool>,
) -> Wake {
    tokio::select! {
        () = signals.sync.notified() => Wake::Sync,
        () = signals.replay.notified() => Wake::Replay,
        () = running.settings_wake.notified() => Wake::Settings,
        () = stopped(stop) => Wake::Stop,
    }
}

/// Handles a wake-up: "Sync now" wakes every folder and the settings,
/// writes are replayed (mail and PIM), a stop unregisters the engine.
async fn woken(
    wake: Wake,
    service: &Service,
    account: &AccountConfig,
    running: &Arc<RunningEngine>,
    state: &mut LoopState,
) -> Next {
    match wake {
        Wake::Sync => {
            state.poll.user_activity();
            state.folders.wake_all();
            state.settings_due = true;
            Next::Pass
        }
        Wake::Replay => {
            replay(service, &account.id, running).await;
            super::pim::replay_writes(service, account, running.engine.as_ref()).await;
            Next::Wait
        }
        Wake::Settings => {
            state.settings_due = true;
            Next::Pass
        }
        Wake::Stop => {
            service.inner.exchange.unregister(&account.id, running);
            Next::Stop
        }
    }
}

/// The wait after streaming notifications failed: the back-off (doubling
/// up to `max`) or the server's Retry-After, whichever is longer; the
/// poll timer learns the Retry-After for the next round.
fn stream_retry_delay(
    poll: &mut PollTimer,
    backoff: &mut std::time::Duration,
    max: std::time::Duration,
    err: &EngineError,
) -> std::time::Duration {
    poll.observe(&Err(err.clone()));
    let retry_after = std::time::Duration::from_secs(u64::from(err.retry_after_secs.unwrap_or(0)));
    let delay = (*backoff).max(retry_after);
    *backoff = (*backoff * 2).min(max);
    delay
}

async fn record_throttle(service: &Service, account: &AccountId, secs: u32) {
    let id = account.clone();
    let until = unix_now() + i64::from(secs);
    drop(
        service
            .store()
            .call(move |s| {
                if let Some(mut record) = s.exchange_sync(&id)? {
                    record.throttled_until = Some(until);
                    s.set_exchange_sync(&id, &record)?;
                }
                Ok(())
            })
            .await,
    );
    changed(service, account, &[ExchangeArea::Status]);
}

/// The remote ids of the own mailbox's mail folders (what streaming
/// watches).
async fn own_folder_ids(service: &Service, account: &AccountId) -> Vec<String> {
    let id = account.clone();
    service
        .store()
        .call(move |s| s.exchange_folders(&id))
        .await
        .map(|folders| {
            folders
                .into_iter()
                .filter(|f| f.mailbox.is_none())
                .map(|f| f.remote_id)
                .collect()
        })
        .unwrap_or_default()
}

/// One pass (see the module docs).
async fn pass(
    service: &Service,
    account: &AccountConfig,
    running: &Arc<RunningEngine>,
    state: &mut LoopState,
) -> Result<()> {
    let engine = &running.engine;
    tracing::debug!(account = %account.id, pass = state.passes, "Exchange pass");
    if state.focused_inbox.is_none() {
        let info = engine.server_info().await.map_err(CoreError::from)?;
        state.focused_inbox = Some(info.focused_inbox);
        if !info.focused_inbox {
            let mut warnings = lock(&running.warnings);
            if !warnings.contains(&ExchangeWarning::FocusedInboxUnavailable) {
                warnings.push(ExchangeWarning::FocusedInboxUnavailable);
            }
        }
        let (id, kind) = (account.id.clone(), engine.kind());
        let version = info.server_version.clone();
        service
            .store()
            .call(move |s| {
                let mut record = s.exchange_sync(&id)?.unwrap_or(ExchangeSyncRecord {
                    engine: kind,
                    server_version: None,
                    subscription_id: None,
                    watermark: None,
                    throttled_until: None,
                    last_sync_at: None,
                });
                record.engine = kind;
                record.server_version = version;
                s.set_exchange_sync(&id, &record)
            })
            .await?;
        changed(service, &account.id, &[ExchangeArea::Status]);
    }
    let replayed = replay(service, &account.id, running).await;
    state.settings_due |= replayed;
    // Mail: the own mailbox, then the shown other mailboxes.
    sync_mailbox(service, account, engine, None, state).await?;
    let id = account.id.clone();
    let mailboxes = service
        .store()
        .call(move |s| s.exchange_mailboxes(&id, false))
        .await?;
    for mailbox in mailboxes.iter().filter(|m| m.settings.mail) {
        let address = mailbox.settings.address.clone();
        let result = sync_mailbox(service, account, engine, Some(&address), state).await;
        let (new_state, error) = match &result {
            Ok(()) => (ExchangeMailboxState::Synced, None),
            Err(err) => {
                let code = err.code();
                let state = match code {
                    ErrorCode::Forbidden | ErrorCode::AuthFailed => {
                        ExchangeMailboxState::AccessDenied
                    }
                    ErrorCode::NotFound => ExchangeMailboxState::NotFound,
                    _ => ExchangeMailboxState::Error,
                };
                tracing::info!(account = %account.id, mailbox = %address, %err, "mailbox sync failed");
                (
                    state,
                    Some(ApiError::from(CoreError::Engine(EngineError::permanent(
                        code,
                        err.to_string(),
                    )))),
                )
            }
        };
        if new_state != mailbox.state
            || (new_state == ExchangeMailboxState::Synced && mailbox.root_folder_id.is_none())
        {
            let (id, key) = (account.id.clone(), address.clone());
            let root = (new_state == ExchangeMailboxState::Synced)
                .then(|| mailbox_root_id(&account.id, &address));
            service
                .store()
                .call(move |s| {
                    s.set_exchange_mailbox_state(
                        &id,
                        &key,
                        new_state,
                        root.as_ref(),
                        error.as_ref(),
                    )
                })
                .await?;
            changed(service, &account.id, &[ExchangeArea::Mailboxes]);
        }
    }
    // Settings.
    state.passes += 1;
    if state.settings_due || state.passes.is_multiple_of(SETTINGS_EVERY) {
        state.settings_due = false;
        if let Err(err) = super::settings::sync(service, account, running).await {
            tracing::info!(account = %account.id, %err, "Exchange settings sync failed");
        }
    }
    // Calendars, contacts, tasks.
    tracing::debug!(account = %account.id, "Exchange pass: PIM");
    let pim_changed = super::pim::sync(service, account, engine.as_ref()).await?;
    tracing::debug!(account = %account.id, "Exchange pass done");
    state.poll.observe_changes(pim_changed);
    state.poll.end_round();
    let id = account.id.clone();
    service
        .store()
        .call(move |s| {
            if let Some(mut record) = s.exchange_sync(&id)? {
                record.last_sync_at = Some(unix_now());
                record.throttled_until = None;
                s.set_exchange_sync(&id, &record)?;
            }
            Ok(())
        })
        .await?;
    changed(service, &account.id, &[ExchangeArea::Status]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Folders

/// A folder's items cursor, interrupted first sync and last sync.
type CursorState = (Option<String>, Option<String>, Option<i64>);

/// What the hierarchy knows about a folder.
#[derive(Debug, Clone)]
struct KnownFolder {
    name: String,
    parent: Option<String>,
    well_known: Option<ExchangeWellKnownFolder>,
    change_key: Option<String>,
}

/// The stored folders of one mailbox of an account (`None` = its own).
async fn mailbox_folders(
    service: &Service,
    account: &AccountId,
    mailbox: Option<&str>,
) -> Result<Vec<ExchangeFolderRecord>> {
    let (id, key) = (account.clone(), mailbox.map(str::to_owned));
    Ok(service
        .store()
        .call(move |s| {
            Ok(s.exchange_folders(&id)?
                .into_iter()
                .filter(|f| f.mailbox == key)
                .collect())
        })
        .await?)
}

/// Syncs a mailbox: hierarchy, then the due mail folders.
async fn sync_mailbox(
    service: &Service,
    account: &AccountConfig,
    engine: &Arc<dyn ExchangeEngine>,
    mailbox: Option<&str>,
    state: &mut LoopState,
) -> Result<()> {
    sync_hierarchy(service, account, engine.as_ref(), mailbox).await?;
    let folders = mailbox_folders(service, &account.id, mailbox).await?;
    for f in &folders {
        let priority = mailbox.is_none() && f.well_known == Some(ExchangeWellKnownFolder::Inbox);
        state
            .folders
            .track(f.folder_id.as_str(), priority || f.cursor.is_none());
    }
    let due: HashSet<String> = state.folders.due().into_iter().collect();
    let mut since_removal = SinceRemoval::default();
    for folder in &folders {
        let woken = state.woken.remove(&folder.folder_id);
        if !due.contains(folder.folder_id.as_str()) && !woken {
            continue;
        }
        let removed = sync_one(service, account, engine, mailbox, folder, state).await?;
        since_removal.synced(&folder.folder_id, removed);
    }
    // Messages reported removed may have moved to another folder (another
    // client moved them): every folder of the mailbox is synced after the
    // removal was seen before what is still marked is deleted (ADR 0078).
    // A folder synced earlier in the same pass does not count: the move
    // may have happened after its listing (WP175).
    let (id, key) = (account.id.clone(), mailbox.map(str::to_owned));
    let pending = service
        .store()
        .call(move |s| s.exchange_removed(&id, key.as_deref()))
        .await?;
    if pending.is_empty() {
        return Ok(());
    }
    let mut rounds = 0u32;
    loop {
        // Read again: a folder synced in this pass has a newer cursor.
        let current = mailbox_folders(service, &account.id, mailbox).await?;
        let gone = since_removal.stale(current);
        if gone.is_empty() {
            break;
        }
        if rounds == MAX_REMOVAL_ROUNDS {
            tracing::info!(account = %account.id, "messages keep being removed; deciding in the next pass");
            return Ok(());
        }
        rounds += 1;
        for folder in &gone {
            let removed = sync_one(service, account, engine, mailbox, folder, state).await?;
            since_removal.synced(&folder.folder_id, removed);
        }
    }
    flush_removed(service, &account.id, mailbox).await?;
    Ok(())
}

/// The folders of a mailbox synced after the last folder that reported
/// messages removed: only their listings can show where a removed message
/// went (another client may have moved it there after an earlier listing).
#[derive(Debug, Default)]
struct SinceRemoval(HashSet<FolderId>);

impl SinceRemoval {
    /// `folder` was synced; `removed`: its sync marked messages removed,
    /// so every folder synced before it has to be synced again.
    fn synced(&mut self, folder: &FolderId, removed: bool) {
        if removed {
            self.0.clear();
        }
        self.0.insert(folder.clone());
    }

    /// The folders (of `all`) to sync before marked messages may be
    /// deleted.
    fn stale(&self, all: Vec<ExchangeFolderRecord>) -> Vec<ExchangeFolderRecord> {
        all.into_iter()
            .filter(|f| !self.0.contains(&f.folder_id))
            .collect()
    }
}

/// Syncs one mail folder of a mailbox (a folder the server no longer has
/// is removed); `true` when it marked messages removed.
async fn sync_one(
    service: &Service,
    account: &AccountConfig,
    engine: &Arc<dyn ExchangeEngine>,
    mailbox: Option<&str>,
    folder: &ExchangeFolderRecord,
    state: &mut LoopState,
) -> Result<bool> {
    let key = folder.folder_id.as_str().to_owned();
    match sync_folder(service, account, engine.as_ref(), folder).await {
        Ok(FolderSync { changes, removed }) => {
            state.folders.observe(&key, changes);
            state.poll.observe_changes(changes);
            if folder.cursor.is_some() {
                state.folders.track(
                    &key,
                    mailbox.is_none() && folder.well_known == Some(ExchangeWellKnownFolder::Inbox),
                );
            }
            Ok(removed)
        }
        Err(err) if err.code() == ErrorCode::NotFound => {
            tracing::info!(account = %account.id, folder = %folder.remote_id, "folder gone");
            remove_folders(service, std::slice::from_ref(&folder.folder_id)).await?;
            state.folders.untrack(&key);
            Ok(false)
        }
        Err(err) => Err(err),
    }
}

/// The folder hierarchy of a mailbox into the store.
async fn sync_hierarchy(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    mailbox: Option<&str>,
) -> Result<()> {
    let (id, key) = (account.id.clone(), mailbox.map(str::to_owned));
    let mut cursor = service
        .store()
        .call(move |s| s.exchange_hierarchy_cursor(&id, key.as_deref()))
        .await?;
    let mut full = cursor.is_none();
    let mut upserted: Vec<ExchangeFolder> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut pages = 0u32;
    let final_cursor = loop {
        pages += 1;
        if pages > MAX_PAGES {
            return Err(
                ApiError::new(ErrorCode::Internal, "the folder listing does not end").into(),
            );
        }
        match engine
            .folder_hierarchy(mailbox, cursor.as_deref())
            .await
            .map_err(CoreError::from)?
        {
            HierarchyOutcome::CursorExpired => {
                if full {
                    return Err(ApiError::new(
                        ErrorCode::Internal,
                        "the server refused a full folder listing",
                    )
                    .into());
                }
                cursor = None;
                full = true;
                upserted.clear();
                removed.clear();
            }
            HierarchyOutcome::Delta(delta) => {
                upserted.extend(delta.upserted);
                removed.extend(delta.removed);
                if !delta.more {
                    break delta.cursor;
                }
                cursor = Some(delta.cursor);
            }
        }
    };
    apply_hierarchy(service, account, mailbox, &upserted, &removed, full).await?;
    let (id, key) = (account.id.clone(), mailbox.map(str::to_owned));
    let now = unix_now();
    service
        .store()
        .call(move |s| {
            s.set_exchange_hierarchy_cursor(&id, key.as_deref(), Some(&final_cursor), now)
        })
        .await?;
    Ok(())
}

/// Applies folder changes: rows (paths from the parent chain, roles from
/// well-known names for the own mailbox), Exchange records, removals.
async fn apply_hierarchy(
    service: &Service,
    account: &AccountConfig,
    mailbox: Option<&str>,
    upserted: &[ExchangeFolder],
    removed: &[String],
    full: bool,
) -> Result<()> {
    let id = account.id.clone();
    let key = mailbox.map(str::to_owned);
    let records: Vec<(ExchangeFolderRecord, String)> = service
        .store()
        .call(move |s| {
            let mut out = Vec::new();
            for record in s.exchange_folders(&id)? {
                if record.mailbox != key {
                    continue;
                }
                let name = s
                    .get_folder(&record.folder_id)
                    .map(|f| f.name)
                    .unwrap_or_default();
                out.push((record, name));
            }
            Ok(out)
        })
        .await?;
    let mut known: HashMap<String, KnownFolder> = HashMap::new();
    let mut cursors: HashMap<String, CursorState> = HashMap::new();
    for (record, name) in records {
        cursors.insert(
            record.remote_id.clone(),
            (
                record.cursor.clone(),
                record.full_sync_page.clone(),
                record.last_sync_at,
            ),
        );
        if !full {
            known.insert(
                record.remote_id.clone(),
                KnownFolder {
                    name,
                    parent: record.parent_remote_id.clone(),
                    well_known: record.well_known,
                    change_key: record.change_key.clone(),
                },
            );
        }
    }
    let before: HashSet<String> = cursors.keys().cloned().collect();
    for folder in upserted {
        if folder.class != ExchangeFolderClass::Mail {
            known.remove(&folder.remote_id);
            continue;
        }
        known.insert(
            folder.remote_id.clone(),
            KnownFolder {
                name: folder.name.clone(),
                parent: folder.parent.clone(),
                well_known: folder.well_known,
                change_key: folder.change_key.clone(),
            },
        );
    }
    for gone in removed {
        known.remove(gone);
    }
    // Paths from the parent chain (parents outside the mail tree: top level).
    let path_of = |remote: &str| -> (String, usize) {
        let mut parts = Vec::new();
        let mut at = Some(remote.to_owned());
        let mut guard = 0;
        while let Some(r) = at {
            guard += 1;
            let Some(k) = known.get(&r) else { break };
            parts.push(k.name.replace('/', "\u{2215}"));
            at = k.parent.clone().filter(|p| known.contains_key(p));
            if guard > 64 {
                break;
            }
        }
        parts.reverse();
        let depth = parts.len();
        (parts.join("/"), depth)
    };
    let mut rows: Vec<(usize, Folder, ExchangeFolderRecord)> = Vec::new();
    for (remote, k) in &known {
        let (display, depth) = path_of(remote);
        let folder_id = exchange_folder_id(&account.id, mailbox, remote);
        let parent_id = match k.parent.as_ref().filter(|p| known.contains_key(*p)) {
            Some(parent) => Some(exchange_folder_id(&account.id, mailbox, parent)),
            None => mailbox.map(|m| mailbox_root_id(&account.id, m)),
        };
        let role = if mailbox.is_none() {
            k.well_known
                .and_then(ExchangeWellKnownFolder::role)
                .unwrap_or(FolderRole::Other)
        } else {
            FolderRole::Other
        };
        let (cursor, page, at) = cursors.get(remote).cloned().unwrap_or_default();
        rows.push((
            depth,
            Folder {
                id: folder_id.clone(),
                account_id: account.id.clone(),
                parent_id,
                name: k.name.clone(),
                path: mailbox_folder_path(mailbox, &display),
                role,
                unread: 0,
                total: 0,
            },
            ExchangeFolderRecord {
                folder_id,
                mailbox: mailbox.map(str::to_owned),
                remote_id: remote.clone(),
                change_key: k.change_key.clone(),
                parent_remote_id: k.parent.clone(),
                well_known: k.well_known,
                cursor,
                full_sync_page: page,
                last_sync_at: at,
            },
        ));
    }
    rows.sort_by_key(|(depth, f, _)| (*depth, f.path.clone()));
    let gone: Vec<FolderId> = before
        .iter()
        .filter(|r| !known.contains_key(*r))
        .map(|r| exchange_folder_id(&account.id, mailbox, r))
        .collect();
    let (id, root) = (
        account.id.clone(),
        mailbox.map(|m| (mailbox_root_id(&account.id, m), m.to_owned())),
    );
    let changed_rows = service
        .store()
        .call(move |s| {
            let mut changed = false;
            if let Some((root_id, address)) = &root {
                let name = s
                    .exchange_mailboxes(&id, true)?
                    .into_iter()
                    .find(|m| &m.settings.address == address)
                    .and_then(|m| m.settings.name)
                    .unwrap_or_else(|| address.clone());
                s.upsert_folder(&Folder {
                    id: root_id.clone(),
                    account_id: id.clone(),
                    parent_id: None,
                    name,
                    path: mailbox_folder_path(Some(address), ""),
                    role: FolderRole::Other,
                    unread: 0,
                    total: 0,
                })?;
            }
            for (_, folder, record) in &rows {
                let existing = s.exchange_folder(&folder.id)?;
                let same = match (&existing, s.get_folder(&folder.id).ok()) {
                    (Some(e), Some(f)) => {
                        e == record
                            && f.path == folder.path
                            && f.name == folder.name
                            && f.parent_id == folder.parent_id
                    }
                    _ => false,
                };
                if !same {
                    s.upsert_exchange_folder(folder, record)?;
                    changed = true;
                }
            }
            Ok(changed)
        })
        .await?;
    if !gone.is_empty() {
        remove_folders(service, &gone).await?;
    }
    if changed_rows || !gone.is_empty() {
        service.broadcast(Event::MailChanged {
            folder_ids: Vec::new(),
        });
    }
    Ok(())
}

/// Removes folders with their messages (and from the index).
async fn remove_folders(service: &Service, folders: &[FolderId]) -> Result<()> {
    let list = folders.to_vec();
    let messages: Vec<MessageId> = service
        .store()
        .call(move |s| {
            let mut out = Vec::new();
            for folder in &list {
                if let Ok(page) = s.list_messages(folder, 0, u32::MAX) {
                    out.extend(page.messages.into_iter().map(|m| m.id));
                }
            }
            Ok(out)
        })
        .await?;
    crate::ingest::remove_messages(service, messages).await?;
    let list = folders.to_vec();
    service
        .store()
        .call(move |s| {
            for folder in &list {
                s.clear_exchange_listed(folder.as_str())?;
                s.delete_folder(folder)?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Forgets the local copy of another mailbox's folders (`RemoveExchangeMailbox`,
/// mail no longer shown).
pub(crate) async fn forget_mailbox(
    service: &Service,
    account: &AccountId,
    address: &str,
) -> Result<()> {
    let (id, key) = (account.clone(), address.trim().to_ascii_lowercase());
    let (folders, root) = service
        .store()
        .call(move |s| {
            let folders: Vec<FolderId> = s
                .exchange_folders(&id)?
                .into_iter()
                .filter(|f| f.mailbox.as_deref() == Some(key.as_str()))
                .map(|f| f.folder_id)
                .collect();
            s.set_exchange_hierarchy_cursor(&id, Some(&key), None, unix_now())?;
            Ok((folders, mailbox_root_id(&id, &key)))
        })
        .await?;
    let mut all = folders;
    all.push(root);
    remove_folders(service, &all).await?;
    service.broadcast(Event::MailChanged {
        folder_ids: Vec::new(),
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Items

/// What kind of full listing runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListingKind {
    /// The folder was never listed: what it holds is not new mail.
    First,
    /// Listed again (an expired cursor): merged with the store, and what
    /// arrived meanwhile is new mail.
    Full,
}

/// A full listing of a mail folder or PIM collection, kept across passes
/// (`full_sync_page`) until its last page arrived: an interrupted first
/// listing stays a first listing, and the merge runs only after the last
/// page (ADR 0078).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listing {
    pub kind: ListingKind,
    /// The page to go on with (`None` = from the start).
    pub resume: Option<String>,
}

impl Listing {
    pub(crate) fn new(kind: ListingKind) -> Self {
        Self { kind, resume: None }
    }

    /// The stored form (`first:<page>` / `full:<page>`).
    pub(crate) fn encode(&self) -> String {
        let kind = match self.kind {
            ListingKind::First => "first",
            ListingKind::Full => "full",
        };
        format!("{kind}:{}", self.resume.as_deref().unwrap_or(""))
    }

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let (kind, page) = text.split_once(':')?;
        let kind = match kind {
            "first" => ListingKind::First,
            "full" => ListingKind::Full,
            _ => return None,
        };
        Some(Self {
            kind,
            resume: (!page.is_empty()).then(|| page.to_owned()),
        })
    }

    /// The listing a folder needs: the one in progress, a first one
    /// (never listed), a full one (no cursor), or none (go on with the
    /// cursor).
    pub(crate) fn of(
        cursor: Option<&str>,
        page: Option<&str>,
        last_sync_at: Option<i64>,
    ) -> Option<Self> {
        if let Some(listing) = page.and_then(Self::decode) {
            return Some(listing);
        }
        if cursor.is_some() {
            return None;
        }
        Some(Self::new(if last_sync_at.is_none() {
            ListingKind::First
        } else {
            ListingKind::Full
        }))
    }

    /// Whether new messages of this listing are arrivals (rules, `NewMail`).
    pub(crate) fn arrivals(&self) -> bool {
        self.kind == ListingKind::Full
    }

    /// The listing after the server refused its cursor: a delta cursor
    /// starts a full listing; a listing that went on from a stored page
    /// starts again (once); a listing from the start fails (`None`).
    pub(crate) fn after_expiry(listing: Option<Self>, restarted: &mut bool) -> Option<Self> {
        match listing {
            None => Some(Self::new(ListingKind::Full)),
            Some(l) if l.resume.is_some() && !*restarted => {
                *restarted = true;
                Some(Self::new(l.kind))
            }
            Some(_) => None,
        }
    }
}

/// Stores a folder's cursor and listing in progress (`clear`: forget what
/// a listing saw — it starts or ended).
async fn save_folder_cursor(
    service: &Service,
    folder: &FolderId,
    cursor: Option<String>,
    listing: Option<&Listing>,
    clear: bool,
) -> Result<()> {
    let (id, page, now) = (folder.clone(), listing.map(Listing::encode), unix_now());
    service
        .store()
        .call(move |s| {
            if clear {
                s.clear_exchange_listed(id.as_str())?;
            }
            s.set_exchange_folder_cursor(&id, cursor.as_deref(), page.as_deref(), now)
        })
        .await?;
    Ok(())
}

/// What syncing one folder did.
#[derive(Debug, Clone, Copy)]
struct FolderSync {
    /// Something changed.
    changes: bool,
    /// Messages were marked removed (deleted, or moved elsewhere).
    removed: bool,
}

/// Syncs one folder's items.
async fn sync_folder(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    folder: &ExchangeFolderRecord,
) -> Result<FolderSync> {
    let mailbox = folder.mailbox.as_deref();
    let mut listing = Listing::of(
        folder.cursor.as_deref(),
        folder.full_sync_page.as_deref(),
        folder.last_sync_at,
    );
    let mut cursor = match &listing {
        Some(l) => l.resume.clone(),
        None => folder.cursor.clone(),
    };
    if listing.as_ref().is_some_and(|l| l.resume.is_none()) {
        save_folder_cursor(service, &folder.folder_id, None, listing.as_ref(), true).await?;
    }
    let mut restarted = false;
    let mut any = false;
    let mut removed = false;
    let mut pages = 0u32;
    loop {
        pages += 1;
        if pages > MAX_PAGES {
            // A runaway listing goes on in the next pass; a partial
            // listing is never merged.
            tracing::info!(account = %account.id, folder = %folder.remote_id, "page limit; going on later");
            break;
        }
        match engine
            .folder_items(mailbox, &folder.remote_id, cursor.as_deref(), None)
            .await
            .map_err(CoreError::from)?
        {
            DeltaOutcome::CursorExpired => {
                let was_delta = listing.is_none();
                listing = Listing::after_expiry(listing, &mut restarted);
                if listing.is_none() {
                    return Err(ApiError::new(
                        ErrorCode::Internal,
                        "the server refused a full listing",
                    )
                    .into());
                }
                if was_delta {
                    tracing::info!(account = %account.id, folder = %folder.remote_id, "cursor expired; listing again");
                }
                cursor = None;
                any = true;
                save_folder_cursor(service, &folder.folder_id, None, listing.as_ref(), true)
                    .await?;
            }
            DeltaOutcome::Delta(delta) => {
                any |= !(delta.added.is_empty()
                    && delta.changed.is_empty()
                    && delta.removed.is_empty());
                let arrivals = listing.as_ref().is_none_or(Listing::arrivals);
                removed |= apply_delta(service, account, engine, folder, &delta, arrivals).await?;
                let more = delta.more;
                match &mut listing {
                    None => {
                        save_folder_cursor(
                            service,
                            &folder.folder_id,
                            Some(delta.cursor.clone()),
                            None,
                            false,
                        )
                        .await?;
                    }
                    Some(l) => {
                        let seen: Vec<String> = delta
                            .added
                            .iter()
                            .map(|m| m.remote_id.clone())
                            .chain(delta.changed.iter().map(|m| m.remote_id.clone()))
                            .collect();
                        let (id, scope) = (account.id.clone(), folder.folder_id.clone());
                        service
                            .store()
                            .call(move |s| s.add_exchange_listed(&id, scope.as_str(), &seen))
                            .await?;
                        if more {
                            l.resume = Some(delta.cursor.clone());
                            save_folder_cursor(service, &folder.folder_id, None, Some(l), false)
                                .await?;
                        } else {
                            // The last page: what the server no longer
                            // lists is gone (or moved: see `flush_removed`);
                            // local-only messages without a server id stay.
                            let gone = merge_listing(service, &account.id, folder).await?;
                            any |= gone;
                            removed |= gone;
                            save_folder_cursor(
                                service,
                                &folder.folder_id,
                                Some(delta.cursor.clone()),
                                None,
                                true,
                            )
                            .await?;
                        }
                    }
                }
                if !more {
                    break;
                }
                cursor = Some(delta.cursor);
            }
        }
    }
    if any {
        service.broadcast(Event::MailChanged {
            folder_ids: vec![folder.folder_id.clone()],
        });
    }
    Ok(FolderSync {
        changes: any,
        removed,
    })
}

/// After the last page of a full listing: the folder's messages the
/// listing did not see are marked removed; `true` when there were any.
async fn merge_listing(
    service: &Service,
    account: &AccountId,
    folder: &ExchangeFolderRecord,
) -> Result<bool> {
    let (id, folder_id, mailbox, now) = (
        account.clone(),
        folder.folder_id.clone(),
        folder.mailbox.clone(),
        unix_now(),
    );
    Ok(service
        .store()
        .call(move |s| {
            let gone = s.exchange_unlisted_messages(&folder_id)?;
            for m in &gone {
                s.mark_exchange_removed(&id, mailbox.as_deref(), m, now)?;
            }
            Ok(!gone.is_empty())
        })
        .await?)
}

/// Deletes the messages of a mailbox still marked removed after every
/// folder of it was synced (they did not show up anywhere else).
async fn flush_removed(
    service: &Service,
    account: &AccountId,
    mailbox: Option<&str>,
) -> Result<()> {
    let (id, key) = (account.clone(), mailbox.map(str::to_owned));
    let gone = service
        .store()
        .call(move |s| s.exchange_removed(&id, key.as_deref()))
        .await?;
    if gone.is_empty() {
        return Ok(());
    }
    crate::ingest::remove_messages(service, gone).await?;
    service.broadcast(Event::MailChanged {
        folder_ids: Vec::new(),
    });
    Ok(())
}

/// The master category list as the mapping wants it.
async fn master(service: &Service, account: &AccountId) -> Result<Vec<ExchangeCategorySettings>> {
    let id = account.clone();
    Ok(service
        .store()
        .call(move |s| s.exchange_category_records(&id))
        .await?
        .into_iter()
        .map(|r| ExchangeCategorySettings {
            name: r.name,
            color: r.color,
        })
        .collect())
}

/// Applies one page of item changes; `true` when it marked messages
/// removed.
async fn apply_delta(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    folder: &ExchangeFolderRecord,
    delta: &SyncDelta,
    arrivals: bool,
) -> Result<bool> {
    let master = master(service, &account.id).await?;
    store_messages(
        service,
        account,
        engine,
        folder,
        &delta.added,
        &master,
        arrivals,
    )
    .await?;
    let mut unknown = Vec::new();
    for change in &delta.changed {
        if !apply_change(service, &account.id, folder, change, &master).await? {
            unknown.push(change.remote_id.clone());
        }
    }
    // A change of a message Nail does not have: fetch it as new.
    let fresh: Vec<RemoteMessage> = unknown
        .into_iter()
        .map(|remote_id| RemoteMessage {
            remote_id,
            thread_id: None,
            folder: folder.remote_id.clone(),
            flags: nail_api::Flags::default(),
            tags: Vec::new(),
            internal_date: None,
            size: None,
            raw: None,
            gmail: None,
            change_key: None,
            exchange: None,
        })
        .collect();
    store_messages(service, account, engine, folder, &fresh, &master, arrivals).await?;
    // Removed from this folder: deleted, or moved by another client (or a
    // server rule). Marked until every folder of the mailbox was synced
    // (`flush_removed`), so a moved message is moved locally when it shows
    // up in its new folder. A message already moved elsewhere stays.
    let (id, removed, folder_id, mailbox, now) = (
        account.id.clone(),
        delta.removed.clone(),
        folder.folder_id.clone(),
        folder.mailbox.clone(),
        unix_now(),
    );
    Ok(service
        .store()
        .call(move |s| {
            let mut marked = false;
            for remote in &removed {
                if let Some(m) = s.message_by_remote_id(&id, remote)?
                    && s.get_message(&m)?.summary.folder_id == folder_id
                {
                    s.mark_exchange_removed(&id, mailbox.as_deref(), &m, now)?;
                    marked = true;
                }
            }
            Ok(marked)
        })
        .await?)
}

/// The Exchange extras a message keeps (Focused Inbox only in the Inbox).
fn item_record(
    folder: &ExchangeFolderRecord,
    change_key: Option<&String>,
    info: Option<&nail_api::exchange::ExchangeMessageInfo>,
) -> ExchangeItemRecord {
    ExchangeItemRecord {
        mailbox: folder.mailbox.clone(),
        change_key: change_key.cloned(),
        conversation_id: info.and_then(|i| i.conversation_id.clone()),
        inference: nail_exchange::mapping::focus::stored_inference(
            folder.well_known.filter(|_| folder.mailbox.is_none()),
            info.and_then(|i| i.inference),
        ),
    }
}

/// A metadata change of a stored message; `false` when it is unknown.
async fn apply_change(
    service: &Service,
    account: &AccountId,
    folder: &ExchangeFolderRecord,
    change: &RemoteMessageChange,
    master: &[ExchangeCategorySettings],
) -> Result<bool> {
    let (id, change, folder, master) = (
        account.clone(),
        change.clone(),
        folder.clone(),
        master.to_vec(),
    );
    Ok(service
        .store()
        .call(move |s| {
            let Some(message_id) = s.message_by_remote_id(&id, &change.remote_id)? else {
                return Ok(false);
            };
            apply_change_to(s, &id, &message_id, &folder, &change, &master)?;
            Ok(true)
        })
        .await?)
}

/// Applies a server change to the stored message `message_id` (moving it
/// to the change's folder; a message marked removed is on the server
/// after all).
fn apply_change_to(
    s: &nail_store::Store,
    id: &AccountId,
    message_id: &MessageId,
    folder: &ExchangeFolderRecord,
    change: &RemoteMessageChange,
    master: &[ExchangeCategorySettings],
) -> nail_store::Result<()> {
    s.clear_exchange_removed(message_id)?;
    let mut stored = s.get_message(message_id)?;
    let target = match &change.folder {
        Some(remote) => s
            .exchange_folder_by_remote(id, folder.mailbox.as_deref(), remote)?
            .unwrap_or_else(|| folder.clone()),
        None => folder.clone(),
    };
    stored.summary.folder_id = target.folder_id.clone();
    if let Some(flags) = change.flags {
        stored.summary.flags = flags;
    }
    if let Some(tags) = &change.tags {
        let mut keep: Vec<String> = stored
            .summary
            .tags
            .iter()
            .filter(|t| !nail_exchange::mapping::categories::is_category_name(t))
            .cloned()
            .collect();
        keep.extend(nail_exchange::mapping::categories::tags_for_categories(
            tags, master,
        ));
        stored.summary.tags = keep;
    }
    s.merge_messages(&[SyncedMessage {
        message: stored,
        meta: None,
    }])?;
    let previous = s.exchange_item(message_id)?;
    let mut record = item_record(
        &target,
        change.change_key.as_ref(),
        change.exchange.as_ref(),
    );
    if change.exchange.is_none()
        && let Some(previous) = previous
    {
        record.conversation_id = previous.conversation_id;
        if target.well_known == Some(ExchangeWellKnownFolder::Inbox) {
            record.inference = previous.inference;
        }
    }
    s.set_exchange_item(message_id, &record)?;
    Ok(())
}

/// A message another client moved, found again in `folder` under its new
/// server id: the stored copy (with its local state) moves there instead
/// of a second copy being downloaded (ADR 0078). Candidates carry the same
/// `Message-ID` in the same mailbox: first one marked removed (its old
/// folder reported it gone); else, outside a first listing, one in another
/// folder whose old server id no longer exists. `None`: a new message.
async fn moved_copy(
    service: &Service,
    account: &AccountId,
    engine: &dyn ExchangeEngine,
    folder: &ExchangeFolderRecord,
    raw: &[u8],
    probe: bool,
) -> Result<Option<MessageId>> {
    let Some(message_id) = nail_mime::parse_message(raw).headers.message_id else {
        return Ok(None);
    };
    let (id, mailbox, folder_id, mid) = (
        account.clone(),
        folder.mailbox.clone(),
        folder.folder_id.clone(),
        message_id.clone(),
    );
    let (removed, others) = service
        .store()
        .call(move |s| {
            let removed = s.exchange_removed_by_message_id(&id, mailbox.as_deref(), &mid)?;
            let mut others = Vec::new();
            if removed.is_empty() {
                for m in s.find_messages_by_message_id(&mid)? {
                    if m.account_id != id || m.folder_id == folder_id {
                        continue;
                    }
                    let same_mailbox = s.exchange_item(&m.id)?.and_then(|i| i.mailbox) == mailbox;
                    if let (true, Some(remote)) = (same_mailbox, s.message_remote_id(&m.id)?) {
                        others.push((m.id, remote));
                    }
                }
            }
            Ok((removed, others))
        })
        .await?;
    if let Some(first) = removed.into_iter().next() {
        return Ok(Some(first));
    }
    if !probe {
        return Ok(None);
    }
    for (candidate, remote) in others {
        match engine.fetch_mime(folder.mailbox.as_deref(), &remote).await {
            // Still there: a copy, not the moved message.
            Ok(_) => {}
            Err(err) if err.code == ErrorCode::NotFound => return Ok(Some(candidate)),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(None)
}

/// Stores listed / new messages: known ones get the server's metadata,
/// new ones are fetched as MIME and ingested.
async fn store_messages(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
    folder: &ExchangeFolderRecord,
    messages: &[RemoteMessage],
    master: &[ExchangeCategorySettings],
    arrivals: bool,
) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let local_folder = {
        let id = folder.folder_id.clone();
        service.store().call(move |s| s.get_folder(&id)).await?
    };
    // M4c: the folder's sync policy decides per message whether the body
    // is downloaded now or when it is opened.
    let policy =
        nail_exchange::policy::effective(&account.sync, &local_folder.path, local_folder.role);
    let now = unix_now();
    let mut new_ids: Vec<MessageId> = Vec::new();
    // Only mail arriving in the own mailbox's inbox or a user folder counts.
    let counts_as_new =
        folder.mailbox.is_none() && crate::sync::runs_arrival_rules(local_folder.role);
    for chunk in messages.chunks(API_INGEST_CHUNK) {
        let mut fresh = Vec::new();
        for m in chunk {
            let change = RemoteMessageChange {
                remote_id: m.remote_id.clone(),
                folder: Some(folder.remote_id.clone()),
                flags: Some(m.flags),
                tags: Some(m.tags.clone()),
                gmail: None,
                change_key: m.change_key.clone(),
                exchange: m.exchange.clone(),
            };
            if apply_change(service, &account.id, folder, &change, master).await? {
                continue;
            }
            let headers_only = m.raw.is_none()
                && nail_exchange::policy::body_fetch(policy.bodies, m.internal_date, now)
                    == nail_exchange::policy::BodyFetch::HeadersOnly;
            let raw = if let Some(raw) = &m.raw {
                raw.clone()
            } else {
                let fetched = if headers_only {
                    engine
                        .fetch_headers(folder.mailbox.as_deref(), &m.remote_id)
                        .await
                } else {
                    engine
                        .fetch_mime(folder.mailbox.as_deref(), &m.remote_id)
                        .await
                };
                match fetched {
                    Ok(raw) => raw,
                    Err(err) if err.code == ErrorCode::NotFound => continue,
                    Err(err) => return Err(err.into()),
                }
            };
            if let Some(moved) =
                moved_copy(service, &account.id, engine, folder, &raw, arrivals).await?
            {
                tracing::debug!(account = %account.id, message = ?moved, "moved on the server");
                let (id, f, master) = (account.id.clone(), folder.clone(), master.to_vec());
                let remote = m.remote_id.clone();
                service
                    .store()
                    .call(move |s| {
                        if let Some(old) = s.message_remote_id(&moved)? {
                            s.rebase_remote_ids(&moved, &old, &remote)?;
                        }
                        s.set_message_remote_id(&moved, Some(&remote))?;
                        apply_change_to(s, &id, &moved, &f, &change, &master)
                    })
                    .await?;
                continue;
            }
            let id = exchange_message_id(&account.id, &m.remote_id);
            let tags = nail_exchange::mapping::categories::tags_for_categories(&m.tags, master);
            let incoming = IncomingMessage {
                uid: None,
                id: Some(id.clone()),
                flags: m.flags,
                keywords: tags,
                internal_date: nail_util::time::rfc3339_utc_opt(m.internal_date),
                raw,
                headers_only,
                size: m.size,
                remote_thread_id: m.thread_id.clone(),
            };
            let extras = RemoteExtras {
                remote_id: Some(m.remote_id.clone()),
                exchange: Some(item_record(
                    folder,
                    m.change_key.as_ref(),
                    m.exchange.as_ref(),
                )),
                gmail: None,
            };
            fresh.push((incoming, extras));
        }
        if !fresh.is_empty() {
            let report = ingest_api_messages(service, local_folder.clone(), fresh).await?;
            if counts_as_new {
                new_ids.extend(report.new);
            }
        }
    }
    if arrivals && !new_ids.is_empty() {
        let queued = new_ids.clone();
        crate::rules::queue_arrivals(service, queued).await;
        if local_folder.role == FolderRole::Inbox {
            service.broadcast(Event::NewMail {
                account_id: account.id.clone(),
                folder_id: local_folder.id.clone(),
                message_ids: new_ids,
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Replay

/// Replays the mail queue and the Exchange queue; `true` when Exchange
/// operations ran (the settings are read again).
pub(crate) async fn replay(
    service: &Service,
    account: &AccountId,
    running: &Arc<RunningEngine>,
) -> bool {
    let executor = ExchangeExecutor {
        service,
        engine: Arc::clone(&running.engine),
        batch: running.batch.clone(),
    };
    tracing::debug!(%account, "Exchange replay: mail");
    if let Err(err) = replay_account(service, account, &executor).await {
        tracing::warn!(%account, %err, "replaying Exchange writes failed");
    }
    tracing::debug!(%account, "Exchange replay: Exchange operations");
    match replay_exchange_ops(service, account, running.engine.as_ref()).await {
        Ok(ran) => ran,
        Err(err) => {
            tracing::info!(%account, %err, "Exchange operations deferred");
            false
        }
    }
}

/// Replays the Exchange operation queue, one at a time (a create's id
/// moves later operations to the server's id).
async fn replay_exchange_ops(
    service: &Service,
    account: &AccountId,
    engine: &dyn ExchangeEngine,
) -> Result<bool> {
    let mut areas: Vec<ExchangeArea> = Vec::new();
    let mut ran = false;
    let mut mail = false;
    for _ in 0..1000 {
        let id = account.clone();
        let pending = service
            .store()
            .call(move |s| s.pending_exchange_ops(&id))
            .await?;
        let Some(op) = pending.into_iter().next() else {
            break;
        };
        ran = true;
        tracing::debug!(%account, op = ?op.op, "Exchange operation");
        let target = op.target.clone().unwrap_or_default();
        let (result, area) = match &op.op {
            ExchangeOp::CreateCategory { settings } => (
                engine.create_category(settings).await.map(Some),
                ExchangeArea::Categories,
            ),
            ExchangeOp::UpdateCategory { settings, .. } => (
                engine.update_category(&target, settings).await,
                ExchangeArea::Categories,
            ),
            ExchangeOp::DeleteCategory { .. } => (
                engine.delete_category(&target).await.map(|()| None),
                ExchangeArea::Categories,
            ),
            ExchangeOp::SetAutomaticReplies { settings } => (
                engine.set_automatic_replies(settings).await.map(|()| None),
                ExchangeArea::AutomaticReplies,
            ),
            ExchangeOp::SetRuleEnabled {
                server_rule_id,
                enabled,
            } => {
                let rule = super::settings::server_rule(service, account, server_rule_id).await;
                let result = match rule {
                    Ok(mut rule) => {
                        rule.enabled = *enabled;
                        engine.update_inbox_rule(&rule).await.map(|()| None)
                    }
                    // Gone meanwhile: nothing to switch.
                    Err(err) if err.code() == ErrorCode::NotFound => Ok(None),
                    Err(err) => Err(EngineError::from(err)),
                };
                (result, ExchangeArea::Rules)
            }
            ExchangeOp::DeleteRule { server_rule_id } => (
                engine
                    .delete_inbox_rule(server_rule_id)
                    .await
                    .map(|()| None),
                ExchangeArea::Rules,
            ),
            ExchangeOp::Classify {
                message_ids,
                classification,
                always,
            } => {
                mail = true;
                (
                    classify(service, engine, message_ids, *classification, *always)
                        .await
                        .map(|()| None),
                    ExchangeArea::Status,
                )
            }
        };
        let (outcome, stop) = match result {
            Ok(server_id) => (ExchangeOpOutcome::Done { server_id }, false),
            Err(err) if err.retryable && op.attempts < 5 => {
                (ExchangeOpOutcome::Retry(err.message.clone()), true)
            }
            Err(err) => {
                tracing::warn!(%account, error = %err, "an Exchange operation failed");
                (ExchangeOpOutcome::Failed(err.message.clone()), false)
            }
        };
        let row = op.id;
        service
            .store()
            .call(move |s| s.finish_exchange_op(row, &outcome))
            .await?;
        if !areas.contains(&area) {
            areas.push(area);
        }
        if stop {
            break;
        }
    }
    if ran {
        if !areas.contains(&ExchangeArea::Status) {
            areas.push(ExchangeArea::Status);
        }
        changed(service, account, &areas);
        if mail || areas.contains(&ExchangeArea::Categories) {
            service.broadcast(Event::MailChanged {
                folder_ids: Vec::new(),
            });
        }
    }
    Ok(ran)
}

/// Moves messages between Focused and Other on the server (per mailbox).
async fn classify(
    service: &Service,
    engine: &dyn ExchangeEngine,
    messages: &[MessageId],
    classification: nail_api::exchange::InferenceClassification,
    always: bool,
) -> Result<(), EngineError> {
    let ids = messages.to_vec();
    let found: Vec<(Option<String>, String)> = service
        .store()
        .call(move |s| {
            let mut out = Vec::new();
            for id in &ids {
                let Ok(Some(remote)) = s.message_remote_id(id) else {
                    continue;
                };
                let mailbox = s.exchange_item(id)?.and_then(|i| i.mailbox);
                out.push((mailbox, remote));
            }
            Ok(out)
        })
        .await
        .map_err(|e| EngineError::from(CoreError::from(e)))?;
    let mut by_mailbox: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for (mailbox, remote) in found {
        by_mailbox.entry(mailbox).or_default().push(remote);
    }
    for (mailbox, remotes) in by_mailbox {
        engine
            .classify(mailbox.as_deref(), &remotes, classification, always)
            .await?;
    }
    Ok(())
}

/// Replays queued mail writes of an Exchange account through its engine.
pub(crate) struct ExchangeExecutor<'a> {
    pub service: &'a Service,
    pub engine: Arc<dyn ExchangeEngine>,
    /// Grouped writes (P3); `None` = one write at a time.
    pub batch: Option<Arc<dyn super::batch::BatchWrites>>,
}

/// What follows a grouped write of an operation (P3).
enum AfterWrite {
    /// Nothing.
    Nothing,
    /// The message has a new remote id in this folder (a move).
    Moved(Option<ExchangeFolderRecord>),
}

/// A grouped write: the mailbox, the write and what follows it.
struct GroupedWrite {
    mailbox: Option<String>,
    write: RemoteWrite,
    after: AfterWrite,
}

/// The grouped writes of one mailbox (index in the group, write, what
/// follows).
type MailboxWrites = (Option<String>, Vec<(usize, RemoteWrite, AfterWrite)>);

/// A grouped write (with the others of the same mailbox).
fn grouped(
    mailbox: Option<String>,
    write: RemoteWrite,
    after: AfterWrite,
) -> Grouped<Box<GroupedWrite>> {
    Grouped::Write(Box::new(GroupedWrite {
        mailbox,
        write,
        after,
    }))
}

/// Most operations one Exchange replay sends together.
const EXCHANGE_GROUP: usize = 200;

impl ExchangeExecutor<'_> {
    /// The mailbox and remote id of a local folder; a mailbox root
    /// (`@<address>`, no remote folder) answers `None` as the id.
    async fn remote_folder(
        &self,
        folder: &Folder,
    ) -> Result<(Option<String>, Option<String>), EngineError> {
        let id = folder.id.clone();
        let record = self
            .service
            .store()
            .call(move |s| s.exchange_folder(&id))
            .await
            .map_err(|e| EngineError::from(CoreError::from(e)))?;
        match record {
            Some(r) => Ok((r.mailbox, Some(r.remote_id))),
            None => match folder.path.strip_prefix('@') {
                Some(address) if !address.contains('/') => Ok((Some(address.to_lowercase()), None)),
                _ => Err(EngineError::permanent(
                    ErrorCode::NotFound,
                    format!("folder {} has no Exchange folder", folder.id),
                )),
            },
        }
    }

    /// Creates, renames, moves or deletes a folder (ADR 0196), then reads
    /// the mailbox's folder tree again.
    async fn change_folder(
        &self,
        change: &crate::folders::FolderChange,
    ) -> Result<crate::folders::FolderApplied, EngineError> {
        use crate::folders::FolderChange;
        use nail_api::exchange::engine::FolderWrite;
        let (account, mailbox, writes) = match change {
            FolderChange::Create {
                account,
                parent,
                name,
            } => {
                let (mailbox, parent) = match parent {
                    Some(p) => self.remote_folder(p).await?,
                    None => (None, None),
                };
                let write = FolderWrite::Create {
                    parent,
                    name: name.clone(),
                };
                (account.clone(), mailbox, vec![write])
            }
            FolderChange::Update {
                folder,
                parent,
                name,
            } => {
                let (mailbox, remote) = self.remote_folder(folder).await?;
                let remote = remote.ok_or_else(|| {
                    EngineError::permanent(
                        ErrorCode::InvalidRequest,
                        "a mailbox root cannot change",
                    )
                })?;
                let mut writes = Vec::new();
                if name != &folder.name {
                    writes.push(FolderWrite::Rename {
                        folder: remote.clone(),
                        name: name.clone(),
                    });
                }
                if parent.as_ref().map(|p| &p.id) != folder.parent_id.as_ref() {
                    let (target_mailbox, target) = match parent {
                        Some(p) => self.remote_folder(p).await?,
                        None => (mailbox.clone(), None),
                    };
                    if target_mailbox != mailbox {
                        return Err(EngineError::permanent(
                            ErrorCode::InvalidRequest,
                            "a folder cannot move into another mailbox",
                        ));
                    }
                    writes.push(FolderWrite::Move {
                        folder: remote,
                        parent: target,
                    });
                }
                (folder.account_id.clone(), mailbox, writes)
            }
            FolderChange::Delete { folder, .. } => {
                let (mailbox, remote) = self.remote_folder(folder).await?;
                let remote = remote.ok_or_else(|| {
                    EngineError::permanent(
                        ErrorCode::InvalidRequest,
                        "a mailbox root cannot change",
                    )
                })?;
                (
                    folder.account_id.clone(),
                    mailbox,
                    vec![FolderWrite::Delete { folder: remote }],
                )
            }
        };
        for write in &writes {
            self.engine.folder_write(mailbox.as_deref(), write).await?;
        }
        let config = self
            .service
            .account_config(&account)
            .map_err(EngineError::from)?;
        sync_hierarchy(
            self.service,
            &config,
            self.engine.as_ref(),
            mailbox.as_deref(),
        )
        .await
        .map_err(EngineError::from)?;
        Ok(crate::folders::FolderApplied::Synced)
    }

    async fn remote_of(
        &self,
        message: &MessageId,
    ) -> Result<Option<(Option<String>, String)>, EngineError> {
        let id = message.clone();
        self.service
            .store()
            .call(move |s| {
                let remote = match s.message_remote_id(&id) {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(None),
                    Err(err) if err.is_not_found() => return Ok(None),
                    Err(err) => return Err(err),
                };
                let mailbox = s.exchange_item(&id)?.and_then(|i| i.mailbox);
                Ok(Some((mailbox, remote)))
            })
            .await
            .map_err(|e| EngineError::from(CoreError::from(e)))
    }

    /// The remote id of an operation's message: the recorded one, else the
    /// row's.
    async fn remote_id_of(
        &self,
        message: &MessageId,
        source: Option<&nail_store::MessageSource>,
    ) -> Result<Option<String>, EngineError> {
        recorded_or(source, async {
            Ok(self.remote_of(message).await?.map(|(_, r)| r))
        })
        .await
    }

    /// Deletes a message on the server: to Trash (the copy there keeps
    /// the message's id) or for good.
    async fn delete(
        &self,
        message_id: &MessageId,
        source: Option<&nail_store::MessageSource>,
        trash_folder_id: Option<&FolderId>,
    ) -> Result<(), EngineError> {
        let Some((remote_id, mailbox)) = self.delete_target(message_id, source).await? else {
            return Ok(());
        };
        let write = if trash_folder_id.is_some() {
            RemoteWrite::Trash { remote_id }
        } else {
            RemoteWrite::Delete { remote_id }
        };
        let outcome = self.engine.write(mailbox.as_deref(), &write).await?;
        if let (
            WriteOutcome::Done {
                remote_id: Some(new),
            },
            Some(trash),
        ) = (outcome, trash_folder_id)
        {
            let folder = self.folder(trash).await?;
            self.set_remote(message_id, &new, folder.as_ref()).await;
        }
        Ok(())
    }

    /// What a deletion names: the message's remote id and the mailbox of
    /// the folder it was in; `None` when the server never had it.
    async fn delete_target(
        &self,
        message: &MessageId,
        source: Option<&nail_store::MessageSource>,
    ) -> Result<Option<(String, Option<String>)>, EngineError> {
        let Some(remote_id) = self.remote_id_of(message, source).await? else {
            return Ok(None);
        };
        let mailbox = match source {
            Some(source) => self
                .folder(&source.folder_id)
                .await?
                .and_then(|f| f.mailbox),
            None => None,
        };
        Ok(Some((remote_id, mailbox)))
    }

    async fn folder(&self, id: &FolderId) -> Result<Option<ExchangeFolderRecord>, EngineError> {
        let id = id.clone();
        self.service
            .store()
            .call(move |s| s.exchange_folder(&id))
            .await
            .map_err(|e| EngineError::from(CoreError::from(e)))
    }

    async fn set_remote(
        &self,
        message: &MessageId,
        remote: &str,
        folder: Option<&ExchangeFolderRecord>,
    ) {
        let (id, remote, folder) = (message.clone(), remote.to_owned(), folder.cloned());
        drop(
            self.service
                .store()
                .call(move |s| {
                    // EWS gives a moved item a new id: operations queued
                    // meanwhile (an undo) recorded the old one.
                    if let Some(old) = s.message_remote_id(&id)? {
                        s.rebase_remote_ids(&id, &old, &remote)?;
                    }
                    s.set_message_remote_id(&id, Some(&remote))?;
                    // A listing of the target in progress must not take the
                    // moved message for one the server no longer has.
                    if let Some(folder) = &folder
                        && folder.full_sync_page.is_some()
                    {
                        let account = s.get_message(&id)?.summary.account_id;
                        s.add_exchange_listed(
                            &account,
                            folder.folder_id.as_str(),
                            std::slice::from_ref(&remote),
                        )?;
                    }
                    if let Some(folder) = folder
                        && let Some(mut item) = s.exchange_item(&id)?
                    {
                        item.mailbox = folder.mailbox;
                        if folder.well_known != Some(ExchangeWellKnownFolder::Inbox) {
                            item.inference = None;
                        }
                        s.set_exchange_item(&id, &item)?;
                    }
                    Ok(())
                })
                .await,
        );
    }

    /// The write of a grouped operation: a flag change, a move within one
    /// mailbox or a delete; anything else goes on its own.
    async fn group_write(&self, op: &QueuedOp) -> Result<Grouped<Box<GroupedWrite>>, EngineError> {
        match &op.operation {
            Operation::UpdateFlags { message_id, change } => {
                let Some((mailbox, remote_id)) = self.remote_of(message_id).await? else {
                    return Ok(Grouped::Nothing);
                };
                Ok(grouped(
                    mailbox,
                    RemoteWrite::SetFlags {
                        remote_id,
                        change: category_change(change),
                    },
                    AfterWrite::Nothing,
                ))
            }
            Operation::MoveMessage {
                message_id,
                target_folder_id,
                source: Some(source),
            } => {
                let remote = self.remote_id_of(message_id, Some(source)).await?;
                let Some(remote_id) = remote else {
                    return Ok(Grouped::Nothing);
                };
                let (Some(from), Some(to)) = (
                    self.folder(&source.folder_id).await?,
                    self.folder(target_folder_id).await?,
                ) else {
                    return Ok(Grouped::Alone);
                };
                let same_account = {
                    let (a, b) = (from.folder_id.clone(), to.folder_id.clone());
                    self.service
                        .store()
                        .call(move |s| {
                            Ok(s.get_folder(&a)?.account_id == s.get_folder(&b)?.account_id)
                        })
                        .await
                        .map_err(|e| EngineError::from(CoreError::from(e)))?
                };
                if !same_account || from.mailbox != to.mailbox {
                    return Ok(Grouped::Alone);
                }
                Ok(grouped(
                    from.mailbox.clone(),
                    RemoteWrite::Move {
                        remote_id,
                        from_folder: from.remote_id.clone(),
                        to_folder: to.remote_id.clone(),
                    },
                    AfterWrite::Moved(Some(to)),
                ))
            }
            Operation::DeleteMessage {
                message_id,
                source,
                trash_folder_id,
                ..
            } => {
                let Some((remote_id, mailbox)) =
                    self.delete_target(message_id, source.as_ref()).await?
                else {
                    return Ok(Grouped::Nothing);
                };
                Ok(match trash_folder_id {
                    Some(trash) => grouped(
                        mailbox,
                        RemoteWrite::Trash { remote_id },
                        AfterWrite::Moved(self.folder(trash).await?),
                    ),
                    None => grouped(
                        mailbox,
                        RemoteWrite::Delete { remote_id },
                        AfterWrite::Nothing,
                    ),
                })
            }
            _ => Ok(Grouped::Alone),
        }
    }

    /// A group of operations (P3, ADR 0164): the writes of each mailbox go
    /// to the engine together (Graph `$batch`, EWS multi-item requests).
    async fn run_group(&self, ops: &[QueuedOp]) -> Vec<Result<(), EngineError>> {
        let Some(batch) = &self.batch else {
            let mut out = Vec::with_capacity(ops.len());
            for op in ops {
                out.push(self.run(op).await);
            }
            return out;
        };
        let mut out = group_slots(ops.len());
        let mut by_mailbox: Vec<MailboxWrites> = Vec::new();
        let grouped = sort_group(ops, &mut out, |op| self.group_write(op), |op| self.run(op)).await;
        for (index, grouped) in grouped {
            let GroupedWrite {
                mailbox,
                write,
                after,
            } = *grouped;
            nail_util::group::push(&mut by_mailbox, mailbox, (index, write, after));
        }
        for (mailbox, list) in by_mailbox {
            let writes: Vec<RemoteWrite> = list.iter().map(|(_, w, _)| w.clone()).collect();
            let results = batch.write_many(mailbox.as_deref(), &writes).await;
            for ((index, _, after), result) in list.into_iter().zip(results) {
                let result = match (result, after) {
                    (
                        Ok(WriteOutcome::Done {
                            remote_id: Some(new),
                        }),
                        AfterWrite::Moved(folder),
                    ) => {
                        self.set_remote(ops[index].operation.message_id(), &new, folder.as_ref())
                            .await;
                        Ok(())
                    }
                    (Ok(_), _) => Ok(()),
                    (Err(err), _) => Err(err),
                };
                out[index] = Some(result);
            }
        }
        group_results(out)
    }

    async fn run(&self, op: &QueuedOp) -> Result<(), EngineError> {
        match &op.operation {
            Operation::UpdateFlags { message_id, change } => {
                let Some((mailbox, remote_id)) = self.remote_of(message_id).await? else {
                    return Ok(());
                };
                let write = RemoteWrite::SetFlags {
                    remote_id,
                    change: category_change(change),
                };
                self.engine.write(mailbox.as_deref(), &write).await?;
                Ok(())
            }
            Operation::MoveMessage {
                message_id,
                target_folder_id,
                source,
            } => {
                let remote = match source.as_ref().and_then(|s| s.remote_id.clone()) {
                    Some(r) => {
                        let mailbox = self.remote_of(message_id).await?.and_then(|(m, _)| m);
                        Some((mailbox, r))
                    }
                    None => self.remote_of(message_id).await?,
                };
                let Some((_, remote_id)) = remote else {
                    return Ok(());
                };
                let Some(from) = (match source {
                    Some(source) => self.folder(&source.folder_id).await?,
                    None => None,
                }) else {
                    return Ok(());
                };
                // M4c: into another account (Exchange or Gmail).
                let accounts = {
                    let (a, b) = (from.folder_id.clone(), target_folder_id.clone());
                    self.service
                        .store()
                        .call(move |s| {
                            Ok((s.get_folder(&a)?.account_id, s.get_folder(&b)?.account_id))
                        })
                        .await
                        .map_err(|e| EngineError::from(CoreError::from(e)))?
                };
                if accounts.0 != accounts.1 {
                    let Some(target) =
                        crate::api_moves::api_target(self.service, target_folder_id).await?
                    else {
                        return Err(crate::api_moves::unsupported_target());
                    };
                    let source = nail_exchange::transfer::ApiMailbox::Exchange {
                        engine: self.engine.as_ref(),
                        mailbox: from.mailbox.as_deref(),
                    };
                    return crate::api_moves::move_out(
                        self.service,
                        message_id,
                        source,
                        &remote_id,
                        &target,
                    )
                    .await;
                }
                let Some(to) = self.folder(target_folder_id).await? else {
                    return Err(EngineError::permanent(
                        ErrorCode::Unsupported,
                        "the target folder is not an Exchange folder",
                    ));
                };
                if from.mailbox != to.mailbox {
                    // M4c: into another mailbox of the same account.
                    let id = message_id.clone();
                    let flags = self
                        .service
                        .store()
                        .call(move |s| Ok(s.get_message(&id)?.summary.flags))
                        .await
                        .map_err(|e| EngineError::from(CoreError::from(e)))?;
                    let new = self
                        .engine
                        .move_to_mailbox(
                            from.mailbox.as_deref(),
                            &remote_id,
                            to.mailbox.as_deref(),
                            &to.remote_id,
                            flags,
                        )
                        .await?;
                    self.set_remote(message_id, &new, Some(&to)).await;
                    return Ok(());
                }
                let write = RemoteWrite::Move {
                    remote_id,
                    from_folder: from.remote_id.clone(),
                    to_folder: to.remote_id.clone(),
                };
                if let WriteOutcome::Done {
                    remote_id: Some(new),
                } = self.engine.write(from.mailbox.as_deref(), &write).await?
                {
                    self.set_remote(message_id, &new, Some(&to)).await;
                }
                Ok(())
            }
            Operation::DeleteMessage {
                message_id,
                source,
                trash_folder_id,
                ..
            } => {
                self.delete(message_id, source.as_ref(), trash_folder_id.as_ref())
                    .await
            }
            Operation::AppendMessage {
                message_id,
                folder_id,
            } => {
                let Some(folder) = self.folder(folder_id).await? else {
                    return Ok(());
                };
                let Some((raw, flags)) =
                    crate::replay::stored_raw(self.service, message_id).await?
                else {
                    return Ok(());
                };
                let write = RemoteWrite::Append {
                    folder: folder.remote_id.clone(),
                    raw,
                    flags,
                };
                if let WriteOutcome::Done {
                    remote_id: Some(remote),
                } = self.engine.write(folder.mailbox.as_deref(), &write).await?
                {
                    let (id, f) = (message_id.clone(), folder.clone());
                    let record = item_record(&f, None, None);
                    drop(
                        self.service
                            .store()
                            .call(move |s| {
                                s.set_message_remote_id(&id, Some(&remote))?;
                                s.set_exchange_item(&id, &record)
                            })
                            .await,
                    );
                }
                Ok(())
            }
            other => Err(EngineError::permanent(
                ErrorCode::Internal,
                format!("cannot replay {} on an Exchange account", other.kind()),
            )),
        }
    }
}

impl OpExecutor for ExchangeExecutor<'_> {
    fn execute<'a>(&'a self, op: &'a QueuedOp) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(self.run(op))
    }

    fn folder_change<'a>(
        &'a self,
        change: &'a crate::folders::FolderChange,
    ) -> BoxFuture<'a, Result<crate::folders::FolderApplied, EngineError>> {
        Box::pin(self.change_folder(change))
    }

    fn max_group(&self) -> usize {
        if self.batch.is_some() {
            EXCHANGE_GROUP
        } else {
            1
        }
    }

    fn execute_group<'a>(
        &'a self,
        ops: &'a [QueuedOp],
    ) -> BoxFuture<'a, Vec<Result<(), EngineError>>> {
        Box::pin(self.run_group(ops))
    }
}

/// A flag change as Exchange stores it: tags that are not category names
/// stay local.
fn category_change(change: &nail_store::FlagChange) -> nail_api::FlagChange {
    let keep = |tags: &[String]| -> Vec<String> {
        tags.iter()
            .filter(|t| nail_exchange::mapping::categories::is_category_name(t))
            .cloned()
            .collect()
    };
    nail_api::FlagChange {
        seen: change.seen,
        flagged: change.flagged,
        answered: change.answered,
        draft: change.draft,
        add_tags: keep(&change.add_tags),
        remove_tags: keep(&change.remove_tags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_listed_before_a_removal_are_synced_again() {
        let folder = |id: &str| ExchangeFolderRecord::new(id, format!("r-{id}"));
        let ids = |list: Vec<ExchangeFolderRecord>| -> Vec<String> {
            list.into_iter().map(|f| f.folder_id.0).collect()
        };
        let all = || vec![folder("a"), folder("b"), folder("inbox"), folder("c")];
        // Nothing removed: every folder synced in the pass counts.
        let mut since = SinceRemoval::default();
        for id in ["a", "b", "inbox", "c"] {
            since.synced(&id.into(), false);
        }
        assert!(since.stale(all()).is_empty(), "{:?}", since.stale(all()));
        // The inbox reports a message gone after `a` and `b` were listed:
        // the message may have been moved there after their listings.
        let mut since = SinceRemoval::default();
        since.synced(&"a".into(), false);
        since.synced(&"b".into(), false);
        since.synced(&"inbox".into(), true);
        since.synced(&"c".into(), false);
        assert_eq!(ids(since.stale(all())), ["a", "b"]);
        // A folder not synced in the pass at all is stale too.
        let mut since = SinceRemoval::default();
        since.synced(&"inbox".into(), true);
        assert_eq!(ids(since.stale(all())), ["a", "b", "c"]);
        // Syncing them again settles it, unless one of them reports a
        // removal itself: then the others go again.
        since.synced(&"a".into(), false);
        since.synced(&"b".into(), true);
        since.synced(&"c".into(), false);
        assert_eq!(ids(since.stale(all())), ["a", "inbox"]);
        since.synced(&"a".into(), false);
        since.synced(&"inbox".into(), false);
        assert!(since.stale(all()).is_empty(), "{:?}", since.stale(all()));
    }

    #[test]
    fn ids_are_stable_and_separate() {
        let account = AccountId::new("ex");
        assert_eq!(
            exchange_message_id(&account, "AAMk1"),
            exchange_message_id(&account, "AAMk1")
        );
        assert_ne!(
            exchange_message_id(&account, "AAMk1"),
            crate::gmail::gmail_message_id(&account, "AAMk1")
        );
        assert_ne!(
            exchange_folder_id(&account, None, "f1"),
            exchange_folder_id(&account, Some("support@contoso.example"), "f1")
        );
        assert_ne!(
            mailbox_root_id(&account, "support@contoso.example"),
            exchange_folder_id(&account, Some("support@contoso.example"), "")
        );
    }

    #[test]
    fn an_interrupted_first_listing_stays_a_first_listing() {
        // Never listed: a first listing (no arrivals).
        let first = Listing::of(None, None, None).unwrap();
        assert_eq!(first.kind, ListingKind::First);
        assert!(!first.arrivals());
        // Interrupted after a page: the next pass goes on with it, still
        // without arrivals, although a page was stored and synced.
        let mut going = first.clone();
        going.resume = Some("https://graph/next?$skiptoken=a:b".into());
        let stored = going.encode();
        let resumed = Listing::of(None, Some(&stored), Some(1_700_000_000)).unwrap();
        assert_eq!(resumed, going);
        assert!(!resumed.arrivals());
        // A cursor and no listing: deltas.
        assert_eq!(Listing::of(Some("delta"), None, Some(1)), None);
        // No cursor after a sync: listed again, with arrivals.
        let full = Listing::of(None, None, Some(1)).unwrap();
        assert_eq!(full.kind, ListingKind::Full);
        assert!(full.arrivals());
        assert_eq!(
            Listing::decode("full:"),
            Some(Listing::new(ListingKind::Full))
        );
        assert_eq!(Listing::decode("garbage"), None);
    }

    #[test]
    fn an_expired_cursor_during_a_listing_restarts_once_then_fails() {
        let mut restarted = false;
        // A delta cursor expired: a full listing.
        let full = Listing::after_expiry(None, &mut restarted).unwrap();
        assert_eq!(full.kind, ListingKind::Full);
        assert!(!restarted);
        // A stored page expired: the listing starts again (same kind).
        let going = Listing {
            kind: ListingKind::First,
            resume: Some("page".into()),
        };
        let again = Listing::after_expiry(Some(going.clone()), &mut restarted).unwrap();
        assert_eq!(again, Listing::new(ListingKind::First));
        // Again, or from the start: an error, never a merge of nothing.
        assert_eq!(Listing::after_expiry(Some(going), &mut restarted), None);
        let mut fresh = false;
        assert_eq!(
            Listing::after_expiry(Some(Listing::new(ListingKind::Full)), &mut fresh),
            None
        );
    }

    #[test]
    fn failed_streaming_waits_for_backoff_and_retry_after() {
        use std::time::Duration;
        let mut poll = PollTimer::new(&nail_api::exchange::ExchangeOptions::default());
        let mut backoff = Duration::from_millis(200);
        let max = Duration::from_secs(1);
        let err = EngineError::permanent(ErrorCode::Forbidden, "ErrorProxyRequestNotAllowed");
        assert_eq!(
            stream_retry_delay(&mut poll, &mut backoff, max, &err),
            Duration::from_millis(200)
        );
        assert_eq!(
            stream_retry_delay(&mut poll, &mut backoff, max, &err),
            Duration::from_millis(400)
        );
        let mut busy = EngineError::temporary(ErrorCode::Network, "busy");
        busy.retry_after_secs = Some(30);
        assert_eq!(
            stream_retry_delay(&mut poll, &mut backoff, max, &busy),
            Duration::from_secs(30)
        );
        assert_eq!(backoff, max);
        // The poll timer waits for the Retry-After too.
        poll.end_round();
        assert!(poll.next_delay() >= Duration::from_secs(30));
    }

    #[test]
    fn focused_inbox_is_kept_for_the_own_inbox_only() {
        let info = nail_api::exchange::ExchangeMessageInfo {
            item_id: "x".into(),
            conversation_id: Some("c".into()),
            inference: Some(nail_api::exchange::InferenceClassification::Other),
            mailbox: None,
        };
        let mut inbox = ExchangeFolderRecord::new("f", "inbox-id");
        inbox.well_known = Some(ExchangeWellKnownFolder::Inbox);
        let record = item_record(&inbox, None, Some(&info));
        assert_eq!(
            record.inference,
            Some(nail_api::exchange::InferenceClassification::Other)
        );
        assert_eq!(record.conversation_id.as_deref(), Some("c"));
        let mut shared = inbox.clone();
        shared.mailbox = Some("support@contoso.example".into());
        assert_eq!(item_record(&shared, None, Some(&info)).inference, None);
        let archive = ExchangeFolderRecord::new("g", "archive-id");
        assert_eq!(item_record(&archive, None, Some(&info)).inference, None);
    }
}
