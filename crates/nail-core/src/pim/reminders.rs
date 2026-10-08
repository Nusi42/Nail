//! Reminders (ADR 0061 / 0063): the `nail-triage` reminder scheduler on
//! the store's `pim_reminders`, on the triage clock (tests and demos move
//! it), planned for a window of days from the instance expansion
//! (`nail-ical`), and a task that fires them as `ReminderDue` events and
//! notifications.
//!
//! Notifications follow the notification settings: none when they are
//! switched off; during quiet hours a reminder is still shown (in the app
//! and as a notification) but without sound. Dismissing writes the
//! alarm's `ACKNOWLEDGED` (RFC 9074) into the item through the PIM queue,
//! so other devices stop showing it; snoozing stays on this device.
//!
//! Floods are bounded (ADR 0064): series repeating every minute or second
//! get no reminders, each item keeps a few late and the next upcoming
//! reminders ([`nail_triage::reminders::limit_plan`]), a planning pass
//! shares one expansion budget, and more than a few reminders due at once
//! become one summary notification.

use std::sync::Arc;

use chrono_tz::Tz;
use nail_api::pim::calendar::{AlarmTrigger, EventStatus, Frequency};
use nail_api::pim::{Alarm, CalendarEvent, PimItemRef, ReminderState, Task};
use nail_api::{ApiError, Event, EventId, ReminderId, Request, Response, TaskId};
use nail_store::Store;
use nail_store::pim::{PlannedReminder, ReminderItemKind, ReminderRecord};
use nail_triage::Clock;
use nail_triage::reminders::{
    FiredReminder, MAX_SINGLE_NOTIFICATIONS, Occurrence, PlannedAlarm, ReminderEntry,
    ReminderError, ReminderItem, ReminderOp, ReminderPolicy, ReminderScheduler, ReminderStore,
    limit_plan, plan_alarms,
};

use super::{local_zone, local_zone_name, parse_instant};
use crate::error::Result;
use crate::service::{Service, lock};

/// Days ahead reminders are planned for (planned again hourly).
pub(crate) const PLAN_DAYS: i64 = 7;
/// Reminders up to this far back are planned too (they fire late, once).
const CATCH_UP_SECS: i64 = 2 * 86_400;
/// How often the window moves on.
const REPLAN_SECS: i64 = 3600;

pub(super) type Scheduler = ReminderScheduler<StoreReminders, Arc<dyn Clock>>;

/// The store as the scheduler's persistence.
#[derive(Clone)]
pub(crate) struct StoreReminders(pub Store);

fn entry_of(record: ReminderRecord) -> ReminderEntry {
    ReminderEntry {
        id: record.id,
        item: match record.item_kind {
            ReminderItemKind::Event => ReminderItem::Event(EventId::new(record.item_id)),
            ReminderItemKind::Task => ReminderItem::Task(TaskId::new(record.item_id)),
        },
        recurrence_id: record.recurrence_id,
        alarm_key: record.alarm_key,
        fire_at: record.fire_at,
        state: record.state,
        snoozed_until: record.snoozed_until,
    }
}

fn item_parts(item: &ReminderItem) -> (ReminderItemKind, &str) {
    match item {
        ReminderItem::Event(id) => (ReminderItemKind::Event, id.as_str()),
        ReminderItem::Task(id) => (ReminderItemKind::Task, id.as_str()),
    }
}

use nail_util::time::unix_now as wall_now;

impl ReminderStore for StoreReminders {
    type Error = nail_store::Error;

    fn load(&self) -> nail_store::Result<Vec<ReminderEntry>> {
        Ok(self
            .0
            .reminder_records(&[
                ReminderState::Pending,
                ReminderState::Snoozed,
                ReminderState::Fired,
            ])?
            .into_iter()
            .map(entry_of)
            .collect())
    }

    fn replace(
        &self,
        item: &ReminderItem,
        planned: &[PlannedAlarm],
    ) -> nail_store::Result<Vec<ReminderEntry>> {
        let (kind, id) = item_parts(item);
        let planned: Vec<PlannedReminder> = planned
            .iter()
            .map(|p| PlannedReminder {
                recurrence_id: p.recurrence_id.clone(),
                alarm_key: p.alarm_key.clone(),
                fire_at: p.fire_at,
            })
            .collect();
        Ok(self
            .0
            .replace_item_reminders(kind, id, &planned, wall_now())?
            .into_iter()
            .map(entry_of)
            .collect())
    }

