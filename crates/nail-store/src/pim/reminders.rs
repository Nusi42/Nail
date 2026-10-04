//! Reminders (`pim_reminders`): one row per alarm occurrence of an event
//! occurrence or a task, with its state — pending, fired, snoozed,
//! dismissed. The reminder scheduler (`nail-triage::reminders`) plans the
//! rows ([`Store::replace_item_reminders`]) and moves their states; the
//! core implements its store trait on these methods. Rows go with their
//! event or task (triggers of schema v10).
//!
//! Ids are derived from the item, the occurrence and the alarm, so
//! re-planning after a restart or an edit finds the same rows and keeps
//! their state unless the fire time changed.

use nail_api::pim::{PimTime, Reminder, ReminderState, ReminderTarget};
use nail_api::{EventId, ReminderId, TaskId};
use rusqlite::{OptionalExtension, Row, params};
use sha2::{Digest, Sha256};

use super::items::{in_list, load_events};
use super::time::{cache_time, parse_zone, place, utc_string};
use crate::error::{Error, Result};
use crate::store::Store;

/// What a reminder belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReminderItemKind {
    /// An event (series).
    Event,
    /// A task.
    Task,
}

impl ReminderItemKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Task => "task",
        }
    }
}

/// A reminder the scheduler wants (one alarm of one occurrence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedReminder {
    /// The occurrence (`None` for single events and tasks).
    pub recurrence_id: Option<PimTime>,
    /// The alarm's `UID`, else its position (repeats get a suffix).
    pub alarm_key: String,
    /// When it fires (Unix seconds).
    pub fire_at: i64,
}

/// A stored reminder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderRecord {
    /// Its id.
    pub id: ReminderId,
    /// Event or task.
    pub item_kind: ReminderItemKind,
    /// The event or task id.
    pub item_id: String,
    /// The occurrence.
    pub recurrence_id: Option<PimTime>,
    /// The alarm.
    pub alarm_key: String,
    /// When it fires (Unix seconds).
    pub fire_at: i64,
    /// Its state.
    pub state: ReminderState,
    /// When a snoozed reminder comes back (Unix seconds).
    pub snoozed_until: Option<i64>,
    /// Last change (Unix seconds).
    pub updated_at: i64,
}

impl ReminderRecord {
    /// When the scheduler has to act: the snooze end for snoozed ones,
    /// the fire time for pending ones; `None` otherwise.
    pub fn due_at(&self) -> Option<i64> {
        match self.state {
            ReminderState::Pending => Some(self.fire_at),
            ReminderState::Snoozed => Some(self.snoozed_until.unwrap_or(self.fire_at)),
            ReminderState::Fired | ReminderState::Dismissed => None,
        }
    }

    /// The contract's target.
    pub fn target(&self) -> ReminderTarget {
        match self.item_kind {
            ReminderItemKind::Event => ReminderTarget::Event {
                event_id: EventId::new(self.item_id.clone()),
                recurrence_id: self.recurrence_id.clone(),
            },
            ReminderItemKind::Task => ReminderTarget::Task {
                task_id: TaskId::new(self.item_id.clone()),
            },
        }
    }
}

pub(crate) fn state_str(state: ReminderState) -> &'static str {
    match state {
        ReminderState::Pending => "pending",
        ReminderState::Fired => "fired",
        ReminderState::Snoozed => "snoozed",
        ReminderState::Dismissed => "dismissed",
    }
}

fn parse_state(text: &str) -> ReminderState {
    match text {
        "fired" => ReminderState::Fired,
        "snoozed" => ReminderState::Snoozed,
        "dismissed" => ReminderState::Dismissed,
        _ => ReminderState::Pending,
    }
}

fn rid_text(rid: Option<&PimTime>) -> Result<String> {
    Ok(match rid {
        Some(t) => serde_json::to_string(t)?,
        None => String::new(),
    })
}

/// The id of a reminder (stable across re-planning).
pub fn reminder_id(
    kind: ReminderItemKind,
    item_id: &str,
    recurrence_id: Option<&PimTime>,
    alarm_key: &str,
) -> ReminderId {
    let rid = recurrence_id
        .and_then(|t| serde_json::to_string(t).ok())
        .unwrap_or_default();
    let mut hash = Sha256::new();
    for part in [kind.as_str(), item_id, &rid, alarm_key] {
        hash.update(part.as_bytes());
        hash.update([0u8]);
    }
    ReminderId::new(format!("rem-{}", &hex::encode(hash.finalize())[..24]))
}

