//! Calendar, contact and task writes: `create_event`, `update_event`,
//! `respond_to_invite`, `upsert_contact`, `upsert_task`.
//!
//! A client writes to the calendars, address books and task lists of a
//! source its grant reaches (as for reads, ADR 0058); read-only ones
//! answer `forbidden`. Mail to attendees (invitations, updates, answers)
//! is the `invite` kind and needs the `send` scope.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use nail_api::agents::{ActionKind, ActionTargetKind};
use nail_api::mcp::McpScope;
use nail_api::mcp::tools::{
    CreateEventArgs, McpEditScope, McpInviteResponse, RespondToInviteArgs, UpdateEventArgs,
    UpsertContactArgs, UpsertTaskArgs,
};
use nail_api::pim::contact::{AddressBookRole, ContactOrg};
use nail_api::pim::{
    AddressBook, Attendee, Calendar, CalendarEvent, Contact, ContactFields, EditScope, EventFields,
    LabeledValue, PimIssue, PimLink, PimTime, Recurrence, Task, TaskFields, TaskStatus,
};
use nail_api::{
    AccountId, AddressBookId, ApiError, CalendarId, ContactId, ErrorCode, EventId,
    InvitationResponse, InvitationTarget, LocalizedMessage, Request, Response, TaskId,
};

use super::plan::{
    Action, Plan, change, invalid, joined, not_found, parse_address, pim_time_text, target,
    unexpected,
};
use super::{Ctx, audit};
use crate::data::McpData;
use crate::server::Failure;

fn issue(issues: &[PimIssue]) -> Result<(), Failure> {
    match issues.first() {
        None => Ok(()),
        Some(first) => Err(Failure::Api(
            ApiError::invalid_request(format!("invalid fields ({})", first.message_id()))
                .with_localized(LocalizedMessage::new(first.message_id())),
        )),
    }
}

fn forbidden(what: impl std::fmt::Display) -> Failure {
    Failure::Api(ApiError::new(
        ErrorCode::Forbidden,
        format!("{what} is read-only"),
    ))
}

fn allowed<D: McpData>(ctx: &Ctx<'_, D>, account: Option<&AccountId>) -> bool {
    ctx.access.all_accounts() || account.is_some_and(|a| ctx.access.allows_account(a))
}

/// A time an agent gave: `YYYY-MM-DD` (all day), RFC 3339, or a local
/// `YYYY-MM-DDTHH:MM:SS` in `tz`. With `tz`, instants are stored as
/// wall-clock times in that zone; without, in UTC.
pub(crate) fn parse_time(text: &str, tz: Option<&str>) -> Result<PimTime, Failure> {
    let text = text.trim();
    let zone = match tz {
        Some(name) if name.eq_ignore_ascii_case("UTC") => Some(chrono_tz::UTC),
        Some(name) => Some(
            name.parse::<chrono_tz::Tz>()
                .map_err(|_| invalid(format!("unknown time zone {name:?}")))?,
        ),
        None => None,
    };
    let zone_name = |z: chrono_tz::Tz| {
        if z == chrono_tz::UTC {
            nail_api::pim::UTC_ZONE.to_owned()
        } else {
            z.name().to_owned()
        }
    };
    if text.len() == 10 {
        return NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map(|d| PimTime::date(d.format("%Y-%m-%d").to_string()))
            .map_err(|_| invalid(format!("invalid date {text:?}")));
    }
    if let Ok(instant) = DateTime::parse_from_rfc3339(text) {
        return Ok(match zone {
            Some(z) => PimTime::zoned(
                instant
                    .with_timezone(&z)
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string(),
                zone_name(z),
            ),
            None => PimTime::utc(
                instant
                    .with_timezone(&Utc)
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string(),
            ),
        });
    }
    match (
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"),
        zone,
    ) {
        (Ok(local), Some(z)) => Ok(PimTime::zoned(
            local.format("%Y-%m-%dT%H:%M:%S").to_string(),
            zone_name(z),
        )),
        (Ok(_), None) => Err(invalid(format!(
            "{text:?} needs an offset (RFC 3339) or a tz"
        ))),
        (Err(_), _) => Err(invalid(format!("invalid time {text:?}"))),
    }
}

/// An occurrence's original start in the kind and zone of the series
/// start.
fn occurrence(text: &str, series_start: &PimTime) -> Result<PimTime, Failure> {
    let text = text.trim();
    match series_start {
        PimTime::Date { .. } => {
            let date = text.get(..10).unwrap_or(text);
            parse_time(date, None)
        }
        PimTime::Zoned { tz, .. } => parse_time(text, Some(tz)),
        PimTime::Floating { .. } => {
            let local = DateTime::parse_from_rfc3339(text)
                .map(|t| t.naive_local())
                .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
                .map_err(|_| invalid(format!("invalid time {text:?}")))?;
            Ok(PimTime::floating(
                local.format("%Y-%m-%dT%H:%M:%S").to_string(),
            ))
        }
    }
}

