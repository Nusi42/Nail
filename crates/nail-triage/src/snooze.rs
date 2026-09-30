//! The snooze scheduler.
//!
//! Snoozing a message plans three queue writes — make sure the hidden
//! `Snoozed` folder exists, set the `$Snoozed` keyword, move the message
//! there — and records the wake time through [`SnoozeStore`] (the core
//! implements it on its store: `snooze`, `unsnooze`, `list_snoozed`,
//! `rebind_snooze`). A [`TimerWheel`] keeps the wake times in memory; the
//! core sleeps until [`SnoozeScheduler::next_wake`] and then calls
//! [`SnoozeScheduler::poll`], which returns the due messages with the
//! writes that bring them back (clear the keyword, move to the wake folder
//! — the Inbox when that folder is gone).
//!
//! * **Atomic wake**: `poll` / `unsnooze` do not touch the store. The
//!   caller removes the records *together with* queuing the writes (the
//!   core does both in one store transaction) or calls
//!   [`SnoozeScheduler::commit`]; when that fails it calls
//!   [`SnoozeScheduler::restore`]. A crash in between leaves the records,
//!   so the messages are due again after the restart — never stranded in
//!   the hidden folder without a record.
//!
//! * **Offline**: the wake time is local, so a message comes back on time
//!   without a connection; the moves replay when the server is reachable.
//! * **Restart**: [`SnoozeScheduler::load`] rebuilds the wheel from the
//!   store; wake times that passed while Nail was not running are due at
//!   the first poll.
//! * **Resync**: when a message gets a new local id (UIDVALIDITY change,
//!   moved by another client), [`SnoozeScheduler::rebind_by_message_id`]
//!   moves its snooze to the new id by `Message-ID`.
//!
//! Times are Unix seconds (UTC); the clock is injected ([`Clock`]).

use std::collections::HashMap;

use nail_api::triage::SNOOZE_KEYWORD;
use nail_api::{AccountId, FolderId, FolderRole, MessageId};

use crate::clock::Clock;
use crate::ops::{FolderTarget, TriageOp, coalesce};
use crate::wheel::TimerWheel;

/// Longest snooze accepted (about ten years).
pub const MAX_SNOOZE_SECS: i64 = 10 * 366 * 86_400;

/// A snoozed message as persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozeEntry {
    /// The message (its id while it waits in the Snoozed folder).
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// Wake time (Unix seconds).
    pub until: i64,
    /// The folder it returns to.
    pub wake_folder_id: FolderId,
    /// When it was snoozed (Unix seconds).
    pub snoozed_at: i64,
    /// Its `Message-ID` header, to find it again after a resync.
    pub rfc_message_id: Option<String>,
}

/// Where snoozes are persisted.
pub trait SnoozeStore {
    /// The store's error.
    type Error;

    /// Every snooze (at startup).
    fn load(&self) -> Result<Vec<SnoozeEntry>, Self::Error>;
    /// Records or replaces a snooze.
    fn save(&self, entry: &SnoozeEntry) -> Result<(), Self::Error>;
    /// Removes the snoozes of messages (woken or cancelled).
    fn remove(&self, messages: &[MessageId]) -> Result<(), Self::Error>;
    /// Points a snooze at a message's new id.
    fn rebind(&self, old: &MessageId, new: &MessageId) -> Result<bool, Self::Error>;
}

/// What the scheduler needs to know about folders.
pub trait FolderLookup {
    /// Whether a folder (still) exists.
    fn folder_exists(&self, folder: &FolderId) -> bool;
}

impl<F: Fn(&FolderId) -> bool> FolderLookup for F {
    fn folder_exists(&self, folder: &FolderId) -> bool {
        self(folder)
    }
}

/// Why a snooze was refused.
#[derive(Debug, thiserror::Error)]
pub enum SnoozeError<E> {
    /// The wake time is not in the future.
    #[error("the wake time is not in the future")]
    NotInFuture,
    /// The wake time is too far away.
    #[error("the wake time is more than ten years away")]
    TooFar,
    /// The store failed.
    #[error("snooze store: {0}")]
    Store(E),
}

