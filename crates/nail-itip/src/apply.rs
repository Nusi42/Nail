//! Applying iTIP messages to the user's copy of an event (RFC 5546 §3.2).
//!
//! Everything here works on the contract's [`CalendarEvent`]; the core
//! writes the result through the PIM queue and `nail-ical`'s patching
//! writer (`write_event` with the stored text), so unknown properties and
//! parameters (`SCHEDULE-AGENT`, `X-…`) survive.
//!
//! * `REQUEST` / `PUBLISH` — a new event ([`new_event`]) or an update of
//!   the series or of single occurrences ([`apply_request`]). The user's
//!   own alarms and colour stay (they are personal, not the organizer's).
//! * `ADD` — new occurrences become `RDATE`s with their own fields
//!   ([`apply_add`]).
//! * `CANCEL` — the event goes, or one occurrence becomes an `EXDATE`, or
//!   the series ends before a `THISANDFUTURE` occurrence ([`apply_cancel`]).
//! * `REPLY` — the attendee's `PARTSTAT` on the series or on one
//!   occurrence (an override is created for it); delegation adds the
//!   delegate ([`apply_reply`]).
//! * The attendee's own answer ([`set_my_status`], [`delegate`]) and the
//!   organizer accepting a `COUNTER` ([`accept_counter`]).

use chrono_tz::Tz;
use nail_api::pim::PimTime;
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::pim::calendar::{Attendee, CalendarEvent, EventFields, EventOverride, Recurrence};
use nail_api::scheduling::CounterProposal;
use nail_ical::ParsedEvent;
use nail_ical::edit;
use nail_ical::expand::Span;
use nail_ical::time;

use crate::address::{normalize, same};
use crate::card::same_occurrence;
use crate::object::{ItipAttendee, ItipComponent, ItipObject};

/// Why a message cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    /// The message carries no usable event data (no `DTSTART`).
    #[error("the message carries no event data")]
    NoEventData,
    /// The message is about another `UID`.
    #[error("the message is about another event")]
    WrongEvent,
    /// The recurrence id is not an occurrence of the stored series.
    #[error("not an occurrence of the series")]
    NotAnOccurrence,
    /// A reply without an attendee.
    #[error("the reply names no attendee")]
    NoAttendee,
}

/// The outcome of applying a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The event after the change.
    Updated(Box<CalendarEvent>),
    /// The event is to be deleted.
    Delete,
    /// Nothing to do (e.g. a `CANCEL` for other attendees).
    Unchanged,
}

/// A new event from a `REQUEST` / `PUBLISH`.
#[derive(Debug, Clone)]
pub struct NewEvent {
    /// The series as read (the core gives it ids with `into_event`).
    pub event: ParsedEvent,
    /// The text to store (no `METHOD`).
    pub text: String,
}

/// The event of a `REQUEST` / `PUBLISH` for a calendar that does not have
/// it yet.
pub fn new_event(object: &ItipObject) -> Result<NewEvent, ApplyError> {
    let event = object.event.clone().ok_or(ApplyError::NoEventData)?;
    let text = object.storable_text().ok_or(ApplyError::NoEventData)?;
    Ok(NewEvent { event, text })
}

fn check_uid(existing: &CalendarEvent, object: &ItipObject) -> Result<(), ApplyError> {
    match object.uid.as_deref() {
        Some(uid) if uid == existing.uid => Ok(()),
        _ => Err(ApplyError::WrongEvent),
    }
}

/// The user's personal parts of a field set (alarms, colour) carried over
/// to the organizer's new version. The stored organizer stays too: an
/// update never changes who organizes the event (a message naming another
/// organizer is `organizer-changed` on the card and applied only by hand).
fn keep_personal(new: &mut EventFields, old: &EventFields) {
    new.alarms.clone_from(&old.alarms);
    if new.color.is_none() {
        new.color.clone_from(&old.color);
    }
    if old.organizer.is_some() {
        new.organizer.clone_from(&old.organizer);
    }
}

/// Fields of a new override for an occurrence: the master's, moved to
/// the occurrence.
fn occurrence_fields(event: &CalendarEvent, recurrence_id: &PimTime) -> Option<EventFields> {
    let master = &event.fields;
    let local = time::to_frame(recurrence_id, &master.start)?;
    let start = time::like(local, &master.start);
    let span = Span::between(&master.start, &master.end, Tz::UTC);
    let end = span.end(&start, &master.end, Tz::UTC);
    let mut fields = master.clone();
    fields.start = start;
    fields.end = end;
    Some(fields)
}

