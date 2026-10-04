//! Snooze (ADR 0048, logic in `nail-triage`, ADR 0045): the store as the
//! scheduler's [`SnoozeStore`], `SnoozeMessages` / `UnsnoozeMessages`, and
//! the wake task.
//!
//! Snoozing sets `$Snoozed` and moves the message into the hidden
//! `Snoozed` folder (queue writes; the folder is created on the server by
//! the first move into it); the wake time is local, so a message comes
//! back on time while offline, and after a restart the scheduler reloads
//! every snooze from the store (times that passed are due at once). The
//! wake task sleeps until the next wake time, at most
//! [`EngineTiming::snooze_poll`](crate::EngineTiming) so clock changes are
//! noticed.
//!
//! Waking is atomic (ADR 0049): the snooze records are removed in the same
//! store transaction that applies and queues the writes bringing the
//! messages back ([`Store::wake_snoozed`]); when that fails they stay
//! scheduled. Messages in the hidden folder that no record covers (a snooze
//! lost to an older version, or a resync that renumbered a message without
//! `Message-ID`) are returned to the Inbox: at the first poll after start
//! and whenever a snooze had to be forgotten.

use std::sync::{Arc, Mutex};

use nail_api::{ApiError, Event, FolderId, LocalizedMessage, MessageId, OpId};
use nail_store::Store;
use nail_triage::snooze::{SnoozeRequest, WakePlan};
use nail_triage::{Clock, SnoozeEntry, SnoozeScheduler, SnoozeStore};
use nail_triage::{FolderTarget, TriageOp};
use tokio::sync::Notify;

use crate::error::Result;
use crate::service::{Service, lock};

/// The store as the scheduler's persistence.
#[derive(Clone)]
pub(crate) struct StoreSnooze(pub Store);

impl SnoozeStore for StoreSnooze {
    type Error = nail_store::Error;

    fn load(&self) -> nail_store::Result<Vec<SnoozeEntry>> {
        Ok(self
            .0
            .list_snoozed(None)?
            .into_iter()
            .map(|r| SnoozeEntry {
                message_id: r.message_id,
                account_id: r.account_id,
                until: r.until,
                wake_folder_id: r.wake_folder_id,
                snoozed_at: r.snoozed_at,
                rfc_message_id: r.rfc_message_id,
            })
            .collect())
    }

    fn save(&self, entry: &SnoozeEntry) -> nail_store::Result<()> {
        // The store refuses past times; the scheduler checked the time with
        // its own clock, which may run ahead of the system clock (tests).
        let now = entry.snoozed_at.min(entry.until - 1);
        self.0
            .snooze(&entry.message_id, entry.until, &entry.wake_folder_id, now)?;
        Ok(())
    }

    fn remove(&self, messages: &[MessageId]) -> nail_store::Result<()> {
        self.0.unsnooze(messages)?;
        Ok(())
    }

    fn rebind(&self, old: &MessageId, new: &MessageId) -> nail_store::Result<bool> {
        self.0.rebind_snooze(old, new)
    }
}

type Scheduler = SnoozeScheduler<StoreSnooze, Arc<dyn Clock>>;

/// The scheduler (loaded on first use) and the wake task's signal.
#[derive(Default)]
pub(crate) struct SnoozeCtl {
    scheduler: Mutex<Option<Scheduler>>,
    /// Look for messages stranded in the hidden folder at the next poll.
    sweep: std::sync::atomic::AtomicBool,
    /// A snooze changed: the wake task recomputes its deadline.
    pub(crate) wake: Notify,
}

impl Service {
    /// Now, as the triage logic sees it (Unix seconds).
    pub(crate) fn triage_now(&self) -> i64 {
        self.inner.clock.unix()
    }

    /// Runs `work` on the scheduler (blocking; loads it on first use).
    fn with_scheduler<T>(&self, work: impl FnOnce(&mut Scheduler) -> Result<T>) -> Result<T> {
        let mut guard = lock(&self.inner.snooze.scheduler);
        let scheduler = if let Some(scheduler) = guard.take() {
            scheduler
        } else {
            let loaded = SnoozeScheduler::load(
                StoreSnooze(self.inner.store.clone()),
                Arc::clone(&self.inner.clock),
            )?;
            // First use after the start: sweep the hidden folders once.
            self.inner
                .snooze
                .sweep
                .store(true, std::sync::atomic::Ordering::Release);
            loaded
        };
        work(guard.insert(scheduler))
    }