/// The zone of a time, if it has one.
fn zone_of(time: &PimTime) -> Option<&str> {
    match time {
        PimTime::Zoned { tz, .. } => Some(tz),
        _ => None,
    }
}

fn attendee(text: &str, rsvp: bool) -> Result<Attendee, Failure> {
    let address = parse_address(text)?;
    let mut attendee = Attendee::new(address.email);
    attendee.name = address.name;
    attendee.rsvp = rsvp;
    Ok(attendee)
}

fn attendees_text(list: &[Attendee]) -> Option<String> {
    joined(list.iter().map(|a| a.email.as_str()))
}

fn opt(text: &Option<String>) -> Option<String> {
    text.clone().filter(|t| !t.is_empty())
}

/// An empty string clears an optional text.
fn set_text(field: &mut Option<String>, value: &Option<String>) {
    if let Some(value) = value {
        *field = (!value.trim().is_empty()).then(|| value.clone());
    }
}

impl<D: McpData> Ctx<'_, D> {
    async fn calendars(&self) -> Result<Vec<Calendar>, Failure> {
        match self
            .shared
            .data
            .query(Request::ListCalendars { source_id: None })
            .await?
        {
            Response::Calendars { mut calendars } => {
                calendars.retain(|c| allowed(self, c.account_id.as_ref()));
                calendars.sort_by_key(|c| c.settings.order);
                Ok(calendars)
            }
            other => Err(unexpected(&other)),
        }
    }

    /// The calendar (or task list) to write to: the named one, else the
    /// first writable one the grant reaches.
    async fn calendar(&self, id: Option<&str>, tasks: bool) -> Result<Calendar, Failure> {
        let calendars = self.calendars().await?;
        let kind = |c: &Calendar| if tasks { c.has_tasks() } else { c.has_events() };
        match id {
            Some(id) => {
                let calendar = calendars
                    .into_iter()
                    .find(|c| c.id.as_str() == id && kind(c))
                    .ok_or_else(|| not_found(format!("calendar {id}")))?;
                if calendar.read_only {
                    return Err(forbidden(format!("calendar {id}")));
                }
                Ok(calendar)
            }
            None => calendars
                .into_iter()
                .find(|c| kind(c) && !c.read_only)
                .ok_or_else(|| not_found(if tasks { "a task list" } else { "a calendar" })),
        }
    }

    async fn calendar_by_id(&self, id: &CalendarId) -> Result<Calendar, Failure> {
        self.calendars()
            .await?
            .into_iter()
            .find(|c| &c.id == id)
            .ok_or_else(|| not_found(format!("calendar {id}")))
    }

    /// An event the client may change.
    async fn event(&self, id: &str) -> Result<(CalendarEvent, Calendar), Failure> {
        let response = self
            .shared
            .data
            .query(Request::GetEvent {
                event_id: EventId::new(id),
            })
            .await
            .map_err(|_| not_found(format!("event {id}")))?;
        let Response::PimEvent { event } = response else {
            return Err(unexpected(&response));
        };
        let calendar = self
            .calendar_by_id(&event.calendar_id)
            .await
            .map_err(|_| not_found(format!("event {id}")))?;
        Ok((*event, calendar))
    }

    async fn books(&self) -> Result<Vec<AddressBook>, Failure> {
        match self
            .shared
            .data
            .query(Request::ListAddressBooks { source_id: None })
            .await?
        {
            Response::AddressBooks { address_books } => Ok(address_books
                .into_iter()
                .filter(|b| allowed(self, b.account_id.as_ref()))
                .collect()),
            other => Err(unexpected(&other)),
        }
    }
}

fn kind_and_scopes<D: McpData>(ctx: &Ctx<'_, D>, mails: bool) -> Result<ActionKind, Failure> {
    let kind = if mails {
        ActionKind::Invite
    } else {
        ActionKind::Calendar
    };
    for scope in kind.mcp_scopes() {
        ctx.access.require(*scope)?;
    }
    Ok(kind)
}