    fn set_state(
        &self,
        ids: &[ReminderId],
        state: ReminderState,
        snoozed_until: Option<i64>,
    ) -> nail_store::Result<()> {
        self.0
            .set_reminder_state(ids, state, snoozed_until, wall_now())?;
        Ok(())
    }
}

/// How far before or after an occurrence its alarms reach.
fn reach(alarms: &[Alarm]) -> i64 {
    alarms
        .iter()
        .map(|a| match &a.trigger {
            AlarmTrigger::Relative { offset_secs, .. } => offset_secs.abs(),
            AlarmTrigger::Absolute { .. } => 0,
        })
        .max()
        .unwrap_or(0)
        + 86_400
}

/// Whether a series repeats more often than hourly (its alarms are not
/// planned: they would fire every few minutes or seconds).
fn repeats_sub_hourly(event: &CalendarEvent) -> bool {
    event.recurrence.as_ref().is_some_and(|r| {
        r.rules
            .iter()
            .any(|rule| matches!(rule.freq, Frequency::Minutely | Frequency::Secondly))
    }) && !event.fields.start.is_date()
}

/// The reminders of an event's occurrences in `[from, until)`; the
/// expansion draws on `budget` (shared by a planning pass).
pub(crate) fn plan_event(
    event: &CalendarEvent,
    local: Tz,
    from: i64,
    until: i64,
    budget: &nail_ical::expand::ScanBudget,
) -> Vec<PlannedAlarm> {
    if repeats_sub_hourly(event) {
        return Vec::new();
    }
    let alarms = nail_ical::alarm::all_alarms(event);
    if alarms.is_empty() {
        return Vec::new();
    }
    let reach = reach(&alarms);
    let options = nail_ical::expand::ExpandOptions {
        from: super::utc(from - reach),
        to: super::utc(until + reach),
        viewer: local,
        limit: 10_000,
        this_and_future: &[],
        budget: Some(budget),
    };
    let occurrences: Vec<Occurrence> = nail_ical::expand::instances(event, &options)
        .into_iter()
        .filter(|i| i.status != EventStatus::Cancelled)
        .map(|i| {
            let fields = i
                .recurrence_id
                .as_ref()
                .and_then(|rid| event.overrides.iter().find(|o| &o.recurrence_id == rid))
                .map_or(&event.fields, |o| &o.fields);
            Occurrence::event(i.recurrence_id.clone(), &i.start, &i.end, &fields.alarms)
        })
        .collect();
    plan_alarms(&occurrences, &local, from, until)
}

/// The reminders of a task in `[from, until)`.
pub(crate) fn plan_task(task: &Task, local: Tz, from: i64, until: i64) -> Vec<PlannedAlarm> {
    plan_alarms(&[Occurrence::task(&task.fields)], &local, from, until)
}

/// Whether the item's calendar has reminders on.
fn reminders_on(store: &Store, calendar: &nail_api::CalendarId) -> bool {
    store
        .calendar(calendar)
        .ok()
        .flatten()
        .is_some_and(|c| c.settings.reminders)
}