    /// `SnoozeMessages`.
    pub(crate) async fn snooze_messages(
        &self,
        ids: Vec<MessageId>,
        until: &str,
        wake_folder: Option<FolderId>,
    ) -> Result<Vec<OpId>> {
        let until = chrono::DateTime::parse_from_rfc3339(until)
            .map_err(|_| ApiError::invalid_request(format!("invalid time {until:?}")))?
            .timestamp();
        if ids.is_empty() {
            return Err(ApiError::invalid_request("no messages to snooze").into());
        }
        let plan = self
            .blocking(move |service| {
                let store = service.store();
                let mut requests = Vec::with_capacity(ids.len());
                for id in &ids {
                    let message = store.get_message(id)?;
                    if let Some(folder) = &wake_folder {
                        let folder = store.get_folder(folder)?;
                        if folder.account_id != message.summary.account_id {
                            return Err(ApiError::invalid_request(format!(
                                "folder {} is not in the account of message {id}",
                                folder.id
                            ))
                            .into());
                        }
                    }
                    requests.push(SnoozeRequest {
                        message_id: id.clone(),
                        account_id: message.summary.account_id.clone(),
                        folder_id: message.summary.folder_id.clone(),
                        rfc_message_id: store.message_meta(id)?.message_id,
                        until,
                        wake_folder_id: wake_folder.clone(),
                    });
                }
                service.with_scheduler(|s| {
                    s.snooze(&requests).map_err(|err| {
                        let id = err.message_id();
                        ApiError::invalid_request(err.to_string())
                            .with_localized(LocalizedMessage::new(id))
                            .into()
                    })
                })
            })
            .await?;
        let op_ids = self.run_triage_ops(plan.ops).await?;
        self.inner.snooze.wake.notify_one();
        self.broadcast(Event::SnoozeChanged);
        Ok(op_ids)
    }

    /// `UnsnoozeMessages`: the messages come back now.
    pub(crate) async fn unsnooze_messages(&self, ids: Vec<MessageId>) -> Result<Vec<OpId>> {
        let plan = self
            .blocking(move |service| {
                let store = service.store().clone();
                service.with_scheduler(|s| {
                    Ok(s.unsnooze(&ids, &|f: &FolderId| store.get_folder(f).is_ok())?)
                })
            })
            .await?;
        self.woke(plan, false).await
    }

    /// Carries out a wake plan: removes the records and queues the writes
    /// in one transaction (scheduled again when that fails), then
    /// `SnoozeChanged` and (for messages that came back on their own) a
    /// notification.
    async fn woke(&self, plan: WakePlan, notify: bool) -> Result<Vec<OpId>> {
        if plan.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<MessageId> = plan
            .woken
            .iter()
            .map(|w| w.entry.message_id.clone())
            .collect();
        let committed = match self.triage_operations(plan.ops.clone()).await {
            Ok(operations) => self
                .inner
                .store
                .call(move |s| Ok(s.wake_snoozed(&ids, &operations)?.1))
                .await
                .map_err(crate::error::CoreError::from),
            Err(err) => Err(err),
        };
        let queued = match committed {
            Ok(queued) => queued,
            Err(err) => {
                let woken = plan.woken.clone();
                self.blocking(move |service| {
                    service.with_scheduler(|s| {
                        s.restore(&woken);
                        Ok(())
                    })
                })
                .await?;
                self.inner.snooze.wake.notify_one();
                return Err(err);
            }
        };
        let op_ids = self.announce_enqueued(&queued);
        self.broadcast(Event::SnoozeChanged);
        if notify {
            self.notify_snooze_woke(&plan.woken).await;
        }
        Ok(op_ids)
    }

