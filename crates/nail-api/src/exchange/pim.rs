//! Exchange calendar, contact and task items as the engines see them
//! (M3c, ADR 0072; Rust only, not part of the wire protocol).
//!
//! Graph (JSON) and EWS (XML) describe events, contacts and tasks the
//! Exchange way: local wall times with a Windows (or IANA) zone name,
//! recurrence *patterns* instead of `RRULE`s, attendee types and
//! response states, Microsoft To Do checklist items. Both engines
//! (`nail-proto-graph`, `nail-proto-ews`) produce and consume these
//! provider-neutral shapes; the mapping layer (`nail-exchange`) converts
//! them to and from Nail's PIM model ([`crate::pim`]: zones to IANA,
//! patterns to `RRULE`, exceptions to overrides, …). That keeps protocol
//! code and model mapping apart.
//!
//! Times are `YYYY-MM-DDTHH:MM:SS` wall times in [`ExchangeDateTime::time_zone`]
//! (Graph `dateTimeTimeZone`); dates are `YYYY-MM-DD`.

use serde::{Deserialize, Serialize};

use super::rules::Importance;
use super::{CategoryColor, ExchangeCollectionRole};

/// A wall time in a zone (Graph `dateTimeTimeZone`; EWS times are
/// converted to this by the engine).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExchangeDateTime {
    /// `YYYY-MM-DDTHH:MM:SS` (fractions dropped).
    pub date_time: String,
    /// The zone as the server names it: a Windows zone (`W. Europe
    /// Standard Time`), an IANA zone or `UTC`.
    pub time_zone: String,
}

impl ExchangeDateTime {
    /// A UTC time.
    pub fn utc(date_time: impl Into<String>) -> Self {
        Self {
            date_time: date_time.into(),
            time_zone: "UTC".to_owned(),
        }
    }

    /// The date part (`YYYY-MM-DD`).
    pub fn date(&self) -> &str {
        self.date_time.get(..10).unwrap_or(&self.date_time)
    }
}

/// A body (Graph `itemBody`, EWS `Body`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExchangeBody {
    /// HTML (`true`) or plain text.
    pub html: bool,
    /// The content.
    pub content: String,
}

/// Day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeDayOfWeek {
    /// Sunday.
    Sunday,
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
}

/// Which week of the month a relative pattern means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WeekIndex {
    /// First.
    First,
    /// Second.
    Second,
    /// Third.
    Third,
    /// Fourth.
    Fourth,
    /// Last.
    Last,
}

/// The kind of a recurrence pattern (Graph `recurrencePatternType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternKind {
    /// Every `interval` days.
    Daily,
    /// Every `interval` weeks on `days_of_week`.
    Weekly,
    /// Every `interval` months on `day_of_month`.
    AbsoluteMonthly,
    /// Every `interval` months on the `index` `days_of_week`.
    RelativeMonthly,
    /// Every `interval` years on `month` / `day_of_month`.
    AbsoluteYearly,
    /// Every `interval` years on the `index` `days_of_week` of `month`.
    RelativeYearly,
}

/// A recurrence pattern (Graph `recurrencePattern`, EWS `*Recurrence`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRecurrencePattern {
    /// The kind.
    pub kind: PatternKind,
    /// Every n days / weeks / months / years (at least 1).
    pub interval: u32,
    /// Weekdays (weekly, relative patterns).
    #[serde(default)]
    pub days_of_week: Vec<ExchangeDayOfWeek>,
    /// Day of the month (absolute patterns).
    #[serde(default)]
    pub day_of_month: Option<u32>,
    /// Month 1–12 (yearly patterns).
    #[serde(default)]
    pub month: Option<u32>,
    /// Week of the month (relative patterns).
    #[serde(default)]
    pub index: Option<WeekIndex>,
    /// First day of the week (weekly patterns).
    #[serde(default)]
    pub first_day_of_week: Option<ExchangeDayOfWeek>,
}

/// How long a recurrence runs (Graph `recurrenceRange`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExchangeRecurrenceRange {
    /// Until a date (inclusive).
    EndDate {
        /// First date (`YYYY-MM-DD`).
        start_date: String,
        /// Last date.
        end_date: String,
    },
    /// A number of occurrences.
    Numbered {
        /// First date.
        start_date: String,
        /// Occurrences.
        occurrences: u32,
    },
    /// Forever.
    NoEnd {
        /// First date.
        start_date: String,
    },
}

/// A recurrence: pattern and range, in the zone of
/// [`Self::time_zone`] (`recurrenceTimeZone`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRecurrence {
    /// The pattern.
    pub pattern: ExchangeRecurrencePattern,
    /// The range.
    pub range: ExchangeRecurrenceRange,
    /// The zone the dates are in, if the server says.
    #[serde(default)]
    pub time_zone: Option<String>,
}

