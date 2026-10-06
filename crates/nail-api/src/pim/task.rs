//! Tasks (iCalendar `VTODO`): due dates, priorities, status, subtasks
//! (`RELATED-TO;RELTYPE=PARENT`) and recurrence. Task lists are calendars
//! with the `task` component ([`super::CalendarComponent::Task`]).
//!
//! Completing a recurring task ([`crate::Request::CompleteTask`])
//! completes the current occurrence and moves the task to the next one
//! (the completed occurrence is kept as an override), as Thunderbird and
//! Apple Reminders do.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::calendar::{Alarm, Recurrence, Visibility};
use super::source::PimItemSync;
use super::{
    MAX_LIST_ENTRIES, PimIssue, PimLink, PimTime, has_url_scheme, is_utc_instant, push_issue,
};
use crate::ids::{CalendarId, TaskId};

/// `STATUS` of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    /// Open (the default).
    #[default]
    NeedsAction,
    /// In progress.
    InProcess,
    /// Done.
    Completed,
    /// Cancelled.
    Cancelled,
}

impl TaskStatus {
    /// Whether the task is finished (completed or cancelled).
    pub fn is_closed(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// The editable fields of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskFields {
    /// `SUMMARY`.
    pub title: String,
    /// `DESCRIPTION`.
    #[serde(default)]
    pub description: Option<String>,
    /// `DTSTART`.
    #[serde(default)]
    pub start: Option<PimTime>,
    /// `DUE`.
    #[serde(default)]
    pub due: Option<PimTime>,
    /// `STATUS`.
    #[serde(default)]
    pub status: TaskStatus,
    /// `PERCENT-COMPLETE` (0–100).
    #[serde(default)]
    pub percent_complete: Option<u8>,
    /// `PRIORITY` (1 = highest … 9 = lowest), `null` = undefined.
    #[serde(default)]
    pub priority: Option<u8>,
    /// `COMPLETED` (RFC 3339 UTC).
    #[serde(default)]
    pub completed_at: Option<String>,
    /// `CATEGORIES`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// `LOCATION`.
    #[serde(default)]
    pub location: Option<String>,
    /// `URL`.
    #[serde(default)]
    pub url: Option<String>,
    /// `CLASS`.
    #[serde(default)]
    pub visibility: Visibility,
    /// `VALARM`s; relative ones count from the due time with
    /// [`AlarmRelated::End`](super::calendar::AlarmRelated::End) (see there).
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    /// The parent task (in the same list), `null` = top level.
    #[serde(default)]
    pub parent_id: Option<TaskId>,
    /// Manual position among its siblings (`X-APPLE-SORT-ORDER`), `null` =
    /// by due date.
    #[serde(default)]
    pub order: Option<i64>,
    /// Links to messages (create task from email).
    #[serde(default)]
    pub links: Vec<PimLink>,
}

impl TaskFields {
    /// An open task with a title.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            start: None,
            due: None,
            status: TaskStatus::NeedsAction,
            percent_complete: None,
            priority: None,
            completed_at: None,
            categories: Vec::new(),
            location: None,
            url: None,
            visibility: Visibility::Public,
            alarms: Vec::new(),
            parent_id: None,
            order: None,
            links: Vec::new(),
        }
    }

    /// Problems (empty = valid). Cycles through `parent_id` are checked
    /// by the core.
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        for time in self.start.iter().chain(&self.due) {
            if let Some(issue) = time.issue() {
                push_issue(&mut out, issue);
            }
        }
        if let (Some(start), Some(due)) = (&self.start, &self.due) {
            if start.is_date() != due.is_date() {
                push_issue(&mut out, PimIssue::MixedAllDay);
            } else if start.tz() == due.tz()
                && std::mem::discriminant(start) == std::mem::discriminant(due)
                && due.sort_key() < start.sort_key()
            {
                push_issue(&mut out, PimIssue::EndBeforeStart);
            }
        }
        if self.priority.is_some_and(|p| p > 9) || self.percent_complete.is_some_and(|p| p > 100) {
            push_issue(&mut out, PimIssue::InvalidPriority);
        }
        if self
            .completed_at
            .as_deref()
            .is_some_and(|c| !is_utc_instant(c))
        {
            push_issue(&mut out, PimIssue::InvalidTime);
        }
        if self
            .url
            .as_deref()
            .is_some_and(|u| !has_url_scheme(u, &["http", "https"]))
        {
            push_issue(&mut out, PimIssue::InvalidUrl);
        }
        for alarm in &self.alarms {
            for issue in alarm.issues() {
                push_issue(&mut out, issue);
            }
        }
        if [self.categories.len(), self.alarms.len(), self.links.len()]
            .into_iter()
            .any(|n| n > MAX_LIST_ENTRIES)
        {
            push_issue(&mut out, PimIssue::TooMany);
        }
        out
    }
}

/// A task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Task {
    /// The task.
    pub id: TaskId,
    /// Its task list.
    pub calendar_id: CalendarId,
    /// iCalendar `UID`.
    pub uid: String,
    /// The fields.
    pub fields: TaskFields,
    /// Recurrence, `null` = a single task.
    pub recurrence: Option<Recurrence>,
    /// Direct subtasks, in display order.
    pub subtask_ids: Vec<TaskId>,
    /// `CREATED` (RFC 3339 UTC), if known.
    pub created_at: Option<String>,
    /// `LAST-MODIFIED` (RFC 3339 UTC), if known.
    pub updated_at: Option<String>,
    /// Read-only (its list is).
    pub read_only: bool,
    /// Sync state.
    pub sync: PimItemSync,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_defaults_and_issues() {
        let fields: TaskFields = serde_json::from_value(json!({"title": "Pay rent"})).unwrap();
        assert_eq!(fields, TaskFields::titled("Pay rent"));
        assert_eq!(fields.issues().len(), 0);
        let mut f = fields;
        f.start = Some(PimTime::date("2026-10-05"));
        f.due = Some(PimTime::date("2026-10-01"));
        f.priority = Some(12);
        f.completed_at = Some("yesterday".into());
        assert_eq!(
            f.issues(),
            [
                PimIssue::EndBeforeStart,
                PimIssue::InvalidPriority,
                PimIssue::InvalidTime
            ]
        );
        f.due = Some(PimTime::utc("2026-10-06T10:00:00"));
        f.priority = None;
        f.completed_at = None;
        assert_eq!(f.issues(), [PimIssue::MixedAllDay]);
        assert_eq!(
            serde_json::to_value(TaskStatus::InProcess).unwrap(),
            json!("in-process")
        );
        assert!(TaskStatus::Cancelled.is_closed() && !TaskStatus::NeedsAction.is_closed());
    }
}
