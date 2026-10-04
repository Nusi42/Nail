//! The invitation card: one iTIP message compared with the user's
//! identities and calendars (contract `nail_api::scheduling::Invitation`).
//!
//! * **Role** — the user organizes the event when one of the user's
//!   addresses is the `ORGANIZER`; is an attendee when one is an
//!   `ATTENDEE` (the address of the account the message arrived in wins
//!   when several are listed); else an observer (forwarded invitation,
//!   `PUBLISH`, or a message about someone else's event).
//! * **Sender checks** — organizer methods (`REQUEST`, `ADD`, `CANCEL`,
//!   `DECLINECOUNTER`) must come from the organizer; attendee methods
//!   (`REPLY`, `COUNTER`, `REFRESH`) from the attendee they name. A
//!   mismatch is a spoofing warning (`sender-not-organizer` /
//!   `sender-not-attendee`). A sender that matches only the `SENT-BY` the
//!   message itself names is `sent-on-behalf` (the claim cannot be
//!   verified). The card still offers its actions; the core never applies
//!   such a message on its own ([`ItipIssue::blocks_automatic`]).
//! * **Stored copy checks** — a message from the organizer side that names
//!   another organizer than the user's copy is `organizer-changed` (anyone
//!   who knows the `UID` could otherwise rewrite the event); an answer
//!   from someone who is neither invited nor a declared delegate is
//!   `uninvited-attendee`.
//! * **State** — against the stored copy and the stored scheduling state
//!   with [`crate::order::Version`]: newer = `update`, equal = `current`,
//!   older = `outdated` (stale updates are ignored).
//! * **Actions** — what the card may offer for this role, method and
//!   state.

use nail_api::ids::{IdentityId, MessageId};
use nail_api::pim::PimTime;
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::pim::calendar::{Attendee, CalendarEvent, EventFields, EventStatus};
use nail_api::scheduling::{
    CounterProposal, Invitation, InvitationAction, InvitationConflict, InvitationRole,
    InvitationState, ItipIssue, ItipMethod, SchedulingTransport,
};

use crate::address::{UserAddresses, same};
use crate::object::{ItipAttendee, ItipComponent, ItipObject};
use crate::order::Version;

/// The user's copy of the event, as the calendar has it.
#[derive(Debug, Clone, Copy)]
pub struct StoredCopy<'a> {
    /// The event.
    pub event: &'a CalendarEvent,
    /// `DTSTAMP` of the version the calendar has (from the stored
    /// scheduling state; `None` when unknown).
    pub dtstamp: Option<&'a str>,
    /// The calendar cannot be written (subscriptions, read-only shares).
    pub read_only: bool,
}

impl StoredCopy<'_> {
    /// The scheduling version of the copy.
    pub fn version(&self) -> Version {
        Version::new(self.event.sequence, self.dtstamp.map(str::to_owned))
    }

    /// The fields of an occurrence (its override) or of the series.
    pub fn fields_for(&self, recurrence_id: Option<&PimTime>) -> &EventFields {
        recurrence_id
            .and_then(|rid| {
                self.event
                    .overrides
                    .iter()
                    .find(|o| same_occurrence(&o.recurrence_id, rid, &self.event.fields.start))
            })
            .map_or(&self.event.fields, |o| &o.fields)
    }
}

/// Whether two recurrence ids name the same occurrence of a series that
/// starts at `start` (compared in the series' frame).
pub fn same_occurrence(a: &PimTime, b: &PimTime, start: &PimTime) -> bool {
    a == b
        || matches!(
            (nail_ical::time::to_frame(a, start), nail_ical::time::to_frame(b, start)),
            (Some(x), Some(y)) if x == y
        )
}

/// Who the user is in an iTIP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    /// The user's role.
    pub role: InvitationRole,
    /// The user's address in the event.
    pub my_address: Option<String>,
    /// The identity that answers / sends.
    pub identity_id: Option<IdentityId>,
    /// For `REPLY` / `COUNTER` / `REFRESH`: the attendee it came from.
    pub from_attendee: Option<ItipAttendee>,
    /// Problems with the people (sender checks, not addressed to the user).
    pub issues: Vec<ItipIssue>,
}

