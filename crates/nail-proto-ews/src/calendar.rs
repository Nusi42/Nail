//! Calendar items ↔ [`ExchangeEvent`]: reading (`GetItem`, `FindItem`
//! `CalendarView`), writing (`CreateItem` / `UpdateItem` fields in schema
//! order with `TimeZoneContext`), occurrences and meeting responses
//! (`AcceptItem`, `TentativelyAcceptItem`, `DeclineItem`).
//!
//! **Times.** EWS answers in UTC. Timed events become
//! [`ExchangeDateTime`]s in `UTC` (the mapping layer converts to the
//! event's zone, which `recurrence.time_zone` names). All-day events are
//! dates: their UTC start is shifted by the standard bias of the item's
//! `StartTimeZone` and rounded to the nearest midnight, then written as
//! `YYYY-MM-DDT00:00:00` in that zone. Writing needs Windows zone ids
//! (`W. Europe Standard Time`) or `UTC`: wall times are sent without an
//! offset under a `TimeZoneContext` of the event's zone.

use std::fmt::Write as _;

use chrono::{Duration, NaiveDate, Timelike};
use nail_api::exchange::engine::MeetingSend;
use nail_api::exchange::pim::{
    AttendeeKind, EventKind, ExchangeAttendee, ExchangeBody, ExchangeDateTime, ExchangeEmail,
    ExchangeEvent, ExchangeLocation, ExchangeResponse, Sensitivity, ShowAs,
};
use nail_api::exchange::rules::Importance;
use nail_api::scheduling::InvitationResponse;

use crate::client::{EwsClient, Operation};
use crate::error::EwsError;
use crate::mail::{categories_xml, first_item};
use crate::recurrence::{parse_recurrence, recurrence_xml};
use crate::soap;
use crate::types::{
    Field, FolderRef, ItemRef, Update, format_utc, normalize_utc, parse_date, parse_time,
};
use crate::xml::{Element, el, escape};

/// The calendar item properties Nail reads with `GetItem`.
pub const EVENT_FIELDS: [&str; 30] = [
    "item:ParentFolderId",
    "item:Subject",
    "item:Body",
    "item:Categories",
    "item:Importance",
    "item:Sensitivity",
    "item:ReminderIsSet",
    "item:ReminderMinutesBeforeStart",
    "item:LastModifiedTime",
    "calendar:UID",
    "calendar:Start",
    "calendar:End",
    "calendar:OriginalStart",
    "calendar:IsAllDayEvent",
    "calendar:LegacyFreeBusyStatus",
    "calendar:Location",
    "calendar:IsCancelled",
    "calendar:IsResponseRequested",
    "calendar:CalendarItemType",
    "calendar:MyResponseType",
    "calendar:Organizer",
    "calendar:RequiredAttendees",
    "calendar:OptionalAttendees",
    "calendar:Resources",
    "calendar:Recurrence",
    "calendar:ModifiedOccurrences",
    "calendar:DeletedOccurrences",
    "calendar:StartTimeZone",
    "calendar:IsOrganizer",
    "calendar:JoinOnlineMeetingUrl",
];

/// The lighter properties `FindItem` `CalendarView` can return.
pub const VIEW_FIELDS: [&str; 14] = [
    "item:ParentFolderId",
    "item:Subject",
    "item:Categories",
    "item:Sensitivity",
    "calendar:UID",
    "calendar:Start",
    "calendar:End",
    "calendar:OriginalStart",
    "calendar:IsAllDayEvent",
    "calendar:LegacyFreeBusyStatus",
    "calendar:Location",
    "calendar:IsCancelled",
    "calendar:CalendarItemType",
    "calendar:MyResponseType",
];

/// An item shape with `fields`.
pub fn shape(fields: &[&str]) -> String {
    let list: String = fields.iter().fold(String::new(), |mut out, f| {
        let _ = write!(out, r#"<t:FieldURI FieldURI="{f}"/>"#);
        out
    });
    format!(
        "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties>{list}</t:AdditionalProperties></m:ItemShape>"
    )
}

/// Minutes of an `xs:duration` bias (`-PT1H`, `PT5H30M`, `PT0M`).
pub fn parse_bias(text: &str) -> Option<i64> {
    let text = text.trim();
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let rest = rest
        .strip_prefix("PT")
        .or_else(|| rest.strip_prefix("P0DT"))?;
    let mut minutes = 0i64;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' => {
                minutes += whole_number(&number)? * 60;
                number.clear();
            }
            'M' => {
                minutes += whole_number(&number)?;
                number.clear();
            }
            'S' => number.clear(),
            _ => return None,
        }
    }
    Some(if negative { -minutes } else { minutes })
}

/// The whole part of a duration number (`1.5` is 1; fractions of hours
/// and minutes are dropped, as Exchange never writes them).
fn whole_number(number: &str) -> Option<i64> {
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if whole.is_empty() {
        return (!fraction.is_empty()).then_some(0);
    }
    whole.parse().ok()
}

