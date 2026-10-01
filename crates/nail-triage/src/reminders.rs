//! The reminder scheduler (M3a WP73; contract ADR 0058): when calendar and
//! task alarms fire, across time zones and restarts, with snooze and
//! dismiss — on the same injectable clock ([`crate::clock::Clock`]) and
//! timer wheel ([`crate::wheel::TimerWheel`]) as snooze. Pure logic like
//! the rest of the crate: persistence through [`ReminderStore`] (the core
//! implements it on the store's `pim_reminders`), notifications as
//! [`NotificationKind::Reminder`] with texts from `locales/en/core-pim.ftl`.
//!
//! # Planning
//!
//! [`plan_alarms`] turns occurrences ([`Occurrence`]: an event occurrence
//! or a task with its alarms) into fire times:
//!
//! * relative triggers count from the occurrence's start or end (a task's
//!   start or due), absolute ones are instants (once per series);
//! * zoned times are instants (a 09:00 Berlin meeting is 07:00 UTC in
//!   summer and 08:00 in winter, and its "15 minutes before" follows);
//!   **all-day dates and floating times are wall-clock times in the local
//!   zone** passed in — when that zone changes the core plans again and
//!   the moved reminders become pending at their new time;
//! * `REPEAT` adds further reminders (`<key>+1`, `<key>+2`, …);
//! * an alarm acknowledged (RFC 9074 `ACKNOWLEDGED`) at or after its fire
//!   time — on this device or another — is not planned.
//!
//! The core plans a window (e.g. the next days, expanding recurring events
//! for it) and plans again when items change, the window moves on, or the
//! zone changes; [`ReminderScheduler::replan`] stores the plan (states of
//! unchanged reminders are kept).
//!
//! # Firing, restarts, snooze, dismiss
//!
//! * The core sleeps until [`ReminderScheduler::next_wake`] (and at most a
//!   minute, to notice clock changes) and calls [`ReminderScheduler::poll`]:
//!   due reminders become `fired` in the store and come back as
//!   [`FiredReminder`]s for `ReminderDue` and the notification.
//! * **Restart**: [`ReminderScheduler::load`] rebuilds the wheel from the
//!   store; reminders that came due while Nail was not running fire at the
//!   first poll, marked late — beyond [`ReminderPolicy::notify_late_up_to`]
//!   they are listed but not notified.
//! * [`ReminderScheduler::snooze`] / [`ReminderScheduler::dismiss`] change
//!   the state and return [`ReminderOp`]s: the core writes them into the
//!   item (RFC 9074 snooze alarm / `ACKNOWLEDGED`) through the PIM queue so
//!   other devices follow.
//!
//! Times are Unix seconds (UTC); the clock is injected.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone};
use nail_api::pim::calendar::{AlarmRelated, AlarmTrigger};
use nail_api::pim::{Alarm, PimTime, Reminder, ReminderState, TaskFields};
use nail_api::triage::{MailNotification, NotificationKind};
use nail_api::{EventId, LocalizedMessage, ReminderId, TaskId};

use crate::clock::Clock;
use crate::localtime::resolve_local;
use crate::wheel::TimerWheel;

/// The notification kind reminders are shown with.
pub const NOTIFICATION_KIND: NotificationKind = NotificationKind::Reminder;

/// Every Fluent id this module produces (`locales/en/core-pim.ftl`).
pub const FLUENT_IDS: &[&str] = &[
    "notification-reminder-title",
    "notification-reminder-body",
    "notification-reminder-body-location",
    "reminder-error-not-in-future",
    "reminder-error-too-far",
    "reminder-error-store",
];

/// Longest snooze accepted (about a year).
pub const MAX_REMINDER_SNOOZE_SECS: i64 = 366 * 86_400;

/// What a reminder belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReminderItem {
    /// An event (series).
    Event(EventId),
    /// A task.
    Task(TaskId),
}

/// A calendar time as the planner needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceTime {
    /// An instant (a zoned time), Unix seconds.
    Instant(i64),
    /// A wall-clock time placed in the local zone (all-day dates at
    /// midnight, floating times).
    Wall(NaiveDateTime),
}

impl OccurrenceTime {
    /// The planner's view of a calendar time; `None` when malformed. Zones
    /// Nail does not know read as wall-clock times.
    pub fn of(time: &PimTime) -> Option<Self> {
        let local = |text: &str| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").ok();
        match time {
            PimTime::Date { date } => Some(Self::Wall(
                NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .ok()?
                    .and_hms_opt(0, 0, 0)?,
            )),
            PimTime::Floating { local: text } => Some(Self::Wall(local(text)?)),
            PimTime::Zoned { local: text, tz } => {
                let wall = local(text)?;
                let zone = if tz.eq_ignore_ascii_case(nail_api::pim::UTC_ZONE) {
                    Some(chrono_tz::UTC)
                } else {
                    chrono_tz::Tz::from_str(tz).ok()
                };
                Some(match zone {
                    Some(zone) => Self::Instant(resolve_local(&zone, wall).timestamp()),
                    None => Self::Wall(wall),
                })
            }
        }
    }

    /// The instant in `local` (DST gaps move forward, repeated times take
    /// the earlier instant).
    pub fn resolve<Tz: TimeZone>(&self, local: &Tz) -> i64 {
        match self {
            Self::Instant(secs) => *secs,
            Self::Wall(wall) => resolve_local(local, *wall).timestamp(),
        }
    }
}