/// Works out the user's role and checks the sender. `senders` are the
/// message's `From` and `Sender` addresses (empty = unknown: no check).
pub fn analyze(object: &ItipObject, senders: &[String], me: &UserAddresses) -> Analysis {
    let mut out = Analysis {
        role: InvitationRole::Observer,
        my_address: None,
        identity_id: None,
        from_attendee: None,
        issues: Vec::new(),
    };
    let (Some(method), Some(main)) = (object.method, object.main()) else {
        return out;
    };

    if let Some(organizer) = main.organizer.as_ref().filter(|o| me.contains(&o.email)) {
        out.role = InvitationRole::Organizer;
        out.my_address = Some(organizer.email.clone());
    } else if let Some(mine) = main
        .attendees
        .iter()
        .filter_map(|a| me.rank(&a.attendee.email).map(|rank| (rank, a)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, a)| a)
    {
        out.role = InvitationRole::Attendee;
        out.my_address = Some(mine.attendee.email.clone());
    }
    out.identity_id = match &out.my_address {
        Some(address) => me.find(address).and_then(|e| e.identity_id.clone()),
        // Answering a forwarded invitation adds the user (RFC 5546 §2.1.5).
        None if method == ItipMethod::Request => {
            me.entries().iter().find_map(|e| e.identity_id.clone())
        }
        None => None,
    };

    let direct = |a: &ItipAttendee| senders.iter().any(|s| same(&a.attendee.email, s));
    let on_behalf = |a: &ItipAttendee| {
        a.sent_by
            .as_deref()
            .is_some_and(|b| senders.iter().any(|s| same(b, s)))
    };
    let from_sender = |a: &ItipAttendee| direct(a) || on_behalf(a);
    if !method.from_organizer() {
        out.from_attendee = match method {
            ItipMethod::Counter if main.attendees.len() > 1 => {
                main.attendees.iter().find(|a| from_sender(a)).cloned()
            }
            _ => main.attendees.first().cloned(),
        };
        if out.from_attendee.is_none()
            && !main.attendees.is_empty()
            && !object.issues.contains(&ItipIssue::AttendeeCount)
        {
            out.issues.push(ItipIssue::AttendeeCount);
        }
    }

    if !senders.is_empty() {
        if method.from_organizer() && method != ItipMethod::Publish {
            let organizer = main.organizer.as_ref();
            if senders
                .iter()
                .any(|s| organizer.is_some_and(|o| same(&o.email, s)))
            {
                // The organizer itself.
            } else if senders.iter().any(|s| main.sent_by_organizer(s)) {
                out.issues.push(ItipIssue::SentOnBehalf);
            } else {
                out.issues.push(ItipIssue::SenderNotOrganizer);
            }
        } else if !method.from_organizer()
            && let Some(attendee) = &out.from_attendee
            && !direct(attendee)
        {
            out.issues.push(if on_behalf(attendee) {
                ItipIssue::SentOnBehalf
            } else {
                ItipIssue::SenderNotAttendee
            });
        }
    }

    let addressed = match method {
        ItipMethod::Publish => true,
        ItipMethod::Request | ItipMethod::Add | ItipMethod::Cancel | ItipMethod::DeclineCounter => {
            out.role != InvitationRole::Observer
        }
        ItipMethod::Reply | ItipMethod::Counter | ItipMethod::Refresh => {
            out.role == InvitationRole::Organizer
        }
    };
    if !addressed {
        out.issues.push(ItipIssue::NotAddressedToMe);
    }
    out
}

/// What the card is built from.
#[derive(Debug, Clone)]
pub struct CardInput<'a> {
    /// The message carrying the iTIP object.
    pub message_id: MessageId,
    /// The parsed object.
    pub object: &'a ItipObject,
    /// `From` / `Sender` addresses of the message.
    pub senders: &'a [String],
    /// The user's addresses.
    pub me: &'a UserAddresses,
    /// The user's copy of the event, if a calendar has it.
    pub stored: Option<StoredCopy<'a>>,
    /// Organizer side: the last answer applied from the replying attendee
    /// (stored scheduling state), to ignore stale replies.
    pub last_reply: Option<Version>,
    /// The message was already applied (stored iTIP message state).
    pub applied: bool,
    /// Overlapping events ([`crate::conflicts`]).
    pub conflicts: Vec<InvitationConflict>,
    /// How answers travel ([`crate::server`]).
    pub transport: SchedulingTransport,
}