impl<E> SnoozeError<E> {
    /// Fluent id of the problem (`snooze-error-*`, `core-triage.ftl`).
    pub fn message_id(&self) -> &'static str {
        match self {
            Self::NotInFuture => "snooze-error-not-in-future",
            Self::TooFar => "snooze-error-too-far",
            Self::Store(_) => "snooze-error-store",
        }
    }
}

/// A message to snooze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozeRequest {
    /// The message.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// The folder it is in now.
    pub folder_id: FolderId,
    /// Its `Message-ID` header.
    pub rfc_message_id: Option<String>,
    /// Wake time (Unix seconds).
    pub until: i64,
    /// Where it returns to; `None` = where it is now (or, when it is
    /// snoozed again, where it would have returned to).
    pub wake_folder_id: Option<FolderId>,
}

/// The result of snoozing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozePlan {
    /// The recorded snoozes.
    pub entries: Vec<SnoozeEntry>,
    /// The writes (already-snoozed messages only get a new wake time).
    pub ops: Vec<TriageOp>,
}

/// A message that comes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WokenMessage {
    /// Its snooze.
    pub entry: SnoozeEntry,
    /// Where it goes.
    pub target: FolderTarget,
    /// The wake folder was gone; it goes to the Inbox.
    pub fell_back: bool,
}

/// Messages that come back, with their writes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WakePlan {
    /// The messages, in wake order.
    pub woken: Vec<WokenMessage>,
    /// The writes: clear `$Snoozed`, move back (one operation per target).
    pub ops: Vec<TriageOp>,
}

impl WakePlan {
    /// Whether nothing woke.
    pub fn is_empty(&self) -> bool {
        self.woken.is_empty()
    }
}

/// The scheduler: snoozes in a timer wheel, persisted through a store.
pub struct SnoozeScheduler<S, C> {
    store: S,
    clock: C,
    wheel: TimerWheel<MessageId>,
    entries: HashMap<MessageId, SnoozeEntry>,
}

impl<S: SnoozeStore, C: Clock> SnoozeScheduler<S, C> {
    /// Loads every persisted snooze (at startup; survives restarts).
    pub fn load(store: S, clock: C) -> Result<Self, S::Error> {
        let mut wheel = TimerWheel::new(clock.unix());
        let mut entries = HashMap::new();
        for entry in store.load()? {
            wheel.insert(entry.message_id.clone(), entry.until);
            entries.insert(entry.message_id.clone(), entry);
        }
        Ok(Self {
            store,
            clock,
            wheel,
            entries,
        })
    }

    /// The store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The snooze of a message.
    pub fn get(&self, message: &MessageId) -> Option<&SnoozeEntry> {
        self.entries.get(message)
    }

    /// All snoozes, soonest first.
    pub fn list(&self) -> Vec<&SnoozeEntry> {
        let mut out: Vec<&SnoozeEntry> = self.entries.values().collect();
        out.sort_by(|a, b| (a.until, &a.message_id).cmp(&(b.until, &b.message_id)));
        out
    }

    /// The next wake time: the core sleeps until then (and at most, say,
    /// a minute, to notice clock changes).
    pub fn next_wake(&self) -> Option<i64> {
        self.wheel.next_deadline()
    }