pub(super) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::ListReminders { until } => {
            let until = until.as_deref().map(parse_instant).transpose()?;
            let tz = local_zone_name();
            Ok(Response::Reminders {
                reminders: service
                    .inner
                    .store
                    .call(move |s| s.list_reminders(until, &tz))
                    .await?,
            })
        }
        Request::SnoozeReminders {
            reminder_ids,
            until,
        } => {
            let until = parse_instant(&until)?;
            service
                .blocking(move |s| {
                    s.with_reminders(|scheduler| {
                        scheduler
                            .snooze(&reminder_ids, until)
                            .map_err(|err| match err {
                                ReminderError::Store(e) => e.into(),
                                other => ApiError::invalid_localized(
                                    other.to_string(),
                                    other.message_id(),
                                )
                                .into(),
                            })
                    })
                })
                .await?;
            service.inner.pim.reminder_wake.notify_one();
            service.broadcast(Event::RemindersChanged);
            Ok(Response::Done)
        }
        Request::DismissReminders { reminder_ids } => {
            let ops = service
                .blocking(move |s| {
                    s.with_reminders(|scheduler| Ok(scheduler.dismiss(&reminder_ids)?))
                })
                .await?;
            service.inner.pim.reminder_wake.notify_one();
            service.broadcast(Event::RemindersChanged);
            for op in ops {
                if let Err(err) = service.acknowledge(op).await {
                    tracing::warn!(%err, "cannot write a dismissed alarm into its item");
                }
            }
            Ok(Response::Done)
        }
        other => {
            Err(ApiError::internal(format!("{} routed to reminders", other.wire_type())).into())
        }
    }
}

/// The window reminders are planned for around `now`: the catch-up time
/// before it and [`PLAN_DAYS`] after it.
fn plan_window(now: i64) -> (i64, i64) {
    (
        now - CATCH_UP_SECS,
        now + PLAN_DAYS * nail_util::time::SECS_PER_DAY,
    )
}

impl Service {
    /// Runs `work` on the scheduler (blocking; loads it on first use).
    pub(crate) fn with_reminders<T>(
        &self,
        work: impl FnOnce(&mut Scheduler) -> Result<T>,
    ) -> Result<T> {
        let mut guard = lock(&self.inner.pim.reminders);
        let scheduler = match guard.take() {
            Some(scheduler) => scheduler,
            None => ReminderScheduler::load(
                StoreReminders(self.inner.store.clone()),
                Arc::clone(&self.inner.clock),
                ReminderPolicy::default(),
            )?,
        };
        work(guard.insert(scheduler))
    }

    /// Plans one item's reminders again (after it changed).
    pub(crate) async fn replan_item_reminders(&self, item: &PimItemRef) {
        let item = item.clone();
        let result = self
            .blocking(move |s| {
                let now = s.pim_now();
                let (from, until) = plan_window(now);
                let local = local_zone();
                let store = s.store();
                let (key, plan) = match &item {
                    PimItemRef::Event { event_id } => {
                        let plan = match store.event(event_id)? {
                            Some(e) if reminders_on(store, &e.calendar_id) => limit_plan(
                                plan_event(
                                    &e,
                                    local,
                                    from,
                                    until,
                                    &nail_ical::expand::ScanBudget::default(),
                                ),
                                now,
                            ),
                            Some(_) => Vec::new(),
                            None => {
                                s.forget_item_reminders(&item);
                                return Ok(());
                            }
                        };
                        (ReminderItem::Event(event_id.clone()), plan)
                    }
                    PimItemRef::Task { task_id } => {
                        let plan = match store.task(task_id)? {
                            Some(t) if reminders_on(store, &t.calendar_id) => {
                                limit_plan(plan_task(&t, local, from, until), now)
                            }
                            Some(_) => Vec::new(),
                            None => {
                                s.forget_item_reminders(&item);
                                return Ok(());
                            }
                        };
                        (ReminderItem::Task(task_id.clone()), plan)
                    }
                    _ => return Ok(()),
                };
                s.with_reminders(|scheduler| Ok(scheduler.replan(&key, &plan)?))
            })
            .await;
        if let Err(err) = result {
            tracing::warn!(%err, "cannot plan reminders");
        }
        self.inner.pim.reminder_wake.notify_one();
        self.broadcast(Event::RemindersChanged);
    }

    /// Drops a deleted item's reminders from the scheduler.
    pub(crate) fn forget_item_reminders(&self, item: &PimItemRef) {
        let key = match item {
            PimItemRef::Event { event_id } => ReminderItem::Event(event_id.clone()),
            PimItemRef::Task { task_id } => ReminderItem::Task(task_id.clone()),
            _ => return,
        };
        if let Some(scheduler) = lock(&self.inner.pim.reminders).as_mut() {
            scheduler.forget(&key);
        }
    }

