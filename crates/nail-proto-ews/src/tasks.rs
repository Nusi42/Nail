//! Tasks (`t:Task` of the Tasks folder) ↔ [`ExchangeTask`]. EWS tasks
//! have no To Do checklist items or linked resources (those stay empty);
//! times are UTC like calendar items.

use nail_api::exchange::pim::{ExchangeDateTime, ExchangeTask, ExchangeTaskStatus};

use crate::calendar::{body, body_xml, importance, importance_name};
use crate::error::EwsError;
use crate::mail::categories_xml;
use crate::recurrence::{parse_recurrence, recurrence_xml};
use crate::types::{Field, ItemRef, Update, format_utc, normalize_utc, parse_time};
use crate::xml::{Element, el};

/// The task properties Nail reads with `GetItem`.
pub const TASK_FIELDS: [&str; 14] = [
    "item:Subject",
    "item:Body",
    "item:Categories",
    "item:Importance",
    "item:ReminderDueBy",
    "item:ReminderIsSet",
    "item:DateTimeCreated",
    "item:LastModifiedTime",
    "task:CompleteDate",
    "task:DueDate",
    "task:Recurrence",
    "task:StartDate",
    "task:Status",
    "task:IsComplete",
];

fn status(text: &str) -> ExchangeTaskStatus {
    match text {
        "InProgress" => ExchangeTaskStatus::InProgress,
        "Completed" => ExchangeTaskStatus::Completed,
        "WaitingOnOthers" => ExchangeTaskStatus::WaitingOnOthers,
        "Deferred" => ExchangeTaskStatus::Deferred,
        _ => ExchangeTaskStatus::NotStarted,
    }
}

fn status_name(status: ExchangeTaskStatus) -> &'static str {
    match status {
        ExchangeTaskStatus::NotStarted => "NotStarted",
        ExchangeTaskStatus::InProgress => "InProgress",
        ExchangeTaskStatus::Completed => "Completed",
        ExchangeTaskStatus::WaitingOnOthers => "WaitingOnOthers",
        ExchangeTaskStatus::Deferred => "Deferred",
    }
}

fn utc_time(item: &Element, name: &str) -> Option<ExchangeDateTime> {
    let time = parse_time(item.child_text(name)?)?;
    Some(ExchangeDateTime::utc(
        time.format("%Y-%m-%dT%H:%M:%S").to_string(),
    ))
}

/// A task element as an [`ExchangeTask`].
pub fn parse_task(item: &Element) -> Option<ExchangeTask> {
    let id = item.child("ItemId").and_then(ItemRef::from_element)?;
    let mut status = status(item.child_text("Status").unwrap_or(""));
    if item.child_bool("IsComplete") == Some(true) {
        status = ExchangeTaskStatus::Completed;
    }
    Some(ExchangeTask {
        remote_id: id.id,
        change_key: id.change_key,
        title: item.child_text("Subject").unwrap_or_default().to_owned(),
        body: body(item).filter(|b| !b.content.trim().is_empty()),
        status,
        importance: item.child_text("Importance").and_then(importance),
        start: utc_time(item, "StartDate"),
        due: utc_time(item, "DueDate"),
        completed: utc_time(item, "CompleteDate"),
        reminder: item
            .child_bool("ReminderIsSet")
            .filter(|set| *set)
            .and_then(|_| utc_time(item, "ReminderDueBy")),
        recurrence: item
            .child("Recurrence")
            .and_then(|r| parse_recurrence(r, None)),
        categories: item.strings("Categories", "String"),
        checklist: Vec::new(),
        linked_resources: Vec::new(),
        created: item.child_text("DateTimeCreated").and_then(normalize_utc),
        last_modified: item.child_text("LastModifiedTime").and_then(normalize_utc),
    })
}

fn is_utc(zone: &str) -> bool {
    zone.eq_ignore_ascii_case("UTC") || zone.eq_ignore_ascii_case("Etc/UTC")
}

