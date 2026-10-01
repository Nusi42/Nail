//! Read-only calendar, contact and task access (M3a; payloads in
//! `nail_api::pim::mcp`, ADR 0058; integration ADR 0063).
//!
//! Everything needs the `read` scope. A client sees the calendars and
//! address books of a source its grant reaches
//! ([`nail_api::McpGrant::allows_pim_source`]): sources of a granted mail
//! account, and every source (local, subscriptions, standalone DAV
//! servers) with a grant for all accounts. A calendar or contact outside
//! the grant is reported as not found.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use nail_api::mcp::McpScope;
use nail_api::pim::mcp::{
    MCP_DEFAULT_EVENT_DAYS, MCP_MAX_EVENT_DAYS, McpCalendarsResource, McpContactResource,
    McpContactSearchResult, McpContactsResource, McpEventsResource, McpFreeTimeResult,
    McpTasksResource,
};
use nail_api::pim::{AddressBook, Calendar, CalendarEvent, FreeSlot, PimItemRef};
use nail_api::{AccountId, ApiError, CalendarId, ContactId, Request, Response};
use rmcp::schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::access::Access;
use crate::data::McpData;
use crate::server::{Failure, Shared};

/// `list_events` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListEventsArgs {
    /// Range start, RFC 3339 (default: now).
    #[serde(default)]
    pub from: Option<String>,
    /// Range end, RFC 3339 (default: 30 days after `from`; at most 366 days).
    #[serde(default)]
    pub to: Option<String>,
    /// Only these calendars (ids from `nail://calendars`); empty = every visible one.
    #[serde(default)]
    pub calendar_ids: Vec<String>,
    /// IANA zone for all-day and floating events (default UTC).
    #[serde(default)]
    pub tz: Option<String>,
}

/// `find_free_time` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FreeTimeArgs {
    /// Range start, RFC 3339.
    pub from: String,
    /// Range end, RFC 3339 (at most 366 days after `from`).
    pub to: String,
    /// Shortest slot in minutes.
    pub duration_minutes: u32,
    /// Calendars that count; empty = every visible one.
    #[serde(default)]
    pub calendar_ids: Vec<String>,
    /// IANA zone of `day_start` / `day_end` and of all-day events (default UTC).
    #[serde(default)]
    pub tz: Option<String>,
    /// Earliest time of day for slots, `HH:MM` (e.g. `09:00`).
    #[serde(default)]
    pub day_start: Option<String>,
    /// Latest time of day for slots, `HH:MM` (e.g. `17:00`).
    #[serde(default)]
    pub day_end: Option<String>,
}

/// `search_contacts` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchContactsArgs {
    /// Name, address, phone number or organization to look for.
    pub query: String,
    /// Most matches (default 20, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// `list_tasks` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListTasksArgs {
    /// Only these task lists; empty = every visible one.
    #[serde(default)]
    pub calendar_ids: Vec<String>,
    /// Include completed and cancelled tasks.
    #[serde(default)]
    pub include_completed: bool,
    /// Only tasks due before this instant (RFC 3339).
    #[serde(default)]
    pub due_before: Option<String>,
}

fn unexpected(response: &Response) -> Failure {
    Failure::Api(ApiError::internal(format!(
        "unexpected answer {}",
        serde_json::to_value(response)
            .ok()
            .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default()
    )))
}

fn instant(text: &str) -> Result<DateTime<Utc>, Failure> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| Failure::Api(ApiError::invalid_request(format!("invalid time {text:?}"))))
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A range from optional ends, defaulted and capped.
fn event_range(from: Option<&str>, to: Option<&str>) -> Result<(String, String), Failure> {
    let from = match from {
        Some(text) => instant(text)?,
        None => Utc::now(),
    };
    let to = match to {
        Some(text) => instant(text)?,
        None => from + Duration::days(i64::from(MCP_DEFAULT_EVENT_DAYS)),
    };
    if to < from || to - from > Duration::days(i64::from(MCP_MAX_EVENT_DAYS)) {
        return Err(Failure::Api(ApiError::invalid_request(format!(
            "the range must be at most {MCP_MAX_EVENT_DAYS} days"
        ))));
    }
    Ok((stamp(from), stamp(to)))
}

fn allowed(access: &Access, account: Option<&AccountId>) -> bool {
    access.all_accounts() || account.is_some_and(|a| access.allows_account(a))
}