    /// Snoozes messages: records the wake times and plans the writes.
    /// Messages already snoozed only get the new wake time (and keep their
    /// wake folder unless a new one is given).
    pub fn snooze(
        &mut self,
        requests: &[SnoozeRequest],
    ) -> Result<SnoozePlan, SnoozeError<S::Error>> {
        let now = self.clock.unix();
        for r in requests {
            if r.until <= now {
                return Err(SnoozeError::NotInFuture);
            }
            if r.until - now > MAX_SNOOZE_SECS {
                return Err(SnoozeError::TooFar);
            }
        }
        let mut plan = SnoozePlan {
            entries: Vec::new(),
            ops: Vec::new(),
        };
        for r in requests {
            let existing = self.entries.get(&r.message_id);
            let entry = SnoozeEntry {
                message_id: r.message_id.clone(),
                account_id: r.account_id.clone(),
                until: r.until,
                wake_folder_id: r
                    .wake_folder_id
                    .clone()
                    .or_else(|| existing.map(|e| e.wake_folder_id.clone()))
                    .unwrap_or_else(|| r.folder_id.clone()),
                snoozed_at: now,
                rfc_message_id: r.rfc_message_id.clone(),
            };
            if existing.is_none() {
                let ids = vec![r.message_id.clone()];
                plan.ops.push(TriageOp::EnsureSnoozedFolder {
                    account_id: r.account_id.clone(),
                });
                plan.ops.push(TriageOp::AddKeyword {
                    message_ids: ids.clone(),
                    keyword: SNOOZE_KEYWORD.to_owned(),
                });
                plan.ops.push(TriageOp::Move {
                    message_ids: ids,
                    to: FolderTarget::Snoozed {
                        account_id: r.account_id.clone(),
                    },
                });
            }
            self.store.save(&entry).map_err(SnoozeError::Store)?;
            self.wheel.insert(entry.message_id.clone(), entry.until);
            self.entries.insert(entry.message_id.clone(), entry.clone());
            plan.entries.push(entry);
        }
        plan.ops = group_ops(plan.ops);
        Ok(plan)
    }

    /// Wakes messages now (the user's "Unsnooze"). Unknown ids are
    /// ignored.
    pub fn unsnooze(
        &mut self,
        messages: &[MessageId],
        folders: &impl FolderLookup,
    ) -> Result<WakePlan, S::Error> {
        let mut due = Vec::new();
        for id in messages {
            if let Some(entry) = self.entries.get(id) {
                self.wheel.remove(id);
                due.push(entry.clone());
            }
        }
        self.finish(due, folders)
    }

    /// Wakes every message whose time has come.
    pub fn poll(&mut self, folders: &impl FolderLookup) -> Result<WakePlan, S::Error> {
        let fired = self.wheel.advance(self.clock.unix());
        let due: Vec<SnoozeEntry> = fired
            .into_iter()
            .filter_map(|(id, _)| self.entries.get(&id).cloned())
            .collect();
        self.finish(due, folders)
    }

    /// The plan for due entries. The records stay in the store until the
    /// caller commits the plan (see the module docs).
    fn finish(
        &mut self,
        due: Vec<SnoozeEntry>,
        folders: &impl FolderLookup,
    ) -> Result<WakePlan, S::Error> {
        if due.is_empty() {
            return Ok(WakePlan::default());
        }
        let ids: Vec<MessageId> = due.iter().map(|e| e.message_id.clone()).collect();
        let mut plan = WakePlan::default();
        let mut moves: Vec<TriageOp> = Vec::new();
        for entry in due {
            self.entries.remove(&entry.message_id);
            let fell_back = !folders.folder_exists(&entry.wake_folder_id);
            let target = if fell_back {
                FolderTarget::Role {
                    account_id: entry.account_id.clone(),
                    role: FolderRole::Inbox,
                }
            } else {
                FolderTarget::Folder(entry.wake_folder_id.clone())
            };
            moves.push(TriageOp::Move {
                message_ids: vec![entry.message_id.clone()],
                to: target.clone(),
            });
            plan.woken.push(WokenMessage {
                entry,
                target,
                fell_back,
            });
        }
        let mut ops = vec![TriageOp::RemoveKeyword {
            message_ids: ids,
            keyword: SNOOZE_KEYWORD.to_owned(),
        }];
        ops.extend(moves);
        plan.ops = group_ops(ops);
        Ok(plan)
    }

    /// Removes the records of a carried-out wake plan from the store (for
    /// callers that do not remove them in the transaction that queues the
    /// writes). On failure the messages are scheduled again.
    pub fn commit(&mut self, plan: &WakePlan) -> Result<(), S::Error> {
        if plan.is_empty() {
            return Ok(());
        }
        let ids: Vec<MessageId> = plan
            .woken
            .iter()
            .map(|w| w.entry.message_id.clone())
            .collect();
        if let Err(e) = self.store.remove(&ids) {
            self.restore(&plan.woken);
            return Err(e);
        }
        Ok(())
    }

