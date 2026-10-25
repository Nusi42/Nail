//! Calendars, contacts and Microsoft To Do: folders, deltas, writes and
//! meeting responses, converted between Graph JSON and the
//! provider-neutral shapes of [`nail_api::exchange::pim`].
//!
//! * **Calendars**: `calendarView/delta` over a window (default one year
//!   back, two ahead) tells which events changed; the changed single
//!   events and series masters are then read in their own zone
//!   (`Prefer: outlook.timezone` = the event's `originalStartTimeZone`)
//!   through `$batch`, a series with its exceptions from `instances`.
//!   Cancelled occurrences come from the master's `cancelledOccurrences`
//!   when the server lists them.
//! * **Contacts**: `contactFolders/{id}/contacts/delta`. The default
//!   contacts folder is not listed by `contactFolders`; its id is the
//!   parent of the listed folders or of a contact. In a mailbox with
//!   neither, it is [`DEFAULT_CONTACTS`], read by a full listing.
//! * **To Do** (own mailbox only): `todo/lists`, `tasks/delta`, checklist
//!   items and linked resources through `$batch`. Task ids are
//!   `<list id>/<task id>` because To Do addresses tasks through their
//!   list (Graph ids never contain `/`).

use std::collections::{BTreeMap, BTreeSet};

use nail_api::exchange::engine::{
    ExchangePimWrite, ExchangePimWriteOutcome, MeetingSend, PimDelta, PimDeltaOutcome,
};
use nail_api::exchange::pim::{
    AttendeeKind, ChecklistItem, EventKind, ExchangeAttendee, ExchangeBody, ExchangeContact,
    ExchangeDateTime, ExchangeEmail, ExchangeEvent, ExchangeLocation, ExchangePhone,
    ExchangePimFolder, ExchangePimItem, ExchangePostalAddress, ExchangeRecurrence,
    ExchangeRecurrencePattern, ExchangeRecurrenceRange, ExchangeResponse, ExchangeTask,
    ExchangeTaskStatus, LinkedResource, PimFolderKind, Sensitivity, ShowAs,
};
use nail_api::exchange::rules::Importance;
use nail_api::exchange::{CategoryColor, ExchangeCollectionRole};
use nail_api::scheduling::InvitationResponse;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::batch::BatchRequest;
use crate::engine::GraphEngine;
use crate::error::GraphError;
use crate::wire::{Calendar, ContactFolder, Page, TodoTaskList, id_of, is_removed};
use crate::{mailbox_path, path_segment, text};

/// The default contacts folder of a mailbox whose id is not known yet.
pub const DEFAULT_CONTACTS: &str = "contacts";

/// The cursor of a full listing of [`DEFAULT_CONTACTS`].
const LISTING_CURSOR: &str = "nail:contacts-listing";

/// Pages of folder listings read at most.
const MAX_LIST_PAGES: usize = 20;

/// The id of a To Do task as the engine hands it out.
pub fn todo_task_id(list: &str, task: &str) -> String {
    format!("{list}/{task}")
}

/// The list and task of an id from [`todo_task_id`].
pub fn split_todo_task_id(id: &str) -> Option<(&str, &str)> {
    id.rsplit_once('/')
        .filter(|(l, t)| !l.is_empty() && !t.is_empty())
}

fn calendar_color(name: Option<&str>) -> Option<CategoryColor> {
    Some(match name? {
        "lightBlue" => CategoryColor::Preset7,
        "lightGreen" => CategoryColor::Preset4,
        "lightOrange" => CategoryColor::Preset1,
        "lightGray" => CategoryColor::Preset12,
        "lightYellow" => CategoryColor::Preset3,
        "lightTeal" => CategoryColor::Preset5,
        "lightPink" => CategoryColor::Preset9,
        "lightBrown" => CategoryColor::Preset2,
        "lightRed" => CategoryColor::Preset0,
        _ => return None,
    })
}

/// Whether `outlook.timezone` can name this zone (Graph's own names for
/// custom zones cannot be asked for).
fn plain_zone(zone: &str) -> bool {
    !zone.is_empty()
        && !zone.starts_with("tzone://")
        && !zone.eq_ignore_ascii_case("Customized Time Zone")
        && zone.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        && !zone.contains('"')
}

fn zone_header(zone: Option<&str>) -> Option<(String, String)> {
    zone.filter(|z| plain_zone(z))
        .map(|z| ("Prefer".to_owned(), format!("outlook.timezone=\"{z}\"")))
}

fn if_match(change_key: Option<&str>) -> Vec<(&'static str, String)> {
    match change_key.filter(|k| !k.is_empty()) {
        Some(k) if k.starts_with("W/") || k.starts_with('"') => vec![("If-Match", k.to_owned())],
        Some(k) => vec![("If-Match", format!("W/\"{k}\""))],
        None => Vec::new(),
    }
}

fn decode<T: DeserializeOwned>(value: Option<&Value>) -> Option<T> {
    value
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn string_list(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn date_time(value: Option<&Value>) -> Option<ExchangeDateTime> {
    let value = value.filter(|v| !v.is_null())?;
    let wall = value.get("dateTime")?.as_str()?;
    if wall.is_empty() {
        return None;
    }
    Some(ExchangeDateTime {
        date_time: text::trim_fraction(wall),
        time_zone: value
            .get("timeZone")
            .and_then(Value::as_str)
            .filter(|z| !z.is_empty())
            .unwrap_or("UTC")
            .to_owned(),
    })
}

fn date_time_json(time: &ExchangeDateTime) -> Value {
    json!({ "dateTime": time.date_time, "timeZone": time.time_zone })
}

fn body_of(value: Option<&Value>) -> Option<ExchangeBody> {
    let value = value.filter(|v| !v.is_null())?;
    let content = value.get("content").and_then(Value::as_str).unwrap_or("");
    if content.is_empty() {
        return None;
    }
    Some(ExchangeBody {
        html: value
            .get("contentType")
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case("html")),
        content: content.to_owned(),
    })
}

fn body_json(body: &ExchangeBody) -> Value {
    json!({
        "contentType": if body.html { "html" } else { "text" },
        "content": body.content,
    })
}

fn email(value: Option<&Value>) -> Option<ExchangeEmail> {
    let value = value?.get("emailAddress")?;
    let address = value.get("address")?.as_str()?;
    if address.is_empty() {
        return None;
    }
    Some(ExchangeEmail {
        address: address.to_owned(),
        name: str_of(value, "name").map(str::to_owned),
    })
}

fn email_json(e: &ExchangeEmail) -> Value {
    match &e.name {
        Some(name) => json!({ "address": e.address, "name": name }),
        None => json!({ "address": e.address }),
    }
}

fn importance_name(i: Importance) -> &'static str {
    match i {
        Importance::Low => "low",
        Importance::Normal => "normal",
        Importance::High => "high",
    }
}

/// A Graph `patternedRecurrence` as the contract's recurrence.
pub fn recurrence_from_graph(value: Option<&Value>) -> Option<ExchangeRecurrence> {
    let value = value.filter(|v| !v.is_null())?;
    let pattern = value.get("pattern")?;
    let range = value.get("range")?;
    let positive = |key: &str| {
        pattern
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
    };
    let pattern = ExchangeRecurrencePattern {
        kind: decode(pattern.get("type"))?,
        interval: positive("interval").unwrap_or(1),
        days_of_week: pattern
            .get("daysOfWeek")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|d| decode(Some(d))).collect())
            .unwrap_or_default(),
        day_of_month: positive("dayOfMonth"),
        month: positive("month"),
        index: decode(pattern.get("index")),
        first_day_of_week: decode(pattern.get("firstDayOfWeek")),
    };
    let start_date = str_of(range, "startDate")?.to_owned();
    let range_kind = str_of(range, "type").unwrap_or("noEnd");
    let range_value = match range_kind {
        "endDate" => match str_of(range, "endDate").filter(|d| !d.starts_with("0001")) {
            Some(end) => ExchangeRecurrenceRange::EndDate {
                start_date,
                end_date: end.to_owned(),
            },
            None => ExchangeRecurrenceRange::NoEnd { start_date },
        },
        "numbered" => ExchangeRecurrenceRange::Numbered {
            start_date,
            occurrences: range
                .get("numberOfOccurrences")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(1)
                .max(1),
        },
        _ => ExchangeRecurrenceRange::NoEnd { start_date },
    };
    Some(ExchangeRecurrence {
        pattern,
        range: range_value,
        time_zone: str_of(range, "recurrenceTimeZone").map(str::to_owned),
    })
}