    /// Plans every item's reminders for the window (start, hourly, after a
    /// sync, zone or calendar changes).
    pub(crate) async fn replan_all_reminders(&self) {
        let result = self
            .blocking(|s| {
                let now = s.pim_now();
                let (from, until) = plan_window(now);
                let local = local_zone();
                let store = s.store();
                let calendars = store.calendars(None)?;
                let on = |id: &nail_api::CalendarId| {
                    calendars
                        .iter()
                        .any(|c| &c.id == id && c.settings.reminders)
                };
                // Items that have reminders now must be planned (maybe to none).
                let mut known: std::collections::HashSet<(bool, String)> = store
                    .reminder_records(&[])?
                    .into_iter()
                    .map(|r| (r.item_kind == ReminderItemKind::Event, r.item_id))
                    .collect();
                let mut plans: Vec<(ReminderItem, Vec<PlannedAlarm>)> = Vec::new();
                // One expansion budget for the whole pass.
                let budget = nail_ical::expand::ScanBudget::default();
                for event in store.events(None)? {
                    let had = known.remove(&(true, event.id.0.clone()));
                    let plan = if on(&event.calendar_id) {
                        limit_plan(plan_event(&event, local, from, until, &budget), now)
                    } else {
                        Vec::new()
                    };
                    if had || !plan.is_empty() {
                        plans.push((ReminderItem::Event(event.id), plan));
                    }
                }
                let lists: Vec<nail_api::CalendarId> = calendars
                    .iter()
                    .filter(|c| c.has_tasks())
                    .map(|c| c.id.clone())
                    .collect();
                for task in store.list_tasks(Some(&lists), true)? {
                    let had = known.remove(&(false, task.id.0.clone()));
                    let plan = if on(&task.calendar_id) {
                        limit_plan(plan_task(&task, local, from, until), now)
                    } else {
                        Vec::new()
                    };
                    if had || !plan.is_empty() {
                        plans.push((ReminderItem::Task(task.id), plan));
                    }
                }
                s.with_reminders(|scheduler| {
                    for (item, plan) in &plans {
                        scheduler.replan(item, plan)?;
                    }
                    scheduler.reload()?;
                    Ok(())
                })
            })
            .await;
        if let Err(err) = result {
            tracing::warn!(%err, "cannot plan reminders");
        }
        self.inner.pim.reminder_wake.notify_one();
    }

    /// Fires due reminders: `ReminderDue` plus a notification each.
    async fn fire_reminders(&self) -> Result<usize> {
        let fired: Vec<FiredReminder> = self
            .blocking(|s| s.with_reminders(|scheduler| Ok(scheduler.poll()?)))
            .await?;
        if fired.is_empty() {
            return Ok(0);
        }
        let ids: Vec<ReminderId> = fired.iter().map(|f| f.entry.id.clone()).collect();
        let tz = local_zone_name();
        let (views, settings) = self
            .inner
            .store
            .call(move |s| {
                let mut records = Vec::new();
                for id in &ids {
                    records.extend(s.reminder_record(id)?);
                }
                Ok((s.reminder_views(&records, &tz)?, s.notification_settings()?))
            })
            .await?;
        let local = local_zone();
        let quiet = settings.quiet_hours.as_ref().is_some_and(|q| {
            let now = self.inner.clock.now().with_timezone(&chrono::Local);
            nail_triage::notify::in_quiet_hours(q, &now)
        });
        let notify_ids: std::collections::HashSet<&ReminderId> = fired
            .iter()
            .filter(|f| f.notify)
            .map(|f| &f.entry.id)
            .collect();
        let to_notify: Vec<&nail_api::pim::Reminder> = views
            .iter()
            .filter(|r| notify_ids.contains(&r.id))
            .collect();
        // Many at once (after a restart, a flood of alarms): one summary.
        let summarize = to_notify.len() > MAX_SINGLE_NOTIFICATIONS;
        if summarize
            && settings.enabled
            && let Some(first) = to_notify
                .iter()
                .min_by(|a, b| (&a.fire_at, &a.id).cmp(&(&b.fire_at, &b.id)))
        {
            let notification = nail_triage::reminders::reminders_summary_notification(
                first,
                to_notify.len(),
                settings.sound && !quiet,
            );
            tracing::info!(id = %notification.id, count = to_notify.len(), "reminder summary notification");
            self.broadcast(Event::NotificationRequested { notification });
        }
        for reminder in &views {
            self.broadcast(Event::ReminderDue {
                reminder: Box::new(reminder.clone()),
            });
            let notify = notify_ids.contains(&reminder.id);
            if notify && settings.enabled && !summarize {
                let when = reminder
                    .starts_at
                    .as_deref()
                    .and_then(|t| parse_instant(t).ok())
                    .map(|t| nail_triage::reminders::when_text(t, reminder.all_day, &local))
                    .unwrap_or_default();
                let notification = nail_triage::reminders::reminder_notification(
                    reminder,
                    &when,
                    settings.sound && !quiet,
                );
                tracing::info!(id = %notification.id, "reminder notification");
                self.broadcast(Event::NotificationRequested { notification });
            }
        }
        self.broadcast(Event::RemindersChanged);
        // Plug-in jobs subscribed to `event.reminder` (M4a; no message ids).
        if !views.is_empty()
            && let Some(host) = self.inner.plugins.existing_host()
        {
            host.deliver_event(nail_api::plugins::jobs::PluginEventName::EventReminder, &[]);
        }
        Ok(views.len())
    }

