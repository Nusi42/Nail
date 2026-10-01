//! Read-only MCP access to calendars, events, contacts and tasks
//! (design.md, "MCP server"; plan M3 scope: writes stay in M4).
//!
//! Resources and tools need the `read` scope ([`crate::McpScope::Read`]).
//! A client sees a source when its grant reaches it
//! ([`crate::McpGrant::allows_pim_source`]): the sources of a granted mail
//! account (DAV sources with `account=`), and every source — including
//! the local one, subscriptions and standalone DAV servers — with
//! `all_accounts`. (Per-source grants may come with the M4 writes.) The
//! payload types below are the JSON the MCP server returns; event and
//! contact resources also carry the iCalendar / vCard text.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::calendar::{Calendar, CalendarEvent, EventInstance, FreeSlot};
use super::contact::{AddressBook, Contact, ContactMatch};
use super::task::Task;
use crate::ids::CalendarId;

/// `nail://calendars`: every visible calendar and task list.
pub const MCP_CALENDARS_URI: &str = "nail://calendars";
/// `nail://calendars/{id}/events{?from,to}`: expanded instances of a
/// calendar in a range (RFC 3339; default: today and the next 30 days).
pub const MCP_CALENDAR_EVENTS_TEMPLATE: &str = "nail://calendars/{id}/events{?from,to}";
/// `nail://contacts`: every address book with its contacts (summaries).
pub const MCP_CONTACTS_URI: &str = "nail://contacts";
/// `nail://contacts/{id}`: one contact as JSON plus vCard.
pub const MCP_CONTACT_TEMPLATE: &str = "nail://contacts/{id}";
/// `nail://tasks`: open tasks across all lists.
pub const MCP_TASKS_URI: &str = "nail://tasks";

/// The read-only PIM tools.
pub const MCP_PIM_TOOLS: [&str; 4] = [
    "list_events",
    "find_free_time",
    "search_contacts",
    "list_tasks",
];

/// Days `nail://calendars/{id}/events` and `list_events` cover when no
/// range is given.
pub const MCP_DEFAULT_EVENT_DAYS: u32 = 30;
/// Longest range `list_events` / `find_free_time` accept, days.
pub const MCP_MAX_EVENT_DAYS: u32 = 366;

/// Arguments of the `list_events` tool.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpListEventsArgs {
    /// Range start (RFC 3339), `null` = now.
    pub from: Option<String>,
    /// Range end (RFC 3339), `null` = [`MCP_DEFAULT_EVENT_DAYS`] later.
    pub to: Option<String>,
    /// Calendars, empty = every visible one.
    pub calendar_ids: Vec<CalendarId>,
    /// Zone for all-day and floating events (IANA), `null` = the system's.
    pub tz: Option<String>,
}

/// Arguments of the `find_free_time` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpFindFreeTimeArgs {
    /// Range start (RFC 3339).
    pub from: String,
    /// Range end (RFC 3339).
    pub to: String,
    /// Shortest slot, minutes.
    pub duration_minutes: u32,
    /// Calendars that count as busy, empty = every visible one.
    #[serde(default)]
    pub calendar_ids: Vec<CalendarId>,
    /// Zone (IANA), `null` = the system's.
    #[serde(default)]
    pub tz: Option<String>,
    /// Earliest time of day, `HH:MM` in `tz`, `null` = 00:00.
    #[serde(default)]
    pub day_start: Option<String>,
    /// Latest time of day, `HH:MM`, `null` = 24:00.
    #[serde(default)]
    pub day_end: Option<String>,
}

/// Arguments of the `search_contacts` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpSearchContactsArgs {
    /// Name, address, phone or organization (substring).
    pub query: String,
    /// At most this many (default 20, at most 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Arguments of the `list_tasks` tool.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpListTasksArgs {
    /// Task lists, empty = all.
    pub calendar_ids: Vec<CalendarId>,
    /// Include completed and cancelled tasks.
    pub include_completed: bool,
    /// Only tasks due before this instant (RFC 3339).
    pub due_before: Option<String>,
}

/// `nail://calendars`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpCalendarsResource {
    /// Calendars and task lists the client may see.
    pub calendars: Vec<Calendar>,
}

/// `nail://calendars/{id}/events` and the `list_events` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpEventsResource {
    /// Range start (RFC 3339 UTC).
    pub from: String,
    /// Range end (RFC 3339 UTC).
    pub to: String,
    /// The occurrences, by start.
    pub instances: Vec<EventInstance>,
    /// The events (series) the occurrences belong to.
    pub events: Vec<CalendarEvent>,
    /// The same events as one iCalendar object.
    pub icalendar: String,
}

/// The `find_free_time` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpFreeTimeResult {
    /// Free slots of at least the asked duration, by start.
    pub slots: Vec<FreeSlot>,
}

/// `nail://contacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpContactsResource {
    /// The address books the client may see.
    pub address_books: Vec<AddressBook>,
    /// Their contacts.
    pub contacts: Vec<super::contact::ContactSummary>,
}

/// `nail://contacts/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpContactResource {
    /// The contact.
    pub contact: Contact,
    /// Its vCard.
    pub vcard: String,
}

/// The `search_contacts` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpContactSearchResult {
    /// Matches, best first.
    pub matches: Vec<ContactMatch>,
}

/// `nail://tasks` and the `list_tasks` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpTasksResource {
    /// The task lists.
    pub lists: Vec<Calendar>,
    /// The tasks, by due date.
    pub tasks: Vec<Task>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_arguments_have_defaults() {
        let args: McpListEventsArgs = serde_json::from_value(json!({})).unwrap();
        assert_eq!(args, McpListEventsArgs::default());
        let args: McpFindFreeTimeArgs = serde_json::from_value(json!({
            "from": "2026-10-01T00:00:00Z", "to": "2026-10-02T00:00:00Z", "duration_minutes": 30
        }))
        .unwrap();
        assert!(args.calendar_ids.is_empty() && args.day_start.is_none());
        let args: McpListTasksArgs =
            serde_json::from_value(json!({"include_completed": true})).unwrap();
        assert!(args.include_completed);
        assert!(MCP_CALENDAR_EVENTS_TEMPLATE.starts_with(MCP_CALENDARS_URI));
        assert_eq!(MCP_PIM_TOOLS.len(), 4);
    }
}
