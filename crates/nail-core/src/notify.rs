//! New-mail notifications (ADR 0048, policy in `nail-triage`, ADR 0045):
//! arriving messages go through the policy (switch, quiet hours in the
//! local time zone, the rules' `notify` actions, junk, overrides, folder
//! choice), bursts are batched per account and folder, and each batch is
//! broadcast as [`Event::NotificationRequested`]. The desktop shell shows
//! it as an OS notification, the browser UI as a Web Notification.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nail_api::triage::MailNotification;
use nail_api::{AccountId, Event, FolderId, MessageId};
use nail_triage::notify::{self, Arrival, WokeMessage};
use nail_triage::snooze::WokenMessage;
use nail_triage::{Clock, NotificationBatcher};
use tokio::sync::Notify;

use crate::service::{Service, lock};

/// The batcher and the flush task's signal.
pub(crate) struct NotifyCtl {
    batcher: Mutex<NotificationBatcher>,
    /// Something was queued: the flush task recomputes its deadline.
    wake: Notify,
    sequence: AtomicU64,
}

impl NotifyCtl {
    pub(crate) fn new(timing: &crate::EngineTiming) -> Self {
        let to_chrono =
            |d: std::time::Duration| chrono::Duration::from_std(d).unwrap_or(chrono::Duration::MAX);
        Self {
            batcher: Mutex::new(NotificationBatcher::new(
                to_chrono(timing.notification_quiet),
                to_chrono(timing.notification_max_delay),
            )),
            wake: Notify::new(),
            sequence: AtomicU64::new(0),
        }
    }

    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// The account and folder a woken message returned to.
type WokeKey = (AccountId, FolderId);

/// What the rules decided per message (`notify` actions).
pub(crate) type RuleNotify = HashMap<MessageId, bool>;

impl Service {
    /// The local time as the triage clock says.
    fn local_now(&self) -> chrono::DateTime<chrono::Local> {
        self.inner.clock.now().with_timezone(&chrono::Local)
    }

    /// Runs the notification policy over newly arrived messages and
    /// queues the ones that notify.
    pub(crate) async fn notify_arrivals(
        &self,
        ids: &[MessageId],
        rule_notify: &RuleNotify,
        junk: &[MessageId],
    ) {
        if ids.is_empty() {
            return;
        }
        let ids = ids.to_vec();
        let config = self.inner.accounts.config();
        let loaded = self
            .inner
            .store
            .call(move |s| {
                let settings = s.notification_settings()?;
                let mut arrivals = Vec::new();
                for summary in s.message_summaries(&ids)? {
                    let Ok(folder) = s.get_folder(&summary.folder_id) else {
                        continue;
                    };
                    let account_name = config
                        .account(&summary.account_id)
                        .map(|a| a.name.clone())
                        .or_else(|| s.get_account(&summary.account_id).ok().map(|a| a.name))
                        .unwrap_or_else(|| summary.account_id.to_string());
                    arrivals.push(Arrival {
                        message_id: summary.id.clone(),
                        account_id: summary.account_id.clone(),
                        account_name,
                        folder_id: summary.folder_id.clone(),
                        folder_role: folder.role,
                        sender: summary
                            .from
                            .as_ref()
                            .map(|a| a.name.clone().unwrap_or_else(|| a.email.clone()))
                            .unwrap_or_default(),
                        subject: summary.subject.clone(),
                        rule_notify: None,
                        junk: false,
                    });
                }
                Ok((settings, arrivals))
            })
            .await;
        let (settings, mut arrivals) = match loaded {
            Ok(loaded) => loaded,
            Err(err) => {
                tracing::warn!(%err, "cannot load new mail for notifications");
                return;
            }
        };
        for arrival in &mut arrivals {
            arrival.rule_notify = rule_notify.get(&arrival.message_id).copied();
            arrival.junk = junk.contains(&arrival.message_id);
        }
        let local = self.local_now();
        let notifying = notify::notifying(&settings, &arrivals, &local);
        if notifying.is_empty() {
            tracing::debug!(count = arrivals.len(), "new mail does not notify");
            return;
        }
        lock(&self.inner.notify.batcher).push(&settings, &notifying, self.triage_now());
        self.inner.notify.wake.notify_one();
    }

    /// The notifications for snoozed messages that came back on their
    /// own: one per account and folder they returned to, so no
    /// notification carries mail of another account or folder (remote
    /// sessions get only those their grant covers).
    pub(crate) async fn notify_snooze_woke(&self, woken: &[WokenMessage]) {
        let ids: Vec<MessageId> = woken.iter().map(|w| w.entry.message_id.clone()).collect();
        let loaded = self
            .inner
            .store
            .call(move |s| Ok((s.notification_settings()?, s.message_summaries(&ids)?)))
            .await;
        let Ok((settings, summaries)) = loaded else {
            return;
        };
        // In wake order (the summaries come back in any order).
        let order: HashMap<&MessageId, usize> = woken
            .iter()
            .enumerate()
            .map(|(i, w)| (&w.entry.message_id, i))
            .collect();
        let mut groups: Vec<(WokeKey, Vec<(usize, WokeMessage)>)> = Vec::new();
        for m in &summaries {
            let key = (m.account_id.clone(), m.folder_id.clone());
            let message = WokeMessage {
                message_id: m.id.clone(),
                sender: m
                    .from
                    .as_ref()
                    .map(|a| a.name.clone().unwrap_or_else(|| a.email.clone()))
                    .unwrap_or_default(),
                subject: m.subject.clone(),
            };
            let rank = order.get(&m.id).copied().unwrap_or(usize::MAX);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push((rank, message)),
                None => groups.push((key, vec![(rank, message)])),
            }
        }
        let local = self.local_now();
        for ((account, folder), mut list) in groups {
            list.sort_by_key(|(rank, _)| *rank);
            let messages: Vec<WokeMessage> = list.into_iter().map(|(_, m)| m).collect();
            let sequence = self.inner.notify.next_sequence();
            if let Some(notification) = notify::snooze_woke(
                &settings,
                Some(&account),
                Some(&folder),
                &messages,
                &local,
                sequence,
            ) {
                self.request_notification(notification);
            }
        }
    }

    /// The notification `TestNotification` pushes.
    pub(crate) async fn test_notification(&self) -> crate::Result<()> {
        let settings = self
            .inner
            .store
            .call(nail_store::Store::notification_settings)
            .await?;
        let sequence = self.inner.notify.next_sequence();
        self.request_notification(notify::test_notification(&settings, sequence));
        Ok(())
    }

    fn request_notification(&self, notification: MailNotification) {
        tracing::info!(id = %notification.id, "notification requested");
        self.broadcast(Event::NotificationRequested { notification });
    }

    /// Releases the batches that are due.
    fn flush_notifications(&self) -> Option<i64> {
        let now = self.triage_now();
        let (due, next) = {
            let mut batcher = lock(&self.inner.notify.batcher);
            let due = batcher.flush(now);
            (due, batcher.next_due())
        };
        for notification in due {
            self.request_notification(notification);
        }
        next
    }
}

/// The flush task: waits for the next batch to be due.
pub(crate) async fn run_notifications(service: Service) {
    let idle = std::time::Duration::from_mins(1);
    loop {
        let next = service.flush_notifications();
        let wait = next.map_or(idle, |at| {
            let secs = (at - service.triage_now()).max(0);
            std::time::Duration::from_secs(u64::try_from(secs).unwrap_or(0))
                .clamp(std::time::Duration::from_millis(100), idle)
        });
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = service.inner.notify.wake.notified() => {}
        }
    }
}