    /// Writes a dismissal (RFC 9074 `ACKNOWLEDGED`) into the item.
    async fn acknowledge(&self, op: ReminderOp) -> Result<()> {
        let ReminderOp::Acknowledge {
            item,
            recurrence_id,
            alarm_key,
            at,
        } = op
        else {
            return Ok(());
        };
        let base = alarm_key.split('+').next().unwrap_or_default().to_owned();
        let stamp = nail_store::pim::time::utc_string(at);
        let find = |alarms: &mut Vec<Alarm>| -> bool {
            let found = alarms
                .iter_mut()
                .enumerate()
                .find(|(i, a)| a.uid.as_deref() == Some(base.as_str()) || i.to_string() == base);
            match found {
                Some((_, alarm)) => {
                    alarm.acknowledged = Some(stamp.clone());
                    true
                }
                None => false,
            }
        };
        match item {
            ReminderItem::Event(id) => {
                let mut event = self.event(&id).await?;
                if event.read_only {
                    return Ok(());
                }
                let changed = match recurrence_id
                    .as_ref()
                    .and_then(|rid| event.overrides.iter_mut().find(|o| &o.recurrence_id == rid))
                {
                    Some(o) => find(&mut o.fields.alarms),
                    None => find(&mut event.fields.alarms),
                };
                if changed {
                    let reference = PimItemRef::Event { event_id: id };
                    let original = self.item_raw(&reference).await?;
                    let raw = self.event_text(&event, original.as_deref(), None);
                    self.save_item(nail_store::pim::PimItem::Event(event), raw)
                        .await?;
                }
            }
            ReminderItem::Task(id) => {
                let mut task = self.task(&id).await?;
                if task.read_only || !find(&mut task.fields.alarms) {
                    return Ok(());
                }
                let parent_uid = match &task.fields.parent_id {
                    Some(p) => Some(self.task(p).await?.uid),
                    None => None,
                };
                let reference = PimItemRef::Task { task_id: id };
                let original = self.item_raw(&reference).await?;
                let raw = self.task_text(&task, parent_uid.as_deref(), original.as_deref());
                self.save_item(nail_store::pim::PimItem::Task { task, parent_uid }, raw)
                    .await?;
            }
        }
        Ok(())
    }

    fn next_reminder(&self) -> Option<i64> {
        lock(&self.inner.pim.reminders)
            .as_ref()
            .and_then(Scheduler::next_wake)
    }
}