/// The fields of an occurrence for writing, creating its override from
/// the master when needed. `None` = the series (the master's fields).
fn fields_mut<'a>(
    event: &'a mut CalendarEvent,
    recurrence_id: Option<&PimTime>,
) -> Result<&'a mut EventFields, ApplyError> {
    let Some(rid) = recurrence_id else {
        return Ok(&mut event.fields);
    };
    if !event.is_recurring() {
        // A single event: the occurrence is the event itself.
        return if same_occurrence(rid, &event.fields.start, &event.fields.start) {
            Ok(&mut event.fields)
        } else {
            Err(ApplyError::NotAnOccurrence)
        };
    }
    let start = event.fields.start.clone();
    if !event
        .overrides
        .iter()
        .any(|o| same_occurrence(&o.recurrence_id, rid, &start))
    {
        let fields = occurrence_fields(event, rid).ok_or(ApplyError::NotAnOccurrence)?;
        edit::override_occurrence(event, rid, fields).map_err(|_| ApplyError::NotAnOccurrence)?;
    }
    event
        .overrides
        .iter_mut()
        .find(|o| same_occurrence(&o.recurrence_id, rid, &start))
        .map(|o| &mut o.fields)
        .ok_or(ApplyError::NotAnOccurrence)
}

/// Applies a `REQUEST` / `PUBLISH` to the stored copy: the series (when
/// the message has a master) or the occurrences it names. Occurrences
/// the series does not generate are added as `RDATE`s.
pub fn apply_request(
    existing: &CalendarEvent,
    object: &ItipObject,
) -> Result<CalendarEvent, ApplyError> {
    check_uid(existing, object)?;
    let mut out = existing.clone();
    let sequence = object.version().sequence;
    if object.master().is_some() {
        let parsed = object.event.as_ref().ok_or(ApplyError::NoEventData)?;
        let mut fields = parsed.fields.clone();
        keep_personal(&mut fields, &existing.fields);
        out.fields = fields;
        out.recurrence.clone_from(&parsed.recurrence);
        out.overrides = parsed
            .overrides
            .iter()
            .map(|o| {
                let mut o = o.clone();
                let old = existing
                    .overrides
                    .iter()
                    .find(|p| {
                        same_occurrence(&p.recurrence_id, &o.recurrence_id, &existing.fields.start)
                    })
                    .map_or(&existing.fields, |p| &p.fields);
                keep_personal(&mut o.fields, old);
                o
            })
            .collect();
    } else {
        let mut any = false;
        for c in object.occurrences() {
            let (Some(rid), Some(fields)) = (&c.recurrence_id, &c.fields) else {
                continue;
            };
            any = true;
            put_occurrence(&mut out, rid, fields.clone(), c.this_and_future)?;
        }
        if !any {
            return Err(ApplyError::NoEventData);
        }
    }
    out.sequence = out.sequence.max(sequence);
    Ok(out)
}

/// Sets an occurrence's fields, adding it as an `RDATE` when the series
/// does not generate it.
fn put_occurrence(
    event: &mut CalendarEvent,
    rid: &PimTime,
    mut fields: EventFields,
    this_and_future: bool,
) -> Result<(), ApplyError> {
    if !event.is_recurring() {
        if same_occurrence(rid, &event.fields.start, &event.fields.start) {
            keep_personal(&mut fields, &event.fields);
            event.fields = fields;
            return Ok(());
        }
        // A single event gains a second occurrence: it recurs by dates.
        event.recurrence = Some(Recurrence::default());
    }
    let start = event.fields.start.clone();
    if !edit::is_occurrence(event, rid) {
        let local = time::to_frame(rid, &start).ok_or(ApplyError::NotAnOccurrence)?;
        let date = time::like(local, &start);
        let recurrence = event.recurrence.get_or_insert_with(Recurrence::default);
        recurrence
            .exceptions
            .retain(|x| !same_occurrence(x, &date, &start));
        if !recurrence.dates.contains(&date) {
            recurrence.dates.push(date);
        }
    }
    let old = event
        .overrides
        .iter()
        .find(|o| same_occurrence(&o.recurrence_id, rid, &start))
        .map_or(&event.fields, |o| &o.fields)
        .clone();
    keep_personal(&mut fields, &old);
    edit::override_occurrence(event, rid, fields).map_err(|_| ApplyError::NotAnOccurrence)?;
    if this_and_future
        && let Some(o) = event
            .overrides
            .iter_mut()
            .find(|o| same_occurrence(&o.recurrence_id, rid, &start))
    {
        o.this_and_future = true;
    }
    Ok(())
}