/// An address with a name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExchangeEmail {
    /// Address.
    pub address: String,
    /// Display name.
    #[serde(default)]
    pub name: Option<String>,
}

/// The kind of an attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttendeeKind {
    /// Required.
    Required,
    /// Optional.
    Optional,
    /// A resource (room, equipment).
    Resource,
}

/// A response state (Graph `responseType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeResponse {
    /// No response.
    #[default]
    None,
    /// The organizer.
    Organizer,
    /// Tentatively accepted.
    TentativelyAccepted,
    /// Accepted.
    Accepted,
    /// Declined.
    Declined,
    /// Not responded.
    NotResponded,
}

/// An attendee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeAttendee {
    /// Who.
    pub email: ExchangeEmail,
    /// Required, optional or resource.
    pub kind: AttendeeKind,
    /// Their response.
    #[serde(default)]
    pub response: ExchangeResponse,
    /// A new time they proposed (start, end).
    #[serde(default)]
    pub proposed: Option<(ExchangeDateTime, ExchangeDateTime)>,
}

/// How an event shows in free/busy (Graph `freeBusyStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShowAs {
    /// Free.
    Free,
    /// Tentative.
    Tentative,
    /// Busy.
    #[default]
    Busy,
    /// Out of office.
    Oof,
    /// Working elsewhere.
    WorkingElsewhere,
    /// Unknown.
    Unknown,
}

/// Sensitivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sensitivity {
    /// Normal.
    #[default]
    Normal,
    /// Personal.
    Personal,
    /// Private.
    Private,
    /// Confidential.
    Confidential,
}

/// The kind of an event (Graph `eventType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// Not recurring.
    #[default]
    SingleInstance,
    /// The series.
    SeriesMaster,
    /// An unchanged occurrence.
    Occurrence,
    /// A changed occurrence.
    Exception,
}

/// A location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeLocation {
    /// Display name.
    pub display_name: String,
    /// The room's address, for rooms.
    #[serde(default)]
    pub email: Option<String>,
    /// Whether it is a room.
    #[serde(default)]
    pub room: bool,
}

/// A calendar event (Graph `event`, EWS `CalendarItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors independent boolean properties of a server event"
)]
pub struct ExchangeEvent {
    /// The server's id.
    pub remote_id: String,
    /// The version (EWS `ChangeKey`, Graph `changeKey`).
    #[serde(default)]
    pub change_key: Option<String>,
    /// The iCalendar `UID` (Graph `iCalUId` / `uid`, EWS `UID`).
    #[serde(default)]
    pub uid: Option<String>,
    /// Single, series, occurrence or exception.
    #[serde(default)]
    pub kind: EventKind,
    /// The series of an occurrence / exception.
    #[serde(default)]
    pub series_master_id: Option<String>,
    /// The original start of an occurrence / exception.
    #[serde(default)]
    pub original_start: Option<ExchangeDateTime>,
    /// Title.
    #[serde(default)]
    pub subject: String,
    /// Description.
    #[serde(default)]
    pub body: Option<ExchangeBody>,
    /// Start.
    pub start: ExchangeDateTime,
    /// End.
    pub end: ExchangeDateTime,
    /// All day (start / end at midnight).
    #[serde(default)]
    pub is_all_day: bool,
    /// Locations (the first is the main one).
    #[serde(default)]
    pub locations: Vec<ExchangeLocation>,
    /// Free/busy status.
    #[serde(default)]
    pub show_as: ShowAs,
    /// Sensitivity.
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Importance.
    #[serde(default)]
    pub importance: Option<Importance>,
    /// Reminder minutes before the start; `None` = no reminder.
    #[serde(default)]
    pub reminder_minutes: Option<u32>,
    /// Organizer.
    #[serde(default)]
    pub organizer: Option<ExchangeEmail>,
    /// Attendees (without the organizer).
    #[serde(default)]
    pub attendees: Vec<ExchangeAttendee>,
    /// The user organizes it.
    #[serde(default)]
    pub is_organizer: bool,
    /// The organizer wants responses.
    #[serde(default)]
    pub response_requested: bool,
    /// The user's own response.
    #[serde(default)]
    pub my_response: ExchangeResponse,
    /// Cancelled by the organizer.
    #[serde(default)]
    pub is_cancelled: bool,
    /// Online meeting join URL.
    #[serde(default)]
    pub online_meeting_url: Option<String>,
    /// Categories (Nail tags).
    #[serde(default)]
    pub categories: Vec<String>,
    /// The recurrence of a series.
    #[serde(default)]
    pub recurrence: Option<ExchangeRecurrence>,
    /// Dates (`YYYY-MM-DD`, original starts) of deleted occurrences.
    #[serde(default)]
    pub cancelled_occurrences: Vec<String>,
    /// Changed occurrences of a series (kind `exception`).
    #[serde(default)]
    pub exceptions: Vec<ExchangeEvent>,
    /// Last change (RFC 3339 UTC).
    #[serde(default)]
    pub last_modified: Option<String>,
}