/// The zone id and standard bias (minutes; UTC = local + bias) of a
/// `StartTimeZone` element.
fn start_zone(item: &Element) -> (Option<String>, i64) {
    let Some(zone) = item.child("StartTimeZone") else {
        return (None, 0);
    };
    let id = zone.attr("Id").map(str::to_owned);
    let bias = zone
        .child("Periods")
        .and_then(|p| {
            p.children
                .iter()
                .find(|period| period.attr("Name") == Some("Standard"))
                .or_else(|| p.children.first())
        })
        .and_then(|period| period.attr("Bias"))
        .and_then(parse_bias)
        .unwrap_or(0);
    (id, bias)
}

/// The local date of a UTC time for a bias, rounded to the nearest
/// midnight (daylight saving shifts all-day starts by an hour).
pub fn local_date(utc: &str, bias_minutes: i64) -> Option<NaiveDate> {
    let time = parse_time(utc)?.naive_utc() - Duration::minutes(bias_minutes);
    let rounded = if time.hour() >= 12 {
        time.date() + Duration::days(1)
    } else {
        time.date()
    };
    Some(rounded)
}

fn utc_wall(text: &str) -> Option<ExchangeDateTime> {
    let time = parse_time(text)?;
    Some(ExchangeDateTime::utc(
        time.format("%Y-%m-%dT%H:%M:%S").to_string(),
    ))
}

fn mailbox(element: &Element) -> Option<ExchangeEmail> {
    let mailbox = element.child("Mailbox").unwrap_or(element);
    Some(ExchangeEmail {
        address: mailbox.child_value("EmailAddress")?,
        name: mailbox.child_value("Name"),
    })
}

fn response_type(text: &str) -> ExchangeResponse {
    match text {
        "Organizer" => ExchangeResponse::Organizer,
        "Tentative" => ExchangeResponse::TentativelyAccepted,
        "Accept" => ExchangeResponse::Accepted,
        "Decline" => ExchangeResponse::Declined,
        "NoResponseReceived" => ExchangeResponse::NotResponded,
        _ => ExchangeResponse::None,
    }
}

fn show_as(text: &str) -> ShowAs {
    match text {
        "Free" => ShowAs::Free,
        "Tentative" => ShowAs::Tentative,
        "Busy" => ShowAs::Busy,
        "OOF" => ShowAs::Oof,
        "WorkingElsewhere" => ShowAs::WorkingElsewhere,
        _ => ShowAs::Unknown,
    }
}

fn show_as_name(show: ShowAs) -> &'static str {
    match show {
        ShowAs::Free => "Free",
        ShowAs::Tentative => "Tentative",
        ShowAs::Busy => "Busy",
        ShowAs::Oof => "OOF",
        ShowAs::WorkingElsewhere => "WorkingElsewhere",
        ShowAs::Unknown => "NoData",
    }
}

/// The importance of an EWS `Importance` value.
pub fn importance(text: &str) -> Option<Importance> {
    match text {
        "Low" => Some(Importance::Low),
        "Normal" => Some(Importance::Normal),
        "High" => Some(Importance::High),
        _ => None,
    }
}

/// The EWS name of an importance.
pub fn importance_name(importance: Importance) -> &'static str {
    match importance {
        Importance::Low => "Low",
        Importance::Normal => "Normal",
        Importance::High => "High",
    }
}

fn sensitivity(text: &str) -> Sensitivity {
    match text {
        "Personal" => Sensitivity::Personal,
        "Private" => Sensitivity::Private,
        "Confidential" => Sensitivity::Confidential,
        _ => Sensitivity::Normal,
    }
}

fn sensitivity_name(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Normal => "Normal",
        Sensitivity::Personal => "Personal",
        Sensitivity::Private => "Private",
        Sensitivity::Confidential => "Confidential",
    }
}

/// The body of an item element.
pub fn body(item: &Element) -> Option<ExchangeBody> {
    let body = item.child("Body")?;
    Some(ExchangeBody {
        html: body
            .attr("BodyType")
            .is_some_and(|t| t.eq_ignore_ascii_case("HTML")),
        content: body.text.clone(),
    })
}

/// `<t:Body BodyType=…>`.
pub fn body_xml(body: &ExchangeBody) -> String {
    format!(
        r#"<t:Body BodyType="{}">{}</t:Body>"#,
        if body.html { "HTML" } else { "Text" },
        escape(&body.content)
    )
}

