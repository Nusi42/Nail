//! Reminders: alarms of event occurrences and tasks that are due
//! (design.md, "Reminders").
//!
//! The core's reminder scheduler (`nail-triage`, using the same injectable
//! clock as snooze) computes when each alarm fires across time zones and
//! restarts. When one fires it pushes [`crate::Event::ReminderDue`] and a
//! notification ([`crate::triage::NotificationKind::Reminder`]) through
//! the notification path (OS notification on the desktop, Web
//! Notification in the remote UI). Snoozing and dismissing write the
//! alarm's `ACKNOWLEDGED` / a snooze alarm (RFC 9074) through the PIM
//! queue, so other devices stop showing it where the server keeps them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::PimTime;
use crate::ids::{CalendarId, EventId, ReminderId, TaskId};

/// What a reminder is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ReminderTarget {
    /// An event occurrence.
    Event {
        /// The event (series).
        event_id: EventId,
        /// The occurrence, `null` for a single event.
        recurrence_id: Option<PimTime>,
    },
    /// A task.
    Task {
        /// The task.
        task_id: TaskId,
    },
}

/// State of a reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ReminderState {
    /// Not due yet.
    Pending,
    /// Fired and waiting for the user.
    Fired,
    /// Snoozed until [`Reminder::snoozed_until`].
    Snoozed,
    /// Dismissed.
    Dismissed,
}

/// A reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Reminder {
    /// The reminder.
    pub id: ReminderId,
    /// What it is about.
    pub target: ReminderTarget,
    /// The calendar / task list.
    pub calendar_id: CalendarId,
    /// The item's title.
    pub title: String,
    /// Its location, if any.
    pub location: Option<String>,
    /// The alarm's own text, if it has one.
    pub description: Option<String>,
    /// When the event starts / the task is due (RFC 3339 UTC), if known.
    pub starts_at: Option<String>,
    /// Whether that is an all-day date.
    pub all_day: bool,
    /// When the alarm fires / fired (RFC 3339 UTC).
    pub fire_at: String,
    /// State.
    pub state: ReminderState,
    /// Until when it is snoozed (RFC 3339 UTC).
    pub snoozed_until: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reminders_serialize() {
        let reminder = Reminder {
            id: "r1".into(),
            target: ReminderTarget::Event {
                event_id: "ev1".into(),
                recurrence_id: Some(PimTime::utc("2026-10-01T09:00:00")),
            },
            calendar_id: "c1".into(),
            title: "Standup".into(),
            location: None,
            description: None,
            starts_at: Some("2026-10-01T09:00:00Z".into()),
            all_day: false,
            fire_at: "2026-10-01T08:45:00Z".into(),
            state: ReminderState::Fired,
            snoozed_until: None,
        };
        let value = serde_json::to_value(&reminder).unwrap();
        assert_eq!(value["target"]["type"], "event");
        assert_eq!(value["state"], "fired");
        assert_eq!(serde_json::from_value::<Reminder>(value).unwrap(), reminder);
        assert_eq!(
            serde_json::to_value(ReminderTarget::Task {
                task_id: "t1".into()
            })
            .unwrap(),
            json!({"type": "task", "task_id": "t1"})
        );
    }
}