/// The reminder task: plans at start and hourly, fires what is due,
/// sleeps until the next reminder (at most `snooze_poll`, so clock
/// changes are noticed).
pub(super) async fn run(service: Service) {
    service.replan_all_reminders().await;
    let mut planned_at = service.pim_now();
    let cap = service.inner.timing.snooze_poll;
    loop {
        match service.fire_reminders().await {
            Ok(0) => {}
            Ok(n) => tracing::info!(count = n, "reminders fired"),
            Err(err) => tracing::warn!(%err, "cannot fire reminders"),
        }
        let now = service.pim_now();
        if (now - planned_at).abs() >= REPLAN_SECS {
            service.replan_all_reminders().await;
            planned_at = now;
        }
        let wait = crate::clock::timer_wait(service.next_reminder(), service.pim_now(), cap);
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = service.inner.pim.reminder_wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::{EventFields, PimItemSync, PimTime, Recurrence, RecurrenceRule};

    fn at(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp()
    }

    #[test]
    fn events_are_planned_per_occurrence() {
        let mut fields = EventFields::new(
            "Standup",
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-05T09:15:00", "Europe/Berlin"),
        );
        fields.alarms.push(Alarm::minutes_before(10));
        let event = CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields,
            recurrence: Some(Recurrence::rule(RecurrenceRule::new(Frequency::Daily))),
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        };
        let plan = plan_event(
            &event,
            Tz::Europe__Berlin,
            at("2026-10-24T00:00:00Z"),
            at("2026-10-27T00:00:00Z"),
            &nail_ical::expand::ScanBudget::default(),
        );
        let fires: Vec<String> = plan
            .iter()
            .map(|p| nail_store::pim::time::utc_string(p.fire_at))
            .collect();
        // Across the end of summer time (25 October): 06:50Z, then 07:50Z.
        assert_eq!(
            fires,
            [
                "2026-10-24T06:50:00Z",
                "2026-10-25T07:50:00Z",
                "2026-10-26T07:50:00Z"
            ]
        );
    }

    #[test]
    fn closed_tasks_have_no_reminders() {
        let mut fields = nail_api::pim::TaskFields::titled("Pay");
        fields.due = Some(PimTime::utc("2026-10-05T12:00:00"));
        let mut alarm = Alarm::minutes_before(30);
        alarm.trigger = AlarmTrigger::Relative {
            offset_secs: -1800,
            related: nail_api::pim::calendar::AlarmRelated::End,
        };
        fields.alarms.push(alarm);
        let mut task = Task {
            id: "t".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields,
            recurrence: None,
            subtask_ids: Vec::new(),
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        };
        let window = (at("2026-10-01T00:00:00Z"), at("2026-10-10T00:00:00Z"));
        let plan = plan_task(&task, Tz::UTC, window.0, window.1);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].fire_at, at("2026-10-05T11:30:00Z"));
        task.fields.status = nail_api::pim::TaskStatus::Completed;
        assert_eq!(plan_task(&task, Tz::UTC, window.0, window.1).len(), 0);
    }

    /// Series repeating every minute or second plan no reminders at all;
    /// hourly ones still do.
    #[test]
    fn sub_hourly_series_get_no_reminders() {
        let mut fields = EventFields::new(
            "Spam",
            PimTime::utc("2026-10-05T09:00:00"),
            PimTime::utc("2026-10-05T09:00:01"),
        );
        fields.alarms.push(Alarm::minutes_before(0));
        let mut event = CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields,
            recurrence: Some(Recurrence::rule(RecurrenceRule::new(Frequency::Secondly))),
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        };
        let window = (at("2026-10-05T00:00:00Z"), at("2026-10-06T00:00:00Z"));
        let budget = nail_ical::expand::ScanBudget::default();
        for frequency in [Frequency::Secondly, Frequency::Minutely] {
            event.recurrence = Some(Recurrence::rule(RecurrenceRule::new(frequency)));
            assert_eq!(
                plan_event(&event, Tz::UTC, window.0, window.1, &budget).len(),
                0,
                "{frequency:?}"
            );
        }
        event.recurrence = Some(Recurrence::rule(RecurrenceRule::new(Frequency::Hourly)));
        let plan = plan_event(&event, Tz::UTC, window.0, window.1, &budget);
        assert_eq!(plan.len(), 15);
        // The per-item cap keeps a few late and the next ones.
        let now = at("2026-10-05T20:30:00Z");
        let limited = limit_plan(plan, now);
        assert_eq!(limited.len(), 3 + 3);
    }
}