/// Applies an `ADD`: every component is a new occurrence.
pub fn apply_add(
    existing: &CalendarEvent,
    object: &ItipObject,
) -> Result<CalendarEvent, ApplyError> {
    check_uid(existing, object)?;
    let mut out = existing.clone();
    let mut any = false;
    for c in &object.components {
        let Some(fields) = &c.fields else { continue };
        let rid = c
            .recurrence_id
            .clone()
            .unwrap_or_else(|| fields.start.clone());
        put_occurrence(&mut out, &rid, fields.clone(), false)?;
        any = true;
    }
    if !any {
        return Err(ApplyError::NoEventData);
    }
    out.sequence = out.sequence.max(object.version().sequence);
    Ok(out)
}

/// Applies a `CANCEL`. A cancel that lists attendees without the user
/// (`my_address`) is for others and changes nothing.
pub fn apply_cancel(
    existing: &CalendarEvent,
    object: &ItipObject,
    my_address: Option<&str>,
) -> Result<Applied, ApplyError> {
    check_uid(existing, object)?;
    let Some(main) = object.main() else {
        return Err(ApplyError::NoEventData);
    };
    if let Some(me) = my_address
        && !main.attendees.is_empty()
        && main.attendee(me).is_none()
    {
        return Ok(Applied::Unchanged);
    }
    let mut out = existing.clone();
    let mut changed = false;
    for c in &object.components {
        let Some(rid) = &c.recurrence_id else {
            return Ok(Applied::Delete);
        };
        if !out.is_recurring() {
            if same_occurrence(rid, &out.fields.start, &out.fields.start) {
                return Ok(Applied::Delete);
            }
            continue;
        }
        if c.this_and_future {
            match edit::end_before(&mut out, rid) {
                Ok(_) => changed = true,
                Err(edit::EditError::FirstOccurrence) => return Ok(Applied::Delete),
                Err(_) => {}
            }
        } else if edit::delete_occurrence(&mut out, rid).is_ok() {
            changed = true;
        }
    }
    if !changed {
        return Ok(Applied::Unchanged);
    }
    out.sequence = out.sequence.max(object.version().sequence);
    Ok(Applied::Updated(Box::new(out)))
}

/// Sets an attendee's answer on a field set, adding the attendee when
/// missing (an answer from someone not invited: RFC 5546 §2.1.5).
fn set_status(fields: &mut EventFields, from: &Attendee) {
    match fields
        .attendees
        .iter_mut()
        .find(|a| same(&a.email, &from.email))
    {
        Some(a) => {
            a.status = from.status;
            a.rsvp = false;
            a.delegated_to.clone_from(&from.delegated_to);
            if a.name.is_none() {
                a.name.clone_from(&from.name);
            }
        }
        None => {
            let mut added = from.clone();
            added.rsvp = false;
            fields.attendees.push(added);
        }
    }
}

/// Adds delegates (attendees with `DELEGATED-FROM`) that are not listed.
fn add_delegates(fields: &mut EventFields, delegator: &Attendee, delegates: &[ItipAttendee]) {
    for d in delegates {
        if fields
            .attendees
            .iter()
            .any(|a| same(&a.email, &d.attendee.email))
        {
            continue;
        }
        let mut added = d.attendee.clone();
        added.role = delegator.role;
        if added.delegated_from.is_empty() {
            added.delegated_from = vec![normalize(&delegator.email)];
        }
        fields.attendees.push(added);
    }
}

fn apply_reply_component(event: &mut CalendarEvent, c: &ItipComponent) -> Result<(), ApplyError> {
    let (first, delegates) = c.attendees.split_first().ok_or(ApplyError::NoAttendee)?;
    let from = &first.attendee;
    if c.recurrence_id.is_none() {
        // Occurrences that had the same answer as the series follow it;
        // ones answered on their own keep their answer.
        let before = event
            .fields
            .attendees
            .iter()
            .find(|a| same(&a.email, &from.email))
            .map(|a| a.status);
        for o in &mut event.overrides {
            let own = o
                .fields
                .attendees
                .iter()
                .find(|a| same(&a.email, &from.email));
            if own.is_some_and(|a| Some(a.status) == before) {
                set_status(&mut o.fields, from);
                add_delegates(&mut o.fields, from, delegates);
            }
        }
    }
    let fields = fields_mut(event, c.recurrence_id.as_ref())?;
    set_status(fields, from);
    add_delegates(fields, from, delegates);
    Ok(())
}