fn not_found(what: impl std::fmt::Display) -> Failure {
    Failure::Api(ApiError::not_found(format!("{what} not found")))
}

/// Clips free slots to working hours (`HH:MM`) of each day in `tz`.
fn working_hours(
    slots: Vec<FreeSlot>,
    tz: chrono_tz::Tz,
    day_start: Option<NaiveTime>,
    day_end: Option<NaiveTime>,
    min: Duration,
) -> Vec<FreeSlot> {
    if day_start.is_none() && day_end.is_none() {
        return slots;
    }
    let start_of_day = day_start.unwrap_or(NaiveTime::MIN);
    let end_of_day = day_end;
    let mut out = Vec::new();
    for slot in slots {
        let (Ok(s), Ok(e)) = (instant(&slot.start), instant(&slot.end)) else {
            continue;
        };
        let mut day = s.with_timezone(&tz).date_naive();
        let last = e.with_timezone(&tz).date_naive();
        while day <= last {
            let open = tz
                .from_local_datetime(&day.and_time(start_of_day))
                .earliest()
                .map(|t| t.with_timezone(&Utc));
            let close = match end_of_day {
                Some(end) => tz.from_local_datetime(&day.and_time(end)).earliest(),
                None => day.succ_opt().and_then(|next| {
                    tz.from_local_datetime(&next.and_time(NaiveTime::MIN))
                        .earliest()
                }),
            }
            .map(|t| t.with_timezone(&Utc));
            if let (Some(open), Some(close)) = (open, close) {
                let (a, b) = (s.max(open), e.min(close));
                if b - a >= min {
                    out.push(FreeSlot {
                        start: stamp(a),
                        end: stamp(b),
                    });
                }
            }
            let Some(next) = day.succ_opt() else { break };
            day = next;
        }
    }
    out
}

fn hhmm(text: Option<&str>) -> Result<Option<NaiveTime>, Failure> {
    text.map(|t| {
        NaiveTime::parse_from_str(t, "%H:%M").map_err(|_| {
            Failure::Api(ApiError::invalid_request(format!(
                "invalid time of day {t:?}"
            )))
        })
    })
    .transpose()
}

impl<D: McpData> Shared<D> {
    async fn pim(&self, request: Request) -> Result<Response, Failure> {
        Ok(self.data.pim(request).await?)
    }

    /// The calendars (and task lists) the client may see.
    async fn visible_calendars(&self, access: &Access) -> Result<Vec<Calendar>, Failure> {
        access.require(McpScope::Read)?;
        match self.pim(Request::ListCalendars { source_id: None }).await? {
            Response::Calendars { calendars } => Ok(calendars
                .into_iter()
                .filter(|c| allowed(access, c.account_id.as_ref()))
                .collect()),
            other => Err(unexpected(&other)),
        }
    }

    async fn visible_books(&self, access: &Access) -> Result<Vec<AddressBook>, Failure> {
        access.require(McpScope::Read)?;
        match self
            .pim(Request::ListAddressBooks { source_id: None })
            .await?
        {
            Response::AddressBooks { address_books } => Ok(address_books
                .into_iter()
                .filter(|b| allowed(access, b.account_id.as_ref()))
                .collect()),
            other => Err(unexpected(&other)),
        }
    }

    /// The asked calendars, all visible; empty = every visible one with
    /// `component`.
    async fn chosen_calendars(
        &self,
        access: &Access,
        ids: &[String],
        tasks: bool,
    ) -> Result<Vec<CalendarId>, Failure> {
        let visible = self.visible_calendars(access).await?;
        if ids.is_empty() {
            return Ok(visible
                .into_iter()
                .filter(|c| if tasks { c.has_tasks() } else { c.has_events() })
                .map(|c| c.id)
                .collect());
        }
        ids.iter()
            .map(|id| {
                visible
                    .iter()
                    .find(|c| c.id.as_str() == id)
                    .map(|c| c.id.clone())
                    .ok_or_else(|| not_found(format!("calendar {id}")))
            })
            .collect()
    }

    /// The calendars a `list_events` call names (all visible ones when
    /// empty).
    pub(crate) async fn chosen_calendars_for_tool(
        &self,
        access: &Access,
        ids: &[String],
    ) -> Result<Vec<CalendarId>, Failure> {
        self.chosen_calendars(access, ids, false).await
    }