const COLS: &str =
    "id, item_kind, item_id, recurrence_id, alarm_key, fire_at, state, snoozed_until, updated_at";

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<ReminderRecord> {
    let rid: String = row.get(3)?;
    Ok(ReminderRecord {
        id: ReminderId(row.get(0)?),
        item_kind: if row.get::<_, String>(1)? == "task" {
            ReminderItemKind::Task
        } else {
            ReminderItemKind::Event
        },
        item_id: row.get(2)?,
        recurrence_id: if rid.is_empty() {
            None
        } else {
            serde_json::from_str(&rid).ok()
        },
        alarm_key: row.get(4)?,
        fire_at: row.get(5)?,
        state: parse_state(&row.get::<_, String>(6)?),
        snoozed_until: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

impl Store {
    /// Replaces an item's planned reminders. Rows that stay (same
    /// occurrence and alarm) keep their state when their fire time is
    /// unchanged and become pending again when it moved; rows no longer
    /// planned go. Returns the item's reminders.
    pub fn replace_item_reminders(
        &self,
        kind: ReminderItemKind,
        item_id: &str,
        planned: &[PlannedReminder],
        now: i64,
    ) -> Result<Vec<ReminderRecord>> {
        self.with_tx(|tx| {
            let table = match kind {
                ReminderItemKind::Event => "pim_events",
                ReminderItemKind::Task => "pim_tasks",
            };
            let exists: bool = tx.query_row(
                &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"),
                [item_id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::not_found(table, item_id));
            }
            let mut keep: Vec<String> = Vec::with_capacity(planned.len());
            for p in planned {
                let id = reminder_id(kind, item_id, p.recurrence_id.as_ref(), &p.alarm_key);
                tx.execute(
                    "INSERT INTO pim_reminders
                         (id, item_kind, item_id, recurrence_id, alarm_key, fire_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (id) DO UPDATE SET
                         state = CASE WHEN fire_at = excluded.fire_at THEN state ELSE 'pending' END,
                         snoozed_until = CASE WHEN fire_at = excluded.fire_at
                                              THEN snoozed_until ELSE NULL END,
                         updated_at = CASE WHEN fire_at = excluded.fire_at
                                           THEN updated_at ELSE excluded.updated_at END,
                         fire_at = excluded.fire_at",
                    params![
                        id.as_str(),
                        kind.as_str(),
                        item_id,
                        rid_text(p.recurrence_id.as_ref())?,
                        p.alarm_key,
                        p.fire_at,
                        now
                    ],
                )?;
                keep.push(id.0);
            }
            let mut args: Vec<&str> = vec![kind.as_str(), item_id];
            args.extend(keep.iter().map(String::as_str));
            tx.execute(
                &format!(
                    "DELETE FROM pim_reminders WHERE item_kind = ?1 AND item_id = ?2 AND NOT {}",
                    in_list("id", keep.len(), 3)
                ),
                rusqlite::params_from_iter(args),
            )?;
            let mut stmt = tx.prepare(&format!(
                "SELECT {COLS} FROM pim_reminders WHERE item_kind = ?1 AND item_id = ?2
                 ORDER BY fire_at, id"
            ))?;
            let rows = stmt.query_map(params![kind.as_str(), item_id], record_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Reminders in some states (all when empty), by fire time.
    pub fn reminder_records(&self, states: &[ReminderState]) -> Result<Vec<ReminderRecord>> {
        let names: Vec<&str> = states.iter().map(|s| state_str(*s)).collect();
        self.with_conn(|conn| {
            let filter = if names.is_empty() {
                "1".to_owned()
            } else {
                in_list("state", names.len(), 1)
            };
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLS} FROM pim_reminders WHERE {filter} ORDER BY fire_at, id"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(names), record_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// One reminder.
    pub fn reminder_record(&self, id: &ReminderId) -> Result<Option<ReminderRecord>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLS} FROM pim_reminders WHERE id = ?1"),
                    [id.as_str()],
                    record_from_row,
                )
                .optional()?)
        })
    }

    /// Sets the state of reminders (and the snooze end for `snoozed`).
    /// Returns how many exist.
    pub fn set_reminder_state(
        &self,
        ids: &[ReminderId],
        state: ReminderState,
        snoozed_until: Option<i64>,
        now: i64,
    ) -> Result<usize> {
        let snoozed_until = if state == ReminderState::Snoozed {
            snoozed_until
        } else {
            None
        };
        self.with_tx(|tx| {
            let mut n = 0;
            for id in ids {
                n += tx.execute(
                    "UPDATE pim_reminders SET state = ?2, snoozed_until = ?3, updated_at = ?4
                     WHERE id = ?1",
                    params![id.as_str(), state_str(state), snoozed_until, now],
                )?;
            }
            Ok(n)
        })
    }

    /// Reminders as the UI shows them, by fire time: fired and snoozed
    /// ones, plus pending ones firing before `until` (Unix seconds). Dates
    /// and floating times are placed in `local_tz`.
    pub fn list_reminders(&self, until: Option<i64>, local_tz: &str) -> Result<Vec<Reminder>> {
        let records: Vec<ReminderRecord> = self
            .reminder_records(&[])?
            .into_iter()
            .filter(|r| match r.state {
                ReminderState::Fired | ReminderState::Snoozed => true,
                ReminderState::Pending => until.is_some_and(|u| r.fire_at < u),
                ReminderState::Dismissed => false,
            })
            .collect();
        self.reminder_views(&records, local_tz)
    }

    /// The views of reminder records (records whose item is gone are
    /// skipped).
    pub fn reminder_views(
        &self,
        records: &[ReminderRecord],
        local_tz: &str,
    ) -> Result<Vec<Reminder>> {
        let tz = parse_zone(local_tz)
            .ok_or_else(|| Error::InvalidData(format!("unknown time zone {local_tz:?}")))?;
        let instant = |time: &PimTime| cache_time(time).map(|c| utc_string(place(c, &tz)));
        let mut out = Vec::with_capacity(records.len());
        for record in records {
            let view = match record.item_kind {
                ReminderItemKind::Event => {
                    let event = self.with_conn(|conn| {
                        load_events(conn, "e.id = ?1", &[record.item_id.as_str()])
                    })?;
                    let Some(event) = event.into_iter().next() else {
                        continue;
                    };
                    let fields = record
                        .recurrence_id
                        .as_ref()
                        .and_then(|rid| event.overrides.iter().find(|o| &o.recurrence_id == rid))
                        .map_or(&event.fields, |o| &o.fields);
                    let overridden = !std::ptr::eq(fields, &raw const event.fields);
                    let start = if overridden {
                        Some(&fields.start)
                    } else {
                        record.recurrence_id.as_ref().or(Some(&event.fields.start))
                    };
                    Reminder {
                        id: record.id.clone(),
                        target: record.target(),
                        calendar_id: event.calendar_id.clone(),
                        title: fields.title.clone(),
                        location: fields.location.clone(),
                        description: fields.description.clone(),
                        starts_at: start.and_then(instant),
                        all_day: start.is_some_and(PimTime::is_date),
                        fire_at: utc_string(record.fire_at),
                        state: record.state,
                        snoozed_until: record.snoozed_until.map(utc_string),
                    }
                }
                ReminderItemKind::Task => {
                    let Some(task) = self.task(&TaskId::new(record.item_id.clone()))? else {
                        continue;
                    };
                    let when = task.fields.due.as_ref().or(task.fields.start.as_ref());
                    Reminder {
                        id: record.id.clone(),
                        target: record.target(),
                        calendar_id: task.calendar_id.clone(),
                        title: task.fields.title.clone(),
                        location: task.fields.location.clone(),
                        description: task.fields.description.clone(),
                        starts_at: when.and_then(instant),
                        all_day: when.is_some_and(PimTime::is_date),
                        fire_at: utc_string(record.fire_at),
                        state: record.state,
                        snoozed_until: record.snoozed_until.map(utc_string),
                    }
                }
            };
            out.push(view);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use nail_api::pim::{EventOverride, PimItemKind};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::pim::testing::{self, raw};
    use crate::pim::{PimItem, item_ref};

    fn plan(rid: Option<PimTime>, key: &str, fire_at: i64) -> PlannedReminder {
        PlannedReminder {
            recurrence_id: rid,
            alarm_key: key.into(),
            fire_at,
        }
    }

    #[test]
    fn replanning_keeps_states_unless_the_time_moved() {
        let store = testing::store();
        let event = testing::event("e", "uid-e", "Meeting");
        store
            .save_pim_item(&PimItem::Event(event), &raw("e"), 1)
            .unwrap();
        let rid = PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin");
        let records = store
            .replace_item_reminders(
                ReminderItemKind::Event,
                "e",
                &[
                    plan(Some(rid.clone()), "0", 100),
                    plan(Some(rid.clone()), "1", 200),
                ],
                5,
            )
            .unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.state == ReminderState::Pending));
        assert_eq!(records[0].due_at(), Some(100));
        let first = records[0].id.clone();
        let second = records[1].id.clone();
        assert_eq!(
            first,
            reminder_id(ReminderItemKind::Event, "e", Some(&rid), "0")
        );
        store
            .set_reminder_state(
                std::slice::from_ref(&first),
                ReminderState::Dismissed,
                None,
                6,
            )
            .unwrap();
        store
            .set_reminder_state(
                std::slice::from_ref(&second),
                ReminderState::Snoozed,
                Some(500),
                6,
            )
            .unwrap();
        assert_eq!(
            store.reminder_record(&second).unwrap().unwrap().due_at(),
            Some(500)
        );
        // Same plan (a restart): states stay.
        let again = store
            .replace_item_reminders(
                ReminderItemKind::Event,
                "e",
                &[
                    plan(Some(rid.clone()), "0", 100),
                    plan(Some(rid.clone()), "1", 200),
                ],
                7,
            )
            .unwrap();
        assert_eq!(again[0].state, ReminderState::Dismissed);
        assert_eq!(again[0].due_at(), None);
        assert_eq!(again[1].snoozed_until, Some(500));
        // The event moved: alarm 0 fires later (pending again), alarm 1 is gone.
        let moved = store
            .replace_item_reminders(
                ReminderItemKind::Event,
                "e",
                &[plan(Some(rid.clone()), "0", 150)],
                8,
            )
            .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].state, ReminderState::Pending);
        assert_eq!(moved[0].fire_at, 150);
        assert_eq!(moved[0].updated_at, 8);
        assert!(store.reminder_record(&second).unwrap().is_none());
        // Unknown items are refused (no orphans).
        assert!(
            store
                .replace_item_reminders(ReminderItemKind::Task, "nope", &[], 1)
                .is_err()
        );
        // Deleting the event drops its reminders.
        store
            .delete_pim_item(&item_ref(PimItemKind::Event, "e"), 9)
            .unwrap();
        assert!(store.reminder_records(&[]).unwrap().is_empty());
    }

    #[test]
    fn views_carry_the_occurrence() {
        let store = testing::store();
        let mut event = testing::event("e", "uid-e", "Weekly");
        event.fields.location = Some("Room 1".into());
        event.recurrence = Some(testing::daily(nail_api::pim::RecurrenceEnd::Never));
        let rid = PimTime::zoned("2026-10-02T09:00:00", "Europe/Berlin");
        let mut moved = event.fields.clone();
        moved.title = "Moved".into();
        moved.start = PimTime::zoned("2026-10-02T11:00:00", "Europe/Berlin");
        moved.end = PimTime::zoned("2026-10-02T12:00:00", "Europe/Berlin");
        event.overrides = vec![EventOverride {
            recurrence_id: rid.clone(),
            fields: moved,
            this_and_future: false,
        }];
        store
            .save_pim_item(&PimItem::Event(event), &raw("e"), 1)
            .unwrap();
        let mut task = testing::task("t", "ut", "Pay bill");
        task.fields.due = Some(PimTime::date("2026-10-05"));
        store
            .save_pim_item(&PimItem::task(task), &raw("t"), 1)
            .unwrap();
        let first = PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin");
        store
            .replace_item_reminders(
                ReminderItemKind::Event,
                "e",
                &[
                    plan(
                        Some(first.clone()),
                        "0",
                        testing::at("2026-10-01T06:45:00Z"),
                    ),
                    plan(Some(rid.clone()), "0", testing::at("2026-10-02T08:45:00Z")),
                ],
                1,
            )
            .unwrap();
        let task_reminders = store
            .replace_item_reminders(
                ReminderItemKind::Task,
                "t",
                &[plan(None, "0", testing::at("2026-10-04T07:00:00Z"))],
                1,
            )
            .unwrap();
        // Nothing fired yet: only pending ones before `until`.
        assert!(
            store
                .list_reminders(None, "Europe/Berlin")
                .unwrap()
                .is_empty()
        );
        let soon = store
            .list_reminders(Some(testing::at("2026-10-03T00:00:00Z")), "Europe/Berlin")
            .unwrap();
        assert_eq!(soon.len(), 2);
        assert_eq!(soon[0].starts_at.as_deref(), Some("2026-10-01T07:00:00Z"));
        assert_eq!(soon[0].location.as_deref(), Some("Room 1"));
        assert_eq!(soon[1].title, "Moved");
        assert_eq!(soon[1].starts_at.as_deref(), Some("2026-10-02T09:00:00Z"));
        assert_eq!(
            soon[1].target,
            ReminderTarget::Event {
                event_id: "e".into(),
                recurrence_id: Some(rid)
            }
        );
        // The task fires; its all-day due date is placed in the local zone.
        store
            .set_reminder_state(
                &[task_reminders[0].id.clone()],
                ReminderState::Fired,
                None,
                2,
            )
            .unwrap();
        let fired = store.list_reminders(None, "Europe/Berlin").unwrap();
        assert_eq!(fired.len(), 1);
        assert!(fired[0].all_day);
        assert_eq!(fired[0].starts_at.as_deref(), Some("2026-10-04T22:00:00Z"));
        assert_eq!(fired[0].fire_at, "2026-10-04T07:00:00Z");
        assert_eq!(fired[0].state, ReminderState::Fired);
        assert!(store.list_reminders(None, "Bad/Zone").is_err());
        assert_eq!(
            store
                .reminder_records(&[ReminderState::Pending])
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn states_and_the_cache_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nail.sqlite3");
        let rid = PimTime::zoned("2026-03-29T09:00:00", "Europe/Berlin");
        let snoozed = {
            let store = Store::open(&path).unwrap();
            store
                .save_calendar(
                    &nail_api::pim::Calendar {
                        id: testing::CAL.into(),
                        source_id: nail_api::PimSourceId::local(),
                        account_id: None,
                        settings: nail_api::pim::CalendarSettings::named("Cal"),
                        components: vec![nail_api::pim::CalendarComponent::Event],
                        read_only: false,
                        timezone: None,
                        exchange: None,
                    },
                    None,
                )
                .unwrap();
            let mut event = testing::event_at(
                "s",
                "us",
                "Standup",
                PimTime::zoned("2026-03-27T09:00:00", "Europe/Berlin"),
                PimTime::zoned("2026-03-27T09:15:00", "Europe/Berlin"),
            );
            event.recurrence = Some(testing::daily(nail_api::pim::RecurrenceEnd::Count {
                count: 5,
            }));
            store
                .save_pim_item(&PimItem::Event(event), &raw("s"), 1)
                .unwrap();
            let expander = testing::TestExpander::default();
            let instances = store
                .list_event_instances(
                    testing::at("2026-03-27T00:00:00Z"),
                    testing::at("2026-04-01T00:00:00Z"),
                    None,
                    "Europe/Berlin",
                    &expander,
                )
                .unwrap();
            assert_eq!(instances.len(), 5);
            let records = store
                .replace_item_reminders(
                    ReminderItemKind::Event,
                    "s",
                    &[plan(
                        Some(rid.clone()),
                        "0",
                        testing::at("2026-03-29T06:45:00Z"),
                    )],
                    2,
                )
                .unwrap();
            store
                .set_reminder_state(
                    &[records[0].id.clone()],
                    ReminderState::Snoozed,
                    Some(testing::at("2026-03-29T06:50:00Z")),
                    3,
                )
                .unwrap();
            records[0].id.clone()
        };
        let store = Store::open(&path).unwrap();
        let record = store.reminder_record(&snoozed).unwrap().unwrap();
        assert_eq!(record.state, ReminderState::Snoozed);
        assert_eq!(record.due_at(), Some(testing::at("2026-03-29T06:50:00Z")));
        assert_eq!(record.recurrence_id, Some(rid));
        // The expanded window is still there: no re-expansion needed.
        assert!(store.instance_window(&"s".into()).unwrap().is_some());
        let expander = testing::TestExpander::default();
        let instances = store
            .list_event_instances(
                testing::at("2026-03-29T00:00:00Z"),
                testing::at("2026-03-30T00:00:00Z"),
                None,
                "Europe/Berlin",
                &expander,
            )
            .unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].start_utc, "2026-03-29T07:00:00Z");
        assert_eq!(expander.calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}