/// Task status (Graph To Do `taskStatus`, EWS `TaskStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeTaskStatus {
    /// Not started.
    #[default]
    NotStarted,
    /// In progress.
    InProgress,
    /// Completed.
    Completed,
    /// Waiting on others.
    WaitingOnOthers,
    /// Deferred.
    Deferred,
}

/// A To Do checklist item (Nail: a subtask).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// The server's id.
    pub id: String,
    /// Text.
    pub title: String,
    /// Done.
    #[serde(default)]
    pub checked: bool,
}

/// A link of a To Do task to something else (e.g. the flagged message).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedResource {
    /// The server's id.
    #[serde(default)]
    pub id: Option<String>,
    /// URL.
    #[serde(default)]
    pub web_url: Option<String>,
    /// The app that made it.
    #[serde(default)]
    pub application: Option<String>,
    /// Shown text.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The linked item's id in that app (the message id for
    /// "Flagged email").
    #[serde(default)]
    pub external_id: Option<String>,
}

/// A task (Graph To Do `todoTask`, EWS `Task`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeTask {
    /// The server's id.
    pub remote_id: String,
    /// The version.
    #[serde(default)]
    pub change_key: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Notes.
    #[serde(default)]
    pub body: Option<ExchangeBody>,
    /// Status.
    #[serde(default)]
    pub status: ExchangeTaskStatus,
    /// Importance.
    #[serde(default)]
    pub importance: Option<Importance>,
    /// Start.
    #[serde(default)]
    pub start: Option<ExchangeDateTime>,
    /// Due.
    #[serde(default)]
    pub due: Option<ExchangeDateTime>,
    /// Completed at.
    #[serde(default)]
    pub completed: Option<ExchangeDateTime>,
    /// Reminder time.
    #[serde(default)]
    pub reminder: Option<ExchangeDateTime>,
    /// Recurrence.
    #[serde(default)]
    pub recurrence: Option<ExchangeRecurrence>,
    /// Categories.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Checklist items (To Do).
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
    /// Links (To Do).
    #[serde(default)]
    pub linked_resources: Vec<LinkedResource>,
    /// Created (RFC 3339 UTC).
    #[serde(default)]
    pub created: Option<String>,
    /// Last change (RFC 3339 UTC).
    #[serde(default)]
    pub last_modified: Option<String>,
}

/// A postal address.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExchangePostalAddress {
    /// `home`, `business` or `other`.
    pub kind: String,
    /// Street.
    #[serde(default)]
    pub street: Option<String>,
    /// City.
    #[serde(default)]
    pub city: Option<String>,
    /// State.
    #[serde(default)]
    pub state: Option<String>,
    /// Postal code.
    #[serde(default)]
    pub postal_code: Option<String>,
    /// Country.
    #[serde(default)]
    pub country: Option<String>,
}

/// A phone number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangePhone {
    /// `home`, `business`, `mobile`, `fax`, `other`.
    pub kind: String,
    /// The number.
    pub number: String,
}

/// A contact (Graph `contact`, EWS `Contact`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeContact {
    /// The server's id.
    pub remote_id: String,
    /// The version.
    #[serde(default)]
    pub change_key: Option<String>,
    /// Display name.
    #[serde(default)]
    pub display_name: String,
    /// Given name.
    #[serde(default)]
    pub given_name: Option<String>,
    /// Family name.
    #[serde(default)]
    pub surname: Option<String>,
    /// Middle name.
    #[serde(default)]
    pub middle_name: Option<String>,
    /// Nickname.
    #[serde(default)]
    pub nickname: Option<String>,
    /// Company.
    #[serde(default)]
    pub company: Option<String>,
    /// Department.
    #[serde(default)]
    pub department: Option<String>,
    /// Job title.
    #[serde(default)]
    pub job_title: Option<String>,
    /// Email addresses (the first is the preferred one).
    #[serde(default)]
    pub emails: Vec<ExchangeEmail>,
    /// Phone numbers.
    #[serde(default)]
    pub phones: Vec<ExchangePhone>,
    /// Postal addresses.
    #[serde(default)]
    pub addresses: Vec<ExchangePostalAddress>,
    /// Birthday (`YYYY-MM-DD`).
    #[serde(default)]
    pub birthday: Option<String>,
    /// Notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Web pages.
    #[serde(default)]
    pub urls: Vec<String>,
    /// Categories.
    #[serde(default)]
    pub categories: Vec<String>,
    /// A distribution list (`members` set) rather than a person.
    #[serde(default)]
    pub is_group: bool,
    /// Members of a distribution list.
    #[serde(default)]
    pub members: Vec<ExchangeEmail>,
    /// Photo bytes (JPEG / PNG), when fetched.
    #[serde(default, skip_serializing)]
    pub photo: Option<Vec<u8>>,
}