/// Builds the invitation card.
pub fn build_card(input: CardInput<'_>) -> Invitation {
    let object = input.object;
    let analysis = analyze(object, input.senders, input.me);
    let main = object.main();
    let mut issues = object.issues.clone();
    let method = object.method.unwrap_or(ItipMethod::Publish);
    let recurrence_id = main.and_then(|c| c.recurrence_id.clone());
    let stored_issues = match (input.stored, main, object.method) {
        (Some(stored), Some(main), Some(_)) => {
            copy_issues(&stored, main, method, &analysis, input.me)
        }
        _ => Vec::new(),
    };
    for issue in analysis.issues.iter().chain(&stored_issues) {
        if !issues.contains(issue) {
            issues.push(*issue);
        }
    }
    let state = if object.is_invalid() || main.is_none() {
        InvitationState::Invalid
    } else {
        state_of(&input, method, main, &analysis)
    };

    let my_status = my_status(
        &analysis,
        main,
        input.stored.as_ref(),
        recurrence_id.as_ref(),
    );
    let counter = (method == ItipMethod::Counter)
        .then(|| main.and_then(counter_proposal))
        .flatten();
    let actions = if state == InvitationState::Invalid {
        Vec::new()
    } else {
        actions_for(&input, method, state, &analysis, main)
    };
    let conflicts = match (analysis.role, method) {
        (
            InvitationRole::Attendee | InvitationRole::Observer,
            ItipMethod::Request | ItipMethod::Add | ItipMethod::Publish,
        )
        | (InvitationRole::Organizer, ItipMethod::Counter) => input.conflicts,
        _ => Vec::new(),
    };

    Invitation {
        message_id: input.message_id,
        method,
        uid: object.uid.clone().unwrap_or_default(),
        recurrence_id,
        sequence: main.map_or(0, |c| c.sequence),
        dtstamp: main.and_then(|c| c.dtstamp.clone()),
        role: analysis.role,
        state,
        organizer: main.and_then(|c| c.organizer.clone()),
        attendees: main.map(ItipComponent::plain_attendees).unwrap_or_default(),
        my_address: analysis.my_address.clone(),
        my_status,
        identity_id: analysis.identity_id.clone(),
        from_attendee: analysis.from_attendee.as_ref().map(|a| a.attendee.clone()),
        fields: main.and_then(|c| c.fields.clone()).map(Box::new),
        recurrence: main
            .filter(|c| c.recurrence_id.is_none())
            .and(object.event.as_ref())
            .and_then(|e| e.recurrence.clone()),
        counter,
        comment: main.and_then(|c| c.comment.clone()),
        event_id: input.stored.map(|s| s.event.id.clone()),
        calendar_id: input.stored.map(|s| s.event.calendar_id.clone()),
        conflicts,
        actions,
        transport: input.transport,
        issues,
    }
}

/// Problems of a message compared with the user's copy of the event:
/// another organizer (`organizer-changed`), an answer from someone who is
/// not invited (`uninvited-attendee`).
fn copy_issues(
    stored: &StoredCopy<'_>,
    main: &ItipComponent,
    method: ItipMethod,
    analysis: &Analysis,
    me: &UserAddresses,
) -> Vec<ItipIssue> {
    let mut out = Vec::new();
    if method.from_organizer() {
        let same_organizer = match (&stored.event.fields.organizer, &main.organizer) {
            (Some(have), Some(named)) => {
                same(&have.email, &named.email)
                    || (me.contains(&have.email) && me.contains(&named.email))
            }
            (None, None) => true,
            _ => false,
        };
        if !same_organizer {
            out.push(ItipIssue::OrganizerChanged);
        }
    } else if let Some(from) = &analysis.from_attendee
        && !is_invited(stored, main.recurrence_id.as_ref(), &from.attendee.email)
    {
        out.push(ItipIssue::UninvitedAttendee);
    }
    out
}

