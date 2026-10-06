//! Microsoft To Do / EWS tasks ↔ Nail tasks (ADR 0072). To Do lists are
//! task lists of the account's `exchange:<account>` PIM source; a To Do
//! task's checklist items are Nail **subtasks** (one level: To Do
//! checklists are flat), its reminder an absolute alarm, its linked
//! resources (e.g. the flagged message) links / the task's URL.
//!
//! To Do keeps due and start dates as midnight in a zone; those become
//! all-day dates in Nail (and back).

use nail_api::exchange::pim::{
    ChecklistItem, ExchangeDateTime, ExchangeTask, ExchangeTaskStatus, LinkedResource,
};
use nail_api::localized::LocalizedMessage;
use nail_api::pim::{Alarm, AlarmTrigger, PimTime, Recurrence, Task, TaskFields, TaskStatus};

use super::events::{NotExpressible, pattern_of, rrule_of};
use super::text::{body_text, exchange_body};
use super::time::{exchange_time, exchange_utc, pim_time, utc_text};
use super::{derived_uid, importance_of, priority_of};

/// A checklist item as a subtask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedSubtask {
    /// The checklist item's id (kept by the core to write it back).
    pub checklist_id: String,
    /// A stable `UID`.
    pub uid: String,
    /// Its fields (`parent_id` is set by the core).
    pub fields: TaskFields,
}

/// An Exchange task in Nail's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedTask {
    /// A stable `UID` (derived from the server id).
    pub uid: String,
    /// The fields.
    pub fields: TaskFields,
    /// Recurrence (anchored on the due date).
    pub recurrence: Option<Recurrence>,
    /// Checklist items, in order.
    pub subtasks: Vec<MappedSubtask>,
    /// The links the server keeps (written back unchanged).
    pub linked_resources: Vec<LinkedResource>,
    /// `CREATED`, `LAST-MODIFIED`.
    pub created_at: Option<String>,
    /// `LAST-MODIFIED`.
    pub updated_at: Option<String>,
    /// What did not map exactly.
    pub notes: Vec<LocalizedMessage>,
}

/// A task date: midnight → an all-day date; any other time → that time.
fn task_time(time: &ExchangeDateTime, notes: &mut Vec<LocalizedMessage>) -> PimTime {
    let midnight = time.date_time.get(11..19).is_none_or(|t| t == "00:00:00");
    let (value, note) = pim_time(time, midnight);
    notes.extend(note);
    value
}

fn status_of(status: ExchangeTaskStatus) -> TaskStatus {
    match status {
        ExchangeTaskStatus::NotStarted | ExchangeTaskStatus::Deferred => TaskStatus::NeedsAction,
        ExchangeTaskStatus::InProgress | ExchangeTaskStatus::WaitingOnOthers => {
            TaskStatus::InProcess
        }
        ExchangeTaskStatus::Completed => TaskStatus::Completed,
    }
}