/// The zone a task write runs in (`TimeZoneContext`): `None` when every
/// time is UTC, else the one Windows zone all non-UTC times share.
pub fn task_zone(task: &ExchangeTask) -> Result<Option<String>, EwsError> {
    let mut zone: Option<&str> = None;
    for time in [&task.start, &task.due, &task.completed, &task.reminder]
        .into_iter()
        .flatten()
    {
        if is_utc(&time.time_zone) {
            continue;
        }
        if time.time_zone.contains('/') {
            return Err(EwsError::Unsupported(format!(
                "EWS needs Windows time zone names, not {}",
                time.time_zone
            )));
        }
        match zone {
            Some(z) if z != time.time_zone => {
                return Err(EwsError::Unsupported(
                    "task times in different time zones".to_owned(),
                ));
            }
            _ => zone = Some(&time.time_zone),
        }
    }
    Ok(zone.map(str::to_owned))
}

/// A task time: `…Z` for UTC, else the wall time (read in the write's
/// `TimeZoneContext`, see [`task_zone`]).
fn time_value(time: &ExchangeDateTime) -> Result<String, EwsError> {
    let invalid = || EwsError::Rejected {
        code: "InvalidTime".into(),
        message: time.date_time.clone(),
    };
    let parsed = parse_time(&time.date_time).ok_or_else(invalid)?;
    Ok(if is_utc(&time.time_zone) {
        format_utc(parsed)
    } else {
        parsed.format("%Y-%m-%dT%H:%M:%S").to_string()
    })
}

/// The fields Nail writes of a task, in `Task` schema order.
pub fn task_fields(task: &ExchangeTask) -> Result<Vec<(Field, Option<String>)>, EwsError> {
    task_zone(task)?;
    let time = |t: &Option<ExchangeDateTime>, tag: &str| -> Result<Option<String>, EwsError> {
        t.as_ref().map(|t| Ok(el(tag, &time_value(t)?))).transpose()
    };
    Ok(vec![
        (
            Field::Uri("item:Subject"),
            Some(el("t:Subject", &task.title)),
        ),
        (Field::Uri("item:Body"), task.body.as_ref().map(body_xml)),
        (
            Field::Uri("item:Categories"),
            (!task.categories.is_empty()).then(|| categories_xml(&task.categories)),
        ),
        (
            Field::Uri("item:Importance"),
            task.importance
                .map(|i| el("t:Importance", importance_name(i))),
        ),
        (
            Field::Uri("item:ReminderDueBy"),
            time(&task.reminder, "t:ReminderDueBy")?,
        ),
        (
            Field::Uri("item:ReminderIsSet"),
            Some(el(
                "t:ReminderIsSet",
                if task.reminder.is_some() {
                    "true"
                } else {
                    "false"
                },
            )),
        ),
        (
            Field::Uri("task:CompleteDate"),
            if task.status == ExchangeTaskStatus::Completed {
                time(&task.completed, "t:CompleteDate")?
            } else {
                None
            },
        ),
        (Field::Uri("task:DueDate"), time(&task.due, "t:DueDate")?),
        (
            Field::Uri("task:Recurrence"),
            task.recurrence.as_ref().map(recurrence_xml).transpose()?,
        ),
        (
            Field::Uri("task:StartDate"),
            time(&task.start, "t:StartDate")?,
        ),
        (
            Field::Uri("task:Status"),
            Some(el("t:Status", status_name(task.status))),
        ),
    ])
}

/// The `<t:Task>` of a new task.
pub fn task_create_xml(task: &ExchangeTask) -> Result<String, EwsError> {
    let fields: String = task_fields(task)?
        .into_iter()
        .filter_map(|(_, v)| v)
        .collect();
    Ok(format!("<t:Task>{fields}</t:Task>"))
}