/// Applies a `REPLY` (organizer side): the attendee's `PARTSTAT` (and
/// delegation) on the series or the occurrences it names.
pub fn apply_reply(
    existing: &CalendarEvent,
    object: &ItipObject,
) -> Result<CalendarEvent, ApplyError> {
    check_uid(existing, object)?;
    let mut out = existing.clone();
    if object.components.is_empty() {
        return Err(ApplyError::NoAttendee);
    }
    for c in &object.components {
        apply_reply_component(&mut out, c)?;
    }
    Ok(out)
}

/// The attendee's own answer in their copy (`RespondToInvitation`): the
/// user's `PARTSTAT` on the series or one occurrence; the user is added
/// when not listed (answering a forwarded invitation).
pub fn set_my_status(
    existing: &CalendarEvent,
    my_address: &str,
    my_name: Option<&str>,
    status: ParticipationStatus,
    recurrence_id: Option<&PimTime>,
) -> Result<CalendarEvent, ApplyError> {
    let mut out = existing.clone();
    let mut me = Attendee::new(normalize(my_address));
    me.name = my_name.map(str::to_owned);
    me.status = status;
    let fields = fields_mut(&mut out, recurrence_id)?;
    set_status(fields, &me);
    Ok(out)
}

/// Delegation (RFC 5546 §3.2.2.3) in the attendee's copy: the user's
/// entry becomes `DELEGATED` with `DELEGATED-TO`, the delegate is added
/// with `DELEGATED-FROM` and `RSVP=TRUE`.
pub fn delegate(
    existing: &CalendarEvent,
    my_address: &str,
    delegate_to: &Attendee,
    recurrence_id: Option<&PimTime>,
) -> Result<CalendarEvent, ApplyError> {
    let mut out = existing.clone();
    let me_email = normalize(my_address);
    let delegate_email = normalize(&delegate_to.email);
    let fields = fields_mut(&mut out, recurrence_id)?;
    let role = fields
        .attendees
        .iter()
        .find(|a| same(&a.email, &me_email))
        .map(|a| a.role)
        .unwrap_or_default();
    let mut me = Attendee::new(me_email.clone());
    me.status = ParticipationStatus::Delegated;
    me.delegated_to = vec![delegate_email.clone()];
    set_status(fields, &me);
    fields
        .attendees
        .retain(|a| !same(&a.email, &delegate_email));
    let mut added = delegate_to.clone();
    added.email = delegate_email;
    added.role = role;
    added.status = ParticipationStatus::NeedsAction;
    added.rsvp = true;
    added.delegated_from = vec![me_email];
    fields.attendees.push(added);
    Ok(out)
}

/// The organizer accepts a `COUNTER`: the proposal's time (and location)
/// on the series or the occurrence, then the update rules of
/// [`crate::order::prepare_update`] (`SEQUENCE` + 1, answers reset).
pub fn accept_counter(
    existing: &CalendarEvent,
    proposal: &CounterProposal,
    recurrence_id: Option<&PimTime>,
    organizer_address: &str,
) -> Result<CalendarEvent, ApplyError> {
    let mut out = existing.clone();
    {
        let fields = fields_mut(&mut out, recurrence_id)?;
        fields.start = proposal.start.clone();
        fields.end = proposal.end.clone();
        if let Some(location) = proposal.location.as_ref().filter(|l| !l.trim().is_empty()) {
            fields.location = Some(location.clone());
        }
    }
    crate::order::prepare_update(existing, &mut out, organizer_address);
    Ok(out)
}

/// The override of an occurrence, if the event has one.
pub fn override_for<'a>(event: &'a CalendarEvent, rid: &PimTime) -> Option<&'a EventOverride> {
    event
        .overrides
        .iter()
        .find(|o| same_occurrence(&o.recurrence_id, rid, &event.fields.start))
}

#[cfg(test)]
mod tests;