/// An Exchange task in Nail's shape.
pub fn task_from_exchange(t: &ExchangeTask) -> MappedTask {
    let mut notes = Vec::new();
    let uid = derived_uid("task", &t.remote_id);
    let mut fields = TaskFields::titled(t.title.clone());
    fields.description = body_text(t.body.as_ref());
    fields.status = status_of(t.status);
    fields.percent_complete = (fields.status == TaskStatus::Completed).then_some(100);
    // To Do's default importance is "normal": shown as no priority.
    fields.priority = priority_of(t.importance).filter(|p| *p != 5);
    fields.start = t.start.as_ref().map(|s| task_time(s, &mut notes));
    fields.due = t.due.as_ref().map(|d| task_time(d, &mut notes));
    fields.completed_at = t.completed.as_ref().and_then(utc_text);
    fields.categories.clone_from(&t.categories);
    fields.url = t.linked_resources.iter().find_map(|l| l.web_url.clone());
    fields.alarms = t
        .reminder
        .as_ref()
        .and_then(utc_text)
        .map(|at| Alarm {
            trigger: AlarmTrigger::Absolute { at },
            ..Alarm::minutes_before(0)
        })
        .into_iter()
        .collect();
    let anchor = fields
        .due
        .clone()
        .or_else(|| fields.start.clone())
        .unwrap_or_else(|| {
            PimTime::date(
                t.created
                    .as_deref()
                    .and_then(|c| c.get(..10))
                    .unwrap_or("1970-01-01"),
            )
        });
    let recurrence = t
        .recurrence
        .as_ref()
        .map(|r| Recurrence::rule(rrule_of(r, &anchor)));
    let subtasks = t
        .checklist
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut sub = TaskFields::titled(item.title.clone());
            if item.checked {
                sub.status = TaskStatus::Completed;
                sub.percent_complete = Some(100);
            }
            sub.order = i64::try_from(i).ok();
            MappedSubtask {
                checklist_id: item.id.clone(),
                uid: derived_uid("checklist", &format!("{}/{}", t.remote_id, item.id)),
                fields: sub,
            }
        })
        .collect();
    MappedTask {
        uid,
        fields,
        recurrence,
        subtasks,
        linked_resources: t.linked_resources.clone(),
        created_at: t.created.clone(),
        updated_at: t.last_modified.clone(),
        notes,
    }
}

/// What [`task_to_exchange`] needs besides the task.
#[derive(Debug, Clone, Copy)]
pub struct TaskWriteContext<'a> {
    /// The task's server id and version (`None` for a new task).
    pub remote: Option<(&'a str, Option<&'a str>)>,
    /// The mailbox's zone (dates are midnight there).
    pub default_zone: &'a str,
    /// The links the server had (kept).
    pub linked_resources: &'a [LinkedResource],
}