/// `create_event`.
pub(crate) async fn create_event<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: CreateEventArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Calendar)?;
    let invite = a.send_invitations && !a.attendees.is_empty();
    let kind = kind_and_scopes(ctx, invite)?;
    if a.title.trim().is_empty() {
        return Err(invalid("the title is empty"));
    }
    let calendar = ctx.calendar(a.calendar_id.as_deref(), false).await?;
    let start = parse_time(&a.start, a.tz.as_deref())?;
    let end = parse_time(&a.end, a.tz.as_deref())?;
    let mut fields = EventFields::new(a.title.trim(), start, end);
    fields.location = opt(&a.location);
    fields.description = opt(&a.description);
    fields.attendees = a
        .attendees
        .iter()
        .map(|t| attendee(t, invite))
        .collect::<Result<_, _>>()?;
    issue(&fields.issues())?;
    let recurrence = match &a.recurrence {
        Some(text) => Some(Recurrence::rule(
            nail_ical::recur::parse_rrule(text, &fields.start)
                .ok_or_else(|| invalid(format!("invalid recurrence {text:?}")))?,
        )),
        None => None,
    };
    if let Some(r) = &recurrence {
        issue(&r.issues(&fields.start))?;
    }
    let mut targets = vec![target(ActionTargetKind::Event, "new", &fields.title)];
    if invite {
        targets.extend(
            fields
                .attendees
                .iter()
                .map(|t| target(ActionTargetKind::Recipient, t.email.as_str(), &t.email)),
        );
    }
    let summary = if invite {
        LocalizedMessage::new("confirmation-summary-invite")
            .arg("actor", ctx.actor())
            .arg("title", fields.title.as_str())
            .arg("count", fields.attendees.len())
    } else {
        LocalizedMessage::new("confirmation-summary-calendar")
            .arg("actor", ctx.actor())
            .arg("title", fields.title.as_str())
    };
    let mut changes = vec![
        change("action-field-title", None, Some(fields.title.clone())),
        change(
            "action-field-start",
            None,
            Some(pim_time_text(&fields.start)),
        ),
        change("action-field-end", None, Some(pim_time_text(&fields.end))),
    ];
    if fields.location.is_some() {
        changes.push(change(
            "action-field-location",
            None,
            fields.location.clone(),
        ));
    }
    if !fields.attendees.is_empty() {
        changes.push(change(
            "action-field-attendees",
            None,
            attendees_text(&fields.attendees),
        ));
    }
    if let Some(text) = &a.recurrence {
        changes.push(change(
            "mcp-tool-field-recurrence",
            None,
            Some(text.clone()),
        ));
    }
    Ok(Plan {
        kind,
        summary,
        targets,
        changes,
        accounts: calendar.account_id.iter().cloned().collect(),
        audit: vec![
            audit("calendar", None, Some(calendar.id.as_str().to_owned())),
            audit("start", None, Some(pim_time_text(&fields.start))),
            audit("end", None, Some(pim_time_text(&fields.end))),
            audit(
                "attendees",
                None,
                super::plan::count(fields.attendees.len()),
            ),
            audit("invitations", None, Some(invite.to_string())),
        ],
        sends: u32::from(invite),
        action: Action::CreateEvent {
            calendar_id: calendar.id,
            fields: Box::new(fields),
            recurrence,
            invite,
        },
    })
}