/// Whether `address` is an attendee of the copy (the series or the
/// occurrence) or a delegate an attendee declared.
pub fn is_invited(stored: &StoredCopy<'_>, recurrence_id: Option<&PimTime>, address: &str) -> bool {
    let mut sets = vec![&stored.event.fields];
    if recurrence_id.is_some() {
        sets.push(stored.fields_for(recurrence_id));
    }
    sets.iter().any(|fields| {
        fields
            .attendees
            .iter()
            .any(|a| same(&a.email, address) || a.delegated_to.iter().any(|d| same(d, address)))
    })
}

/// The proposal of a `COUNTER` component.
pub fn counter_proposal(component: &ItipComponent) -> Option<CounterProposal> {
    let fields = component.fields.as_ref()?;
    Some(CounterProposal {
        start: fields.start.clone(),
        end: fields.end.clone(),
        location: fields.location.clone(),
        comment: component.comment.clone(),
    })
}

fn my_status(
    analysis: &Analysis,
    main: Option<&ItipComponent>,
    stored: Option<&StoredCopy<'_>>,
    recurrence_id: Option<&PimTime>,
) -> Option<ParticipationStatus> {
    if analysis.role != InvitationRole::Attendee {
        return None;
    }
    let address = analysis.my_address.as_deref()?;
    let from_calendar = stored.and_then(|s| {
        s.fields_for(recurrence_id)
            .attendees
            .iter()
            .find(|a| same(&a.email, address))
            .map(|a| a.status)
    });
    from_calendar.or_else(|| {
        main.and_then(|c| c.attendee(address))
            .map(|a| a.attendee.status)
    })
}

/// Whether the stored copy no longer has the occurrence (or the event is
/// cancelled as a whole).
fn is_cancelled_in(stored: &StoredCopy<'_>, recurrence_id: Option<&PimTime>) -> bool {
    match recurrence_id {
        None => stored.event.fields.status == EventStatus::Cancelled,
        Some(rid) => {
            stored.fields_for(Some(rid)).status == EventStatus::Cancelled
                || (stored.event.is_recurring()
                    && !nail_ical::edit::is_occurrence(stored.event, rid))
        }
    }
}

fn state_of(
    input: &CardInput<'_>,
    method: ItipMethod,
    main: Option<&ItipComponent>,
    analysis: &Analysis,
) -> InvitationState {
    let Some(main) = main else {
        return InvitationState::Invalid;
    };
    let message = input.object.version();
    let rid = main.recurrence_id.as_ref();
    match method {
        ItipMethod::Request | ItipMethod::Publish | ItipMethod::Add => {
            let Some(stored) = input.stored else {
                return if method == ItipMethod::Add {
                    InvitationState::UnknownEvent
                } else if input.applied {
                    // Applied, then removed from the calendar.
                    InvitationState::Cancelled
                } else {
                    InvitationState::New
                };
            };
            let have = stored.version();
            if message.is_newer_than(&have) {
                if input.applied {
                    InvitationState::Current
                } else {
                    InvitationState::Update
                }
            } else if message.is_older_than(&have) {
                InvitationState::Outdated
            } else {
                InvitationState::Current
            }
        }
        ItipMethod::Cancel => {
            let Some(stored) = input.stored else {
                return InvitationState::Cancelled;
            };
            if input.applied || is_cancelled_in(&stored, rid) {
                InvitationState::Cancelled
            } else if message.is_older_than(&stored.version()) {
                InvitationState::Outdated
            } else {
                // Equal versions apply too: several producers cancel
                // without raising SEQUENCE.
                InvitationState::Update
            }
        }
        ItipMethod::DeclineCounter => match input.stored {
            Some(stored) if message.is_older_than(&stored.version()) => InvitationState::Outdated,
            Some(_) => InvitationState::Current,
            None => InvitationState::UnknownEvent,
        },
        ItipMethod::Reply | ItipMethod::Counter | ItipMethod::Refresh => {
            let Some(stored) = input.stored else {
                return InvitationState::UnknownEvent;
            };
            if input.applied {
                return InvitationState::Current;
            }
            if method == ItipMethod::Refresh {
                return InvitationState::Update;
            }
            // An answer to an older version of the event, or older than
            // the last answer applied, is stale.
            if main.sequence < stored.event.sequence
                || input
                    .last_reply
                    .as_ref()
                    .is_some_and(|last| !message.is_newer_than(last))
            {
                return InvitationState::Outdated;
            }
            if method == ItipMethod::Counter {
                return InvitationState::Update;
            }
            let Some(from) = &analysis.from_attendee else {
                return InvitationState::Invalid;
            };
            let known = stored
                .fields_for(rid)
                .attendees
                .iter()
                .find(|a| same(&a.email, &from.attendee.email));
            match known {
                Some(a)
                    if a.status == from.attendee.status
                        && a.delegated_to == from.attendee.delegated_to =>
                {
                    InvitationState::Current
                }
                _ => InvitationState::Update,
            }
        }
    }
}