    /// Wakes the messages that are due; returns how many came back.
    pub(crate) async fn poll_snoozes(&self) -> Result<usize> {
        let (plan, forgot) = self
            .blocking(|service| {
                let store = service.store().clone();
                service.with_scheduler(|s| {
                    let forgot = rebind_moved(&store, s)?;
                    Ok((s.poll(&|f: &FolderId| store.get_folder(f).is_ok())?, forgot))
                })
            })
            .await?;
        let count = plan.woken.len();
        self.woke(plan, true).await?;
        if forgot {
            self.inner
                .snooze
                .sweep
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let swept = if self
            .inner
            .snooze
            .sweep
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            self.sweep_snoozed_folders().await?
        } else {
            0
        };
        Ok(count + swept)
    }

    /// Returns messages stranded in the hidden Snoozed folders (no snooze
    /// record, no queued write) to their account's Inbox.
    async fn sweep_snoozed_folders(&self) -> Result<usize> {
        let orphans: Vec<(nail_api::AccountId, Vec<MessageId>)> = self
            .inner
            .store
            .call(|s| {
                let mut out = Vec::new();
                for account in s.list_accounts()? {
                    if let Some(folder) = s.snoozed_folder_of(&account.id)? {
                        let lost = s.orphaned_in_folder(&folder)?;
                        if !lost.is_empty() {
                            out.push((account.id, lost));
                        }
                    }
                }
                Ok(out)
            })
            .await?;
        let mut count = 0;
        for (account, ids) in orphans {
            tracing::warn!(
                %account,
                count = ids.len(),
                "returning messages without a snooze from the Snoozed folder to the Inbox"
            );
            count += ids.len();
            let ops = vec![
                TriageOp::RemoveKeyword {
                    message_ids: ids.clone(),
                    keyword: nail_api::triage::SNOOZE_KEYWORD.to_owned(),
                },
                TriageOp::Move {
                    message_ids: ids,
                    to: FolderTarget::Role {
                        account_id: account,
                        role: nail_api::FolderRole::Inbox,
                    },
                },
            ];
            self.run_triage_ops(ops).await?;
        }
        if count > 0 {
            self.broadcast(Event::SnoozeChanged);
        }
        Ok(count)
    }

    /// The next wake time, if anything is snoozed.
    pub(crate) async fn next_snooze_wake(&self) -> Result<Option<i64>> {
        self.blocking(|service| service.with_scheduler(|s| Ok(s.next_wake())))
            .await
    }
}

/// Snoozes whose message got a new id (a resync renumbered its folder)
/// follow it by `Message-ID`; snoozes whose message is gone are forgotten
/// (returns whether any was: the hidden folders are swept then, since the
/// message may sit there under a new id).
fn rebind_moved(store: &Store, scheduler: &mut Scheduler) -> Result<bool> {
    let entries: Vec<SnoozeEntry> = scheduler.list().into_iter().cloned().collect();
    let mut gone = Vec::new();
    for entry in entries {
        if store.get_message(&entry.message_id).is_ok() {
            continue;
        }
        let found = entry.rfc_message_id.as_deref().and_then(|rfc| {
            store
                .find_messages_by_message_id(rfc.trim_matches(|c| c == '<' || c == '>'))
                .ok()?
                .into_iter()
                .find(|m| m.account_id == entry.account_id)
                .map(|m| (rfc.to_owned(), m.id))
        });
        match found {
            Some((rfc, new_id)) => {
                scheduler.rebind_by_message_id(&entry.account_id, &rfc, &new_id)?;
            }
            None => gone.push(entry.message_id),
        }
    }
    if !gone.is_empty() {
        tracing::info!(
            count = gone.len(),
            "forgetting snoozes of messages that are gone"
        );
        scheduler.forget(&gone)?;
    }
    Ok(!gone.is_empty())
}

/// The wake task: sleeps until the next wake time (at most
/// `timing.snooze_poll`), then brings due messages back.
pub(crate) async fn run_snoozes(service: Service) {
    let cap = service.inner.timing.snooze_poll;
    loop {
        match service.poll_snoozes().await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "snoozed messages came back"),
            Err(err) => tracing::warn!(%err, "cannot wake snoozed messages"),
        }
        let next = service.next_snooze_wake().await.ok().flatten();
        let wait = next.map_or(cap, |at| {
            let secs = (at - service.triage_now()).max(0);
            std::time::Duration::from_secs(u64::try_from(secs).unwrap_or(0))
                .clamp(std::time::Duration::from_millis(50), cap)
        });
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = service.inner.snooze.wake.notified() => {}
        }
    }
}
