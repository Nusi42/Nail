//! When alarms fire (RFC 5545 `VALARM`, RFC 9074 `ACKNOWLEDGED`), for
//! the reminder scheduler.
//!
//! * Relative triggers count from the occurrence's start, or its end with
//!   `RELATED=END`; absolute triggers fire once for the whole series.
//! * `REPEAT` / `DURATION` add further firings after the first.
//! * Firings at or before the alarm's `ACKNOWLEDGED` time were dismissed
//!   and are left out.
//! * Tasks: `RELATED=START` counts from `DTSTART`, `RELATED=END` from
//!   `DUE`; a task with only one of them uses that one (so "15 minutes
//!   before due" works however it was written). Closed tasks never fire.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use nail_api::pim::calendar::AlarmRelated;
use nail_api::pim::{Alarm, AlarmTrigger, CalendarEvent, PimTime, Task};

use crate::expand::{ExpandOptions, instances};
use crate::time;

/// Every time an alarm fires for one occurrence that starts at `start`
/// and ends at `end`, in order, acknowledged ones left out.
pub fn fire_times(alarm: &Alarm, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let first = match &alarm.trigger {
        AlarmTrigger::Relative {
            offset_secs,
            related,
        } => {
            let base = match related {
                AlarmRelated::Start => start,
                AlarmRelated::End => end,
            };
            base + Duration::seconds(*offset_secs)
        }
        AlarmTrigger::Absolute { at } => match time::parse_instant(at) {
            Some(at) => at,
            None => return Vec::new(),
        },
    };
    let mut out = vec![first];
    if let Some(repeat) = alarm.repeat {
        for n in 1..=i64::from(repeat.count.min(1000)) {
            out.push(first + Duration::seconds(i64::from(repeat.interval_secs) * n));
        }
    }
    let acknowledged = alarm.acknowledged.as_deref().and_then(time::parse_instant);
    out.retain(|t| acknowledged.is_none_or(|a| *t > a));
    out
}

/// One firing of an alarm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmFiring {
    /// The occurrence (`None` for single events and tasks).
    pub recurrence_id: Option<PimTime>,
    /// Index of the alarm in the occurrence's alarms.
    pub alarm_index: usize,
    /// The alarm's `UID`, if it has one.
    pub alarm_uid: Option<String>,
    /// When it fires.
    pub fire_at: DateTime<Utc>,
    /// When the occurrence starts (RFC 3339 UTC).
    pub occurrence_start: String,
}

fn max_reach(alarms: &[Alarm]) -> Duration {
    alarms
        .iter()
        .filter_map(|a| match a.trigger {
            AlarmTrigger::Relative { offset_secs, .. } => {
                let repeat = a.repeat.map_or(0, |r| {
                    i64::from(r.count.min(1000)) * i64::from(r.interval_secs)
                });
                Some(
                    offset_secs
                        .unsigned_abs()
                        .saturating_add(repeat.unsigned_abs()),
                )
            }
            AlarmTrigger::Absolute { .. } => None,
        })
        .max()
        .map_or_else(Duration::zero, |s| {
            Duration::seconds(i64::try_from(s).unwrap_or(i64::MAX / 2_000))
        })
}

/// The firings of an event's alarms in `[from, to)`, ordered by time.
/// Overrides use their own alarms.
pub fn event_firings(
    event: &CalendarEvent,
    this_and_future: &[PimTime],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    viewer: Tz,
) -> Vec<AlarmFiring> {
    let all_alarms: Vec<Alarm> = event
        .fields
        .alarms
        .iter()
        .chain(event.overrides.iter().flat_map(|o| o.fields.alarms.iter()))
        .cloned()
        .collect();
    if all_alarms.is_empty() {
        return Vec::new();
    }
    let reach = max_reach(&all_alarms) + Duration::days(1);
    let options = ExpandOptions {
        from: from - reach,
        to: to + reach,
        viewer,
        limit: 100_000,
        this_and_future,
        budget: None,
    };
    let mut out = Vec::new();
    let mut absolute_done: Vec<String> = Vec::new();
    for instance in instances(event, &options) {
        let fields = instance
            .recurrence_id
            .as_ref()
            .and_then(|rid| event.overrides.iter().find(|o| o.recurrence_id == *rid))
            .map_or(&event.fields, |o| &o.fields);
        let (Some(start), Some(end)) = (
            time::parse_instant(&instance.start_utc),
            time::parse_instant(&instance.end_utc),
        ) else {
            continue;
        };
        for (index, alarm) in fields.alarms.iter().enumerate() {
            if let AlarmTrigger::Absolute { at } = &alarm.trigger {
                if absolute_done.contains(at) {
                    continue;
                }
                absolute_done.push(at.clone());
            }
            for fire_at in fire_times(alarm, start, end) {
                if fire_at >= from && fire_at < to {
                    out.push(AlarmFiring {
                        recurrence_id: instance.recurrence_id.clone(),
                        alarm_index: index,
                        alarm_uid: alarm.uid.clone(),
                        fire_at,
                        occurrence_start: instance.start_utc.clone(),
                    });
                }
            }
        }
    }
    out.sort_by_key(|f| f.fire_at);
    out
}