/// One occurrence with its alarms.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// The occurrence's `RECURRENCE-ID` (`None` for single events, tasks).
    pub recurrence_id: Option<PimTime>,
    /// Start (a task's start; `None` when it has none).
    pub start: Option<OccurrenceTime>,
    /// End (a task's due).
    pub end: Option<OccurrenceTime>,
    /// Its alarms.
    pub alarms: Vec<Alarm>,
}

impl Occurrence {
    /// An event occurrence.
    pub fn event(
        recurrence_id: Option<PimTime>,
        start: &PimTime,
        end: &PimTime,
        alarms: &[Alarm],
    ) -> Self {
        Self {
            recurrence_id,
            start: OccurrenceTime::of(start),
            end: OccurrenceTime::of(end),
            alarms: alarms.to_vec(),
        }
    }

    /// A task (closed tasks have no reminders).
    pub fn task(fields: &TaskFields) -> Self {
        Self {
            recurrence_id: None,
            start: fields.start.as_ref().and_then(OccurrenceTime::of),
            end: fields.due.as_ref().and_then(OccurrenceTime::of),
            alarms: if fields.status.is_closed() {
                Vec::new()
            } else {
                fields.alarms.clone()
            },
        }
    }
}

/// A reminder the planner wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAlarm {
    /// When it fires (Unix seconds).
    pub fire_at: i64,
    /// The occurrence (`None` for single events and tasks).
    pub recurrence_id: Option<PimTime>,
    /// The alarm (its `UID`, else its position; repeats get `+n`).
    pub alarm_key: String,
}

/// The occurrence as text (stable order and keys).
fn occurrence_key(recurrence_id: Option<&PimTime>) -> String {
    recurrence_id
        .and_then(|r| serde_json::to_string(r).ok())
        .unwrap_or_default()
}

fn parse_instant(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.timestamp())
}

/// The fire times of the occurrences' alarms in `[from, until)`, with
/// dates and floating times in `local` (see the module docs). Sorted by
/// fire time.
pub fn plan_alarms<Tz: TimeZone>(
    occurrences: &[Occurrence],
    local: &Tz,
    from: i64,
    until: i64,
) -> Vec<PlannedAlarm> {
    let mut out = Vec::new();
    let mut absolute_seen: HashSet<(String, i64)> = HashSet::new();
    for occurrence in occurrences {
        let start = occurrence.start.map(|t| t.resolve(local));
        let end = occurrence.end.map(|t| t.resolve(local));
        for (index, alarm) in occurrence.alarms.iter().enumerate() {
            let key = alarm
                .uid
                .clone()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| index.to_string());
            let base = match &alarm.trigger {
                AlarmTrigger::Relative {
                    offset_secs,
                    related,
                } => {
                    let anchor = match related {
                        AlarmRelated::Start => start.or(end),
                        AlarmRelated::End => end.or(start),
                    };
                    let Some(anchor) = anchor else { continue };
                    anchor.saturating_add(*offset_secs)
                }
                AlarmTrigger::Absolute { at } => {
                    let Some(at) = parse_instant(at) else {
                        continue;
                    };
                    // An absolute alarm fires once for the whole series.
                    if !absolute_seen.insert((key.clone(), at)) {
                        continue;
                    }
                    at
                }
            };
            let acknowledged = alarm.acknowledged.as_deref().and_then(parse_instant);
            let repeats = alarm
                .repeat
                .filter(|r| r.interval_secs > 0)
                .map_or(0, |r| r.count.min(100));
            for n in 0..=repeats {
                let interval = alarm.repeat.map_or(0, |r| i64::from(r.interval_secs));
                let fire_at = base.saturating_add(i64::from(n) * interval);
                if acknowledged.is_some_and(|ack| ack >= fire_at) {
                    continue;
                }
                if fire_at < from || fire_at >= until {
                    continue;
                }
                out.push(PlannedAlarm {
                    fire_at,
                    recurrence_id: occurrence.recurrence_id.clone(),
                    alarm_key: if n == 0 {
                        key.clone()
                    } else {
                        format!("{key}+{n}")
                    },
                });
            }
        }
    }
    out.sort_by_cached_key(|p| {
        (
            p.fire_at,
            occurrence_key(p.recurrence_id.as_ref()),
            p.alarm_key.clone(),
        )
    });
    out
}

/// A reminder as the scheduler keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderEntry {
    /// Its id.
    pub id: ReminderId,
    /// Its item.
    pub item: ReminderItem,
    /// The occurrence.
    pub recurrence_id: Option<PimTime>,
    /// The alarm.
    pub alarm_key: String,
    /// When it fires (Unix seconds).
    pub fire_at: i64,
    /// Its state.
    pub state: ReminderState,
    /// When a snoozed reminder comes back.
    pub snoozed_until: Option<i64>,
}

impl ReminderEntry {
    /// When the scheduler acts on it (`None`: fired or dismissed).
    pub fn due_at(&self) -> Option<i64> {
        match self.state {
            ReminderState::Pending => Some(self.fire_at),
            ReminderState::Snoozed => Some(self.snoozed_until.unwrap_or(self.fire_at)),
            ReminderState::Fired | ReminderState::Dismissed => None,
        }
    }
}