/// A calendar item element as an [`ExchangeEvent`] (exceptions are left
/// empty; the engine fetches them).
pub fn parse_event(item: &Element) -> Option<ExchangeEvent> {
    let id = item.child("ItemId").and_then(ItemRef::from_element)?;
    let (zone, bias) = start_zone(item);
    let all_day = item.child_bool("IsAllDayEvent").unwrap_or(false);
    let time = |name: &str| -> Option<ExchangeDateTime> {
        let text = item.child_text(name)?;
        if all_day {
            let date = local_date(text, bias)?;
            Some(ExchangeDateTime {
                date_time: format!("{}T00:00:00", date.format("%Y-%m-%d")),
                time_zone: zone.clone().unwrap_or_else(|| "UTC".to_owned()),
            })
        } else {
            utc_wall(text)
        }
    };
    let mut attendees = Vec::new();
    for (list, kind) in [
        ("RequiredAttendees", AttendeeKind::Required),
        ("OptionalAttendees", AttendeeKind::Optional),
        ("Resources", AttendeeKind::Resource),
    ] {
        if let Some(list) = item.child(list) {
            for attendee in list.children_named("Attendee") {
                if let Some(email) = mailbox(attendee) {
                    attendees.push(ExchangeAttendee {
                        email,
                        kind,
                        response: response_type(attendee.child_text("ResponseType").unwrap_or("")),
                        proposed: None,
                    });
                }
            }
        }
    }
    let my_response = response_type(item.child_text("MyResponseType").unwrap_or(""));
    let location = item.child_value("Location");
    let locations = location
        .map(|name| {
            let room = attendees.iter().find(|a| {
                a.kind == AttendeeKind::Resource && a.email.name.as_deref() == Some(&name)
            });
            ExchangeLocation {
                email: room.map(|r| r.email.address.clone()),
                room: room.is_some(),
                display_name: name,
            }
        })
        .into_iter()
        .collect();
    let cancelled_occurrences = item
        .child("DeletedOccurrences")
        .map(|list| {
            list.children
                .iter()
                .filter_map(|o| o.child_text("Start"))
                .filter_map(|s| {
                    if all_day {
                        local_date(s, bias)
                    } else {
                        parse_time(s).map(|t| (t.naive_utc() - Duration::minutes(bias)).date())
                    }
                })
                .map(|d| d.format("%Y-%m-%d").to_string())
                .collect()
        })
        .unwrap_or_default();
    Some(ExchangeEvent {
        remote_id: id.id,
        change_key: id.change_key,
        uid: item.child_value("UID"),
        kind: match item.child_text("CalendarItemType").unwrap_or("Single") {
            "RecurringMaster" => EventKind::SeriesMaster,
            "Occurrence" => EventKind::Occurrence,
            "Exception" => EventKind::Exception,
            _ => EventKind::SingleInstance,
        },
        series_master_id: None,
        original_start: item.child_text("OriginalStart").and_then(|t| {
            if all_day {
                local_date(t, bias).map(|d| ExchangeDateTime {
                    date_time: format!("{}T00:00:00", d.format("%Y-%m-%d")),
                    time_zone: zone.clone().unwrap_or_else(|| "UTC".to_owned()),
                })
            } else {
                utc_wall(t)
            }
        }),
        subject: item.child_text("Subject").unwrap_or_default().to_owned(),
        body: body(item),
        start: time("Start")?,
        end: time("End")?,
        is_all_day: all_day,
        locations,
        show_as: show_as(item.child_text("LegacyFreeBusyStatus").unwrap_or("Busy")),
        sensitivity: sensitivity(item.child_text("Sensitivity").unwrap_or("")),
        importance: item.child_text("Importance").and_then(importance),
        reminder_minutes: item
            .child_bool("ReminderIsSet")
            .filter(|set| *set)
            .and_then(|_| item.child_number("ReminderMinutesBeforeStart")),
        organizer: item.child("Organizer").and_then(mailbox),
        attendees,
        is_organizer: item
            .child_bool("IsOrganizer")
            .unwrap_or(my_response == ExchangeResponse::Organizer),
        response_requested: item.child_bool("IsResponseRequested").unwrap_or(false),
        my_response,
        is_cancelled: item.child_bool("IsCancelled").unwrap_or(false),
        online_meeting_url: item.child_value("JoinOnlineMeetingUrl"),
        categories: item.strings("Categories", "String"),
        recurrence: item
            .child("Recurrence")
            .and_then(|r| parse_recurrence(r, zone.as_deref())),
        cancelled_occurrences,
        exceptions: Vec::new(),
        last_modified: item.child_text("LastModifiedTime").and_then(normalize_utc),
    })
}