/// `update_event`.
pub(crate) async fn update_event<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: UpdateEventArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Calendar)?;
    let (event, calendar) = ctx.event(&a.event_id).await?;
    if event.read_only || calendar.read_only {
        return Err(forbidden(format!("event {}", a.event_id)));
    }
    let scope = match a.scope {
        McpEditScope::This => EditScope::This,
        McpEditScope::ThisAndFollowing => EditScope::ThisAndFollowing,
        McpEditScope::All => EditScope::All,
    };
    let recurrence_id = match (&a.occurrence, scope.needs_occurrence()) {
        (Some(text), _) => Some(occurrence(text, &event.fields.start)?),
        (None, true) => return Err(invalid("this scope needs the occurrence")),
        (None, false) => None,
    };
    let before = match (&recurrence_id, scope) {
        (Some(id), EditScope::This) => event
            .overrides
            .iter()
            .find(|o| &o.recurrence_id == id)
            .map_or_else(|| event.fields.clone(), |o| o.fields.clone()),
        _ => event.fields.clone(),
    };
    let mut fields = before.clone();
    if let Some(title) = &a.title {
        if title.trim().is_empty() {
            return Err(invalid("the title is empty"));
        }
        fields.title = title.trim().to_owned();
    }
    let tz = a.tz.as_deref().or_else(|| zone_of(&before.start));
    if let Some(start) = &a.start {
        fields.start = parse_time(start, tz)?;
    }
    if let Some(end) = &a.end {
        fields.end = parse_time(end, tz)?;
    }
    set_text(&mut fields.location, &a.location);
    set_text(&mut fields.description, &a.description);
    for text in &a.add_attendees {
        let new = attendee(text, a.send_updates)?;
        if !fields
            .attendees
            .iter()
            .any(|t| t.email.eq_ignore_ascii_case(&new.email))
        {
            fields.attendees.push(new);
        }
    }
    for text in &a.remove_attendees {
        let gone = parse_address(text)?;
        fields
            .attendees
            .retain(|t| !t.email.eq_ignore_ascii_case(&gone.email));
    }
    issue(&fields.issues())?;
    let invite = a.send_updates && !fields.attendees.is_empty();
    let kind = kind_and_scopes(ctx, invite)?;
    let mut changes = Vec::new();
    let mut lines = Vec::new();
    if before.title != fields.title {
        changes.push(change(
            "action-field-title",
            Some(before.title.clone()),
            Some(fields.title.clone()),
        ));
        lines.push(audit("title", None, Some("changed".into())));
    }
    for (label, field, old, new) in [
        ("action-field-start", "start", &before.start, &fields.start),
        ("action-field-end", "end", &before.end, &fields.end),
    ] {
        if old != new {
            changes.push(change(
                label,
                Some(pim_time_text(old)),
                Some(pim_time_text(new)),
            ));
            lines.push(audit(
                field,
                Some(pim_time_text(old)),
                Some(pim_time_text(new)),
            ));
        }
    }
    if before.location != fields.location {
        changes.push(change(
            "action-field-location",
            before.location.clone(),
            fields.location.clone(),
        ));
        lines.push(audit("location", None, Some("changed".into())));
    }
    if before.description != fields.description {
        changes.push(change(
            "mcp-tool-field-description",
            before.description.clone(),
            fields.description.clone(),
        ));
        lines.push(audit("description", None, Some("changed".into())));
    }
    if before.attendees != fields.attendees {
        changes.push(change(
            "action-field-attendees",
            attendees_text(&before.attendees),
            attendees_text(&fields.attendees),
        ));
        lines.push(audit(
            "attendees",
            super::plan::count(before.attendees.len()),
            super::plan::count(fields.attendees.len()),
        ));
    }
    if changes.is_empty() && !invite {
        return Err(invalid("nothing to change"));
    }
    lines.push(audit("invitations", None, Some(invite.to_string())));
    let mut targets = vec![target(
        ActionTargetKind::Event,
        event.id.as_str(),
        &fields.title,
    )];
    if invite {
        targets.extend(
            fields
                .attendees
                .iter()
                .map(|t| target(ActionTargetKind::Recipient, t.email.as_str(), &t.email)),
        );
    }
    let summary = if invite {
        LocalizedMessage::new("confirmation-summary-invite")
            .arg("actor", ctx.actor())
            .arg("title", fields.title.as_str())
            .arg("count", fields.attendees.len())
    } else {
        LocalizedMessage::new("confirmation-summary-calendar")
            .arg("actor", ctx.actor())
            .arg("title", fields.title.as_str())
    };
    let recurrence = (scope != EditScope::This)
        .then(|| event.recurrence.clone())
        .flatten();
    Ok(Plan {
        kind,
        summary,
        targets,
        changes,
        accounts: calendar.account_id.iter().cloned().collect(),
        audit: lines,
        sends: u32::from(invite),
        action: Action::UpdateEvent {
            event_id: event.id,
            scope,
            recurrence_id,
            fields: Box::new(fields),
            recurrence,
            invite,
        },
    })
}

/// `respond_to_invite`.
pub(crate) async fn respond_to_invite<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: RespondToInviteArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Calendar)?;
    let kind = kind_and_scopes(ctx, a.send)?;
    let (target_ref, title, item, accounts) = match (&a.message_id, &a.event_id) {
        (Some(message), None) => {
            if a.occurrence.is_some() {
                return Err(invalid("occurrence goes with event_id"));
            }
            let summary = ctx.target_message(message).await?;
            (
                InvitationTarget::Message {
                    message_id: summary.id.clone(),
                },
                summary.subject.clone(),
                target(
                    ActionTargetKind::Message,
                    summary.id.as_str(),
                    &summary.subject,
                ),
                vec![summary.account_id],
            )
        }
        (None, Some(event_id)) => {
            let (event, calendar) = ctx.event(event_id).await?;
            let recurrence_id = a
                .occurrence
                .as_deref()
                .map(|o| occurrence(o, &event.fields.start))
                .transpose()?;
            (
                InvitationTarget::Event {
                    event_id: event.id.clone(),
                    recurrence_id,
                },
                event.fields.title.clone(),
                target(
                    ActionTargetKind::Event,
                    event.id.as_str(),
                    &event.fields.title,
                ),
                calendar.account_id.iter().cloned().collect(),
            )
        }
        _ => return Err(invalid("give message_id or event_id")),
    };
    let (response, value_id) = match a.response {
        McpInviteResponse::Accept => (InvitationResponse::Accept, "mcp-tool-value-accept"),
        McpInviteResponse::Tentative => (InvitationResponse::Tentative, "mcp-tool-value-tentative"),
        McpInviteResponse::Decline => (InvitationResponse::Decline, "mcp-tool-value-decline"),
    };
    let mut changes = vec![change(
        "action-field-response",
        None,
        Some(ctx.text(value_id)),
    )];
    if let Some(comment) = opt(&a.comment) {
        changes.push(change("mcp-tool-field-comment", None, Some(comment)));
    }
    Ok(Plan {
        kind,
        summary: LocalizedMessage::new("confirmation-summary-respond")
            .arg("actor", ctx.actor())
            .arg("title", title.as_str()),
        targets: vec![item],
        changes,
        accounts,
        audit: vec![
            audit(
                "response",
                None,
                Some(format!("{:?}", a.response).to_lowercase()),
            ),
            audit("send", None, Some(a.send.to_string())),
        ],
        sends: u32::from(a.send),
        action: Action::Requests {
            requests: vec![Request::RespondToInvitation {
                target: target_ref,
                response,
                comment: opt(&a.comment),
                send: a.send,
                calendar_id: None,
            }],
        },
    })
}