/// Where reminders are persisted (the core: `pim_reminders`).
pub trait ReminderStore {
    /// The store's error.
    type Error;
    /// Every reminder that is not dismissed (at startup).
    fn load(&self) -> Result<Vec<ReminderEntry>, Self::Error>;
    /// Replaces an item's planned reminders: rows that stay keep their
    /// state unless their fire time moved (then pending again), others go.
    /// Returns the item's reminders.
    fn replace(
        &self,
        item: &ReminderItem,
        planned: &[PlannedAlarm],
    ) -> Result<Vec<ReminderEntry>, Self::Error>;
    /// Sets the state of reminders.
    fn set_state(
        &self,
        ids: &[ReminderId],
        state: ReminderState,
        snoozed_until: Option<i64>,
    ) -> Result<(), Self::Error>;
}

/// Why a snooze was refused.
#[derive(Debug, thiserror::Error)]
pub enum ReminderError<E> {
    /// The snooze end is not in the future.
    #[error("the snooze end is not in the future")]
    NotInFuture,
    /// The snooze end is too far away.
    #[error("the snooze end is more than a year away")]
    TooFar,
    /// The store failed.
    #[error("reminder store: {0}")]
    Store(E),
}

impl<E> ReminderError<E> {
    /// Fluent id (`reminder-error-*`, `core-pim.ftl`).
    pub fn message_id(&self) -> &'static str {
        match self {
            Self::NotInFuture => "reminder-error-not-in-future",
            Self::TooFar => "reminder-error-too-far",
            Self::Store(_) => "reminder-error-store",
        }
    }
}

/// How late reminders are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderPolicy {
    /// A reminder that fires later than this after its time (Nail was not
    /// running) is listed as fired but not notified.
    pub notify_late_up_to: i64,
}

impl Default for ReminderPolicy {
    fn default() -> Self {
        Self {
            notify_late_up_to: 2 * 3600,
        }
    }
}

/// A reminder that fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiredReminder {
    /// It (now `fired`).
    pub entry: ReminderEntry,
    /// Seconds after its time it fired (it came due while Nail was off).
    pub late_secs: i64,
    /// Whether to show a notification (see [`ReminderPolicy`]).
    pub notify: bool,
}

/// A write into the item that makes other devices follow (RFC 9074).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReminderOp {
    /// Set the alarm's `ACKNOWLEDGED` (dismissed).
    Acknowledge {
        /// The item.
        item: ReminderItem,
        /// The occurrence.
        recurrence_id: Option<PimTime>,
        /// The alarm.
        alarm_key: String,
        /// When (Unix seconds).
        at: i64,
    },
    /// Acknowledge the alarm and add a snooze alarm (`RELATED-TO;
    /// RELTYPE=SNOOZE`) firing at `until`.
    Snooze {
        /// The item.
        item: ReminderItem,
        /// The occurrence.
        recurrence_id: Option<PimTime>,
        /// The alarm.
        alarm_key: String,
        /// When it comes back (Unix seconds).
        until: i64,
        /// When it was snoozed (Unix seconds).
        at: i64,
    },
}

/// The scheduler: reminders in a timer wheel, persisted through a store.
pub struct ReminderScheduler<S, C> {
    store: S,
    clock: C,
    policy: ReminderPolicy,
    wheel: TimerWheel<ReminderId>,
    entries: HashMap<ReminderId, ReminderEntry>,
}

impl<S: ReminderStore, C: Clock> ReminderScheduler<S, C> {
    /// Loads every reminder that is not dismissed (at startup).
    pub fn load(store: S, clock: C, policy: ReminderPolicy) -> Result<Self, S::Error> {
        let mut scheduler = Self {
            wheel: TimerWheel::new(clock.unix()),
            store,
            clock,
            policy,
            entries: HashMap::new(),
        };
        scheduler.reload()?;
        Ok(scheduler)
    }

    /// Reads everything from the store again (after bulk changes).
    pub fn reload(&mut self) -> Result<(), S::Error> {
        let entries = self.store.load()?;
        self.wheel = TimerWheel::new(self.clock.unix());
        self.entries.clear();
        for entry in entries {
            self.track(entry);
        }
        Ok(())
    }

    fn track(&mut self, entry: ReminderEntry) {
        self.wheel.remove(&entry.id);
        if entry.state == ReminderState::Dismissed {
            self.entries.remove(&entry.id);
            return;
        }
        if let Some(due) = entry.due_at() {
            self.wheel.insert(entry.id.clone(), due);
        }
        self.entries.insert(entry.id.clone(), entry);
    }

    /// The store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// One reminder.
    pub fn get(&self, id: &ReminderId) -> Option<&ReminderEntry> {
        self.entries.get(id)
    }

    /// Reminders that are not dismissed, by fire time.
    pub fn list(&self) -> Vec<&ReminderEntry> {
        let mut out: Vec<&ReminderEntry> = self.entries.values().collect();
        out.sort_by(|a, b| (a.fire_at, &a.id).cmp(&(b.fire_at, &b.id)));
        out
    }

    /// When the next reminder is due.
    pub fn next_wake(&self) -> Option<i64> {
        self.wheel.next_deadline()
    }

    /// Stores an item's new plan and schedules it.
    pub fn replan(
        &mut self,
        item: &ReminderItem,
        planned: &[PlannedAlarm],
    ) -> Result<(), S::Error> {
        let entries = self.store.replace(item, planned)?;
        self.forget(item);
        for entry in entries {
            self.track(entry);
        }
        Ok(())
    }

    /// Drops an item's reminders from memory (the item is gone; the store
    /// removed its rows).
    pub fn forget(&mut self, item: &ReminderItem) {
        let ids: Vec<ReminderId> = self
            .entries
            .values()
            .filter(|e| &e.item == item)
            .map(|e| e.id.clone())
            .collect();
        for id in ids {
            self.wheel.remove(&id);
            self.entries.remove(&id);
        }
    }