/// A Nail task with its subtasks (`(checklist id, subtask)`, `None` for new
/// ones; deeper levels are flattened by the caller) for Exchange.
pub fn task_to_exchange(
    task: &Task,
    subtasks: &[(Option<String>, &Task)],
    ctx: &TaskWriteContext<'_>,
) -> Result<(ExchangeTask, Vec<LocalizedMessage>), NotExpressible> {
    let mut notes = Vec::new();
    let f = &task.fields;
    let mut time = |t: &Option<PimTime>| {
        t.as_ref().map(|t| {
            let (value, note) = exchange_time(t, ctx.default_zone);
            notes.extend(note);
            value
        })
    };
    let start = time(&f.start);
    let due = time(&f.due);
    let recurrence = match task.recurrence.as_ref().filter(|r| !r.is_empty()) {
        None => None,
        Some(r) if r.rules.len() == 1 && r.dates.is_empty() => {
            let anchor = f.due.clone().or_else(|| f.start.clone()).ok_or_else(|| {
                NotExpressible(LocalizedMessage::new(crate::texts::PIM_RECURRENCE))
            })?;
            Some(pattern_of(&r.rules[0], &anchor)?)
        }
        Some(_) => {
            return Err(NotExpressible(LocalizedMessage::new(
                crate::texts::PIM_RDATE,
            )));
        }
    };
    let reminder = f.alarms.iter().find_map(|a| match &a.trigger {
        AlarmTrigger::Absolute { at } => exchange_utc(at),
        AlarmTrigger::Relative { .. } => None,
    });
    let task_out = ExchangeTask {
        remote_id: ctx.remote.map(|(id, _)| id.to_owned()).unwrap_or_default(),
        change_key: ctx.remote.and_then(|(_, k)| k.map(str::to_owned)),
        title: f.title.clone(),
        body: exchange_body(f.description.as_deref()),
        status: match f.status {
            TaskStatus::NeedsAction | TaskStatus::Cancelled => ExchangeTaskStatus::NotStarted,
            TaskStatus::InProcess => ExchangeTaskStatus::InProgress,
            TaskStatus::Completed => ExchangeTaskStatus::Completed,
        },
        importance: importance_of(f.priority)
            .or(Some(nail_api::exchange::rules::Importance::Normal)),
        start,
        due,
        completed: f.completed_at.as_deref().and_then(exchange_utc),
        reminder,
        recurrence,
        categories: f.categories.clone(),
        checklist: subtasks
            .iter()
            .map(|(id, sub)| ChecklistItem {
                id: id.clone().unwrap_or_default(),
                title: sub.fields.title.clone(),
                checked: sub.fields.status == TaskStatus::Completed,
            })
            .collect(),
        linked_resources: ctx.linked_resources.to_vec(),
        created: None,
        last_modified: None,
    };
    Ok((task_out, notes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::exchange::rules::Importance;
    use nail_api::pim::PimItemSync;
    use pretty_assertions::assert_eq;

    fn todo() -> ExchangeTask {
        ExchangeTask {
            remote_id: "AAkALgAA".into(),
            change_key: Some("ck1".into()),
            title: "Quarterly report".into(),
            body: Some(nail_api::exchange::pim::ExchangeBody {
                html: false,
                content: "Numbers from finance".into(),
            }),
            status: ExchangeTaskStatus::InProgress,
            importance: Some(Importance::High),
            start: None,
            due: Some(ExchangeDateTime {
                date_time: "2026-10-09T00:00:00.0000000".into(),
                time_zone: "UTC".into(),
            }),
            completed: None,
            reminder: Some(ExchangeDateTime {
                date_time: "2026-10-08T09:00:00".into(),
                time_zone: "W. Europe Standard Time".into(),
            }),
            recurrence: None,
            categories: vec!["Work".into()],
            checklist: vec![
                ChecklistItem {
                    id: "c1".into(),
                    title: "Draft".into(),
                    checked: true,
                },
                ChecklistItem {
                    id: "c2".into(),
                    title: "Review".into(),
                    checked: false,
                },
            ],
            linked_resources: vec![LinkedResource {
                id: Some("l1".into()),
                web_url: Some("https://outlook.office.com/mail/id/AAMk".into()),
                application: Some("Outlook".into()),
                display_name: Some("Flagged message".into()),
                external_id: Some("AAMk".into()),
            }],
            created: Some("2026-09-30T10:00:00Z".into()),
            last_modified: Some("2026-10-01T10:00:00Z".into()),
        }
    }

    fn as_task(fields: TaskFields, uid: &str, recurrence: Option<Recurrence>) -> Task {
        Task {
            id: "t1".into(),
            calendar_id: "list".into(),
            uid: uid.into(),
            fields,
            recurrence,
            subtask_ids: Vec::new(),
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    #[test]
    fn todo_tasks_map_to_nail() {
        let mapped = task_from_exchange(&todo());
        assert!(mapped.notes.is_empty(), "{:?}", mapped.notes);
        let f = &mapped.fields;
        assert_eq!(f.status, TaskStatus::InProcess);
        assert_eq!(f.priority, Some(1));
        assert_eq!(f.due, Some(PimTime::date("2026-10-09")));
        assert_eq!(f.description.as_deref(), Some("Numbers from finance"));
        assert_eq!(
            f.url.as_deref(),
            Some("https://outlook.office.com/mail/id/AAMk")
        );
        assert_eq!(
            f.alarms[0].trigger,
            AlarmTrigger::Absolute {
                at: "2026-10-08T07:00:00Z".into()
            }
        );
        assert_eq!(mapped.subtasks.len(), 2);
        assert_eq!(mapped.subtasks[0].fields.status, TaskStatus::Completed);
        assert_eq!(mapped.subtasks[1].fields.order, Some(1));
        assert_ne!(mapped.subtasks[0].uid, mapped.subtasks[1].uid);
        assert_eq!(mapped.uid, task_from_exchange(&todo()).uid, "stable");
    }

    #[test]
    fn tasks_round_trip() {
        let source = todo();
        let mapped = task_from_exchange(&source);
        let task = as_task(mapped.fields.clone(), &mapped.uid, None);
        let subs: Vec<Task> = mapped
            .subtasks
            .iter()
            .map(|s| as_task(s.fields.clone(), &s.uid, None))
            .collect();
        let pairs: Vec<(Option<String>, &Task)> = mapped
            .subtasks
            .iter()
            .zip(&subs)
            .map(|(m, t)| (Some(m.checklist_id.clone()), t))
            .collect();
        let ctx = TaskWriteContext {
            remote: Some(("AAkALgAA", Some("ck1"))),
            default_zone: "UTC",
            linked_resources: &mapped.linked_resources,
        };
        let (back, notes) = task_to_exchange(&task, &pairs, &ctx).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(back.title, source.title);
        assert_eq!(back.status, source.status);
        assert_eq!(back.importance, source.importance);
        assert_eq!(back.checklist, source.checklist);
        assert_eq!(back.linked_resources, source.linked_resources);
        assert_eq!(back.categories, source.categories);
        assert_eq!(
            back.due,
            Some(ExchangeDateTime {
                date_time: "2026-10-09T00:00:00".into(),
                time_zone: "UTC".into()
            })
        );
        assert_eq!(
            back.reminder,
            Some(ExchangeDateTime::utc("2026-10-08T07:00:00"))
        );
    }

    #[test]
    fn recurring_and_completed_tasks() {
        let mut t = todo();
        t.importance = Some(Importance::Normal);
        t.status = ExchangeTaskStatus::Completed;
        t.completed = Some(ExchangeDateTime::utc("2026-10-02T08:00:00"));
        t.recurrence = Some(nail_api::exchange::pim::ExchangeRecurrence {
            pattern: nail_api::exchange::pim::ExchangeRecurrencePattern {
                kind: nail_api::exchange::pim::PatternKind::Weekly,
                interval: 1,
                days_of_week: vec![nail_api::exchange::pim::ExchangeDayOfWeek::Friday],
                day_of_month: None,
                month: None,
                index: None,
                first_day_of_week: Some(nail_api::exchange::pim::ExchangeDayOfWeek::Monday),
            },
            range: nail_api::exchange::pim::ExchangeRecurrenceRange::NoEnd {
                start_date: "2026-10-09".into(),
            },
            time_zone: None,
        });
        let mapped = task_from_exchange(&t);
        assert_eq!(mapped.fields.priority, None, "normal = no priority");
        assert_eq!(mapped.fields.percent_complete, Some(100));
        assert_eq!(
            mapped.fields.completed_at.as_deref(),
            Some("2026-10-02T08:00:00Z")
        );
        let task = as_task(
            mapped.fields.clone(),
            &mapped.uid,
            mapped.recurrence.clone(),
        );
        let ctx = TaskWriteContext {
            remote: None,
            default_zone: "UTC",
            linked_resources: &[],
        };
        let (back, _) = task_to_exchange(&task, &[], &ctx).unwrap();
        assert_eq!(
            back.recurrence.unwrap().pattern,
            t.recurrence.unwrap().pattern
        );
        assert_eq!(back.importance, Some(Importance::Normal));
        assert_eq!(
            back.completed,
            Some(ExchangeDateTime::utc("2026-10-02T08:00:00"))
        );
        // A timed due stays a time.
        let mut timed = todo();
        timed.due = Some(ExchangeDateTime::utc("2026-10-09T17:00:00"));
        assert_eq!(
            task_from_exchange(&timed).fields.due,
            Some(PimTime::utc("2026-10-09T17:00:00"))
        );
        // A recurring task without dates cannot be written.
        let mut undated = as_task(TaskFields::titled("x"), "u", mapped.recurrence);
        undated.fields.due = None;
        assert!(task_to_exchange(&undated, &[], &ctx).is_err());
    }
}