/// The updates that replace a task's fields.
pub fn task_updates(task: &ExchangeTask) -> Result<Vec<Update>, EwsError> {
    Ok(task_fields(task)?
        .into_iter()
        .filter(|(field, value)| {
            // `CompleteDate` is set by the server from `Status`.
            value.is_some() || *field != Field::Uri("task:CompleteDate")
        })
        .map(|(field, value)| match value {
            Some(v) => Update::Set(field, v),
            None => Update::Delete(field),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use nail_api::exchange::rules::Importance;
    use pretty_assertions::assert_eq;

    #[test]
    fn tasks_round_trip() {
        let item = parse(concat!(
            r#"<Task><ItemId Id="T1" ChangeKey="EwAA"/><Subject>Report</Subject>"#,
            r#"<Body BodyType="Text">Write it</Body><Categories><String>Work</String></Categories>"#,
            r#"<Importance>High</Importance><ReminderDueBy>2026-10-08T07:00:00Z</ReminderDueBy>"#,
            r#"<ReminderIsSet>true</ReminderIsSet><DateTimeCreated>2026-09-01T10:00:00Z</DateTimeCreated>"#,
            r#"<DueDate>2026-10-09T22:00:00Z</DueDate><StartDate>2026-10-01T22:00:00Z</StartDate>"#,
            r#"<Status>InProgress</Status><IsComplete>false</IsComplete></Task>"#
        ))
        .unwrap();
        let task = parse_task(&item).unwrap();
        assert_eq!(task.remote_id, "T1");
        assert_eq!(task.title, "Report");
        assert_eq!(task.status, ExchangeTaskStatus::InProgress);
        assert_eq!(task.importance, Some(Importance::High));
        assert_eq!(task.due, Some(ExchangeDateTime::utc("2026-10-09T22:00:00")));
        assert_eq!(
            task.reminder,
            Some(ExchangeDateTime::utc("2026-10-08T07:00:00"))
        );
        assert_eq!(task.created.as_deref(), Some("2026-09-01T10:00:00Z"));
        assert!(task.checklist.is_empty());
        let xml = task_create_xml(&task).unwrap();
        assert!(xml.contains("<t:DueDate>2026-10-09T22:00:00Z</t:DueDate>"));
        assert!(xml.contains("<t:Status>InProgress</t:Status>"));
        assert!(xml.find("<t:DueDate>").unwrap() < xml.find("<t:StartDate>").unwrap());
        let wrapped = parse(&format!(r#"<x xmlns:t="t">{xml}</x>"#)).unwrap();
        let mut back = wrapped.child("Task").unwrap().clone();
        back.children
            .insert(0, parse(r#"<ItemId Id="T1" ChangeKey="EwAA"/>"#).unwrap());
        let reparsed = parse_task(&back).unwrap();
        assert_eq!(reparsed.due, task.due);
        assert_eq!(reparsed.categories, task.categories);
        let updates = task_updates(&task).unwrap();
        assert!(updates.contains(&Update::Delete(Field::Uri("task:Recurrence"))));
        assert!(!updates.contains(&Update::Delete(Field::Uri("task:CompleteDate"))));
        let done = parse(r#"<Task><ItemId Id="T2"/><Status>NotStarted</Status><IsComplete>true</IsComplete></Task>"#)
            .unwrap();
        assert_eq!(
            parse_task(&done).unwrap().status,
            ExchangeTaskStatus::Completed
        );
        let mut zoned = task;
        zoned.due = Some(ExchangeDateTime {
            date_time: "2026-10-09T00:00:00".into(),
            time_zone: "W. Europe Standard Time".into(),
        });
        assert_eq!(
            task_zone(&zoned).unwrap().as_deref(),
            Some("W. Europe Standard Time")
        );
        assert!(
            task_create_xml(&zoned)
                .unwrap()
                .contains("<t:DueDate>2026-10-09T00:00:00</t:DueDate>")
        );
        zoned.start = Some(ExchangeDateTime {
            date_time: "2026-10-01T00:00:00".into(),
            time_zone: "Europe/Berlin".into(),
        });
        assert!(matches!(
            task_create_xml(&zoned),
            Err(EwsError::Unsupported(_))
        ));
    }
}