fn actions_for(
    input: &CardInput<'_>,
    method: ItipMethod,
    state: InvitationState,
    analysis: &Analysis,
    main: Option<&ItipComponent>,
) -> Vec<InvitationAction> {
    use InvitationAction as A;
    use InvitationState as S;
    let mut out = Vec::new();
    let stored = input.stored;
    let writable = stored.is_none_or(|s| !s.read_only);
    let role = analysis.role;
    match method {
        ItipMethod::Request => {
            if matches!(state, S::New | S::Update | S::Current) && writable {
                if role != InvitationRole::Organizer {
                    out.extend([A::Accept, A::Tentative, A::Decline]);
                    if role == InvitationRole::Attendee && !main.is_some_and(|c| c.disallow_counter)
                    {
                        out.push(A::ProposeNewTime);
                    }
                }
                if state == S::New {
                    out.push(A::AddToCalendar);
                }
                if state == S::Update && stored.is_some() {
                    out.push(A::UpdateCalendar);
                }
            }
            if matches!(state, S::Outdated | S::Cancelled) && role == InvitationRole::Attendee {
                out.push(A::Refresh);
            }
            if role == InvitationRole::Attendee && state != S::Outdated {
                out.push(A::Forward);
            }
        }
        ItipMethod::Publish => {
            if writable {
                match state {
                    S::New => out.push(A::AddToCalendar),
                    S::Update => out.push(A::UpdateCalendar),
                    _ => {}
                }
            }
        }
        ItipMethod::Add => match state {
            S::Update if writable => out.push(A::UpdateCalendar),
            S::UnknownEvent if role == InvitationRole::Attendee => out.push(A::Refresh),
            _ => {}
        },
        ItipMethod::Cancel => {
            if state == S::Update && writable {
                out.push(A::RemoveFromCalendar);
            }
        }
        ItipMethod::DeclineCounter => {
            if role == InvitationRole::Attendee {
                out.push(A::Refresh);
            }
        }
        ItipMethod::Reply => {
            if state == S::Update && role == InvitationRole::Organizer && writable {
                out.push(A::ApplyReply);
            }
        }
        ItipMethod::Counter => {
            if state == S::Update && role == InvitationRole::Organizer && writable {
                out.extend([A::AcceptCounter, A::DeclineCounter]);
            }
        }
        ItipMethod::Refresh => {
            if matches!(state, S::Update | S::Current) && role == InvitationRole::Organizer {
                out.push(A::SendCurrent);
            }
        }
    }
    if stored.is_some() && !matches!(state, S::Cancelled) {
        out.push(A::OpenEvent);
    }
    out
}

/// The attendee entry of the user in a field set.
pub fn my_entry<'a>(fields: &'a EventFields, address: &str) -> Option<&'a Attendee> {
    fields.attendees.iter().find(|a| same(&a.email, address))
}

#[cfg(test)]
mod tests;