fn labeled(values: &[String]) -> Vec<LabeledValue> {
    values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .enumerate()
        .map(|(i, v)| LabeledValue {
            value: v.to_owned(),
            kinds: Vec::new(),
            label: None,
            preferred: i == 0,
        })
        .collect()
}

fn values_text(list: &[LabeledValue]) -> Option<String> {
    joined(list.iter().map(|v| v.value.as_str()))
}

/// `upsert_contact`.
pub(crate) async fn upsert_contact<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: UpsertContactArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Pim)?;
    let books = ctx.books().await?;
    let (existing, book): (Option<Contact>, AddressBook) = match &a.contact_id {
        Some(id) => {
            let response = ctx
                .shared
                .data
                .query(Request::GetContact {
                    contact_id: ContactId::new(id.clone()),
                })
                .await
                .map_err(|_| not_found(format!("contact {id}")))?;
            let Response::PimContact { contact } = response else {
                return Err(unexpected(&response));
            };
            let book = books
                .into_iter()
                .find(|b| b.id == contact.address_book_id)
                .ok_or_else(|| not_found(format!("contact {id}")))?;
            if contact.read_only || book.read_only {
                return Err(forbidden(format!("contact {id}")));
            }
            (Some(*contact), book)
        }
        None => {
            let book = match &a.address_book_id {
                Some(id) => {
                    let book = books
                        .into_iter()
                        .find(|b| b.id.as_str() == id)
                        .ok_or_else(|| not_found(format!("address book {id}")))?;
                    if book.read_only {
                        return Err(forbidden(format!("address book {id}")));
                    }
                    book
                }
                None => books
                    .into_iter()
                    .find(|b| !b.read_only && b.role != Some(AddressBookRole::Collected))
                    .ok_or_else(|| not_found("an address book"))?,
            };
            (None, book)
        }
    };
    let before = existing.as_ref().map(|c| c.fields.clone());
    let mut fields = match &before {
        Some(fields) => fields.clone(),
        None => {
            let name = a
                .full_name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or_else(|| a.emails.as_ref().and_then(|e| e.first().cloned()))
                .ok_or_else(|| invalid("a new contact needs full_name or an email address"))?;
            ContactFields::named(name.trim())
        }
    };
    if let Some(name) = &a.full_name {
        if name.trim().is_empty() {
            return Err(invalid("full_name is empty"));
        }
        fields.display_name = name.trim().to_owned();
    }
    if let Some(emails) = &a.emails {
        fields.emails = labeled(emails);
    }
    if let Some(phones) = &a.phones {
        fields.phones = labeled(phones);
    }
    if let Some(org) = &a.organization {
        fields.organization = (!org.trim().is_empty()).then(|| ContactOrg {
            name: org.trim().to_owned(),
            units: Vec::new(),
        });
    }
    set_text(&mut fields.title, &a.title);
    set_text(&mut fields.notes, &a.notes);
    issue(&fields.issues())?;
    let old = before.as_ref();
    let mut changes = Vec::new();
    let mut lines = Vec::new();
    let mut line = |label: &str, field: &'static str, b: Option<String>, n: Option<String>| {
        if b != n {
            lines.push(audit(
                field,
                b.as_ref().map(|_| "set".into()),
                n.as_ref().map(|_| "set".into()),
            ));
            changes.push(change(label, b, n));
        }
    };
    line(
        "action-field-name",
        "name",
        old.map(|f| f.display_name.clone()),
        Some(fields.display_name.clone()),
    );
    line(
        "action-field-emails",
        "emails",
        old.and_then(|f| values_text(&f.emails)),
        values_text(&fields.emails),
    );
    line(
        "mcp-tool-field-phones",
        "phones",
        old.and_then(|f| values_text(&f.phones)),
        values_text(&fields.phones),
    );
    line(
        "mcp-tool-field-organization",
        "organization",
        old.and_then(|f| f.organization.as_ref().map(|o| o.name.clone())),
        fields.organization.as_ref().map(|o| o.name.clone()),
    );
    line(
        "mcp-tool-field-job-title",
        "title",
        old.and_then(|f| f.title.clone()),
        fields.title.clone(),
    );
    line(
        "mcp-tool-field-notes",
        "notes",
        old.and_then(|f| f.notes.clone()),
        fields.notes.clone(),
    );
    if changes.is_empty() {
        return Err(invalid("nothing to change"));
    }
    let request = match &existing {
        Some(contact) => Request::UpdateContact {
            contact_id: contact.id.clone(),
            fields: Box::new(fields.clone()),
        },
        None => Request::CreateContact {
            address_book_id: AddressBookId::new(book.id.as_str()),
            fields: Box::new(fields.clone()),
        },
    };
    Ok(Plan {
        kind: ActionKind::Contact,
        summary: LocalizedMessage::new("confirmation-summary-contact")
            .arg("actor", ctx.actor())
            .arg("name", fields.display_name.as_str()),
        targets: vec![target(
            ActionTargetKind::Contact,
            existing.as_ref().map_or("new", |c| c.id.as_str()),
            &fields.display_name,
        )],
        changes,
        accounts: book.account_id.iter().cloned().collect(),
        audit: lines,
        sends: 0,
        action: Action::Requests {
            requests: vec![request],
        },
    })
}