/// The contract's recurrence as a Graph `patternedRecurrence`.
pub fn recurrence_to_graph(r: &ExchangeRecurrence) -> Value {
    let p = &r.pattern;
    let mut pattern = json!({
        "type": p.kind,
        "interval": p.interval.max(1),
    });
    if !p.days_of_week.is_empty() {
        pattern["daysOfWeek"] = json!(p.days_of_week);
    }
    if let Some(d) = p.day_of_month {
        pattern["dayOfMonth"] = json!(d);
    }
    if let Some(m) = p.month {
        pattern["month"] = json!(m);
    }
    if let Some(i) = p.index {
        pattern["index"] = json!(i);
    }
    if let Some(f) = p.first_day_of_week {
        pattern["firstDayOfWeek"] = json!(f);
    }
    let mut range = match &r.range {
        ExchangeRecurrenceRange::EndDate {
            start_date,
            end_date,
        } => json!({"type": "endDate", "startDate": start_date, "endDate": end_date}),
        ExchangeRecurrenceRange::Numbered {
            start_date,
            occurrences,
        } => {
            json!({"type": "numbered", "startDate": start_date, "numberOfOccurrences": occurrences})
        }
        ExchangeRecurrenceRange::NoEnd { start_date } => {
            json!({"type": "noEnd", "startDate": start_date})
        }
    };
    if let Some(zone) = &r.time_zone {
        range["recurrenceTimeZone"] = json!(zone);
    }
    json!({ "pattern": pattern, "range": range })
}