    /// `nail://calendars`
    pub(crate) async fn calendars_json(&self, access: &Access) -> Result<Value, Failure> {
        let calendars = self.visible_calendars(access).await?;
        Ok(json!(McpCalendarsResource { calendars }))
    }

    /// `nail://calendars/{id}/events` and `list_events`.
    pub(crate) async fn events_json(
        &self,
        access: &Access,
        calendars: Vec<CalendarId>,
        from: Option<&str>,
        to: Option<&str>,
        tz: Option<String>,
    ) -> Result<Value, Failure> {
        access.require(McpScope::Read)?;
        let (from, to) = event_range(from, to)?;
        if calendars.is_empty() {
            return Ok(json!(McpEventsResource {
                from,
                to,
                instances: Vec::new(),
                events: Vec::new(),
                icalendar: String::new(),
            }));
        }
        let instances = match self
            .pim(Request::ListEventInstances {
                from: from.clone(),
                to: to.clone(),
                calendar_ids: Some(calendars),
                tz: tz.unwrap_or_else(|| "UTC".to_owned()),
            })
            .await?
        {
            Response::EventInstances { instances, .. } => instances,
            other => return Err(unexpected(&other)),
        };
        let mut events: Vec<CalendarEvent> = Vec::new();
        let mut icalendar = String::new();
        for instance in &instances {
            if events.iter().any(|e| e.id == instance.event_id) {
                continue;
            }
            if let Response::PimEvent { event } = self
                .pim(Request::GetEvent {
                    event_id: instance.event_id.clone(),
                })
                .await?
            {
                let item = PimItemRef::Event {
                    event_id: event.id.clone(),
                };
                if let Some(raw) = self.data.pim_raw(&item).await? {
                    icalendar.push_str(&raw);
                }
                events.push(*event);
            }
        }
        Ok(json!(McpEventsResource {
            from,
            to,
            instances,
            events,
            icalendar,
        }))
    }