    /// Schedules woken messages again: their writes could not be queued
    /// (their records are still in the store). The next poll retries.
    pub fn restore(&mut self, woken: &[WokenMessage]) {
        for w in woken {
            self.wheel.insert(w.entry.message_id.clone(), w.entry.until);
            self.entries
                .insert(w.entry.message_id.clone(), w.entry.clone());
        }
    }

    /// Forgets snoozes of messages that are gone (deleted elsewhere),
    /// without writes. Returns the forgotten entries.
    pub fn forget(&mut self, messages: &[MessageId]) -> Result<Vec<SnoozeEntry>, S::Error> {
        let known: Vec<MessageId> = messages
            .iter()
            .filter(|id| self.entries.contains_key(*id))
            .cloned()
            .collect();
        if known.is_empty() {
            return Ok(Vec::new());
        }
        self.store.remove(&known)?;
        Ok(known
            .iter()
            .filter_map(|id| {
                self.wheel.remove(id);
                self.entries.remove(id)
            })
            .collect())
    }

    /// After a resync gave a message a new id: moves the snooze of the
    /// message with this `Message-ID` in this account to the new id.
    /// Returns whether one moved.
    pub fn rebind_by_message_id(
        &mut self,
        account: &AccountId,
        rfc_message_id: &str,
        new_id: &MessageId,
    ) -> Result<bool, S::Error> {
        let wanted = normalize_message_id(rfc_message_id);
        let Some(old) = self
            .entries
            .values()
            .filter(|e| &e.account_id == account && &e.message_id != new_id)
            .find(|e| {
                e.rfc_message_id
                    .as_deref()
                    .is_some_and(|m| normalize_message_id(m) == wanted)
            })
            .map(|e| e.message_id.clone())
        else {
            return Ok(false);
        };
        if !self.store.rebind(&old, new_id)? {
            return Ok(false);
        }
        if let Some(mut entry) = self.entries.remove(&old) {
            self.wheel.remove(&old);
            entry.message_id = new_id.clone();
            self.wheel.insert(new_id.clone(), entry.until);
            self.entries.insert(new_id.clone(), entry);
        }
        Ok(true)
    }
}