/// A due date: `YYYY-MM-DD` or RFC 3339 (UTC).
fn due(text: &str) -> Result<PimTime, Failure> {
    parse_time(text, None)
}

/// `upsert_task`.
pub(crate) async fn upsert_task<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: UpsertTaskArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Pim)?;
    if a.priority.is_some_and(|p| p > 9) {
        return Err(invalid("priority is 0 to 9"));
    }
    let (existing, list): (Option<Task>, Calendar) = match &a.task_id {
        Some(id) => {
            let response = ctx
                .shared
                .data
                .query(Request::GetTask {
                    task_id: TaskId::new(id.clone()),
                })
                .await
                .map_err(|_| not_found(format!("task {id}")))?;
            let Response::PimTask { task } = response else {
                return Err(unexpected(&response));
            };
            let list = ctx
                .calendar_by_id(&task.calendar_id)
                .await
                .map_err(|_| not_found(format!("task {id}")))?;
            if task.read_only || list.read_only {
                return Err(forbidden(format!("task {id}")));
            }
            (Some(*task), list)
        }
        None => (None, ctx.calendar(a.list_id.as_deref(), true).await?),
    };
    let before = existing.as_ref().map(|t| t.fields.clone());
    let mut fields = match &before {
        Some(fields) => fields.clone(),
        None => {
            let title = a
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .ok_or_else(|| invalid("a new task needs a title"))?;
            TaskFields::titled(title.trim())
        }
    };
    if let Some(title) = &a.title {
        if title.trim().is_empty() {
            return Err(invalid("the title is empty"));
        }
        fields.title = title.trim().to_owned();
    }
    if let Some(text) = &a.due {
        fields.due = if text.trim().is_empty() {
            None
        } else {
            Some(due(text)?)
        };
    }
    set_text(&mut fields.description, &a.notes);
    if let Some(priority) = a.priority {
        fields.priority = (priority > 0).then_some(priority);
    }
    let mut related = None;
    if let Some(message) = &a.related_message_id {
        let summary = ctx.target_message(message).await?;
        let already = fields.links.iter().any(
            |l| matches!(l, PimLink::Message { message_id: Some(id), .. } if *id == summary.id),
        );
        if !already {
            fields.links.push(PimLink::Message {
                message_id: Some(summary.id.clone()),
                rfc822_id: None,
                subject: Some(summary.subject.clone()),
            });
        }
        related = Some(summary);
    }
    let was_done = before
        .as_ref()
        .is_some_and(|f| f.status == TaskStatus::Completed);
    let complete = a.completed.filter(|c| *c != was_done);
    if existing.is_none() && complete == Some(true) {
        fields.status = TaskStatus::Completed;
        fields.completed_at = Some(Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
    issue(&fields.issues())?;
    let old = before.as_ref();
    let mut changes = Vec::new();
    let mut lines = Vec::new();
    if old.map(|f| &f.title) != Some(&fields.title) {
        changes.push(change(
            "action-field-title",
            old.map(|f| f.title.clone()),
            Some(fields.title.clone()),
        ));
        lines.push(audit("title", None, Some("set".into())));
    }
    if old.map_or(fields.due.is_some(), |f| f.due != fields.due) {
        let text = |t: &Option<PimTime>| t.as_ref().map(pim_time_text);
        changes.push(change(
            "action-field-due",
            old.and_then(|f| text(&f.due)),
            text(&fields.due),
        ));
        lines.push(audit(
            "due",
            old.and_then(|f| text(&f.due)),
            text(&fields.due),
        ));
    }
    if old.map_or(fields.description.is_some(), |f| {
        f.description != fields.description
    }) {
        changes.push(change(
            "mcp-tool-field-notes",
            old.and_then(|f| f.description.clone()),
            fields.description.clone(),
        ));
        lines.push(audit("notes", None, Some("changed".into())));
    }
    if old.map_or(fields.priority.is_some(), |f| f.priority != fields.priority) {
        let text = |p: Option<u8>| p.map(|p| p.to_string());
        changes.push(change(
            "mcp-tool-field-priority",
            old.and_then(|f| text(f.priority)),
            text(fields.priority),
        ));
        lines.push(audit(
            "priority",
            old.and_then(|f| text(f.priority)),
            text(fields.priority),
        ));
    }
    if let Some(done) = complete {
        changes.push(change(
            "action-field-completed",
            existing.as_ref().map(|_| ctx.yes_no(was_done)),
            Some(ctx.yes_no(done)),
        ));
        lines.push(audit(
            "completed",
            Some(was_done.to_string()),
            Some(done.to_string()),
        ));
    }
    if let Some(message) = &related {
        changes.push(change(
            "mcp-tool-field-related-message",
            None,
            Some(message.subject.clone()),
        ));
        lines.push(audit(
            "related_message",
            None,
            Some(message.id.as_str().to_owned()),
        ));
    }
    if changes.is_empty() {
        return Err(invalid("nothing to change"));
    }
    let mut requests = Vec::new();
    match &existing {
        Some(task) => {
            if Some(&fields) != before.as_ref() {
                requests.push(Request::UpdateTask {
                    task_id: task.id.clone(),
                    fields: Box::new(fields.clone()),
                    recurrence: task.recurrence.clone(),
                });
            }
            if let Some(done) = complete {
                requests.push(Request::CompleteTask {
                    task_id: task.id.clone(),
                    completed: done,
                });
            }
        }
        None => requests.push(Request::CreateTask {
            calendar_id: list.id.clone(),
            fields: Box::new(fields.clone()),
            recurrence: None,
        }),
    }
    Ok(Plan {
        kind: ActionKind::Task,
        summary: LocalizedMessage::new("confirmation-summary-task")
            .arg("actor", ctx.actor())
            .arg("title", fields.title.as_str()),
        targets: vec![target(
            ActionTargetKind::Task,
            existing.as_ref().map_or("new", |t| t.id.as_str()),
            &fields.title,
        )],
        changes,
        accounts: list.account_id.iter().cloned().collect(),
        audit: lines,
        sends: 0,
        action: Action::Requests { requests },
    })
}

/// A PIM delete (plug-ins only: `nail.calendar.deleteEvent`,
/// `nail.contacts.delete`, `nail.tasks.delete`; MCP has no delete tool for
/// PIM items). Always the `pim-delete` kind, which always asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PimDelete {
    /// An event, an occurrence or the following occurrences.
    Event {
        /// The event (series).
        event_id: String,
        /// For recurring events: the occurrence's original start.
        occurrence: Option<String>,
        /// Which occurrences.
        scope: McpEditScope,
    },
    /// A contact.
    Contact {
        /// The contact.
        contact_id: String,
    },
    /// A task (its subtasks move up a level).
    Task {
        /// The task.
        task_id: String,
    },
}