/// The firings of a task's alarms in `[from, to)` (its current
/// occurrence; closed tasks none).
pub fn task_firings(
    task: &Task,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    viewer: Tz,
) -> Vec<AlarmFiring> {
    let fields = &task.fields;
    if fields.status.is_closed() {
        return Vec::new();
    }
    let start = fields.start.as_ref().and_then(|t| time::instant(t, viewer));
    let due = fields.due.as_ref().and_then(|t| time::instant(t, viewer));
    let mut out = Vec::new();
    for (index, alarm) in fields.alarms.iter().enumerate() {
        let (from_start, from_due) = match alarm.trigger {
            AlarmTrigger::Relative {
                related: AlarmRelated::Start,
                ..
            } => (start.or(due), start.or(due)),
            _ => (due.or(start), due.or(start)),
        };
        let (Some(a), Some(b)) = (from_start, from_due) else {
            if matches!(alarm.trigger, AlarmTrigger::Absolute { .. }) {
                let now = from;
                out.extend(
                    fire_times(alarm, now, now)
                        .into_iter()
                        .filter(|t| *t >= from && *t < to)
                        .map(|fire_at| AlarmFiring {
                            recurrence_id: None,
                            alarm_index: index,
                            alarm_uid: alarm.uid.clone(),
                            fire_at,
                            occurrence_start: String::new(),
                        }),
                );
            }
            continue;
        };
        // For tasks both "start" and "end" of fire_times mean the anchor.
        for fire_at in fire_times(alarm, a, b) {
            if fire_at >= from && fire_at < to {
                out.push(AlarmFiring {
                    recurrence_id: None,
                    alarm_index: index,
                    alarm_uid: alarm.uid.clone(),
                    fire_at,
                    occurrence_start: time::format_instant(a),
                });
            }
        }
    }
    out.sort_by_key(|f| f.fire_at);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::calendar::AlarmRepeat;
    use nail_api::pim::source::PimItemSync;
    use nail_api::pim::{EventFields, Recurrence, TaskFields, TaskStatus};
    use pretty_assertions::assert_eq;

    fn utc(text: &str) -> DateTime<Utc> {
        time::parse_instant(text).unwrap()
    }

    fn event(alarms: Vec<Alarm>, rule: Option<&str>) -> CalendarEvent {
        let start = PimTime::zoned("2026-10-23T09:00:00", "Europe/Berlin");
        let mut fields = EventFields::new(
            "Standup",
            start.clone(),
            PimTime::zoned("2026-10-23T10:00:00", "Europe/Berlin"),
        );
        fields.alarms = alarms;
        CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            recurrence: rule
                .map(|r| Recurrence::rule(crate::recur::parse_rrule(r, &start).unwrap())),
            fields,
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    #[test]
    fn relative_absolute_and_repeating() {
        let start = utc("2026-10-23T07:00:00Z");
        let end = utc("2026-10-23T08:00:00Z");
        assert_eq!(
            fire_times(&Alarm::minutes_before(15), start, end),
            [utc("2026-10-23T06:45:00Z")]
        );
        let mut after_end = Alarm::minutes_before(0);
        after_end.trigger = AlarmTrigger::Relative {
            offset_secs: 300,
            related: AlarmRelated::End,
        };
        after_end.repeat = Some(AlarmRepeat {
            count: 2,
            interval_secs: 600,
        });
        assert_eq!(
            fire_times(&after_end, start, end),
            [
                utc("2026-10-23T08:05:00Z"),
                utc("2026-10-23T08:15:00Z"),
                utc("2026-10-23T08:25:00Z")
            ]
        );
        after_end.acknowledged = Some("2026-10-23T08:10:00Z".into());
        assert_eq!(fire_times(&after_end, start, end).len(), 2);
        let mut absolute = Alarm::minutes_before(0);
        absolute.trigger = AlarmTrigger::Absolute {
            at: "2026-10-22T18:00:00Z".into(),
        };
        assert_eq!(
            fire_times(&absolute, start, end),
            [utc("2026-10-22T18:00:00Z")]
        );
        absolute.trigger = AlarmTrigger::Absolute {
            at: "garbage".into(),
        };
        assert!(fire_times(&absolute, start, end).is_empty());
    }

    #[test]
    fn recurring_events_fire_per_occurrence_across_dst() {
        let mut absolute = Alarm::minutes_before(0);
        absolute.trigger = AlarmTrigger::Absolute {
            at: "2026-10-22T18:00:00Z".into(),
        };
        let e = event(
            vec![Alarm::minutes_before(10), absolute],
            Some("FREQ=DAILY;COUNT=4"),
        );
        let firings = event_firings(
            &e,
            &[],
            utc("2026-10-22T00:00:00Z"),
            utc("2026-10-26T00:00:00Z"),
            Tz::UTC,
        );
        let times: Vec<String> = firings
            .iter()
            .map(|f| time::format_instant(f.fire_at))
            .collect();
        assert_eq!(
            times,
            [
                "2026-10-22T18:00:00Z",
                "2026-10-23T06:50:00Z",
                "2026-10-24T06:50:00Z",
                "2026-10-25T07:50:00Z"
            ]
        );
        assert_eq!(
            firings[3].recurrence_id,
            Some(PimTime::zoned("2026-10-25T09:00:00", "Europe/Berlin"))
        );
        assert_eq!(firings[1].alarm_index, 0);
        // Only what falls into the window.
        let late = event_firings(
            &e,
            &[],
            utc("2026-10-26T07:00:00Z"),
            utc("2026-10-27T00:00:00Z"),
            Tz::UTC,
        );
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].occurrence_start, "2026-10-26T08:00:00Z");
        assert!(
            event_firings(
                &event(vec![], None),
                &[],
                utc("2026-01-01T00:00:00Z"),
                utc("2027-01-01T00:00:00Z"),
                Tz::UTC
            )
            .is_empty()
        );
    }

    #[test]
    fn task_alarms_use_due_or_start() {
        let mut fields = TaskFields::titled("File taxes");
        fields.due = Some(PimTime::utc("2026-10-31T12:00:00"));
        fields.alarms = vec![Alarm::minutes_before(60)];
        let mut task = Task {
            id: "t".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields,
            recurrence: None,
            subtask_ids: vec![],
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        };
        let window = (utc("2026-10-01T00:00:00Z"), utc("2026-11-01T00:00:00Z"));
        let f = task_firings(&task, window.0, window.1, Tz::UTC);
        assert_eq!(f[0].fire_at, utc("2026-10-31T11:00:00Z"));
        task.fields.start = Some(PimTime::utc("2026-10-20T08:00:00"));
        task.fields.alarms.push(Alarm {
            trigger: AlarmTrigger::Relative {
                offset_secs: -1800,
                related: AlarmRelated::End,
            },
            ..Alarm::minutes_before(0)
        });
        let f = task_firings(&task, window.0, window.1, Tz::UTC);
        let times: Vec<_> = f.iter().map(|f| time::format_instant(f.fire_at)).collect();
        assert_eq!(times, ["2026-10-20T07:00:00Z", "2026-10-31T11:30:00Z"]);
        task.fields.status = TaskStatus::Completed;
        assert!(task_firings(&task, window.0, window.1, Tz::UTC).is_empty());
        let mut undated = TaskFields::titled("Call");
        undated.alarms = vec![Alarm {
            trigger: AlarmTrigger::Absolute {
                at: "2026-10-05T10:00:00Z".into(),
            },
            ..Alarm::minutes_before(0)
        }];
        task.fields = undated;
        assert_eq!(task_firings(&task, window.0, window.1, Tz::UTC).len(), 1);
    }
}