    /// `nail://calendars/{id}/events`: one visible calendar.
    pub(crate) async fn calendar_events_json(
        &self,
        access: &Access,
        calendar: &CalendarId,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Value, Failure> {
        let ids = self
            .chosen_calendars(access, std::slice::from_ref(&calendar.0), false)
            .await?;
        self.events_json(access, ids, from, to, None).await
    }

    /// `nail://contacts`
    pub(crate) async fn contacts_json(&self, access: &Access) -> Result<Value, Failure> {
        let address_books = self.visible_books(access).await?;
        let ids: Vec<_> = address_books.iter().map(|b| b.id.clone()).collect();
        let contacts = if ids.is_empty() {
            Vec::new()
        } else {
            match self
                .pim(Request::ListContacts {
                    address_book_ids: Some(ids),
                    group_id: None,
                    offset: 0,
                    limit: 1000,
                })
                .await?
            {
                Response::Contacts { contacts, .. } => contacts,
                other => return Err(unexpected(&other)),
            }
        };
        Ok(json!(McpContactsResource {
            address_books,
            contacts,
        }))
    }

    /// `nail://contacts/{id}`
    pub(crate) async fn contact_json(
        &self,
        access: &Access,
        id: &ContactId,
    ) -> Result<Value, Failure> {
        let books = self.visible_books(access).await?;
        let contact = match self
            .pim(Request::GetContact {
                contact_id: id.clone(),
            })
            .await
        {
            Ok(Response::PimContact { contact }) => *contact,
            _ => return Err(not_found(format!("contact {id}"))),
        };
        if !books.iter().any(|b| b.id == contact.address_book_id) {
            return Err(not_found(format!("contact {id}")));
        }
        let vcard = self
            .data
            .pim_raw(&PimItemRef::Contact {
                contact_id: id.clone(),
            })
            .await?
            .unwrap_or_default();
        Ok(json!(McpContactResource { contact, vcard }))
    }

    /// `nail://tasks` and `list_tasks`.
    pub(crate) async fn tasks_json(
        &self,
        access: &Access,
        args: ListTasksArgs,
    ) -> Result<Value, Failure> {
        let visible = self.visible_calendars(access).await?;
        let ids = self
            .chosen_calendars(access, &args.calendar_ids, true)
            .await?;
        let lists: Vec<Calendar> = visible
            .into_iter()
            .filter(|c| ids.contains(&c.id))
            .collect();
        let due_before = args.due_before.as_deref().map(instant).transpose()?;
        let mut tasks = if ids.is_empty() {
            Vec::new()
        } else {
            match self
                .pim(Request::ListTasks {
                    calendar_ids: Some(ids),
                    include_completed: args.include_completed,
                })
                .await?
            {
                Response::Tasks { tasks } => tasks,
                other => return Err(unexpected(&other)),
            }
        };
        if let Some(before) = due_before {
            let day = before.format("%Y-%m-%d").to_string();
            let local = before.format("%Y-%m-%dT%H:%M:%S").to_string();
            tasks.retain(|t| {
                t.fields.due.as_ref().is_some_and(|due| match due {
                    nail_api::pim::PimTime::Date { date } => *date < day,
                    other => other.sort_key() < local.as_str(),
                })
            });
        }
        Ok(json!(McpTasksResource { lists, tasks }))
    }

    /// `find_free_time`.
    pub(crate) async fn free_time_json(
        &self,
        access: &Access,
        args: FreeTimeArgs,
    ) -> Result<Value, Failure> {
        let ids = self
            .chosen_calendars(access, &args.calendar_ids, false)
            .await?;
        let (from, to) = event_range(Some(&args.from), Some(&args.to))?;
        let tz_name = args.tz.unwrap_or_else(|| "UTC".to_owned());
        let tz: chrono_tz::Tz = if tz_name.eq_ignore_ascii_case("UTC") {
            chrono_tz::UTC
        } else {
            tz_name.parse().map_err(|_| {
                Failure::Api(ApiError::invalid_request(format!(
                    "unknown time zone {tz_name:?}"
                )))
            })?
        };
        let (day_start, day_end) = (
            hhmm(args.day_start.as_deref())?,
            hhmm(args.day_end.as_deref())?,
        );
        let minutes = args.duration_minutes.max(1);
        let slots = match self
            .pim(Request::FindFreeTime {
                from,
                to,
                duration_minutes: minutes,
                calendar_ids: Some(ids),
                tz: tz_name,
            })
            .await?
        {
            Response::FreeTime { slots } => slots,
            other => return Err(unexpected(&other)),
        };
        let slots = working_hours(
            slots,
            tz,
            day_start,
            day_end,
            Duration::minutes(i64::from(minutes)),
        );
        Ok(json!(McpFreeTimeResult { slots }))
    }

    /// `search_contacts`.
    pub(crate) async fn search_contacts_json(
        &self,
        access: &Access,
        args: SearchContactsArgs,
    ) -> Result<Value, Failure> {
        let books: Vec<_> = self
            .visible_books(access)
            .await?
            .into_iter()
            .map(|b| b.id)
            .collect();
        if books.is_empty() || args.query.trim().is_empty() {
            return Ok(json!(McpContactSearchResult {
                matches: Vec::new()
            }));
        }
        match self
            .pim(Request::SearchContacts {
                query: args.query,
                address_book_ids: Some(books),
                limit: args.limit.unwrap_or(20).clamp(1, 200),
            })
            .await?
        {
            Response::ContactMatches { matches } => Ok(json!(McpContactSearchResult { matches })),
            other => Err(unexpected(&other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_default_and_cap() {
        let (from, to) = event_range(Some("2026-10-01T00:00:00Z"), None).unwrap();
        assert_eq!(from, "2026-10-01T00:00:00Z");
        assert_eq!(to, "2026-10-31T00:00:00Z");
        assert!(event_range(Some("2026-01-01T00:00:00Z"), Some("2027-06-01T00:00:00Z")).is_err());
        assert!(event_range(Some("soon"), None).is_err());
    }

    #[test]
    fn working_hours_clip_slots_per_day() {
        let slots = vec![FreeSlot {
            start: "2026-10-05T06:00:00Z".into(),
            end: "2026-10-06T20:00:00Z".into(),
        }];
        let clipped = working_hours(
            slots,
            chrono_tz::Europe::Berlin,
            NaiveTime::from_hms_opt(9, 0, 0),
            NaiveTime::from_hms_opt(17, 0, 0),
            Duration::minutes(30),
        );
        let spans: Vec<(&str, &str)> = clipped
            .iter()
            .map(|s| (s.start.as_str(), s.end.as_str()))
            .collect();
        assert_eq!(
            spans,
            [
                ("2026-10-05T07:00:00Z", "2026-10-05T15:00:00Z"),
                ("2026-10-06T07:00:00Z", "2026-10-06T15:00:00Z")
            ]
        );
        assert!(hhmm(Some("9am")).is_err());
    }
}