/// What a PIM folder holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PimFolderKind {
    /// Events.
    Calendar,
    /// Contacts.
    Contacts,
    /// Tasks (To Do list or EWS task folder).
    Tasks,
}

/// A calendar, contact folder or task list on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangePimFolder {
    /// The server's id.
    pub remote_id: String,
    /// The version.
    #[serde(default)]
    pub change_key: Option<String>,
    /// Display name.
    pub name: String,
    /// What it holds.
    pub kind: PimFolderKind,
    /// A special role.
    #[serde(default)]
    pub role: Option<ExchangeCollectionRole>,
    /// A To Do list (Graph).
    #[serde(default)]
    pub todo: bool,
    /// The user may only read it.
    #[serde(default)]
    pub read_only: bool,
    /// The calendar's colour, if the server has one.
    #[serde(default)]
    pub color: Option<CategoryColor>,
}

/// One calendar / contact / task item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ExchangePimItem {
    /// An event (a series carries its exceptions).
    Event(Box<ExchangeEvent>),
    /// A task.
    Task(Box<ExchangeTask>),
    /// A contact or distribution list.
    Contact(Box<ExchangeContact>),
}

impl ExchangePimItem {
    /// The server's id.
    pub fn remote_id(&self) -> &str {
        match self {
            Self::Event(e) => &e.remote_id,
            Self::Task(t) => &t.remote_id,
            Self::Contact(c) => &c.remote_id,
        }
    }

    /// The version.
    pub fn change_key(&self) -> Option<&str> {
        match self {
            Self::Event(e) => e.change_key.as_deref(),
            Self::Task(t) => t.change_key.as_deref(),
            Self::Contact(c) => c.change_key.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_shape_round_trips() {
        let event: ExchangeEvent = serde_json::from_value(json!({
            "remote_id": "AAMkAD", "change_key": "DwAAABY", "uid": "040000008200E0",
            "kind": "seriesMaster", "subject": "Weekly sync",
            "start": {"date_time": "2026-10-05T09:00:00", "time_zone": "W. Europe Standard Time"},
            "end": {"date_time": "2026-10-05T09:30:00", "time_zone": "W. Europe Standard Time"},
            "show_as": "busy", "reminder_minutes": 15,
            "attendees": [{"email": {"address": "room1@contoso.example"}, "kind": "resource",
                           "response": "accepted"}],
            "recurrence": {
                "pattern": {"kind": "weekly", "interval": 1, "days_of_week": ["monday"]},
                "range": {"kind": "numbered", "start_date": "2026-10-05", "occurrences": 10}
            },
            "cancelled_occurrences": ["2026-10-12"]
        }))
        .unwrap();
        assert_eq!(event.kind, EventKind::SeriesMaster);
        assert_eq!(event.start.date(), "2026-10-05");
        assert_eq!(event.attendees[0].kind, AttendeeKind::Resource);
        let item = ExchangePimItem::Event(Box::new(event.clone()));
        assert_eq!(item.remote_id(), "AAMkAD");
        assert_eq!(item.change_key(), Some("DwAAABY"));
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["type"], json!("event"));
        assert_eq!(value["recurrence"]["range"]["kind"], json!("numbered"));
        let back: ExchangePimItem = serde_json::from_value(value).unwrap();
        assert_eq!(back, item);
        assert_eq!(
            ExchangeDateTime::utc("2026-10-05T07:00:00").time_zone,
            "UTC"
        );
    }

    #[test]
    fn task_and_contact_defaults() {
        let task: ExchangeTask = serde_json::from_value(json!({
            "remote_id": "AAkALg", "title": "Report",
            "checklist": [{"id": "c1", "title": "Draft", "checked": true}]
        }))
        .unwrap();
        assert_eq!(task.status, ExchangeTaskStatus::NotStarted);
        assert!(task.checklist[0].checked);
        let contact: ExchangeContact = serde_json::from_value(json!({
            "remote_id": "AAMkC", "display_name": "Bob",
            "emails": [{"address": "bob@contoso.example", "name": "Bob"}]
        }))
        .unwrap();
        assert!(!contact.is_group && contact.photo.is_none());
        let folder: ExchangePimFolder = serde_json::from_value(json!({
            "remote_id": "AQMk", "name": "Tasks", "kind": "tasks", "role": "default-tasks",
            "todo": true
        }))
        .unwrap();
        assert_eq!(folder.role, Some(ExchangeCollectionRole::DefaultTasks));
    }
}