    /// Fires every reminder whose time has come: they become `fired` in
    /// the store. On a store failure they are scheduled again.
    pub fn poll(&mut self) -> Result<Vec<FiredReminder>, S::Error> {
        let now = self.clock.unix();
        let due = self.wheel.advance(now);
        if due.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<ReminderId> = due.iter().map(|(id, _)| id.clone()).collect();
        if let Err(err) = self.store.set_state(&ids, ReminderState::Fired, None) {
            for (id, deadline) in due {
                self.wheel.insert(id, deadline);
            }
            return Err(err);
        }
        let mut out = Vec::with_capacity(due.len());
        for (id, deadline) in due {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            entry.state = ReminderState::Fired;
            entry.snoozed_until = None;
            let late_secs = (now - deadline).max(0);
            out.push(FiredReminder {
                entry: entry.clone(),
                late_secs,
                notify: late_secs <= self.policy.notify_late_up_to,
            });
        }
        Ok(out)
    }

    /// Snoozes reminders until `until`. Unknown ids are ignored.
    pub fn snooze(
        &mut self,
        ids: &[ReminderId],
        until: i64,
    ) -> Result<Vec<ReminderOp>, ReminderError<S::Error>> {
        let now = self.clock.unix();
        if until <= now {
            return Err(ReminderError::NotInFuture);
        }
        if until - now > MAX_REMINDER_SNOOZE_SECS {
            return Err(ReminderError::TooFar);
        }
        let known: Vec<ReminderId> = ids
            .iter()
            .filter(|id| self.entries.contains_key(*id))
            .cloned()
            .collect();
        if known.is_empty() {
            return Ok(Vec::new());
        }
        self.store
            .set_state(&known, ReminderState::Snoozed, Some(until))
            .map_err(ReminderError::Store)?;
        let mut ops = Vec::with_capacity(known.len());
        for id in known {
            if let Some(mut entry) = self.entries.get(&id).cloned() {
                entry.state = ReminderState::Snoozed;
                entry.snoozed_until = Some(until);
                ops.push(ReminderOp::Snooze {
                    item: entry.item.clone(),
                    recurrence_id: entry.recurrence_id.clone(),
                    alarm_key: entry.alarm_key.clone(),
                    until,
                    at: now,
                });
                self.track(entry);
            }
        }
        Ok(ops)
    }

    /// Dismisses reminders. Unknown ids are ignored.
    pub fn dismiss(&mut self, ids: &[ReminderId]) -> Result<Vec<ReminderOp>, S::Error> {
        let now = self.clock.unix();
        let known: Vec<ReminderId> = ids
            .iter()
            .filter(|id| self.entries.contains_key(*id))
            .cloned()
            .collect();
        if known.is_empty() {
            return Ok(Vec::new());
        }
        self.store
            .set_state(&known, ReminderState::Dismissed, None)?;
        let mut ops = Vec::with_capacity(known.len());
        for id in known {
            self.wheel.remove(&id);
            if let Some(entry) = self.entries.remove(&id) {
                ops.push(ReminderOp::Acknowledge {
                    item: entry.item,
                    recurrence_id: entry.recurrence_id,
                    alarm_key: entry.alarm_key,
                    at: now,
                });
            }
        }
        Ok(ops)
    }
}

