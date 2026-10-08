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

impl ReminderState {
    /// When the scheduler has to act on a reminder in this state: the
    /// snooze end (`snoozed_until`, else `fire_at`) for a snoozed one,
    /// `fire_at` for a pending one; `None` once fired or dismissed.
    pub fn due_at(self, fire_at: i64, snoozed_until: Option<i64>) -> Option<i64> {
        match self {
            Self::Pending => Some(fire_at),
            Self::Snoozed => Some(snoozed_until.unwrap_or(fire_at)),
            Self::Fired | Self::Dismissed => None,
        }
    }
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
    fn due_times() {
        assert_eq!(ReminderState::Pending.due_at(10, Some(20)), Some(10));
        assert_eq!(ReminderState::Snoozed.due_at(10, Some(20)), Some(20));
        assert_eq!(ReminderState::Snoozed.due_at(10, None), Some(10));
        assert_eq!(ReminderState::Fired.due_at(10, None), None);
        assert_eq!(ReminderState::Dismissed.due_at(10, None), None);
    }

    #[test]
    fn reminders_serialize() {
        let reminder = crate::testing::reminder("r1");
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