/// The plan of a [`PimDelete`].
pub(crate) async fn delete_item<D: McpData>(
    ctx: &Ctx<'_, D>,
    delete: PimDelete,
) -> Result<Plan, Failure> {
    for scope in ActionKind::PimDelete.mcp_scopes() {
        ctx.access.require(*scope)?;
    }
    let (kind, id, label, account, request, audit_line) = match delete {
        PimDelete::Event {
            event_id,
            occurrence: when,
            scope,
        } => {
            let (event, calendar) = ctx.event(&event_id).await?;
            if event.read_only || calendar.read_only {
                return Err(forbidden(format!("event {event_id}")));
            }
            let scope = match scope {
                McpEditScope::This => EditScope::This,
                McpEditScope::ThisAndFollowing => EditScope::ThisAndFollowing,
                McpEditScope::All => EditScope::All,
            };
            let recurrence_id = match (&when, scope.needs_occurrence()) {
                (Some(text), _) => Some(occurrence(text, &event.fields.start)?),
                (None, true) => return Err(invalid("this scope needs the occurrence")),
                (None, false) => None,
            };
            let line = audit(
                "occurrence",
                recurrence_id.as_ref().map(pim_time_text),
                None,
            );
            (
                ActionTargetKind::Event,
                event.id.as_str().to_owned(),
                event.fields.title.clone(),
                calendar.account_id.clone(),
                Request::DeleteEvent {
                    event_id: event.id.clone(),
                    scope,
                    recurrence_id,
                },
                line,
            )
        }
        PimDelete::Contact { contact_id } => {
            let response = ctx
                .shared
                .data
                .query(Request::GetContact {
                    contact_id: ContactId::new(contact_id.clone()),
                })
                .await
                .map_err(|_| not_found(format!("contact {contact_id}")))?;
            let Response::PimContact { contact } = response else {
                return Err(unexpected(&response));
            };
            let book = ctx
                .books()
                .await?
                .into_iter()
                .find(|b| b.id == contact.address_book_id)
                .ok_or_else(|| not_found(format!("contact {contact_id}")))?;
            if contact.read_only || book.read_only {
                return Err(forbidden(format!("contact {contact_id}")));
            }
            (
                ActionTargetKind::Contact,
                contact.id.as_str().to_owned(),
                contact.fields.display_name.clone(),
                book.account_id.clone(),
                Request::DeleteContacts {
                    contact_ids: vec![contact.id.clone()],
                },
                audit("contact", None, None),
            )
        }
        PimDelete::Task { task_id } => {
            let response = ctx
                .shared
                .data
                .query(Request::GetTask {
                    task_id: TaskId::new(task_id.clone()),
                })
                .await
                .map_err(|_| not_found(format!("task {task_id}")))?;
            let Response::PimTask { task } = response else {
                return Err(unexpected(&response));
            };
            let list = ctx
                .calendar_by_id(&task.calendar_id)
                .await
                .map_err(|_| not_found(format!("task {task_id}")))?;
            if task.read_only || list.read_only {
                return Err(forbidden(format!("task {task_id}")));
            }
            (
                ActionTargetKind::Task,
                task.id.as_str().to_owned(),
                task.fields.title.clone(),
                list.account_id.clone(),
                Request::DeleteTask {
                    task_id: task.id.clone(),
                    with_subtasks: false,
                },
                audit("task", None, None),
            )
        }
    };
    Ok(Plan {
        kind: ActionKind::PimDelete,
        summary: LocalizedMessage::new("confirmation-summary-pim-delete")
            .arg("actor", ctx.actor())
            .arg("count", 1),
        targets: vec![target(kind, id, &label)],
        changes: Vec::new(),
        accounts: account.into_iter().collect(),
        audit: vec![audit_line],
        sends: 0,
        action: Action::Requests {
            requests: vec![request],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_as_dates_instants_and_local_times() {
        assert_eq!(
            parse_time("2026-10-05", None).unwrap(),
            PimTime::date("2026-10-05")
        );
        assert_eq!(
            parse_time("2026-10-05T09:00:00+02:00", None).unwrap(),
            PimTime::utc("2026-10-05T07:00:00")
        );
        assert_eq!(
            parse_time("2026-10-05T07:00:00Z", Some("Europe/Berlin")).unwrap(),
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            parse_time("2026-10-05T09:00:00", Some("Europe/Berlin")).unwrap(),
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            parse_time("2026-10-05T09:00:00Z", Some("utc")).unwrap(),
            PimTime::utc("2026-10-05T09:00:00")
        );
        assert!(parse_time("2026-10-05T09:00:00", None).is_err());
        assert!(parse_time("2026-13-05", None).is_err());
        assert!(parse_time("soon", None).is_err());
        assert!(parse_time("2026-10-05T09:00:00Z", Some("Mars/Base")).is_err());
    }

    #[test]
    fn occurrences_follow_the_series_start() {
        let zoned = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin");
        assert_eq!(
            occurrence("2026-10-12T07:00:00Z", &zoned).unwrap(),
            PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            occurrence("2026-10-12T00:00:00Z", &PimTime::date("2026-10-05")).unwrap(),
            PimTime::date("2026-10-12")
        );
        assert_eq!(
            occurrence(
                "2026-10-12T09:00:00+02:00",
                &PimTime::floating("2026-10-05T09:00:00")
            )
            .unwrap(),
            PimTime::floating("2026-10-12T09:00:00")
        );
    }

    #[test]
    fn labeled_values_mark_the_first_preferred() {
        let values = labeled(&["a@x.org".into(), " ".into(), "b@x.org".into()]);
        assert_eq!(values.len(), 2);
        assert!(values[0].preferred && !values[1].preferred);
        assert_eq!(values_text(&values).as_deref(), Some("a@x.org, b@x.org"));
        let mut field = Some("x".to_owned());
        set_text(&mut field, &Some(" ".into()));
        assert_eq!(field, None);
        set_text(&mut field, &None);
        assert_eq!(field, None);
    }
}