/// `when` for the notification body: the local date (all-day) or date and
/// time, `YYYY-MM-DD[ HH:MM]` (the core may pass its own formatting).
pub fn when_text<Tz: TimeZone>(starts_at: i64, all_day: bool, local: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let at = local.timestamp_opt(starts_at, 0).earliest();
    match at {
        Some(at) if all_day => at.format("%Y-%m-%d").to_string(),
        Some(at) => at.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

/// The notification of a fired reminder (`reminder:<id>`; texts
/// `notification-reminder-*`). `when` is the formatted start
/// ([`when_text`]).
pub fn reminder_notification(reminder: &Reminder, when: &str, sound: bool) -> MailNotification {
    let body = match reminder.location.as_deref().map(str::trim) {
        Some(location) if !location.is_empty() => {
            LocalizedMessage::new("notification-reminder-body-location")
                .arg("when", when)
                .arg("location", location)
        }
        _ => LocalizedMessage::new("notification-reminder-body").arg("when", when),
    };
    let id = format!("reminder:{}", reminder.id);
    MailNotification {
        tag: id.clone(),
        id,
        kind: NOTIFICATION_KIND,
        account_id: None,
        folder_id: None,
        message_ids: Vec::new(),
        title: LocalizedMessage::new("notification-reminder-title")
            .arg("title", reminder.title.as_str()),
        body: Some(body),
        sound,
    }
}

/// An in-memory [`ReminderStore`] (tests; survives a "restart" by being
/// passed to a new scheduler). Ids are `<item>/<occurrence>/<alarm>`.
#[derive(Debug, Default, Clone)]
pub struct MemoryReminderStore {
    inner: std::sync::Arc<std::sync::Mutex<HashMap<ReminderId, ReminderEntry>>>,
}

impl MemoryReminderStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ReminderId, ReminderEntry>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// All rows, dismissed ones included.
    pub fn all(&self) -> Vec<ReminderEntry> {
        let mut out: Vec<ReminderEntry> = self.lock().values().cloned().collect();
        out.sort_by(|a, b| (a.fire_at, &a.id).cmp(&(b.fire_at, &b.id)));
        out
    }
}

fn memory_id(item: &ReminderItem, planned: &PlannedAlarm) -> ReminderId {
    let item = match item {
        ReminderItem::Event(id) => format!("event:{id}"),
        ReminderItem::Task(id) => format!("task:{id}"),
    };
    ReminderId::new(format!(
        "{item}/{}/{}",
        occurrence_key(planned.recurrence_id.as_ref()),
        planned.alarm_key
    ))
}

impl ReminderStore for MemoryReminderStore {
    type Error = std::convert::Infallible;

    fn load(&self) -> Result<Vec<ReminderEntry>, Self::Error> {
        Ok(self
            .all()
            .into_iter()
            .filter(|e| e.state != ReminderState::Dismissed)
            .collect())
    }

    fn replace(
        &self,
        item: &ReminderItem,
        planned: &[PlannedAlarm],
    ) -> Result<Vec<ReminderEntry>, Self::Error> {
        let mut map = self.lock();
        let mut keep = HashSet::new();
        for p in planned {
            let id = memory_id(item, p);
            keep.insert(id.clone());
            let entry = map.entry(id.clone()).or_insert_with(|| ReminderEntry {
                id: id.clone(),
                item: item.clone(),
                recurrence_id: p.recurrence_id.clone(),
                alarm_key: p.alarm_key.clone(),
                fire_at: p.fire_at,
                state: ReminderState::Pending,
                snoozed_until: None,
            });
            if entry.fire_at != p.fire_at {
                entry.fire_at = p.fire_at;
                entry.state = ReminderState::Pending;
                entry.snoozed_until = None;
            }
        }
        map.retain(|id, e| &e.item != item || keep.contains(id));
        let mut out: Vec<ReminderEntry> =
            map.values().filter(|e| &e.item == item).cloned().collect();
        out.sort_by(|a, b| (a.fire_at, &a.id).cmp(&(b.fire_at, &b.id)));
        Ok(out)
    }

    fn set_state(
        &self,
        ids: &[ReminderId],
        state: ReminderState,
        snoozed_until: Option<i64>,
    ) -> Result<(), Self::Error> {
        let mut map = self.lock();
        for id in ids {
            if let Some(e) = map.get_mut(id) {
                e.state = state;
                e.snoozed_until = snoozed_until.filter(|_| state == ReminderState::Snoozed);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Duration;
    use chrono_tz::Tz;
    use nail_api::pim::TaskStatus;
    use nail_api::pim::calendar::AlarmRepeat;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::clock::ManualClock;

    fn at(text: &str) -> i64 {
        parse_instant(text).unwrap()
    }

    fn berlin() -> Tz {
        "Europe/Berlin".parse().unwrap()
    }

    fn new_york() -> Tz {
        "America/New_York".parse().unwrap()
    }

    fn fires(plan: &[PlannedAlarm]) -> Vec<String> {
        plan.iter()
            .map(|p| {
                DateTime::from_timestamp(p.fire_at, 0)
                    .unwrap()
                    .format("%Y-%m-%dT%H:%M:%SZ")
                    .to_string()
            })
            .collect()
    }

    fn standup(day: u32) -> Occurrence {
        let start = PimTime::zoned(format!("2026-03-{day:02}T09:00:00"), "Europe/Berlin");
        let end = PimTime::zoned(format!("2026-03-{day:02}T09:15:00"), "Europe/Berlin");
        Occurrence::event(
            Some(start.clone()),
            &start,
            &end,
            &[Alarm::minutes_before(15)],
        )
    }

    #[test]
    fn zoned_alarms_follow_dst() {
        let occurrences: Vec<Occurrence> = (27..=30).map(standup).collect();
        // The local zone does not matter for zoned events.
        for local in [berlin(), new_york()] {
            let plan = plan_alarms(&occurrences, &local, 0, i64::MAX);
            assert_eq!(
                fires(&plan),
                [
                    "2026-03-27T07:45:00Z",
                    "2026-03-28T07:45:00Z",
                    "2026-03-29T06:45:00Z",
                    "2026-03-30T06:45:00Z"
                ]
            );
        }
        // The window cuts.
        let plan = plan_alarms(
            &occurrences,
            &berlin(),
            at("2026-03-28T07:45:00Z"),
            at("2026-03-30T06:45:00Z"),
        );
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].alarm_key, "0");
        assert_eq!(
            plan[0].recurrence_id,
            Some(PimTime::zoned("2026-03-28T09:00:00", "Europe/Berlin"))
        );
    }

    #[test]
    fn floating_and_all_day_alarms_use_the_local_zone() {
        let start = PimTime::floating("2026-10-01T09:00:00");
        let end = PimTime::floating("2026-10-01T10:00:00");
        let gym = Occurrence::event(None, &start, &end, &[Alarm::minutes_before(30)]);
        assert_eq!(
            fires(&plan_alarms(
                std::slice::from_ref(&gym),
                &berlin(),
                0,
                i64::MAX
            )),
            ["2026-10-01T06:30:00Z"]
        );
        assert_eq!(
            fires(&plan_alarms(&[gym], &new_york(), 0, i64::MAX)),
            ["2026-10-01T12:30:00Z"]
        );
        // All-day: "the day before at 18:00" = -6 h from midnight.
        let mut alarm = Alarm::minutes_before(6 * 60);
        alarm.uid = Some("eve".into());
        let holiday = Occurrence::event(
            None,
            &PimTime::date("2026-10-03"),
            &PimTime::date("2026-10-04"),
            &[alarm],
        );
        let plan = plan_alarms(&[holiday], &berlin(), 0, i64::MAX);
        assert_eq!(fires(&plan), ["2026-10-02T16:00:00Z"]);
        assert_eq!(plan[0].alarm_key, "eve");
        // A floating alarm inside the spring-forward gap moves forward.
        let early = Occurrence::event(
            None,
            &PimTime::floating("2026-03-29T03:00:00"),
            &PimTime::floating("2026-03-29T04:00:00"),
            &[Alarm::minutes_before(30)],
        );
        // 03:00 Berlin on that day is 01:00 UTC; 30 minutes earlier.
        assert_eq!(
            fires(&plan_alarms(&[early], &berlin(), 0, i64::MAX)),
            ["2026-03-29T00:30:00Z"]
        );
    }

    #[test]
    fn end_absolute_repeat_and_acknowledged() {
        let start = PimTime::utc("2026-10-01T09:00:00");
        let end = PimTime::utc("2026-10-01T10:00:00");
        let at_end = Alarm {
            trigger: AlarmTrigger::Relative {
                offset_secs: 0,
                related: AlarmRelated::End,
            },
            ..Alarm::minutes_before(0)
        };
        let absolute = Alarm {
            trigger: AlarmTrigger::Absolute {
                at: "2026-09-30T12:00:00Z".into(),
            },
            ..Alarm::minutes_before(0)
        };
        let repeating = Alarm {
            repeat: Some(AlarmRepeat {
                count: 2,
                interval_secs: 300,
            }),
            ..Alarm::minutes_before(10)
        };
        let acknowledged = Alarm {
            acknowledged: Some("2026-10-01T08:55:00Z".into()),
            ..Alarm::minutes_before(5)
        };
        let occurrence = Occurrence::event(
            None,
            &start,
            &end,
            &[at_end, absolute.clone(), repeating, acknowledged],
        );
        let plan = plan_alarms(&[occurrence], &berlin(), 0, i64::MAX);
        let keys: Vec<(&str, String)> = plan
            .iter()
            .map(|p| {
                (
                    p.alarm_key.as_str(),
                    fires(std::slice::from_ref(p))[0].clone(),
                )
            })
            .collect();
        assert_eq!(
            keys,
            [
                ("1", "2026-09-30T12:00:00Z".to_owned()),
                ("2", "2026-10-01T08:50:00Z".to_owned()),
                ("2+1", "2026-10-01T08:55:00Z".to_owned()),
                ("2+2", "2026-10-01T09:00:00Z".to_owned()),
                ("0", "2026-10-01T10:00:00Z".to_owned()),
            ]
        );
        // An absolute alarm of a series fires once.
        let a = Occurrence::event(
            Some(start.clone()),
            &start,
            &end,
            std::slice::from_ref(&absolute),
        );
        let b = Occurrence::event(Some(end.clone()), &end, &end, &[absolute]);
        assert_eq!(plan_alarms(&[a, b], &berlin(), 0, i64::MAX).len(), 1);
    }

    #[test]
    fn tasks_remind_relative_to_due() {
        let mut fields = TaskFields::titled("Pay");
        fields.due = Some(PimTime::zoned("2026-10-05T12:00:00", "Europe/Berlin"));
        fields.alarms = vec![Alarm::minutes_before(60)];
        assert_eq!(
            fires(&plan_alarms(
                &[Occurrence::task(&fields)],
                &berlin(),
                0,
                i64::MAX
            )),
            ["2026-10-05T09:00:00Z"]
        );
        fields.status = TaskStatus::Completed;
        assert!(plan_alarms(&[Occurrence::task(&fields)], &berlin(), 0, i64::MAX).is_empty());
        let mut no_dates = TaskFields::titled("Someday");
        no_dates.alarms = vec![Alarm::minutes_before(5)];
        assert!(plan_alarms(&[Occurrence::task(&no_dates)], &berlin(), 0, i64::MAX).is_empty());
        // Malformed times are skipped.
        assert_eq!(OccurrenceTime::of(&PimTime::date("2026-02-30")), None);
        assert!(matches!(
            OccurrenceTime::of(&PimTime::zoned("2026-01-01T00:00:00", "Mars/Base")),
            Some(OccurrenceTime::Wall(_))
        ));
    }

    fn scheduler(
        store: &MemoryReminderStore,
        clock: &Arc<ManualClock>,
    ) -> ReminderScheduler<MemoryReminderStore, Arc<ManualClock>> {
        ReminderScheduler::load(store.clone(), clock.clone(), ReminderPolicy::default()).unwrap()
    }

    fn event(id: &str) -> ReminderItem {
        ReminderItem::Event(EventId::new(id))
    }

    #[test]
    fn reminders_fire_at_the_controlled_clock_time() {
        let store = MemoryReminderStore::new();
        let clock = Arc::new(ManualClock::at_unix(at("2026-03-27T07:00:00Z")));
        let mut s = scheduler(&store, &clock);
        let occurrences: Vec<Occurrence> = (27..=29).map(standup).collect();
        let plan = plan_alarms(&occurrences, &berlin(), clock.unix(), i64::MAX);
        s.replan(&event("standup"), &plan).unwrap();
        assert_eq!(s.next_wake(), Some(at("2026-03-27T07:45:00Z")));
        assert!(s.poll().unwrap().is_empty());
        clock.set(DateTime::from_timestamp(at("2026-03-27T07:45:00Z"), 0).unwrap());
        let fired = s.poll().unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].late_secs, 0);
        assert!(fired[0].notify);
        assert_eq!(fired[0].entry.state, ReminderState::Fired);
        // After the DST change the next one is an hour earlier in UTC.
        assert_eq!(s.next_wake(), Some(at("2026-03-28T07:45:00Z")));
        clock.advance(Duration::days(2));
        let fired = s.poll().unwrap();
        assert_eq!(
            fired.iter().map(|f| f.entry.fire_at).collect::<Vec<_>>(),
            [at("2026-03-28T07:45:00Z"), at("2026-03-29T06:45:00Z")]
        );
        assert_eq!(s.next_wake(), None);
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|e| e.state == ReminderState::Fired)
                .count(),
            3
        );
    }

    #[test]
    fn restarts_keep_reminders_and_catch_up() {
        let store = MemoryReminderStore::new();
        let clock = Arc::new(ManualClock::at_unix(at("2026-10-01T06:00:00Z")));
        {
            let mut s = scheduler(&store, &clock);
            let start = PimTime::utc("2026-10-01T09:00:00");
            let occ = Occurrence::event(
                None,
                &start,
                &PimTime::utc("2026-10-01T10:00:00"),
                &[Alarm::minutes_before(15), Alarm::minutes_before(120)],
            );
            s.replan(&event("e"), &plan_alarms(&[occ], &berlin(), 0, i64::MAX))
                .unwrap();
            assert_eq!(s.list().len(), 2);
            // Nail stops before either fires.
        }
        // Back three hours later: the 07:00 one is 2 h 15 min late (not
        // notified), the 08:45 one 30 minutes (notified).
        clock.set(DateTime::from_timestamp(at("2026-10-01T09:15:00Z"), 0).unwrap());
        let mut s = scheduler(&store, &clock);
        assert_eq!(s.next_wake(), Some(at("2026-10-01T07:00:00Z")));
        let fired = s.poll().unwrap();
        assert_eq!(fired.len(), 2);
        assert_eq!(fired[0].late_secs, 2 * 3600 + 15 * 60);
        assert!(!fired[0].notify);
        assert_eq!(fired[1].late_secs, 30 * 60);
        assert!(fired[1].notify);
        // Fired reminders survive another restart (listed, not re-fired).
        let mut s = scheduler(&store, &clock);
        assert_eq!(s.list().len(), 2);
        assert!(s.poll().unwrap().is_empty());
        assert_eq!(s.next_wake(), None);
    }

    #[test]
    fn snooze_and_dismiss() {
        let store = MemoryReminderStore::new();
        let clock = Arc::new(ManualClock::at_unix(at("2026-10-01T08:45:00Z")));
        let mut s = scheduler(&store, &clock);
        let start = PimTime::utc("2026-10-01T09:00:00");
        let occ = Occurrence::event(
            Some(start.clone()),
            &start,
            &PimTime::utc("2026-10-01T10:00:00"),
            &[Alarm::minutes_before(15), Alarm::minutes_before(10)],
        );
        s.replan(
            &event("e"),
            &plan_alarms(std::slice::from_ref(&occ), &berlin(), 0, i64::MAX),
        )
        .unwrap();
        let fired = s.poll().unwrap();
        assert_eq!(fired.len(), 1);
        let id = fired[0].entry.id.clone();
        // Snooze for 5 minutes.
        let now = clock.unix();
        assert!(matches!(
            s.snooze(std::slice::from_ref(&id), now),
            Err(ReminderError::NotInFuture)
        ));
        assert!(matches!(
            s.snooze(
                std::slice::from_ref(&id),
                now + MAX_REMINDER_SNOOZE_SECS + 1
            ),
            Err(ReminderError::TooFar)
        ));
        let ops = s
            .snooze(&[id.clone(), ReminderId::new("unknown")], now + 300)
            .unwrap();
        assert_eq!(
            ops,
            [ReminderOp::Snooze {
                item: event("e"),
                recurrence_id: Some(start.clone()),
                alarm_key: "0".into(),
                until: now + 300,
                at: now
            }]
        );
        assert_eq!(s.get(&id).unwrap().state, ReminderState::Snoozed);
        assert_eq!(s.next_wake(), Some(now + 300));
        // The snooze survives a restart.
        let mut s = scheduler(&store, &clock);
        assert_eq!(s.next_wake(), Some(now + 300));
        clock.advance(Duration::minutes(5));
        let fired = s.poll().unwrap();
        // Both: the snoozed one and the 08:50 alarm.
        assert_eq!(fired.len(), 2);
        // Re-planning with the same plan keeps the states.
        s.replan(
            &event("e"),
            &plan_alarms(std::slice::from_ref(&occ), &berlin(), 0, i64::MAX),
        )
        .unwrap();
        assert!(s.list().iter().all(|e| e.state == ReminderState::Fired));
        let ids: Vec<ReminderId> = s.list().iter().map(|e| e.id.clone()).collect();
        let ops = s.dismiss(&ids).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], ReminderOp::Acknowledge { .. }));
        assert!(s.list().is_empty());
        assert!(s.dismiss(&ids).unwrap().is_empty());
        // Dismissed stays dismissed across restarts and re-plans.
        let mut s = scheduler(&store, &clock);
        assert!(s.list().is_empty());
        s.replan(&event("e"), &plan_alarms(&[occ], &berlin(), 0, i64::MAX))
            .unwrap();
        assert!(s.list().is_empty());
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|e| e.state == ReminderState::Dismissed)
                .count(),
            2
        );
    }

    #[test]
    fn a_zone_change_moves_floating_reminders() {
        let store = MemoryReminderStore::new();
        let clock = Arc::new(ManualClock::at_unix(at("2026-10-01T00:00:00Z")));
        let mut s = scheduler(&store, &clock);
        let gym = Occurrence::event(
            None,
            &PimTime::floating("2026-10-01T18:00:00"),
            &PimTime::floating("2026-10-01T19:00:00"),
            &[Alarm::minutes_before(30)],
        );
        s.replan(
            &event("gym"),
            &plan_alarms(std::slice::from_ref(&gym), &berlin(), 0, i64::MAX),
        )
        .unwrap();
        assert_eq!(s.next_wake(), Some(at("2026-10-01T15:30:00Z")));
        // Snoozed earlier today, then the user flies to New York.
        let id = s.list()[0].id.clone();
        s.snooze(std::slice::from_ref(&id), at("2026-10-01T01:00:00Z"))
            .unwrap();
        s.replan(
            &event("gym"),
            &plan_alarms(&[gym], &new_york(), 0, i64::MAX),
        )
        .unwrap();
        let entry = s.get(&id).unwrap();
        assert_eq!(entry.fire_at, at("2026-10-01T21:30:00Z"));
        // Its time moved: pending again at the new time.
        assert_eq!(entry.state, ReminderState::Pending);
        assert_eq!(s.next_wake(), Some(at("2026-10-01T21:30:00Z")));
        // The item is deleted.
        s.forget(&event("gym"));
        assert_eq!(s.next_wake(), None);
    }

    #[test]
    fn store_failures_reschedule() {
        #[derive(Clone)]
        struct Failing(MemoryReminderStore, Arc<std::sync::atomic::AtomicBool>);
        impl ReminderStore for Failing {
            type Error = &'static str;
            fn load(&self) -> Result<Vec<ReminderEntry>, Self::Error> {
                Ok(self.0.load().unwrap())
            }
            fn replace(
                &self,
                item: &ReminderItem,
                planned: &[PlannedAlarm],
            ) -> Result<Vec<ReminderEntry>, Self::Error> {
                Ok(self.0.replace(item, planned).unwrap())
            }
            fn set_state(
                &self,
                ids: &[ReminderId],
                state: ReminderState,
                until: Option<i64>,
            ) -> Result<(), Self::Error> {
                if self.1.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("disk full");
                }
                self.0.set_state(ids, state, until).unwrap();
                Ok(())
            }
        }
        let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let store = Failing(MemoryReminderStore::new(), fail.clone());
        let clock = Arc::new(ManualClock::at_unix(100));
        let mut s =
            ReminderScheduler::load(store, clock.clone(), ReminderPolicy::default()).unwrap();
        let occ = Occurrence {
            recurrence_id: None,
            start: Some(OccurrenceTime::Instant(1_000)),
            end: None,
            alarms: vec![Alarm::minutes_before(1)],
        };
        s.replan(&event("e"), &plan_alarms(&[occ], &berlin(), 0, i64::MAX))
            .unwrap();
        clock.advance(Duration::seconds(10_000));
        assert_eq!(s.poll(), Err("disk full"));
        assert_eq!(s.next_wake(), Some(940));
        let err = s
            .snooze(&[s.list()[0].id.clone()], clock.unix() + 60)
            .unwrap_err();
        assert_eq!(err.message_id(), "reminder-error-store");
        fail.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(s.poll().unwrap().len(), 1);
        assert!(s.store().0.all()[0].state == ReminderState::Fired);
    }

    #[test]
    fn notifications() {
        let reminder = Reminder {
            id: "r1".into(),
            target: nail_api::pim::ReminderTarget::Task {
                task_id: "t".into(),
            },
            calendar_id: "c".into(),
            title: "Pay the bill".into(),
            location: Some(" Bank ".into()),
            description: None,
            starts_at: Some("2026-10-05T10:00:00Z".into()),
            all_day: false,
            fire_at: "2026-10-05T09:00:00Z".into(),
            state: ReminderState::Fired,
            snoozed_until: None,
        };
        let when = when_text(at("2026-10-05T10:00:00Z"), false, &berlin());
        assert_eq!(when, "2026-10-05 12:00");
        assert_eq!(
            when_text(at("2026-10-04T22:00:00Z"), true, &berlin()),
            "2026-10-05"
        );
        let n = reminder_notification(&reminder, &when, true);
        assert_eq!(n.id, "reminder:r1");
        assert_eq!(n.tag, n.id);
        assert_eq!(n.kind, NotificationKind::Reminder);
        assert_eq!(n.title.id, "notification-reminder-title");
        let body = n.body.unwrap();
        assert_eq!(body.id, "notification-reminder-body-location");
        assert_eq!(
            serde_json::to_value(&body).unwrap()["args"]["location"],
            "Bank"
        );
        let mut plain = reminder.clone();
        plain.location = None;
        assert_eq!(
            reminder_notification(&plain, &when, false).body.unwrap().id,
            "notification-reminder-body"
        );
        for e in [
            ReminderError::<()>::NotInFuture,
            ReminderError::TooFar,
            ReminderError::Store(()),
        ] {
            assert!(FLUENT_IDS.contains(&e.message_id()));
        }
        assert_eq!(
            serde_json::to_value(NOTIFICATION_KIND).unwrap(),
            serde_json::json!("reminder")
        );
    }
}