/// A Graph `event` as the contract's event (exceptions are added by the
/// caller). `None` for entries without times.
pub fn event_from_graph(value: &Value) -> Option<ExchangeEvent> {
    let start = date_time(value.get("start"))?;
    let end = date_time(value.get("end"))?;
    let organizer = email(value.get("organizer"));
    let organizer_address = organizer.as_ref().map(|o| o.address.to_ascii_lowercase());
    let attendees = value
        .get("attendees")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let email = email(Some(x))?;
                    if Some(email.address.to_ascii_lowercase()) == organizer_address {
                        return None;
                    }
                    let proposed = x
                        .get("proposedNewTime")
                        .and_then(|p| Some((date_time(p.get("start"))?, date_time(p.get("end"))?)));
                    Some(ExchangeAttendee {
                        email,
                        kind: decode(x.get("type")).unwrap_or(AttendeeKind::Required),
                        response: decode(x.pointer("/status/response")).unwrap_or_default(),
                        proposed,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut locations: Vec<ExchangeLocation> = value
        .get("locations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let name = str_of(l, "displayName")?;
                    Some(ExchangeLocation {
                        display_name: name.to_owned(),
                        email: str_of(l, "locationEmailAddress").map(str::to_owned),
                        room: str_of(l, "locationType") == Some("conferenceRoom"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if locations.is_empty()
        && let Some(name) = value.get("location").and_then(|l| str_of(l, "displayName"))
    {
        locations.push(ExchangeLocation {
            display_name: name.to_owned(),
            email: value
                .get("location")
                .and_then(|l| str_of(l, "locationEmailAddress"))
                .map(str::to_owned),
            room: value
                .get("location")
                .and_then(|l| str_of(l, "locationType"))
                == Some("conferenceRoom"),
        });
    }
    let reminder_on = value
        .get("isReminderOn")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cancelled_occurrences = string_list(value, "cancelledOccurrences")
        .iter()
        .filter_map(|o| {
            let date = o.get(o.len().saturating_sub(10)..)?;
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .ok()
                .map(|_| date.to_owned())
        })
        .collect();
    Some(ExchangeEvent {
        remote_id: id_of(value).to_owned(),
        change_key: str_of(value, "changeKey").map(str::to_owned),
        uid: str_of(value, "uid")
            .or_else(|| str_of(value, "iCalUId"))
            .map(str::to_owned),
        kind: decode(value.get("type")).unwrap_or_default(),
        series_master_id: str_of(value, "seriesMasterId").map(str::to_owned),
        original_start: str_of(value, "originalStart")
            .and_then(text::to_utc)
            .map(|t| ExchangeDateTime::utc(text::trim_fraction(&t))),
        subject: str_of(value, "subject").unwrap_or_default().to_owned(),
        body: body_of(value.get("body")),
        start,
        end,
        is_all_day: value
            .get("isAllDay")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        locations,
        show_as: decode(value.get("showAs")).unwrap_or(ShowAs::Busy),
        sensitivity: decode(value.get("sensitivity")).unwrap_or(Sensitivity::Normal),
        importance: decode(value.get("importance")),
        reminder_minutes: if reminder_on {
            value
                .get("reminderMinutesBeforeStart")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
        } else {
            None
        },
        organizer,
        attendees,
        is_organizer: value
            .get("isOrganizer")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        response_requested: value
            .get("responseRequested")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        my_response: decode(value.pointer("/responseStatus/response")).unwrap_or_default(),
        is_cancelled: value
            .get("isCancelled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        online_meeting_url: value
            .pointer("/onlineMeeting/joinUrl")
            .and_then(Value::as_str)
            .or_else(|| str_of(value, "onlineMeetingUrl"))
            .filter(|u| !u.is_empty())
            .map(str::to_owned),
        categories: string_list(value, "categories"),
        recurrence: recurrence_from_graph(value.get("recurrence")),
        cancelled_occurrences,
        exceptions: Vec::new(),
        last_modified: str_of(value, "lastModifiedDateTime").and_then(text::to_utc),
    })
}

/// What an update of a meeting the user does not organize may change: the
/// own copy (reminder, categories, notes, show-as, …), not the attendees,
/// the response request or the series (ADR 0078).
pub fn event_update_to_graph(e: &ExchangeEvent) -> Value {
    let mut out = event_to_graph(e);
    if !e.is_organizer
        && !e.attendees.is_empty()
        && let Some(map) = out.as_object_mut()
    {
        for key in ["attendees", "responseRequested", "recurrence"] {
            map.remove(key);
        }
    }
    out
}

/// The writable parts of the contract's event as a Graph `event`.
pub fn event_to_graph(e: &ExchangeEvent) -> Value {
    let mut out = json!({
        "subject": e.subject,
        "start": date_time_json(&e.start),
        "end": date_time_json(&e.end),
        "isAllDay": e.is_all_day,
        "showAs": e.show_as,
        "sensitivity": e.sensitivity,
        "isReminderOn": e.reminder_minutes.is_some(),
        "categories": e.categories,
        "responseRequested": e.response_requested,
        "attendees": e.attendees.iter().map(|a| json!({
            "type": a.kind,
            "emailAddress": email_json(&a.email),
        })).collect::<Vec<_>>(),
    });
    if let Some(minutes) = e.reminder_minutes {
        out["reminderMinutesBeforeStart"] = json!(minutes);
    }
    if let Some(body) = &e.body {
        out["body"] = body_json(body);
    }
    if let Some(i) = e.importance {
        out["importance"] = json!(importance_name(i));
    }
    let locations: Vec<Value> = e
        .locations
        .iter()
        .map(|l| {
            let mut v = json!({ "displayName": l.display_name });
            if let Some(address) = &l.email {
                v["locationEmailAddress"] = json!(address);
            }
            if l.room {
                v["locationType"] = json!("conferenceRoom");
            }
            v
        })
        .collect();
    if let Some(first) = locations.first() {
        out["location"] = first.clone();
    }
    out["locations"] = Value::Array(locations);
    if let Some(r) = &e.recurrence {
        out["recurrence"] = recurrence_to_graph(r);
    }
    out
}

/// A Graph `todoTask` (with its checklist items and linked resources) as
/// the contract's task; `list` is its To Do list.
pub fn task_from_graph(
    list: &str,
    value: &Value,
    checklist: &[Value],
    links: &[Value],
) -> ExchangeTask {
    let reminder_on = value
        .get("isReminderOn")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ExchangeTask {
        remote_id: todo_task_id(list, id_of(value)),
        change_key: str_of(value, "@odata.etag").map(str::to_owned),
        title: str_of(value, "title").unwrap_or_default().to_owned(),
        body: body_of(value.get("body")),
        status: decode(value.get("status")).unwrap_or(ExchangeTaskStatus::NotStarted),
        importance: decode(value.get("importance")),
        start: date_time(value.get("startDateTime")),
        due: date_time(value.get("dueDateTime")),
        completed: date_time(value.get("completedDateTime")),
        reminder: if reminder_on {
            date_time(value.get("reminderDateTime"))
        } else {
            None
        },
        recurrence: recurrence_from_graph(value.get("recurrence")),
        categories: string_list(value, "categories"),
        checklist: checklist
            .iter()
            .map(|c| ChecklistItem {
                id: id_of(c).to_owned(),
                title: str_of(c, "displayName").unwrap_or_default().to_owned(),
                checked: c.get("isChecked").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),
        linked_resources: links
            .iter()
            .map(|l| LinkedResource {
                id: str_of(l, "id").map(str::to_owned),
                web_url: str_of(l, "webUrl").map(str::to_owned),
                application: str_of(l, "applicationName").map(str::to_owned),
                display_name: str_of(l, "displayName").map(str::to_owned),
                external_id: str_of(l, "externalId").map(str::to_owned),
            })
            .collect(),
        created: str_of(value, "createdDateTime").and_then(text::to_utc),
        last_modified: str_of(value, "lastModifiedDateTime").and_then(text::to_utc),
    }
}

/// The writable parts of the contract's task as a Graph `todoTask`
/// (checklist items are written separately).
pub fn task_to_graph(t: &ExchangeTask) -> Value {
    let status = serde_json::to_value(t.status).unwrap_or_else(|_| json!("notStarted"));
    let mut out = json!({
        "title": t.title,
        "status": status,
        "categories": t.categories,
        "isReminderOn": t.reminder.is_some(),
    });
    if let Some(body) = &t.body {
        out["body"] = body_json(body);
    }
    if let Some(i) = t.importance {
        out["importance"] = json!(importance_name(i));
    }
    for (key, time) in [
        ("startDateTime", &t.start),
        ("dueDateTime", &t.due),
        ("completedDateTime", &t.completed),
        ("reminderDateTime", &t.reminder),
    ] {
        out[key] = time.as_ref().map_or(Value::Null, date_time_json);
    }
    if let Some(r) = &t.recurrence {
        out["recurrence"] = recurrence_to_graph(r);
    }
    out
}

fn postal(kind: &str, value: Option<&Value>) -> Option<ExchangePostalAddress> {
    let v = value.filter(|v| !v.is_null())?;
    let field = |k: &str| str_of(v, k).map(str::to_owned);
    let address = ExchangePostalAddress {
        kind: kind.to_owned(),
        street: field("street"),
        city: field("city"),
        state: field("state"),
        postal_code: field("postalCode"),
        country: field("countryOrRegion"),
    };
    (address.street.is_some()
        || address.city.is_some()
        || address.state.is_some()
        || address.postal_code.is_some()
        || address.country.is_some())
    .then_some(address)
}

/// A Graph `contact` as the contract's contact.
pub fn contact_from_graph(value: &Value) -> ExchangeContact {
    let field = |k: &str| str_of(value, k).map(str::to_owned);
    let mut phones = Vec::new();
    for (kind, key) in [("home", "homePhones"), ("business", "businessPhones")] {
        for number in string_list(value, key) {
            phones.push(ExchangePhone {
                kind: kind.to_owned(),
                number,
            });
        }
    }
    if let Some(mobile) = field("mobilePhone") {
        phones.push(ExchangePhone {
            kind: "mobile".to_owned(),
            number: mobile,
        });
    }
    ExchangeContact {
        remote_id: id_of(value).to_owned(),
        change_key: field("changeKey"),
        display_name: field("displayName").unwrap_or_default(),
        given_name: field("givenName"),
        surname: field("surname"),
        middle_name: field("middleName"),
        nickname: field("nickName"),
        company: field("companyName"),
        department: field("department"),
        job_title: field("jobTitle"),
        emails: value
            .get("emailAddresses")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let address = str_of(e, "address")?;
                        Some(ExchangeEmail {
                            address: address.to_owned(),
                            name: str_of(e, "name").map(str::to_owned),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        phones,
        addresses: [
            ("home", "homeAddress"),
            ("business", "businessAddress"),
            ("other", "otherAddress"),
        ]
        .iter()
        .filter_map(|(kind, key)| postal(kind, value.get(*key)))
        .collect(),
        birthday: field("birthday").and_then(|b| b.get(..10).map(str::to_owned)),
        notes: field("personalNotes"),
        urls: field("businessHomePage").into_iter().collect(),
        categories: string_list(value, "categories"),
        is_group: false,
        members: Vec::new(),
        photo: None,
    }
}

/// The contract's contact as a Graph `contact`. Graph has no fax or
/// "other" phone fields and one web page: those are left out.
pub fn contact_to_graph(c: &ExchangeContact) -> Value {
    let phones = |kind: &str| -> Vec<&str> {
        c.phones
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| p.number.as_str())
            .collect()
    };
    let mut out = json!({
        "displayName": c.display_name,
        "givenName": c.given_name,
        "surname": c.surname,
        "middleName": c.middle_name,
        "nickName": c.nickname,
        "companyName": c.company,
        "department": c.department,
        "jobTitle": c.job_title,
        "emailAddresses": c.emails.iter().map(email_json).collect::<Vec<_>>(),
        "homePhones": phones("home"),
        "businessPhones": phones("business"),
        "mobilePhone": phones("mobile").first(),
        "personalNotes": c.notes,
        "businessHomePage": c.urls.first(),
        "categories": c.categories,
        "birthday": c.birthday.as_ref().map(|b| format!("{b}T11:59:00Z")),
    });
    for (kind, key) in [
        ("home", "homeAddress"),
        ("business", "businessAddress"),
        ("other", "otherAddress"),
    ] {
        if let Some(a) = c.addresses.iter().find(|a| a.kind == kind) {
            out[key] = json!({
                "street": a.street, "city": a.city, "state": a.state,
                "postalCode": a.postal_code, "countryOrRegion": a.country,
            });
        }
    }
    out
}

fn response_action(response: InvitationResponse) -> &'static str {
    match response {
        InvitationResponse::Accept => "accept",
        InvitationResponse::Tentative => "tentativelyAccept",
        InvitationResponse::Decline => "decline",
    }
}

impl GraphEngine {
    async fn list_all<T: DeserializeOwned>(
        &self,
        first: Page<T>,
        what: &str,
    ) -> Result<Vec<T>, GraphError> {
        let mut page = first;
        let mut out = Vec::new();
        for _ in 0..MAX_LIST_PAGES {
            out.append(&mut page.value);
            match page.next_link.take() {
                Some(next) => page = self.client.get_link_json(&next, &[], what).await?,
                None => break,
            }
        }
        Ok(out)
    }

    /// The calendars, contact folders and (own mailbox) To Do lists.
    pub async fn list_pim_folders(
        &self,
        mailbox: Option<&str>,
    ) -> Result<Vec<ExchangePimFolder>, GraphError> {
        let base = mailbox_path(mailbox);
        let mut requests = vec![
            BatchRequest::get(format!("{base}/calendars?$top=100")),
            BatchRequest::get(format!("{base}/contactFolders?$top=100")),
            BatchRequest::get(format!("{base}/contacts?$top=1&$select=parentFolderId")),
        ];
        if mailbox.is_none() {
            requests.push(BatchRequest::get("/me/todo/lists?$top=100"));
        }
        let answers = self.client.batch(&requests).await?;
        // Another mailbox may share mail but not its calendar or contacts.
        let tolerate = |e: &GraphError| {
            mailbox.is_some() && matches!(e, GraphError::Forbidden(_) | GraphError::NotFound(_))
        };
        let mut out = Vec::new();
        match answers[0].json::<Page<Calendar>>("GET calendars") {
            Ok(page) => {
                for c in self.list_all(page, "GET calendars").await? {
                    out.push(ExchangePimFolder {
                        role: (c.is_default_calendar == Some(true))
                            .then_some(ExchangeCollectionRole::DefaultCalendar),
                        remote_id: c.id,
                        change_key: c.change_key,
                        name: c.name,
                        kind: PimFolderKind::Calendar,
                        todo: false,
                        read_only: c.can_edit == Some(false),
                        color: calendar_color(c.color.as_deref()),
                    });
                }
            }
            Err(e) if tolerate(&e) => {}
            Err(e) => return Err(e),
        }
        let folders = match answers[1].json::<Page<ContactFolder>>("GET contactFolders") {
            Ok(page) => Some(self.list_all(page, "GET contactFolders").await?),
            Err(e) if tolerate(&e) => None,
            Err(e) => return Err(e),
        };
        if let Some(folders) = folders {
            let from_contact = answers[2]
                .json::<Page<Value>>("GET contacts")
                .ok()
                .and_then(|p| {
                    p.value
                        .first()
                        .and_then(|c| str_of(c, "parentFolderId").map(str::to_owned))
                });
            let default_id = from_contact
                .or_else(|| folders.iter().find_map(|f| f.parent_folder_id.clone()))
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| DEFAULT_CONTACTS.to_owned());
            out.push(ExchangePimFolder {
                remote_id: default_id,
                change_key: None,
                name: String::new(),
                kind: PimFolderKind::Contacts,
                role: Some(ExchangeCollectionRole::DefaultContacts),
                todo: false,
                read_only: false,
                color: None,
            });
            for f in folders {
                out.push(ExchangePimFolder {
                    remote_id: f.id,
                    change_key: None,
                    name: f.display_name.unwrap_or_default(),
                    kind: PimFolderKind::Contacts,
                    role: None,
                    todo: false,
                    read_only: false,
                    color: None,
                });
            }
        }
        if let Some(answer) = answers.get(3) {
            let page: Page<TodoTaskList> = answer.json("GET todo/lists")?;
            for list in self.list_all(page, "GET todo/lists").await? {
                let role = match list.wellknown_list_name.as_deref() {
                    Some("defaultList") => Some(ExchangeCollectionRole::DefaultTasks),
                    Some("flaggedEmails") => Some(ExchangeCollectionRole::FlaggedEmails),
                    _ => None,
                };
                out.push(ExchangePimFolder {
                    read_only: role == Some(ExchangeCollectionRole::FlaggedEmails),
                    remote_id: list.id,
                    change_key: list.etag,
                    name: list.display_name,
                    kind: PimFolderKind::Tasks,
                    role,
                    todo: true,
                    color: None,
                });
            }
        }
        Ok(out)
    }

    /// Item changes of a PIM folder since `cursor`.
    pub async fn pim_delta(
        &self,
        mailbox: Option<&str>,
        folder: &ExchangePimFolder,
        cursor: Option<&str>,
    ) -> Result<PimDeltaOutcome, GraphError> {
        if folder.kind == PimFolderKind::Contacts && folder.remote_id == DEFAULT_CONTACTS {
            return self.contacts_listing(mailbox).await;
        }
        if folder.kind == PimFolderKind::Tasks && mailbox.is_some() {
            return Err(GraphError::Unsupported(
                "To Do lists of other mailboxes".into(),
            ));
        }
        if let Some(c) = cursor
            && !url::Url::parse(c).is_ok_and(|u| self.client.same_origin(&u))
        {
            return Ok(PimDeltaOutcome::CursorExpired);
        }
        let base = mailbox_path(mailbox);
        let id = path_segment(&folder.remote_id);
        let (path, query) = match folder.kind {
            PimFolderKind::Calendar => {
                let now = chrono::Utc::now();
                let from = now - chrono::Duration::days(i64::from(self.options.calendar_past_days));
                let to = now + chrono::Duration::days(i64::from(self.options.calendar_future_days));
                (
                    format!("{base}/calendars/{id}/calendarView/delta"),
                    vec![
                        (
                            "startDateTime",
                            from.format("%Y-%m-%dT00:00:00Z").to_string(),
                        ),
                        ("endDateTime", to.format("%Y-%m-%dT00:00:00Z").to_string()),
                    ],
                )
            }
            PimFolderKind::Contacts => (
                format!("{base}/contactFolders/{id}/contacts/delta"),
                Vec::new(),
            ),
            PimFolderKind::Tasks => (format!("/me/todo/lists/{id}/tasks/delta"), Vec::new()),
        };
        let headers = [self.page_header()];
        let mut entries: Vec<Value> = Vec::new();
        let mut link = cursor.map(str::to_owned);
        let mut next_cursor: Option<(String, bool)> = None;
        for page_no in 0..self.options.pages_per_call.max(1) {
            let page: Result<Page<Value>, GraphError> = match &link {
                Some(l) => self.client.get_link_json(l, &headers, "PIM delta").await,
                None => {
                    self.client
                        .get_json(&path, &query, &headers, "PIM delta")
                        .await
                }
            };
            let mut page = match page {
                Ok(p) => p,
                Err(GraphError::SyncStateExpired(_)) if cursor.is_some() || page_no > 0 => {
                    return Ok(PimDeltaOutcome::CursorExpired);
                }
                Err(e) => return Err(e),
            };
            entries.append(&mut page.value);
            match (page.next_link, page.delta_link) {
                (Some(next), _) => link = Some(next),
                (None, Some(delta_link)) => {
                    next_cursor = Some((delta_link, false));
                    break;
                }
                (None, None) => {
                    return Err(GraphError::Protocol(
                        "PIM delta without a next or delta link".into(),
                    ));
                }
            }
        }
        let (cursor, more) = next_cursor.unwrap_or_else(|| (link.unwrap_or_default(), true));
        let mut delta = PimDelta {
            cursor,
            more,
            ..PimDelta::default()
        };
        match folder.kind {
            PimFolderKind::Calendar => self.fold_events(mailbox, entries, &mut delta).await?,
            PimFolderKind::Contacts => {
                for entry in entries {
                    if is_removed(&entry) {
                        delta.removed.push(id_of(&entry).to_owned());
                    } else {
                        delta.upserted.push(ExchangePimItem::Contact(Box::new(
                            contact_from_graph(&entry),
                        )));
                    }
                }
            }
            PimFolderKind::Tasks => {
                self.fold_tasks(&folder.remote_id, entries, &mut delta)
                    .await?;
            }
        }
        Ok(PimDeltaOutcome::Delta(delta))
    }

    async fn contacts_listing(&self, mailbox: Option<&str>) -> Result<PimDeltaOutcome, GraphError> {
        let first: Page<Value> = self
            .client
            .get_json(
                &format!("{}/contacts", mailbox_path(mailbox)),
                &[("$top", "100".to_owned())],
                &[],
                "GET contacts",
            )
            .await?;
        let contacts = self.list_all(first, "GET contacts").await?;
        Ok(PimDeltaOutcome::Delta(PimDelta {
            cursor: LISTING_CURSOR.to_owned(),
            upserted: contacts
                .iter()
                .map(|c| ExchangePimItem::Contact(Box::new(contact_from_graph(c))))
                .collect(),
            removed: Vec::new(),
            more: false,
        }))
    }

    /// Reads the changed events of a calendar delta in full: singles and
    /// series masters in their own zone, masters with their exceptions.
    async fn fold_events(
        &self,
        mailbox: Option<&str>,
        entries: Vec<Value>,
        delta: &mut PimDelta,
    ) -> Result<(), GraphError> {
        // id → zone hint (the zone the event was created in).
        let mut wanted: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut masters: BTreeSet<String> = BTreeSet::new();
        for entry in &entries {
            let id = id_of(entry).to_owned();
            if is_removed(entry) {
                delta.removed.push(id);
                continue;
            }
            let zone = str_of(entry, "originalStartTimeZone").map(str::to_owned);
            match str_of(entry, "type").unwrap_or("singleInstance") {
                "occurrence" | "exception" => {
                    if let Some(master) = str_of(entry, "seriesMasterId") {
                        masters.insert(master.to_owned());
                        wanted.entry(master.to_owned()).or_insert(zone);
                    }
                }
                "seriesMaster" => {
                    masters.insert(id.clone());
                    wanted.insert(id, zone);
                }
                _ => {
                    wanted.insert(id, zone);
                }
            }
        }
        if wanted.is_empty() {
            return Ok(());
        }
        let base = mailbox_path(mailbox);
        let requests: Vec<BatchRequest> = wanted
            .iter()
            .map(|(id, zone)| {
                let mut r = BatchRequest::get(format!("{base}/events/{}", path_segment(id)));
                if let Some((name, value)) = zone_header(zone.as_deref()) {
                    r = r.header(&name, value);
                }
                r
            })
            .collect();
        let answers = self.client.batch(&requests).await?;
        let now = chrono::Utc::now();
        let from = (now - chrono::Duration::days(i64::from(self.options.calendar_past_days)))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
        let to = (now + chrono::Duration::days(i64::from(self.options.calendar_future_days)))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
        for ((id, zone), answer) in wanted.iter().zip(answers) {
            let value: Value = match answer.json("GET event") {
                Ok(v) => v,
                Err(GraphError::NotFound(_)) => {
                    delta.removed.push(id.clone());
                    continue;
                }
                Err(e) => return Err(e),
            };
            let Some(mut event) = event_from_graph(&value) else {
                continue;
            };
            if masters.contains(id) || event.kind == EventKind::SeriesMaster {
                let zone = zone
                    .clone()
                    .or_else(|| str_of(&value, "originalStartTimeZone").map(str::to_owned));
                event.exceptions = self
                    .instances(mailbox, id, &from, &to, zone.as_deref())
                    .await?
                    .into_iter()
                    .filter(|i| i.kind == EventKind::Exception)
                    .collect();
            }
            delta.upserted.push(ExchangePimItem::Event(Box::new(event)));
        }
        Ok(())
    }

    async fn instances(
        &self,
        mailbox: Option<&str>,
        master: &str,
        from: &str,
        to: &str,
        zone: Option<&str>,
    ) -> Result<Vec<ExchangeEvent>, GraphError> {
        let mut headers = vec![self.page_header()];
        if let Some((_, value)) = zone_header(zone) {
            headers[0].1 = format!("{}, {value}", headers[0].1);
        }
        let first: Page<Value> = self
            .client
            .get_json(
                &format!(
                    "{}/events/{}/instances",
                    mailbox_path(mailbox),
                    path_segment(master)
                ),
                &[
                    ("startDateTime", from.to_owned()),
                    ("endDateTime", to.to_owned()),
                ],
                &headers,
                "GET events/instances",
            )
            .await?;
        let mut page = first;
        let mut out = Vec::new();
        for _ in 0..MAX_LIST_PAGES * 5 {
            out.extend(page.value.iter().filter_map(event_from_graph));
            match page.next_link.take() {
                Some(next) => {
                    page = self
                        .client
                        .get_link_json(&next, &headers, "GET events/instances")
                        .await?;
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// The occurrences and exceptions of a series in a range (RFC 3339
    /// UTC), in the series' zone if given.
    pub async fn event_instances(
        &self,
        mailbox: Option<&str>,
        master: &str,
        from: &str,
        to: &str,
        zone: Option<&str>,
    ) -> Result<Vec<ExchangeEvent>, GraphError> {
        self.instances(mailbox, master, from, to, zone).await
    }

    async fn fold_tasks(
        &self,
        list: &str,
        entries: Vec<Value>,
        delta: &mut PimDelta,
    ) -> Result<(), GraphError> {
        let mut tasks = Vec::new();
        for entry in entries {
            if is_removed(&entry) {
                delta.removed.push(todo_task_id(list, id_of(&entry)));
            } else {
                tasks.push(entry);
            }
        }
        if tasks.is_empty() {
            return Ok(());
        }
        let base = format!("/me/todo/lists/{}/tasks", path_segment(list));
        let requests: Vec<BatchRequest> = tasks
            .iter()
            .flat_map(|t| {
                let task = path_segment(id_of(t));
                [
                    BatchRequest::get(format!("{base}/{task}/checklistItems")),
                    BatchRequest::get(format!("{base}/{task}/linkedResources")),
                ]
            })
            .collect();
        let answers = self.client.batch(&requests).await?;
        for (task, parts) in tasks.iter().zip(answers.chunks(2)) {
            let items = |i: usize| -> Result<Vec<Value>, GraphError> {
                match parts[i].json::<Page<Value>>("GET task parts") {
                    Ok(p) => Ok(p.value),
                    Err(GraphError::NotFound(_)) => Ok(Vec::new()),
                    Err(e) => Err(e),
                }
            };
            let checklist = items(0)?;
            let links = items(1)?;
            delta
                .upserted
                .push(ExchangePimItem::Task(Box::new(task_from_graph(
                    list, task, &checklist, &links,
                ))));
        }
        Ok(())
    }

    /// Replays a queued PIM write.
    pub async fn replay_pim(
        &self,
        mailbox: Option<&str>,
        write: &ExchangePimWrite,
    ) -> Result<ExchangePimWriteOutcome, GraphError> {
        let result = match write {
            ExchangePimWrite::Create { folder, item, .. } => {
                self.create_pim(mailbox, folder, item).await
            }
            ExchangePimWrite::Update { item, .. } => self.update_pim(mailbox, item).await,
            ExchangePimWrite::Delete { remote_id, send } => self
                .delete_pim(mailbox, remote_id, *send)
                .await
                .map(|()| ExchangePimWriteOutcome::Done {
                    remote_id: remote_id.clone(),
                    change_key: None,
                }),
        };
        match result {
            Err(GraphError::NotFound(_)) => Ok(ExchangePimWriteOutcome::Gone),
            Err(GraphError::Conflict(_)) => Ok(ExchangePimWriteOutcome::Conflict),
            other => other,
        }
    }

    fn done(answer: &Value, fallback_id: &str) -> ExchangePimWriteOutcome {
        ExchangePimWriteOutcome::Done {
            remote_id: str_of(answer, "id").unwrap_or(fallback_id).to_owned(),
            change_key: str_of(answer, "changeKey")
                .or_else(|| str_of(answer, "@odata.etag"))
                .map(str::to_owned),
        }
    }

    async fn create_pim(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        item: &ExchangePimItem,
    ) -> Result<ExchangePimWriteOutcome, GraphError> {
        let base = mailbox_path(mailbox);
        match item {
            ExchangePimItem::Event(event) => {
                let created = self
                    .client
                    .send_json(
                        Method::POST,
                        &format!("{base}/calendars/{}/events", path_segment(folder)),
                        Some(&event_to_graph(event)),
                        &[],
                        false,
                        "POST events",
                    )
                    .await?;
                let id = str_of(&created, "id")
                    .ok_or_else(|| GraphError::Protocol("events answered no id".into()))?
                    .to_owned();
                if event.recurrence.is_some()
                    && (!event.exceptions.is_empty() || !event.cancelled_occurrences.is_empty())
                {
                    self.apply_exceptions(mailbox, &id, event).await?;
                    return self.reread_event(mailbox, &id).await;
                }
                Ok(Self::done(&created, &id))
            }
            ExchangePimItem::Contact(contact) => {
                let path = if folder == DEFAULT_CONTACTS {
                    format!("{base}/contacts")
                } else {
                    format!("{base}/contactFolders/{}/contacts", path_segment(folder))
                };
                let created = self
                    .client
                    .send_json(
                        Method::POST,
                        &path,
                        Some(&contact_to_graph(contact)),
                        &[],
                        false,
                        "POST contacts",
                    )
                    .await?;
                Ok(Self::done(&created, ""))
            }
            ExchangePimItem::Task(task) => {
                if mailbox.is_some() {
                    return Err(GraphError::Unsupported(
                        "To Do lists of other mailboxes".into(),
                    ));
                }
                let created = self
                    .client
                    .send_json(
                        Method::POST,
                        &format!("/me/todo/lists/{}/tasks", path_segment(folder)),
                        Some(&task_to_graph(task)),
                        &[],
                        false,
                        "POST todo tasks",
                    )
                    .await?;
                let task_id = str_of(&created, "id")
                    .ok_or_else(|| GraphError::Protocol("tasks answered no id".into()))?
                    .to_owned();
                self.sync_checklist(folder, &task_id, &task.checklist)
                    .await?;
                let mut outcome = Self::done(&created, &task_id);
                if let ExchangePimWriteOutcome::Done { remote_id, .. } = &mut outcome {
                    *remote_id = todo_task_id(folder, &task_id);
                }
                Ok(outcome)
            }
        }
    }

    /// The event's id and change key as the server has them now.
    pub(crate) async fn reread_event(
        &self,
        mailbox: Option<&str>,
        id: &str,
    ) -> Result<ExchangePimWriteOutcome, GraphError> {
        let value: Value = self
            .client
            .get_json(
                &format!("{}/events/{}", mailbox_path(mailbox), path_segment(id)),
                &[("$select", "id,changeKey".to_owned())],
                &[],
                "GET event",
            )
            .await?;
        Ok(Self::done(&value, id))
    }

    /// Writes the exceptions and cancelled occurrences of a series: an
    /// exception with a remote id is patched directly, one without is
    /// matched to its occurrence by original start date.
    async fn apply_exceptions(
        &self,
        mailbox: Option<&str>,
        master: &str,
        event: &ExchangeEvent,
    ) -> Result<(), GraphError> {
        let base = mailbox_path(mailbox);
        let dates: Vec<&str> = event
            .exceptions
            .iter()
            .filter(|x| x.remote_id.is_empty())
            .filter_map(|x| x.original_start.as_ref().map(ExchangeDateTime::date))
            .chain(event.cancelled_occurrences.iter().map(String::as_str))
            .collect();
        let mut by_date: BTreeMap<String, String> = BTreeMap::new();
        if let (Some(first), Some(last)) = (dates.iter().min(), dates.iter().max()) {
            let from = format!("{first}T00:00:00Z");
            let to = chrono::NaiveDate::parse_from_str(last, "%Y-%m-%d")
                .map(|d| {
                    (d + chrono::Duration::days(2))
                        .format("%Y-%m-%dT00:00:00Z")
                        .to_string()
                })
                .map_err(|_| GraphError::Rejected(format!("occurrence date {last}")))?;
            for occurrence in self.instances(mailbox, master, &from, &to, None).await? {
                if let Some(start) = &occurrence.original_start {
                    by_date.insert(start.date().to_owned(), occurrence.remote_id.clone());
                }
                by_date
                    .entry(occurrence.start.date().to_owned())
                    .or_insert(occurrence.remote_id);
            }
        }
        for exception in &event.exceptions {
            let target = if exception.remote_id.is_empty() {
                exception
                    .original_start
                    .as_ref()
                    .and_then(|s| by_date.get(s.date()).cloned())
            } else {
                Some(exception.remote_id.clone())
            };
            let Some(target) = target else {
                tracing::warn!("no occurrence for an exception of a series");
                continue;
            };
            let mut body = event_to_graph(exception);
            if let Some(o) = body.as_object_mut() {
                o.remove("recurrence");
            }
            self.client
                .send_json(
                    Method::PATCH,
                    &format!("{base}/events/{}", path_segment(&target)),
                    Some(&body),
                    &[],
                    true,
                    "PATCH occurrence",
                )
                .await?;
        }
        for date in &event.cancelled_occurrences {
            if let Some(target) = by_date.get(date.as_str()) {
                match self
                    .client
                    .send_json(
                        Method::DELETE,
                        &format!("{base}/events/{}", path_segment(target)),
                        None,
                        &[],
                        true,
                        "DELETE occurrence",
                    )
                    .await
                {
                    Ok(_) | Err(GraphError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    async fn update_pim(
        &self,
        mailbox: Option<&str>,
        item: &ExchangePimItem,
    ) -> Result<ExchangePimWriteOutcome, GraphError> {
        let base = mailbox_path(mailbox);
        match item {
            ExchangePimItem::Event(event) => {
                let answer = self
                    .client
                    .send_json(
                        Method::PATCH,
                        &format!("{base}/events/{}", path_segment(&event.remote_id)),
                        Some(&event_update_to_graph(event)),
                        &if_match(event.change_key.as_deref()),
                        true,
                        "PATCH event",
                    )
                    .await?;
                if !event.exceptions.is_empty() || !event.cancelled_occurrences.is_empty() {
                    self.apply_exceptions(mailbox, &event.remote_id, event)
                        .await?;
                    return self.reread_event(mailbox, &event.remote_id).await;
                }
                Ok(Self::done(&answer, &event.remote_id))
            }
            ExchangePimItem::Contact(contact) => {
                let answer = self
                    .client
                    .send_json(
                        Method::PATCH,
                        &format!("{base}/contacts/{}", path_segment(&contact.remote_id)),
                        Some(&contact_to_graph(contact)),
                        &if_match(contact.change_key.as_deref()),
                        true,
                        "PATCH contact",
                    )
                    .await?;
                Ok(Self::done(&answer, &contact.remote_id))
            }
            ExchangePimItem::Task(task) => {
                let (list, task_id) = split_todo_task_id(&task.remote_id)
                    .ok_or_else(|| GraphError::NotFound(task.remote_id.clone()))?;
                let headers = task
                    .change_key
                    .as_deref()
                    .map(|k| vec![("If-Match", k.to_owned())])
                    .unwrap_or_default();
                let answer = self
                    .client
                    .send_json(
                        Method::PATCH,
                        &format!(
                            "/me/todo/lists/{}/tasks/{}",
                            path_segment(list),
                            path_segment(task_id)
                        ),
                        Some(&task_to_graph(task)),
                        &headers,
                        true,
                        "PATCH todo task",
                    )
                    .await?;
                self.sync_checklist(list, task_id, &task.checklist).await?;
                let mut outcome = Self::done(&answer, task_id);
                if let ExchangePimWriteOutcome::Done { remote_id, .. } = &mut outcome {
                    *remote_id = todo_task_id(list, task_id);
                }
                Ok(outcome)
            }
        }
    }

    /// Makes a task's checklist on the server match `wanted` (items with
    /// a known id are patched, others created, missing ones deleted).
    async fn sync_checklist(
        &self,
        list: &str,
        task: &str,
        wanted: &[ChecklistItem],
    ) -> Result<(), GraphError> {
        let path = format!(
            "/me/todo/lists/{}/tasks/{}/checklistItems",
            path_segment(list),
            path_segment(task)
        );
        let existing: Page<Value> = self
            .client
            .get_json(&path, &[], &[], "GET checklistItems")
            .await?;
        let existing: BTreeMap<String, Value> = existing
            .value
            .into_iter()
            .map(|v| (id_of(&v).to_owned(), v))
            .collect();
        let mut requests = Vec::new();
        for item in wanted {
            let body = json!({ "displayName": item.title, "isChecked": item.checked });
            match existing.get(&item.id) {
                Some(current) => {
                    let same = str_of(current, "displayName") == Some(item.title.as_str())
                        && current
                            .get("isChecked")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            == item.checked;
                    if !same {
                        requests.push(BatchRequest::with_body(
                            Method::PATCH,
                            format!("{path}/{}", path_segment(&item.id)),
                            body,
                        ));
                    }
                }
                None => requests.push(BatchRequest::with_body(Method::POST, path.clone(), body)),
            }
        }
        for id in existing.keys() {
            if !wanted.iter().any(|w| &w.id == id) {
                requests.push(BatchRequest {
                    method: Method::DELETE,
                    url: format!("{path}/{}", path_segment(id)),
                    headers: Vec::new(),
                    body: None,
                });
            }
        }
        if requests.is_empty() {
            return Ok(());
        }
        for answer in self.client.batch(&requests).await? {
            match answer.error("checklistItems") {
                None | Some(GraphError::NotFound(_)) => {}
                Some(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn delete_pim(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
        send: MeetingSend,
    ) -> Result<(), GraphError> {
        let base = mailbox_path(mailbox);
        let delete = |path: String, what: &'static str| async move {
            self.client
                .send_json(Method::DELETE, &path, None, &[], true, what)
                .await
                .map(drop)
        };
        if let Some((list, task)) = split_todo_task_id(remote_id) {
            return delete(
                format!(
                    "/me/todo/lists/{}/tasks/{}",
                    path_segment(list),
                    path_segment(task)
                ),
                "DELETE todo task",
            )
            .await;
        }
        let event_path = format!("{base}/events/{}", path_segment(remote_id));
        let event: Result<Value, GraphError> = self
            .client
            .get_json(
                &event_path,
                &[("$select", "id,isOrganizer,isCancelled,attendees".to_owned())],
                &[],
                "GET event",
            )
            .await;
        match event {
            Ok(event) => {
                let organizer = event.get("isOrganizer").and_then(Value::as_bool) == Some(true);
                let has_attendees = event
                    .get("attendees")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty());
                let cancelled = event.get("isCancelled").and_then(Value::as_bool) == Some(true);
                if send != MeetingSend::None && organizer && has_attendees && !cancelled {
                    self.client
                        .send_json(
                            Method::POST,
                            &format!("{event_path}/cancel"),
                            Some(&json!({})),
                            &[],
                            // Cancellations are mailed: never repeated
                            // after a server error.
                            false,
                            "POST events/cancel",
                        )
                        .await
                        .map(drop)
                } else {
                    delete(event_path, "DELETE event").await
                }
            }
            Err(GraphError::NotFound(_) | GraphError::Rejected(_)) => {
                delete(
                    format!("{base}/contacts/{}", path_segment(remote_id)),
                    "DELETE contact",
                )
                .await
            }
            Err(e) => Err(e),
        }
    }

    /// Answers a meeting invitation (optionally without sending the
    /// response); `proposed` proposes a new time with a tentative or
    /// declining answer.
    pub async fn respond(
        &self,
        mailbox: Option<&str>,
        event_id: &str,
        response: InvitationResponse,
        comment: Option<&str>,
        send: bool,
        proposed: Option<(&ExchangeDateTime, &ExchangeDateTime)>,
    ) -> Result<(), GraphError> {
        let mut body = Map::new();
        body.insert("sendResponse".into(), json!(send));
        if let Some(comment) = comment.filter(|c| !c.is_empty()) {
            body.insert("comment".into(), json!(comment));
        }
        if let Some((start, end)) = proposed {
            if response == InvitationResponse::Accept {
                return Err(GraphError::Rejected(
                    "a new time goes with a tentative or declining answer".into(),
                ));
            }
            body.insert(
                "proposedNewTime".into(),
                json!({ "start": date_time_json(start), "end": date_time_json(end) }),
            );
        }
        self.client
            .send_json(
                Method::POST,
                &format!(
                    "{}/events/{}/{}",
                    mailbox_path(mailbox),
                    path_segment(event_id),
                    response_action(response)
                ),
                Some(&Value::Object(body)),
                &[],
                // Not repeated after a server error: the answer may have
                // been mailed to the organizer already.
                false,
                "POST events/respond",
            )
            .await
            .map(drop)
    }

    /// Proposes a new time for a meeting (with a tentative answer, or a
    /// declining one when `decline`).
    pub async fn propose_new_time(
        &self,
        mailbox: Option<&str>,
        event_id: &str,
        start: &ExchangeDateTime,
        end: &ExchangeDateTime,
        comment: Option<&str>,
        decline: bool,
    ) -> Result<(), GraphError> {
        let response = if decline {
            InvitationResponse::Decline
        } else {
            InvitationResponse::Tentative
        };
        self.respond(
            mailbox,
            event_id,
            response,
            comment,
            true,
            Some((start, end)),
        )
        .await
    }

    /// The photo of a contact (`None` when it has none).
    pub async fn contact_photo(
        &self,
        mailbox: Option<&str>,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, GraphError> {
        match self
            .client
            .get_bytes(
                &format!(
                    "{}/contacts/{}/photo/$value",
                    mailbox_path(mailbox),
                    path_segment(contact_id)
                ),
                "GET contact photo",
            )
            .await
        {
            Ok(bytes) => Ok(Some(bytes)),
            Err(GraphError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// The response state an answer gives the user (for callers that update
/// their copy before the next delta).
pub fn response_state(response: InvitationResponse) -> ExchangeResponse {
    match response {
        InvitationResponse::Accept => ExchangeResponse::Accepted,
        InvitationResponse::Tentative => ExchangeResponse::TentativelyAccepted,
        InvitationResponse::Decline => ExchangeResponse::Declined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::exchange::pim::{ExchangeDayOfWeek, PatternKind, WeekIndex};
    use pretty_assertions::assert_eq;

    fn recorded(name: &str) -> Value {
        let text = match name {
            "event-series" => include_str!("../fixtures/responses/event-series.json"),
            "todo-task" => include_str!("../fixtures/responses/todo-task.json"),
            "contact" => include_str!("../fixtures/responses/contact.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn recorded_series_reads() {
        let event = event_from_graph(&recorded("event-series")).unwrap();
        assert_eq!(event.kind, EventKind::SeriesMaster);
        assert_eq!(
            event.uid.as_deref(),
            Some("040000008200E00074C5B7101A82E008")
        );
        assert_eq!(event.start.date_time, "2026-10-05T09:00:00");
        assert_eq!(event.start.time_zone, "W. Europe Standard Time");
        assert_eq!(event.subject, "Weekly sync");
        assert_eq!(event.reminder_minutes, Some(15));
        assert_eq!(event.show_as, ShowAs::Busy);
        assert_eq!(event.my_response, ExchangeResponse::Organizer);
        assert!(event.is_organizer && event.response_requested);
        assert_eq!(event.attendees.len(), 2, "the organizer is not an attendee");
        assert_eq!(event.attendees[0].response, ExchangeResponse::Accepted);
        assert_eq!(event.attendees[1].kind, AttendeeKind::Resource);
        assert!(event.attendees[1].proposed.is_none());
        assert_eq!(
            event.locations[0].email.as_deref(),
            Some("adams@contoso.example")
        );
        assert!(event.locations[0].room);
        assert_eq!(
            event.online_meeting_url.as_deref(),
            Some("https://teams.example/l/meetup-join/abc")
        );
        assert_eq!(event.cancelled_occurrences, ["2026-10-12"]);
        assert_eq!(event.last_modified.as_deref(), Some("2026-09-30T10:00:00Z"));
        let recurrence = event.recurrence.clone().unwrap();
        assert_eq!(recurrence.pattern.kind, PatternKind::Weekly);
        assert_eq!(recurrence.pattern.days_of_week, [ExchangeDayOfWeek::Monday]);
        assert_eq!(
            recurrence.pattern.first_day_of_week,
            Some(ExchangeDayOfWeek::Monday)
        );
        assert_eq!(recurrence.pattern.day_of_month, None, "0 = unset");
        assert_eq!(
            recurrence.range,
            ExchangeRecurrenceRange::Numbered {
                start_date: "2026-10-05".into(),
                occurrences: 10
            }
        );
        assert_eq!(
            recurrence.time_zone.as_deref(),
            Some("W. Europe Standard Time")
        );
        let graph = event_to_graph(&event);
        assert_eq!(graph["recurrence"]["range"]["type"], "numbered");
        assert_eq!(graph["attendees"][1]["type"], "resource");
        // An attendee's update leaves the organizer's parts alone.
        let mut copy = event.clone();
        copy.is_organizer = false;
        let update = event_update_to_graph(&copy);
        assert!(update.get("attendees").is_none());
        assert!(update.get("recurrence").is_none());
        assert_eq!(update["reminderMinutesBeforeStart"], 15);
        copy.is_organizer = true;
        assert!(event_update_to_graph(&copy).get("attendees").is_some());
        assert_eq!(graph["location"]["locationType"], "conferenceRoom");
        assert_eq!(graph["reminderMinutesBeforeStart"], 15);
        let mut round = event_from_graph(&graph).unwrap();
        round.remote_id.clone_from(&event.remote_id);
        assert_eq!(round.start, event.start);
        assert_eq!(round.recurrence, event.recurrence);
        assert_eq!(round.attendees.len(), 2);
    }

    #[test]
    fn recurrence_patterns() {
        let r = recurrence_from_graph(Some(&json!({
            "pattern": {"type": "relativeMonthly", "interval": 2, "daysOfWeek": ["friday"], "index": "last",
                        "month": 0, "dayOfMonth": 0},
            "range": {"type": "endDate", "startDate": "2026-01-30", "endDate": "2026-12-31"}
        })))
        .unwrap();
        assert_eq!(r.pattern.index, Some(WeekIndex::Last));
        assert_eq!(r.pattern.interval, 2);
        assert_eq!(
            recurrence_from_graph(Some(&recurrence_to_graph(&r))).unwrap(),
            r
        );
        let open = recurrence_from_graph(Some(&json!({
            "pattern": {"type": "daily", "interval": 0},
            "range": {"type": "endDate", "startDate": "2026-01-01", "endDate": "0001-01-01"}
        })))
        .unwrap();
        assert_eq!(open.pattern.interval, 1);
        assert!(matches!(open.range, ExchangeRecurrenceRange::NoEnd { .. }));
        assert!(recurrence_from_graph(Some(&Value::Null)).is_none());
        assert!(
            recurrence_from_graph(Some(
                &json!({"pattern": {"type": "hourly"}, "range": {"startDate": "x"}})
            ))
            .is_none()
        );
    }

    #[test]
    fn recorded_task_reads_and_writes() {
        let checklist = [json!({"id": "c1", "displayName": "Draft", "isChecked": true})];
        let links = [
            json!({"id": "l1", "webUrl": "https://outlook.office.com/mail/x",
            "applicationName": "Outlook", "displayName": "Mail", "externalId": "AAMkMsg"}),
        ];
        let task = task_from_graph("L1", &recorded("todo-task"), &checklist, &links);
        assert_eq!(task.remote_id, "L1/AAkALgTask1");
        assert_eq!(
            task.change_key.as_deref(),
            Some("W/\"vVwdQvxCiE6779iYhchMrAAGgwrltg==\"")
        );
        assert_eq!(task.status, ExchangeTaskStatus::InProgress);
        assert_eq!(task.importance, Some(Importance::High));
        assert_eq!(task.due.as_ref().unwrap().date_time, "2026-10-09T00:00:00");
        assert_eq!(task.reminder.as_ref().unwrap().time_zone, "UTC");
        assert_eq!(task.categories, ["Work"]);
        assert!(task.checklist[0].checked);
        assert_eq!(
            task.linked_resources[0].external_id.as_deref(),
            Some("AAMkMsg")
        );
        assert_eq!(task.created.as_deref(), Some("2026-09-28T08:00:00Z"));
        let graph = task_to_graph(&task);
        assert_eq!(graph["status"], "inProgress");
        assert_eq!(graph["importance"], "high");
        assert_eq!(graph["isReminderOn"], true);
        assert_eq!(graph["completedDateTime"], Value::Null);
        assert_eq!(
            split_todo_task_id(&task.remote_id),
            Some(("L1", "AAkALgTask1"))
        );
        assert_eq!(split_todo_task_id("plain"), None);
        assert_eq!(split_todo_task_id("/x"), None);
    }

    #[test]
    fn recorded_contact_reads_and_writes() {
        let contact = contact_from_graph(&recorded("contact"));
        assert_eq!(contact.display_name, "Bob Example");
        assert_eq!(contact.emails[0].address, "bob@contoso.example");
        assert_eq!(
            contact
                .phones
                .iter()
                .map(|p| p.kind.as_str())
                .collect::<Vec<_>>(),
            ["home", "business", "mobile"]
        );
        assert_eq!(contact.addresses.len(), 1, "empty addresses are left out");
        assert_eq!(contact.addresses[0].city.as_deref(), Some("Redmond"));
        assert_eq!(contact.birthday.as_deref(), Some("1990-05-01"));
        assert_eq!(contact.urls, ["https://bob.example"]);
        let graph = contact_to_graph(&contact);
        assert_eq!(graph["mobilePhone"], "+1 555 0100");
        assert_eq!(graph["businessAddress"]["city"], "Redmond");
        assert_eq!(graph["birthday"], "1990-05-01T11:59:00Z");
        let mut back = contact_from_graph(&graph);
        back.remote_id.clone_from(&contact.remote_id);
        back.change_key.clone_from(&contact.change_key);
        assert_eq!(back, contact);
    }

    #[test]
    fn small_helpers() {
        assert_eq!(
            calendar_color(Some("lightRed")),
            Some(CategoryColor::Preset0)
        );
        assert_eq!(calendar_color(Some("auto")), None);
        assert!(plain_zone("W. Europe Standard Time"));
        assert!(!plain_zone("tzone://Microsoft/Custom"));
        assert!(!plain_zone("evil\"zone"));
        assert_eq!(
            if_match(Some("DwAAAB")),
            [("If-Match", "W/\"DwAAAB\"".to_owned())]
        );
        assert_eq!(
            if_match(Some("W/\"x\"")),
            [("If-Match", "W/\"x\"".to_owned())]
        );
        assert_eq!(if_match(None).len(), 0);
        assert_eq!(
            response_action(InvitationResponse::Tentative),
            "tentativelyAccept"
        );
        assert_eq!(
            response_state(InvitationResponse::Decline),
            ExchangeResponse::Declined
        );
        assert!(event_from_graph(&json!({"id": "x"})).is_none());
    }
}