fn normalize_message_id(id: &str) -> String {
    id.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

/// Groups operations so a batch becomes one operation per kind and
/// target, keeping the order of first appearance of each group.
fn group_ops(ops: Vec<TriageOp>) -> Vec<TriageOp> {
    let rank = |op: &TriageOp| match op {
        TriageOp::EnsureSnoozedFolder { .. } => 0,
        TriageOp::RemoveKeyword { .. } => 1,
        TriageOp::AddKeyword { .. } => 2,
        TriageOp::Move { .. } => 3,
    };
    let key = |op: &TriageOp| -> String {
        match op {
            TriageOp::EnsureSnoozedFolder { account_id } => account_id.to_string(),
            TriageOp::AddKeyword { keyword, .. } | TriageOp::RemoveKeyword { keyword, .. } => {
                keyword.clone()
            }
            TriageOp::Move { to, .. } => format!("{to:?}"),
        }
    };
    let mut indexed: Vec<(usize, TriageOp)> = ops.into_iter().enumerate().collect();
    // Stable: by kind, then by the first appearance of the group.
    let mut first: HashMap<(u8, String), usize> = HashMap::new();
    for (i, op) in &indexed {
        first.entry((rank(op), key(op))).or_insert(*i);
    }
    indexed.sort_by_key(|(i, op)| (rank(op), first[&(rank(op), key(op))], *i));
    coalesce(indexed.into_iter().map(|(_, op)| op).collect())
}

/// An in-memory [`SnoozeStore`] (tests; survives a "restart" by being
/// passed to a new scheduler).
#[derive(Debug, Default, Clone)]
pub struct MemorySnoozeStore {
    inner: std::sync::Arc<std::sync::Mutex<HashMap<MessageId, SnoozeEntry>>>,
}

impl MemorySnoozeStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<MessageId, SnoozeEntry>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Number of stored snoozes.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

impl SnoozeStore for MemorySnoozeStore {
    type Error = std::convert::Infallible;

    fn load(&self) -> Result<Vec<SnoozeEntry>, Self::Error> {
        Ok(self.lock().values().cloned().collect())
    }

    fn save(&self, entry: &SnoozeEntry) -> Result<(), Self::Error> {
        self.lock().insert(entry.message_id.clone(), entry.clone());
        Ok(())
    }

    fn remove(&self, messages: &[MessageId]) -> Result<(), Self::Error> {
        let mut map = self.lock();
        for id in messages {
            map.remove(id);
        }
        Ok(())
    }

    fn rebind(&self, old: &MessageId, new: &MessageId) -> Result<bool, Self::Error> {
        let mut map = self.lock();
        let Some(mut entry) = map.remove(old) else {
            return Ok(false);
        };
        entry.message_id = new.clone();
        map.insert(new.clone(), entry);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use chrono::Duration;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    const T0: i64 = 1_790_000_000;

    fn req(id: &str, until: i64) -> SnoozeRequest {
        SnoozeRequest {
            message_id: id.into(),
            account_id: "work".into(),
            folder_id: "inbox".into(),
            rfc_message_id: Some(format!("<{id}@example>")),
            until,
            wake_folder_id: None,
        }
    }

    fn setup() -> (
        SnoozeScheduler<MemorySnoozeStore, Arc<ManualClock>>,
        MemorySnoozeStore,
        Arc<ManualClock>,
    ) {
        let store = MemorySnoozeStore::new();
        let clock = Arc::new(ManualClock::at_unix(T0));
        let s = SnoozeScheduler::load(store.clone(), clock.clone()).unwrap();
        (s, store, clock)
    }

    fn all_exist(_: &FolderId) -> bool {
        true
    }

    #[test]
    fn snoozing_plans_folder_keyword_and_move() {
        let (mut s, store, _) = setup();
        let plan = s
            .snooze(&[req("m1", T0 + 3600), req("m2", T0 + 60)])
            .unwrap();
        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].wake_folder_id, FolderId::from("inbox"));
        assert_eq!(plan.entries[0].snoozed_at, T0);
        let ids = vec![MessageId::from("m1"), MessageId::from("m2")];
        assert_eq!(
            plan.ops,
            [
                TriageOp::EnsureSnoozedFolder {
                    account_id: "work".into()
                },
                TriageOp::AddKeyword {
                    message_ids: ids.clone(),
                    keyword: "$Snoozed".into()
                },
                TriageOp::Move {
                    message_ids: ids,
                    to: FolderTarget::Snoozed {
                        account_id: "work".into()
                    }
                },
            ]
        );
        assert_eq!(store.len(), 2);
        assert_eq!(s.next_wake(), Some(T0 + 60));
        assert_eq!(
            s.list()
                .iter()
                .map(|e| e.message_id.as_str())
                .collect::<Vec<_>>(),
            ["m2", "m1"]
        );
    }

    #[test]
    fn refuses_past_and_far_wake_times() {
        let (mut s, store, _) = setup();
        let err = s.snooze(&[req("m1", T0 + 10), req("m2", T0)]).unwrap_err();
        assert!(matches!(err, SnoozeError::NotInFuture));
        assert_eq!(err.message_id(), "snooze-error-not-in-future");
        assert!(store.is_empty(), "nothing recorded when one request is bad");
        let err = s
            .snooze(&[req("m1", T0 + MAX_SNOOZE_SECS + 1)])
            .unwrap_err();
        assert_eq!(err.message_id(), "snooze-error-too-far");
    }

    #[test]
    fn wakes_at_the_wake_time_only() {
        let (mut s, store, clock) = setup();
        s.snooze(&[req("m1", T0 + 3600)]).unwrap();
        clock.advance(Duration::seconds(3599));
        assert!(s.poll(&all_exist).unwrap().is_empty());
        clock.advance(Duration::seconds(1));
        let plan = s.poll(&all_exist).unwrap();
        assert_eq!(plan.woken.len(), 1);
        assert!(!plan.woken[0].fell_back);
        assert_eq!(store.len(), 1, "the record stays until the commit");
        assert_eq!(
            plan.ops,
            [
                TriageOp::RemoveKeyword {
                    message_ids: vec!["m1".into()],
                    keyword: "$Snoozed".into()
                },
                TriageOp::Move {
                    message_ids: vec!["m1".into()],
                    to: FolderTarget::Folder("inbox".into())
                },
            ]
        );
        s.commit(&plan).unwrap();
        assert!(store.is_empty());
        assert_eq!(s.next_wake(), None);
        assert!(s.poll(&all_exist).unwrap().is_empty(), "wakes once");
    }

    #[test]
    fn missing_wake_folder_falls_back_to_inbox() {
        let (mut s, _, clock) = setup();
        let mut r = req("m1", T0 + 5);
        r.wake_folder_id = Some("projects".into());
        s.snooze(&[r, req("m2", T0 + 5)]).unwrap();
        clock.advance(Duration::seconds(10));
        let gone = |f: &FolderId| f.as_str() != "projects";
        let plan = s.poll(&gone).unwrap();
        assert_eq!(plan.woken.len(), 2);
        let m1 = plan
            .woken
            .iter()
            .find(|w| w.entry.message_id.as_str() == "m1")
            .unwrap();
        assert!(m1.fell_back);
        assert_eq!(
            m1.target,
            FolderTarget::Role {
                account_id: "work".into(),
                role: FolderRole::Inbox
            }
        );
        // One keyword op, one move per target.
        assert_eq!(plan.ops.len(), 3);
        assert!(plan.ops.iter().all(|o| !o.targets_trash()));
    }

    #[test]
    fn survives_a_restart_and_catches_up() {
        let (mut s, store, clock) = setup();
        s.snooze(&[req("m1", T0 + 60), req("m2", T0 + 86_400)])
            .unwrap();
        drop(s);
        // Nail was off for two hours.
        clock.advance(Duration::hours(2));
        let mut s = SnoozeScheduler::load(store.clone(), clock.clone()).unwrap();
        assert_eq!(s.next_wake(), Some(T0 + 60));
        let plan = s.poll(&all_exist).unwrap();
        assert_eq!(plan.woken.len(), 1);
        assert_eq!(plan.woken[0].entry.message_id.as_str(), "m1");
        assert_eq!(s.next_wake(), Some(T0 + 86_400));
        s.commit(&plan).unwrap();
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn a_crash_before_the_commit_wakes_again_after_the_restart() {
        let (mut s, store, clock) = setup();
        s.snooze(&[req("m1", T0 + 60)]).unwrap();
        clock.advance(Duration::seconds(61));
        assert_eq!(s.poll(&all_exist).unwrap().woken.len(), 1);
        // The writes were never queued: Nail stopped here.
        drop(s);
        let mut s = SnoozeScheduler::load(store.clone(), clock.clone()).unwrap();
        let plan = s.poll(&all_exist).unwrap();
        assert_eq!(plan.woken.len(), 1, "due again");
        // Queuing failed this time: scheduled again, retried next poll.
        s.restore(&plan.woken);
        assert_eq!(s.next_wake(), Some(T0 + 60));
        assert!(s.get(&"m1".into()).is_some());
        let plan = s.poll(&all_exist).unwrap();
        s.commit(&plan).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn resnoozing_keeps_the_wake_folder_and_plans_no_move() {
        let (mut s, _, clock) = setup();
        let mut r = req("m1", T0 + 60);
        r.wake_folder_id = Some("projects".into());
        s.snooze(&[r]).unwrap();
        // Now it is in the Snoozed folder; snooze it for longer.
        let mut again = req("m1", T0 + 7200);
        again.folder_id = "snoozed".into();
        let plan = s.snooze(&[again]).unwrap();
        assert!(plan.ops.is_empty());
        assert_eq!(plan.entries[0].wake_folder_id, FolderId::from("projects"));
        clock.advance(Duration::seconds(61));
        assert!(s.poll(&all_exist).unwrap().is_empty(), "old time is gone");
        assert_eq!(s.next_wake(), Some(T0 + 7200));
    }

    #[test]
    fn manual_unsnooze_and_forget() {
        let (mut s, store, _) = setup();
        s.snooze(&[req("m1", T0 + 60), req("m2", T0 + 60), req("m3", T0 + 90)])
            .unwrap();
        let plan = s
            .unsnooze(&["m1".into(), "unknown".into()], &all_exist)
            .unwrap();
        assert_eq!(plan.woken.len(), 1);
        s.commit(&plan).unwrap();
        let forgotten = s.forget(&["m2".into(), "nope".into()]).unwrap();
        assert_eq!(forgotten.len(), 1);
        assert_eq!(store.len(), 1);
        assert_eq!(s.next_wake(), Some(T0 + 90));
        assert!(s.forget(&[]).unwrap().is_empty());
    }

    #[test]
    fn rebinds_by_message_id() {
        let (mut s, store, clock) = setup();
        s.snooze(&[req("m1", T0 + 60)]).unwrap();
        assert!(
            !s.rebind_by_message_id(&"other".into(), "<m1@example>", &"m9".into())
                .unwrap()
        );
        assert!(
            s.rebind_by_message_id(&"work".into(), " <M1@EXAMPLE> ", &"m9".into())
                .unwrap()
        );
        assert!(s.get(&"m1".into()).is_none());
        assert_eq!(store.load().unwrap()[0].message_id.as_str(), "m9");
        clock.advance(Duration::seconds(60));
        let plan = s.poll(&all_exist).unwrap();
        assert_eq!(plan.woken[0].entry.message_id.as_str(), "m9");
        assert!(
            !s.rebind_by_message_id(&"work".into(), "<m1@example>", &"m10".into())
                .unwrap()
        );
    }

    #[test]
    fn clock_going_backwards_does_not_wake_early() {
        let (mut s, _, clock) = setup();
        s.snooze(&[req("m1", T0 + 600)]).unwrap();
        clock.set(chrono::DateTime::from_timestamp(T0 - 3600, 0).unwrap());
        assert!(s.poll(&all_exist).unwrap().is_empty());
        clock.set(chrono::DateTime::from_timestamp(T0 + 600, 0).unwrap());
        assert_eq!(s.poll(&all_exist).unwrap().woken.len(), 1);
    }

    /// A store that fails on demand.
    #[derive(Default)]
    struct Flaky {
        inner: MemorySnoozeStore,
        fail: std::cell::Cell<bool>,
    }

    impl SnoozeStore for Flaky {
        type Error = String;
        fn load(&self) -> Result<Vec<SnoozeEntry>, String> {
            Ok(self.inner.load().unwrap())
        }
        fn save(&self, e: &SnoozeEntry) -> Result<(), String> {
            if self.fail.get() {
                return Err("disk full".into());
            }
            self.inner.save(e).map_err(|e| e.to_string())
        }
        fn remove(&self, m: &[MessageId]) -> Result<(), String> {
            if self.fail.get() {
                return Err("locked".into());
            }
            self.inner.remove(m).map_err(|e| e.to_string())
        }
        fn rebind(&self, o: &MessageId, n: &MessageId) -> Result<bool, String> {
            self.inner.rebind(o, n).map_err(|e| e.to_string())
        }
    }

    #[test]
    fn store_failures_keep_state_consistent() {
        let clock = Arc::new(ManualClock::at_unix(T0));
        let mut s = SnoozeScheduler::load(Flaky::default(), clock.clone()).unwrap();
        s.snooze(&[req("m1", T0 + 10)]).unwrap();
        s.store().fail.set(true);
        let err = s.snooze(&[req("m2", T0 + 10)]).unwrap_err();
        assert!(matches!(err, SnoozeError::Store(_)));
        assert_eq!(err.to_string(), "snooze store: disk full");
        clock.advance(Duration::seconds(20));
        let plan = s.poll(&all_exist).unwrap();
        assert!(s.commit(&plan).is_err());
        assert_eq!(s.next_wake(), Some(T0 + 10), "retried at the next poll");
        s.store().fail.set(false);
        let plan = s.poll(&all_exist).unwrap();
        assert_eq!(plan.woken.len(), 1);
        s.commit(&plan).unwrap();
    }
}