/// The ids of a master's changed occurrences (`ModifiedOccurrences`).
pub fn modified_occurrences(item: &Element) -> Vec<ItemRef> {
    item.child("ModifiedOccurrences")
        .map(|list| {
            list.children
                .iter()
                .filter_map(|o| o.child("ItemId").and_then(ItemRef::from_element))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a zone name is one EWS accepts (Windows ids; not IANA names).
fn is_windows_zone(zone: &str) -> bool {
    !zone.contains('/')
}

/// The zone a write must run in (`TimeZoneContext`), `None` for UTC.
pub fn write_zone(event: &ExchangeEvent) -> Result<Option<String>, EwsError> {
    let zone = &event.start.time_zone;
    if zone.eq_ignore_ascii_case("UTC") || zone.eq_ignore_ascii_case("Etc/UTC") {
        return Ok(None);
    }
    if !is_windows_zone(zone) || !is_windows_zone(&event.end.time_zone) {
        return Err(EwsError::Unsupported(format!(
            "EWS needs Windows time zone names, not {zone}"
        )));
    }
    Ok(Some(zone.clone()))
}

fn time_value(time: &ExchangeDateTime) -> String {
    let wall = time.date_time.get(..19).unwrap_or(&time.date_time);
    if time.time_zone.eq_ignore_ascii_case("UTC") || time.time_zone.eq_ignore_ascii_case("Etc/UTC")
    {
        parse_time(wall).map_or_else(|| wall.to_owned(), format_utc)
    } else {
        wall.to_owned()
    }
}

fn attendees_xml(event: &ExchangeEvent, kind: AttendeeKind) -> Option<String> {
    let list: String =
        event
            .attendees
            .iter()
            .filter(|a| a.kind == kind)
            .fold(String::new(), |mut out, a| {
                let name = a
                    .email
                    .name
                    .as_deref()
                    .map(|n| el("t:Name", n))
                    .unwrap_or_default();
                let _ = write!(
                    out,
                    "<t:Attendee><t:Mailbox>{name}{}</t:Mailbox></t:Attendee>",
                    el("t:EmailAddress", &a.email.address)
                );
                out
            });
    (!list.is_empty()).then_some(list)
}

/// The fields Nail writes of an event, in `CalendarItem` schema order:
/// (path, element content or `None` = not set). `occurrence` leaves out
/// what an occurrence / exception cannot change.
pub fn event_fields(
    event: &ExchangeEvent,
    occurrence: bool,
) -> Result<Vec<(Field, Option<String>)>, EwsError> {
    let zone = write_zone(event)?;
    let mut out: Vec<(Field, Option<String>)> = vec![
        (
            Field::Uri("item:Subject"),
            Some(el("t:Subject", &event.subject)),
        ),
        (
            Field::Uri("item:Sensitivity"),
            Some(el("t:Sensitivity", sensitivity_name(event.sensitivity))),
        ),
        (Field::Uri("item:Body"), event.body.as_ref().map(body_xml)),
        (
            Field::Uri("item:Categories"),
            (!event.categories.is_empty()).then(|| categories_xml(&event.categories)),
        ),
        (
            Field::Uri("item:Importance"),
            event
                .importance
                .map(|i| el("t:Importance", importance_name(i))),
        ),
        (
            Field::Uri("item:ReminderIsSet"),
            Some(el(
                "t:ReminderIsSet",
                if event.reminder_minutes.is_some() {
                    "true"
                } else {
                    "false"
                },
            )),
        ),
        (
            Field::Uri("item:ReminderMinutesBeforeStart"),
            event
                .reminder_minutes
                .map(|m| el("t:ReminderMinutesBeforeStart", &m.to_string())),
        ),
    ];
    if !occurrence && let Some(uid) = &event.uid {
        out.push((Field::Uri("calendar:UID"), Some(el("t:UID", uid))));
    }
    out.extend([
        (
            Field::Uri("calendar:Start"),
            Some(el("t:Start", &time_value(&event.start))),
        ),
        (
            Field::Uri("calendar:End"),
            Some(el("t:End", &time_value(&event.end))),
        ),
        (
            Field::Uri("calendar:IsAllDayEvent"),
            Some(el(
                "t:IsAllDayEvent",
                if event.is_all_day { "true" } else { "false" },
            )),
        ),
        (
            Field::Uri("calendar:LegacyFreeBusyStatus"),
            Some(el("t:LegacyFreeBusyStatus", show_as_name(event.show_as))),
        ),
        (
            Field::Uri("calendar:Location"),
            event
                .locations
                .first()
                .map(|l| el("t:Location", &l.display_name)),
        ),
        (
            Field::Uri("calendar:IsResponseRequested"),
            (!event.attendees.is_empty()).then(|| {
                el(
                    "t:IsResponseRequested",
                    if event.response_requested {
                        "true"
                    } else {
                        "false"
                    },
                )
            }),
        ),
        (
            Field::Uri("calendar:RequiredAttendees"),
            attendees_xml(event, AttendeeKind::Required)
                .map(|l| format!("<t:RequiredAttendees>{l}</t:RequiredAttendees>")),
        ),
        (
            Field::Uri("calendar:OptionalAttendees"),
            attendees_xml(event, AttendeeKind::Optional)
                .map(|l| format!("<t:OptionalAttendees>{l}</t:OptionalAttendees>")),
        ),
        (
            Field::Uri("calendar:Resources"),
            attendees_xml(event, AttendeeKind::Resource)
                .map(|l| format!("<t:Resources>{l}</t:Resources>")),
        ),
    ]);
    if !occurrence {
        out.push((
            Field::Uri("calendar:Recurrence"),
            event.recurrence.as_ref().map(recurrence_xml).transpose()?,
        ));
    }
    if let Some(zone) = &zone {
        out.push((
            Field::Uri("calendar:StartTimeZone"),
            Some(format!(r#"<t:StartTimeZone Id="{}"/>"#, escape(zone))),
        ));
        out.push((
            Field::Uri("calendar:EndTimeZone"),
            Some(format!(
                r#"<t:EndTimeZone Id="{}"/>"#,
                escape(&event.end.time_zone)
            )),
        ));
    }
    Ok(out)
}

/// The `<t:CalendarItem>` of a new event.
pub fn event_create_xml(event: &ExchangeEvent) -> Result<String, EwsError> {
    let fields: String = event_fields(event, false)?
        .into_iter()
        .filter_map(|(_, v)| v)
        .collect();
    Ok(format!("<t:CalendarItem>{fields}</t:CalendarItem>"))
}

/// The updates that replace an event's fields (unset optional fields are
/// removed; `UID` cannot change).
pub fn event_updates(event: &ExchangeEvent, occurrence: bool) -> Result<Vec<Update>, EwsError> {
    // A meeting the user does not organize: the own copy only (reminder,
    // categories, notes, show-as, …), not the attendees, the response
    // request or the series (ADR 0078).
    let attendee = !event.is_organizer && !event.attendees.is_empty();
    let organizers = [
        Field::Uri("calendar:RequiredAttendees"),
        Field::Uri("calendar:OptionalAttendees"),
        Field::Uri("calendar:Resources"),
        Field::Uri("calendar:IsResponseRequested"),
        Field::Uri("calendar:Recurrence"),
    ];
    Ok(event_fields(event, occurrence)?
        .into_iter()
        .filter(|(f, _)| *f != Field::Uri("calendar:UID"))
        .filter(|(f, _)| !(attendee && organizers.contains(f)))
        .map(|(field, value)| match value {
            Some(v) => Update::Set(field, v),
            None => Update::Delete(field),
        })
        .collect())
}

/// `SendMeetingInvitations` of a `CreateItem`.
pub fn create_send(send: MeetingSend) -> &'static str {
    match send {
        MeetingSend::None => "SendToNone",
        MeetingSend::ToAll | MeetingSend::ToChanged => "SendToAllAndSaveCopy",
    }
}

/// `SendMeetingInvitationsOrCancellations` of an `UpdateItem`.
pub fn update_send(send: MeetingSend) -> &'static str {
    match send {
        MeetingSend::None => "SendToNone",
        MeetingSend::ToAll => "SendToAllAndSaveCopy",
        MeetingSend::ToChanged => "SendToChangedAndSaveCopy",
    }
}

/// `SendMeetingCancellations` of a `DeleteItem`.
pub fn delete_send(send: MeetingSend) -> &'static str {
    match send {
        MeetingSend::None => "SendToNone",
        MeetingSend::ToAll | MeetingSend::ToChanged => "SendToAllAndSaveCopy",
    }
}

/// The response object of an invitation answer.
pub fn response_tag(response: InvitationResponse) -> &'static str {
    match response {
        InvitationResponse::Accept => "AcceptItem",
        InvitationResponse::Tentative => "TentativelyAcceptItem",
        InvitationResponse::Decline => "DeclineItem",
    }
}

impl EwsClient {
    /// `FindItem` with a `CalendarView`: the occurrences between `from` and
    /// `to` (RFC 3339) of a calendar folder, recurring series expanded.
    pub async fn calendar_view(
        &self,
        mailbox: Option<&str>,
        folder: FolderRef<'_>,
        from: &str,
        to: &str,
        max_entries: u32,
    ) -> Result<Vec<ExchangeEvent>, EwsError> {
        let (Some(from), Some(to)) = (normalize_utc(from), normalize_utc(to)) else {
            return Err(EwsError::Rejected {
                code: "InvalidRange".into(),
                message: "the calendar view needs RFC 3339 times".into(),
            });
        };
        let body = format!(
            r#"<m:FindItem Traversal="Shallow">{}<m:CalendarView MaxEntriesReturned="{}" StartDate="{from}" EndDate="{to}"/><m:ParentFolderIds>{}</m:ParentFolderIds></m:FindItem>"#,
            shape(&VIEW_FIELDS),
            max_entries.max(1),
            folder.xml()
        );
        self.find_items(mailbox, body, |items| {
            items
                .map(|items| items.children.iter().filter_map(parse_event).collect())
                .unwrap_or_default()
        })
        .await
    }

    /// The occurrence of the series `uid` whose original start falls on
    /// `date` (`YYYY-MM-DD`, in the series zone with `bias_minutes`).
    pub async fn find_occurrence(
        &self,
        mailbox: Option<&str>,
        folder: FolderRef<'_>,
        uid: &str,
        date: &str,
        bias_minutes: i64,
    ) -> Result<Option<ExchangeEvent>, EwsError> {
        let day = parse_date(date).ok_or_else(|| EwsError::Rejected {
            code: "InvalidDate".into(),
            message: date.to_owned(),
        })?;
        let from = format_utc(
            (day - Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .unwrap_or_default()
                .and_utc(),
        );
        let to = format_utc(
            (day + Duration::days(2))
                .and_hms_opt(0, 0, 0)
                .unwrap_or_default()
                .and_utc(),
        );
        let view = self.calendar_view(mailbox, folder, &from, &to, 200).await?;
        Ok(view.into_iter().find(|e| {
            e.uid.as_deref() == Some(uid)
                && matches!(e.kind, EventKind::Occurrence | EventKind::Exception)
                && e.original_start
                    .as_ref()
                    .or(Some(&e.start))
                    .is_some_and(|s| {
                        let text = format!("{}Z", s.date_time);
                        if s.time_zone == "UTC" {
                            parse_time(&text)
                                .map(|t| (t.naive_utc() - Duration::minutes(bias_minutes)).date())
                                == Some(day)
                        } else {
                            s.date() == date
                        }
                    })
        }))
    }

    /// Answers a meeting request in the calendar (`AcceptItem`,
    /// `TentativelyAcceptItem`, `DeclineItem`), with an optional comment;
    /// without `send`, the response is saved and then removed, so the
    /// calendar changes but nobody is told. Returns the calendar item's
    /// change key afterwards (`None` when it cannot be read back: the
    /// answer went out all the same).
    pub async fn respond_to_meeting(
        &self,
        mailbox: Option<&str>,
        item_id: &str,
        response: InvitationResponse,
        comment: Option<&str>,
        send: bool,
    ) -> Result<Option<String>, EwsError> {
        let id_shape = "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape>";
        let current = self
            .get_item(mailbox, &ItemRef::new(item_id), id_shape)
            .await?;
        let reference = current
            .child("ItemId")
            .and_then(ItemRef::from_element)
            .unwrap_or_else(|| ItemRef::new(item_id));
        let tag = response_tag(response);
        let body_part = comment
            .filter(|c| !c.trim().is_empty())
            .map(el_attr_body)
            .unwrap_or_default();
        let disposition = if send { "SendAndSaveCopy" } else { "SaveOnly" };
        let body = format!(
            r#"<m:CreateItem MessageDisposition="{disposition}"><m:Items><t:{tag}>{body_part}{}</t:{tag}></m:Items></m:CreateItem>"#,
            reference.xml_as("ReferenceItemId")
        );
        let answer = self
            .call(&Operation::write("CreateItem", body).anchored(mailbox))
            .await?;
        let message = soap::single_message(&answer.body)?;
        if !send
            && let Ok(draft) = first_item(message)
            && let Some(id) = draft.child("ItemId").and_then(ItemRef::from_element)
        {
            self.delete_item(mailbox, &id, "HardDelete", "").await?;
        }
        let after = self
            .get_item(mailbox, &ItemRef::new(&reference.id), id_shape)
            .await;
        Ok(after.ok().and_then(|item| {
            item.child("ItemId")
                .and_then(ItemRef::from_element)
                .and_then(|r| r.change_key)
        }))
    }
}

fn el_attr_body(comment: &str) -> String {
    format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, escape(comment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use nail_api::exchange::pim::{ExchangeRecurrenceRange, PatternKind};
    use pretty_assertions::assert_eq;

    const SERIES: &str = concat!(
        r#"<t:CalendarItem xmlns:t="t"><t:ItemId Id="E1" ChangeKey="DwAA"/><t:ParentFolderId Id="F-cal"/>"#,
        r#"<t:Subject>Weekly sync</t:Subject><t:Sensitivity>Normal</t:Sensitivity>"#,
        r#"<t:Body BodyType="HTML">&lt;p&gt;Agenda&lt;/p&gt;</t:Body>"#,
        r#"<t:Categories><t:String>Work</t:String></t:Categories><t:Importance>High</t:Importance>"#,
        r#"<t:ReminderIsSet>true</t:ReminderIsSet><t:ReminderMinutesBeforeStart>15</t:ReminderMinutesBeforeStart>"#,
        r#"<t:LastModifiedTime>2026-09-30T10:00:00.123Z</t:LastModifiedTime>"#,
        r#"<t:UID>040000008200E00074C5B7101A82E008</t:UID>"#,
        r#"<t:Start>2026-10-05T07:00:00Z</t:Start><t:End>2026-10-05T07:30:00Z</t:End>"#,
        r#"<t:IsAllDayEvent>false</t:IsAllDayEvent><t:LegacyFreeBusyStatus>Busy</t:LegacyFreeBusyStatus>"#,
        r#"<t:Location>Room 1</t:Location><t:IsCancelled>false</t:IsCancelled>"#,
        r#"<t:IsResponseRequested>true</t:IsResponseRequested><t:CalendarItemType>RecurringMaster</t:CalendarItemType>"#,
        r#"<t:MyResponseType>Organizer</t:MyResponseType>"#,
        r#"<t:Organizer><t:Mailbox><t:Name>Ada</t:Name><t:EmailAddress>ada@corp.example</t:EmailAddress></t:Mailbox></t:Organizer>"#,
        r#"<t:RequiredAttendees><t:Attendee><t:Mailbox><t:Name>Bob</t:Name><t:EmailAddress>bob@corp.example</t:EmailAddress></t:Mailbox>"#,
        r#"<t:ResponseType>Accept</t:ResponseType></t:Attendee></t:RequiredAttendees>"#,
        r#"<t:Resources><t:Attendee><t:Mailbox><t:Name>Room 1</t:Name><t:EmailAddress>room1@corp.example</t:EmailAddress></t:Mailbox>"#,
        r#"<t:ResponseType>Accept</t:ResponseType></t:Attendee></t:Resources>"#,
        r#"<t:Recurrence><t:WeeklyRecurrence><t:Interval>1</t:Interval><t:DaysOfWeek>Monday</t:DaysOfWeek>"#,
        r#"<t:FirstDayOfWeek>Monday</t:FirstDayOfWeek></t:WeeklyRecurrence><t:NumberedRecurrence>"#,
        r#"<t:StartDate>2026-10-05+02:00</t:StartDate><t:NumberOfOccurrences>10</t:NumberOfOccurrences></t:NumberedRecurrence></t:Recurrence>"#,
        r#"<t:ModifiedOccurrences><t:Occurrence><t:ItemId Id="E1-x2" ChangeKey="a"/><t:Start>2026-10-19T08:00:00Z</t:Start>"#,
        r#"<t:End>2026-10-19T08:30:00Z</t:End><t:OriginalStart>2026-10-19T07:00:00Z</t:OriginalStart></t:Occurrence></t:ModifiedOccurrences>"#,
        r#"<t:DeletedOccurrences><t:DeletedOccurrence><t:Start>2026-10-12T07:00:00Z</t:Start></t:DeletedOccurrence></t:DeletedOccurrences>"#,
        r#"<t:StartTimeZone Id="W. Europe Standard Time" Name="(UTC+01:00) Amsterdam"><t:Periods>"#,
        r#"<t:Period Bias="-PT2H" Name="Daylight" Id="trule:x/Daylight"/><t:Period Bias="-PT1H" Name="Standard" Id="trule:x/Standard"/>"#,
        r#"</t:Periods></t:StartTimeZone><t:IsOrganizer>true</t:IsOrganizer>"#,
        r#"<t:JoinOnlineMeetingUrl>https://meet.corp.example/x</t:JoinOnlineMeetingUrl></t:CalendarItem>"#
    );

    #[test]
    fn reads_a_series() {
        let item = parse(SERIES).unwrap();
        let event = parse_event(&item).unwrap();
        assert_eq!(event.remote_id, "E1");
        assert_eq!(event.change_key.as_deref(), Some("DwAA"));
        assert_eq!(event.kind, EventKind::SeriesMaster);
        assert_eq!(event.subject, "Weekly sync");
        assert_eq!(event.body.as_ref().unwrap().content, "<p>Agenda</p>");
        assert!(event.body.as_ref().unwrap().html);
        assert_eq!(event.start, ExchangeDateTime::utc("2026-10-05T07:00:00"));
        assert_eq!(event.importance, Some(Importance::High));
        assert_eq!(event.reminder_minutes, Some(15));
        assert_eq!(
            event.organizer.as_ref().unwrap().address,
            "ada@corp.example"
        );
        assert_eq!(event.attendees.len(), 2);
        assert_eq!(event.attendees[0].response, ExchangeResponse::Accepted);
        assert_eq!(
            event.locations[0].email.as_deref(),
            Some("room1@corp.example")
        );
        assert!(event.locations[0].room);
        assert!(event.is_organizer && event.response_requested);
        assert_eq!(event.my_response, ExchangeResponse::Organizer);
        let recurrence = event.recurrence.as_ref().unwrap();
        assert_eq!(recurrence.pattern.kind, PatternKind::Weekly);
        assert_eq!(
            recurrence.time_zone.as_deref(),
            Some("W. Europe Standard Time")
        );
        assert!(matches!(
            recurrence.range,
            ExchangeRecurrenceRange::Numbered {
                occurrences: 10,
                ..
            }
        ));
        assert_eq!(event.cancelled_occurrences, ["2026-10-12"]);
        assert_eq!(
            event.online_meeting_url.as_deref(),
            Some("https://meet.corp.example/x")
        );
        assert_eq!(event.last_modified.as_deref(), Some("2026-09-30T10:00:00Z"));
        assert_eq!(
            modified_occurrences(&item),
            [ItemRef::with_key("E1-x2", Some("a".into()))]
        );
    }

    #[test]
    fn all_day_events_become_dates() {
        let item = parse(concat!(
            r#"<CalendarItem><ItemId Id="E2"/><Start>2026-10-04T22:00:00Z</Start><End>2026-10-05T22:00:00Z</End>"#,
            r#"<IsAllDayEvent>true</IsAllDayEvent><CalendarItemType>Single</CalendarItemType>"#,
            r#"<StartTimeZone Id="W. Europe Standard Time"><Periods><Period Bias="-PT1H" Name="Standard"/></Periods></StartTimeZone>"#,
            r#"</CalendarItem>"#
        ))
        .unwrap();
        let event = parse_event(&item).unwrap();
        assert!(event.is_all_day);
        assert_eq!(event.start.date_time, "2026-10-05T00:00:00");
        assert_eq!(event.end.date_time, "2026-10-06T00:00:00");
        assert_eq!(event.start.time_zone, "W. Europe Standard Time");
        assert_eq!(event.kind, EventKind::SingleInstance);
        assert_eq!(
            local_date("2026-10-05T05:00:00Z", 300),
            NaiveDate::from_ymd_opt(2026, 10, 5)
        );
        assert_eq!(parse_bias("-PT5H30M"), Some(-330));
        assert_eq!(parse_bias("PT0M"), Some(0));
        assert_eq!(parse_bias("bogus"), None);
        assert_eq!(parse_bias("PT1.5H"), Some(60));
        assert_eq!(parse_bias("PT.5M"), Some(0));
        assert_eq!(parse_bias("PT.H"), None);
        assert_eq!(parse_bias("PT1.xH"), None);
    }

    #[test]
    fn writes_events_in_schema_order() {
        let mut event = parse_event(&parse(SERIES).unwrap()).unwrap();
        event.start = ExchangeDateTime {
            date_time: "2026-10-05T09:00:00".into(),
            time_zone: "W. Europe Standard Time".into(),
        };
        event.end = ExchangeDateTime {
            date_time: "2026-10-05T09:30:00".into(),
            time_zone: "W. Europe Standard Time".into(),
        };
        assert_eq!(
            write_zone(&event).unwrap().as_deref(),
            Some("W. Europe Standard Time")
        );
        let xml = event_create_xml(&event).unwrap();
        let positions: Vec<usize> = [
            "<t:Subject>",
            "<t:Sensitivity>",
            "<t:Body",
            "<t:Categories>",
            "<t:Importance>",
            "<t:ReminderIsSet>",
            "<t:ReminderMinutesBeforeStart>",
            "<t:UID>",
            "<t:Start>2026-10-05T09:00:00</t:Start>",
            "<t:End>",
            "<t:IsAllDayEvent>",
            "<t:LegacyFreeBusyStatus>",
            "<t:Location>",
            "<t:IsResponseRequested>",
            "<t:RequiredAttendees>",
            "<t:Resources>",
            "<t:Recurrence>",
            "<t:StartTimeZone",
            "<t:EndTimeZone",
        ]
        .iter()
        .map(|tag| {
            xml.find(tag)
                .unwrap_or_else(|| panic!("{tag} missing in {xml}"))
        })
        .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{xml}");
        assert!(parse(&format!(r#"<x xmlns:t="t">{xml}</x>"#)).is_ok());
        let updates = event_updates(&event, true).unwrap();
        assert!(
            updates
                .iter()
                .all(|u| !matches!(u, Update::Set(Field::Uri("calendar:Recurrence"), _)))
        );
        assert!(updates.contains(&Update::Delete(Field::Uri("calendar:OptionalAttendees"))));
        // An attendee's update of the own copy leaves the organizer's
        // parts alone.
        let mut copy = event.clone();
        copy.is_organizer = false;
        let updates = event_updates(&copy, false).unwrap();
        assert!(updates.iter().all(|u| !matches!(
            u,
            Update::Set(
                Field::Uri("calendar:RequiredAttendees" | "calendar:Recurrence"),
                _
            ) | Update::Delete(Field::Uri("calendar:OptionalAttendees"))
        )));
        assert!(updates.iter().any(|u| matches!(
            u,
            Update::Set(Field::Uri("item:ReminderMinutesBeforeStart"), _)
        )));
        let mut utc = event.clone();
        utc.start = ExchangeDateTime::utc("2026-10-05T07:00:00");
        utc.end = ExchangeDateTime::utc("2026-10-05T07:30:00");
        let xml = event_create_xml(&utc).unwrap();
        assert!(xml.contains("<t:Start>2026-10-05T07:00:00Z</t:Start>"));
        assert!(!xml.contains("StartTimeZone"));
        let mut iana = event;
        iana.start.time_zone = "Europe/Berlin".into();
        assert!(matches!(write_zone(&iana), Err(EwsError::Unsupported(_))));
    }

    #[test]
    fn send_modes() {
        assert_eq!(create_send(MeetingSend::None), "SendToNone");
        assert_eq!(
            update_send(MeetingSend::ToChanged),
            "SendToChangedAndSaveCopy"
        );
        assert_eq!(delete_send(MeetingSend::ToAll), "SendToAllAndSaveCopy");
        assert_eq!(
            response_tag(InvitationResponse::Tentative),
            "TentativelyAcceptItem"
        );
    }
}
