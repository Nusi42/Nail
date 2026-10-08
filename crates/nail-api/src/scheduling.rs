//! Invitations and scheduling: iTIP (RFC 5546) over iMIP mail (RFC 6047)
//! or CalDAV auto-scheduling (RFC 6638), and free/busy (M3b, ADR 0065).
//!
//! * **Incoming**: a message carrying a `text/calendar; method=…` part is
//!   an *invitation message*. The core parses it (`nail-itip`), compares it
//!   with the user's calendars and the stored scheduling state, and puts
//!   an [`Invitation`] on [`crate::MessageBody::invitation`] — the data of
//!   the invitation card in the reader: method, organizer, attendees, the
//!   user's own answer, sequence, conflicts and the actions that make
//!   sense now.
//! * **Answering** ([`crate::Request::RespondToInvitation`],
//!   [`crate::Request::ProposeNewTime`],
//!   [`crate::Request::RefreshInvitation`],
//!   [`crate::Request::ForwardInvitation`]) updates the user's copy of the
//!   event and sends the iTIP reply — as iMIP mail through the outbox, or
//!   not at all when the user's CalDAV server schedules itself
//!   ([`SchedulingTransport::Server`]; Nail then only writes the event and
//!   the server delivers the reply, so no duplicate mail).
//! * **Organizing** ([`crate::Request::SendInvitations`],
//!   [`crate::Request::CancelMeeting`],
//!   [`crate::Request::RespondToCounter`]) sends `REQUEST` / `CANCEL` /
//!   `DECLINECOUNTER`; `SEQUENCE` goes up on significant changes (time,
//!   recurrence, location, cancellation).
//! * **Applying** ([`crate::Request::ApplyInvitation`]) processes a
//!   received `REPLY` (attendee status), `CANCEL` (removes the event or
//!   occurrence), `ADD` (occurrences), `REFRESH` (re-sends) or `REQUEST`
//!   (adds it unanswered). Depending on [`SchedulingSettings`] the core
//!   does this on arrival.
//! * **Free/busy** ([`crate::Request::QueryFreeBusy`]): busy periods per
//!   attendee, from the user's own calendars, the CalDAV schedule outbox
//!   (RFC 6638 `VFREEBUSY` POST) or, from M3c, Exchange.
//!
//! Every write goes through the PIM operation queue (event changes) and
//! the outbox (iMIP mail); the store keeps the scheduling state per event
//! and address (`SEQUENCE`, `DTSTAMP`, answer) and every reply sent.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::Address;
use crate::error::ErrorCode;
use crate::ids::{
    AccountId, CalendarId, EventId, IdentityId, MessageId, OpId, OutboxId, PimSourceId,
};
use crate::localized::LocalizedMessage;
use crate::pim::calendar::{
    Attendee, CalendarEvent, EventFields, Organizer, ParticipationStatus, Recurrence,
};
use crate::pim::{PimTime, is_plausible_email, is_utc_instant};

/// Most attendees one free/busy query may name.
pub const MAX_FREE_BUSY_ATTENDEES: usize = 100;

/// Longest free/busy range in days.
pub const MAX_FREE_BUSY_DAYS: i64 = 92;

/// Longest comment Nail sends with a reply (characters).
pub const MAX_COMMENT: usize = 2000;

// ---------------------------------------------------------------------------
// iTIP

/// An iTIP method (RFC 5546 §1.4; the `METHOD` property and the
/// `method=` parameter of the `text/calendar` part).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ItipMethod {
    /// `PUBLISH`: an event shared for information, no answers.
    Publish,
    /// `REQUEST`: an invitation, or an update of one.
    Request,
    /// `REPLY`: an attendee's answer.
    Reply,
    /// `ADD`: occurrences added to a recurring event.
    Add,
    /// `CANCEL`: the event (or occurrences, or the attendee) is cancelled.
    Cancel,
    /// `REFRESH`: an attendee asks for the current version.
    Refresh,
    /// `COUNTER`: an attendee proposes a change (another time).
    Counter,
    /// `DECLINECOUNTER`: the organizer refuses a proposal.
    DeclineCounter,
}

impl ItipMethod {
    /// Every method.
    pub const ALL: [ItipMethod; 8] = [
        Self::Publish,
        Self::Request,
        Self::Reply,
        Self::Add,
        Self::Cancel,
        Self::Refresh,
        Self::Counter,
        Self::DeclineCounter,
    ];

    /// The iCalendar name (`REQUEST`, `DECLINECOUNTER`, …).
    pub fn as_ical(self) -> &'static str {
        match self {
            Self::Publish => "PUBLISH",
            Self::Request => "REQUEST",
            Self::Reply => "REPLY",
            Self::Add => "ADD",
            Self::Cancel => "CANCEL",
            Self::Refresh => "REFRESH",
            Self::Counter => "COUNTER",
            Self::DeclineCounter => "DECLINECOUNTER",
        }
    }

    /// The method of an iCalendar name (case-insensitive).
    pub fn from_ical(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.as_ical().eq_ignore_ascii_case(name.trim()))
    }

    /// Whether the organizer sends this method (the others come from
    /// attendees).
    pub fn from_organizer(self) -> bool {
        matches!(
            self,
            Self::Publish | Self::Request | Self::Add | Self::Cancel | Self::DeclineCounter
        )
    }
}

/// The user's answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InvitationResponse {
    /// `PARTSTAT=ACCEPTED`.
    Accept,
    /// `PARTSTAT=TENTATIVE`.
    Tentative,
    /// `PARTSTAT=DECLINED`.
    Decline,
}

impl InvitationResponse {
    /// The participation status this answer sets.
    pub fn status(self) -> ParticipationStatus {
        match self {
            Self::Accept => ParticipationStatus::Accepted,
            Self::Tentative => ParticipationStatus::Tentative,
            Self::Decline => ParticipationStatus::Declined,
        }
    }
}

/// The user's part in an invitation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InvitationRole {
    /// The user organizes the event (replies, counters, refreshes come in).
    Organizer,
    /// The user is an attendee (requests, cancels, updates come in).
    Attendee,
    /// The user is neither: a forwarded invitation, a `PUBLISH`, or one of
    /// the user's addresses is not listed (answering adds the user as an
    /// attendee, RFC 5546 §2.1.5 / "party crashing").
    Observer,
}

/// How an invitation message relates to the user's calendars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InvitationState {
    /// Not in a calendar yet.
    New,
    /// The calendar has exactly this version (already added or answered).
    Current,
    /// Newer than the calendar's copy (higher `SEQUENCE`, or the same with
    /// a later `DTSTAMP`): applying it updates the event.
    Update,
    /// Older than the calendar's copy or than a message already applied:
    /// nothing to do.
    Outdated,
    /// A `CANCEL` that was applied, or the event was cancelled meanwhile.
    Cancelled,
    /// A reply / counter / refresh for an event the user does not have
    /// (anymore).
    UnknownEvent,
    /// The calendar data is unusable ([`Invitation::issues`]).
    Invalid,
}

/// What the invitation card may offer now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InvitationAction {
    /// [`crate::Request::RespondToInvitation`] with `accept`.
    Accept,
    /// … with `tentative`.
    Tentative,
    /// … with `decline`.
    Decline,
    /// [`crate::Request::ProposeNewTime`].
    ProposeNewTime,
    /// [`crate::Request::ApplyInvitation`]: add a `PUBLISH` / unanswered
    /// `REQUEST` / `ADD` to the calendar.
    AddToCalendar,
    /// [`crate::Request::ApplyInvitation`]: apply a newer version.
    UpdateCalendar,
    /// [`crate::Request::ApplyInvitation`]: apply a `CANCEL`.
    RemoveFromCalendar,
    /// [`crate::Request::ApplyInvitation`]: record a `REPLY`'s answer.
    ApplyReply,
    /// [`crate::Request::RespondToCounter`] with `accept: true`.
    AcceptCounter,
    /// [`crate::Request::RespondToCounter`] with `accept: false`.
    DeclineCounter,
    /// [`crate::Request::ApplyInvitation`] on a `REFRESH`: send the
    /// current version to the attendee.
    SendCurrent,
    /// [`crate::Request::RefreshInvitation`]: ask the organizer for the
    /// current version.
    Refresh,
    /// [`crate::Request::ForwardInvitation`].
    Forward,
    /// Open the event in the calendar.
    OpenEvent,
}

/// How iTIP messages for an event travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulingTransport {
    /// iMIP mail through the outbox of the matching identity (the default).
    #[default]
    Imip,
    /// The CalDAV server schedules (RFC 6638: the calendar's source
    /// advertises `calendar-auto-schedule` and the event is not
    /// `SCHEDULE-AGENT=CLIENT`); Nail only writes the event.
    Server,
    /// Nothing can be sent: no identity matches the organizer / attendee,
    /// or the calendar is read-only.
    None,
}

/// A problem with an invitation message (the card shows it; the core
/// refuses requests that need what is missing with `invalid_request`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ItipIssue {
    /// The `text/calendar` part does not parse.
    Unparsable,
    /// No `METHOD`, or one Nail does not know.
    UnknownMethod,
    /// No `VEVENT` (only `VEVENT` scheduling is supported; `VTODO` /
    /// `VJOURNAL` invitations are shown read-only).
    NoEvent,
    /// No `UID`.
    MissingUid,
    /// No `ORGANIZER` where the method needs one.
    MissingOrganizer,
    /// A `REPLY` / `COUNTER` / `REFRESH` without exactly one attendee.
    AttendeeCount,
    /// The sender is not the organizer of a `REQUEST` / `CANCEL` /
    /// `ADD` / `DECLINECOUNTER` (possible spoofing; the card warns and
    /// nothing is applied automatically).
    SenderNotOrganizer,
    /// The sender is not the attendee of a `REPLY` / `COUNTER` /
    /// `REFRESH`.
    SenderNotAttendee,
    /// None of the user's identities is the organizer or an attendee.
    NotAddressedToMe,
    /// More than one event (different `UID`s) in one message; only the
    /// first is used.
    SeveralEvents,
    /// The message names another organizer than the user's copy of the
    /// event (possible hijacking by someone who knows the `UID`): never
    /// applied automatically, and applying it by hand keeps the stored
    /// organizer.
    OrganizerChanged,
    /// The sender acts for the organizer / attendee only through the
    /// `SENT-BY` the message itself names (unverifiable; never applied
    /// automatically).
    SentOnBehalf,
    /// A `REPLY` / `COUNTER` / `REFRESH` from someone who is not invited
    /// to the event (nor a declared delegate): never applied
    /// automatically.
    UninvitedAttendee,
}

impl ItipIssue {
    /// Every issue.
    pub const ALL: [ItipIssue; 13] = [
        Self::Unparsable,
        Self::UnknownMethod,
        Self::NoEvent,
        Self::MissingUid,
        Self::MissingOrganizer,
        Self::AttendeeCount,
        Self::SenderNotOrganizer,
        Self::SenderNotAttendee,
        Self::NotAddressedToMe,
        Self::SeveralEvents,
        Self::OrganizerChanged,
        Self::SentOnBehalf,
        Self::UninvitedAttendee,
    ];

    /// The Fluent id (`itip-issue-*`, in `core-scheduling.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Unparsable => "itip-issue-unparsable",
            Self::UnknownMethod => "itip-issue-unknown-method",
            Self::NoEvent => "itip-issue-no-event",
            Self::MissingUid => "itip-issue-missing-uid",
            Self::MissingOrganizer => "itip-issue-missing-organizer",
            Self::AttendeeCount => "itip-issue-attendee-count",
            Self::SenderNotOrganizer => "itip-issue-sender-not-organizer",
            Self::SenderNotAttendee => "itip-issue-sender-not-attendee",
            Self::NotAddressedToMe => "itip-issue-not-addressed-to-me",
            Self::SeveralEvents => "itip-issue-several-events",
            Self::OrganizerChanged => "itip-issue-organizer-changed",
            Self::SentOnBehalf => "itip-issue-sent-on-behalf",
            Self::UninvitedAttendee => "itip-issue-uninvited-attendee",
        }
    }

    /// Whether the issue keeps the core from processing the message on
    /// its own (the card still offers its actions, with the warning):
    /// sender checks, organizer changes, `SENT-BY` claims, uninvited
    /// attendees and messages not addressed to the user.
    pub fn blocks_automatic(self) -> bool {
        matches!(
            self,
            Self::SenderNotOrganizer
                | Self::SenderNotAttendee
                | Self::OrganizerChanged
                | Self::SentOnBehalf
                | Self::UninvitedAttendee
                | Self::NotAddressedToMe
        ) || self.is_fatal()
    }

    /// Whether the issue makes the invitation unusable (vs. a warning).
    pub fn is_fatal(self) -> bool {
        matches!(
            self,
            Self::Unparsable | Self::UnknownMethod | Self::NoEvent | Self::MissingUid
        )
    }
}

/// An event of the user's calendars that overlaps the invited time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InvitationConflict {
    /// The event.
    pub event_id: EventId,
    /// Its calendar.
    pub calendar_id: CalendarId,
    /// The occurrence, for recurring events.
    #[serde(default)]
    pub recurrence_id: Option<PimTime>,
    /// Title.
    pub title: String,
    /// Start (RFC 3339 UTC).
    pub start_utc: String,
    /// End (RFC 3339 UTC).
    pub end_utc: String,
    /// All-day.
    pub all_day: bool,
    /// Only tentatively busy (the conflicting event is itself tentative or
    /// not yet answered).
    pub tentative: bool,
}

/// A time proposed with `COUNTER` (or asked for with
/// [`crate::Request::ProposeNewTime`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CounterProposal {
    /// Proposed start.
    pub start: PimTime,
    /// Proposed end.
    pub end: PimTime,
    /// A proposed location, if changed.
    #[serde(default)]
    pub location: Option<String>,
    /// The proposer's comment.
    #[serde(default)]
    pub comment: Option<String>,
}

/// The invitation card's data: one iTIP message compared with the user's
/// calendars ([`crate::MessageBody::invitation`],
/// [`crate::Request::GetInvitation`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Invitation {
    /// The message carrying it.
    pub message_id: MessageId,
    /// The method.
    pub method: ItipMethod,
    /// The event's `UID`.
    pub uid: String,
    /// The occurrence it is about (`RECURRENCE-ID`), `null` = the whole
    /// event / series.
    #[serde(default)]
    pub recurrence_id: Option<PimTime>,
    /// `SEQUENCE` of the message.
    pub sequence: u32,
    /// `DTSTAMP` of the message (RFC 3339 UTC).
    #[serde(default)]
    pub dtstamp: Option<String>,
    /// The user's part.
    pub role: InvitationRole,
    /// How it relates to the calendars.
    pub state: InvitationState,
    /// The organizer.
    #[serde(default)]
    pub organizer: Option<Organizer>,
    /// The attendees as the message lists them (for a `REPLY`: the one
    /// who answered).
    pub attendees: Vec<Attendee>,
    /// The user's address in the event (organizer or attendee), if any.
    #[serde(default)]
    pub my_address: Option<String>,
    /// The user's current answer: from the calendar's copy, else from the
    /// message; `null` when the user is not an attendee.
    #[serde(default)]
    pub my_status: Option<ParticipationStatus>,
    /// The identity that answers / sends for the user.
    #[serde(default)]
    pub identity_id: Option<IdentityId>,
    /// For `REPLY` / `COUNTER` / `REFRESH`: the attendee who sent it, with
    /// the status they set.
    #[serde(default)]
    pub from_attendee: Option<Attendee>,
    /// The event as the message describes it (`null` for a bare `CANCEL`
    /// / `REFRESH` without details).
    #[serde(default)]
    pub fields: Option<Box<EventFields>>,
    /// Its recurrence, for recurring invitations.
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    /// For `COUNTER`: what the attendee proposes.
    #[serde(default)]
    pub counter: Option<CounterProposal>,
    /// The sender's `COMMENT`.
    #[serde(default)]
    pub comment: Option<String>,
    /// The user's copy of the event, if it is in a calendar.
    #[serde(default)]
    pub event_id: Option<EventId>,
    /// Its calendar.
    #[serde(default)]
    pub calendar_id: Option<CalendarId>,
    /// Overlapping events in the user's calendars (attendee side, `REQUEST`
    /// / `ADD` / `COUNTER`).
    #[serde(default)]
    pub conflicts: Vec<InvitationConflict>,
    /// What the card may offer.
    pub actions: Vec<InvitationAction>,
    /// How answers travel.
    pub transport: SchedulingTransport,
    /// Problems; fatal ones make `state` [`InvitationState::Invalid`].
    #[serde(default)]
    pub issues: Vec<ItipIssue>,
}

impl Invitation {
    /// Whether the card may offer `action`.
    pub fn allows(&self, action: InvitationAction) -> bool {
        self.actions.contains(&action)
    }
}

/// What a scheduling request acts on: an invitation message, or an event
/// already in a calendar (answering from the event editor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum InvitationTarget {
    /// The invitation message.
    Message {
        /// The message.
        message_id: MessageId,
    },
    /// An event in a calendar.
    Event {
        /// The event.
        event_id: EventId,
        /// One occurrence of a recurring event; `null` = the series.
        #[serde(default)]
        recurrence_id: Option<PimTime>,
    },
}

/// One iTIP message Nail sent (or had the server send).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SentItip {
    /// The method.
    pub method: ItipMethod,
    /// Recipients (addresses).
    pub recipients: Vec<String>,
    /// How it travelled.
    pub transport: SchedulingTransport,
    /// The outbox entry of an iMIP mail (undo send applies).
    #[serde(default)]
    pub outbox_id: Option<OutboxId>,
}

/// The answer to a scheduling request ([`crate::Response::Scheduled`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SchedulingResult {
    /// The user's event after the change; `null` when it was removed (or
    /// none was written, e.g. a declined invitation that was never added).
    #[serde(default)]
    pub event: Option<Box<CalendarEvent>>,
    /// An event that was deleted.
    #[serde(default)]
    pub removed_event_id: Option<EventId>,
    /// The iTIP messages sent.
    pub sent: Vec<SentItip>,
    /// The queued PIM operations.
    pub op_ids: Vec<OpId>,
    /// The invitation card after the change (when the request named a
    /// message).
    #[serde(default)]
    pub invitation: Option<Box<Invitation>>,
}

/// What [`crate::Request::CancelMeeting`] cancels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CancelScope {
    /// The whole meeting (series): `CANCEL` to every attendee, the event
    /// is deleted (the default).
    #[default]
    Meeting,
    /// One occurrence (`recurrence_id`): `CANCEL` with `RECURRENCE-ID`, an
    /// `EXDATE` in the user's copy.
    Occurrence,
    /// Some attendees only (`attendees`): they get a `CANCEL`, the others
    /// an updated `REQUEST`.
    Attendees,
}

/// Automatic processing of incoming iTIP messages (stored in the app
/// settings; [`crate::Request::GetSchedulingSettings`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent scheduling switches (one checkbox each)"
)]
pub struct SchedulingSettings {
    /// Put new invitations into the calendar unanswered (`NEEDS-ACTION`,
    /// shown as tentative) on arrival. Default true.
    pub add_invitations: bool,
    /// Apply updates and `ADD`s from the organizer on arrival. Default
    /// true.
    pub apply_updates: bool,
    /// Apply attendees' replies on arrival. Default true.
    pub apply_replies: bool,
    /// Apply cancellations on arrival (else the card offers "Remove from
    /// calendar"). Default false.
    pub apply_cancellations: bool,
    /// The calendar new invitations go to; `null` = the default calendar.
    pub calendar_id: Option<CalendarId>,
    /// Ask before sending a reply (else accept / decline send at once).
    /// Default false.
    pub confirm_replies: bool,
}

impl Default for SchedulingSettings {
    fn default() -> Self {
        Self {
            add_invitations: true,
            apply_updates: true,
            apply_replies: true,
            apply_cancellations: false,
            calendar_id: None,
            confirm_replies: false,
        }
    }
}

/// Checks a reply comment: at most [`MAX_COMMENT`] characters, no control
/// characters except line breaks and tabs.
pub fn is_valid_comment(comment: &str) -> bool {
    comment.chars().count() <= MAX_COMMENT
        && !comment
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

// ---------------------------------------------------------------------------
// Free/busy

/// `FBTYPE` of a busy period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum BusyKind {
    /// `BUSY-TENTATIVE`.
    Tentative,
    /// `BUSY` (the default of `FREEBUSY`).
    Busy,
    /// `BUSY-UNAVAILABLE` (out of office, outside working hours).
    Unavailable,
}

/// A busy stretch of time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BusyPeriod {
    /// Start (RFC 3339 UTC).
    pub start: String,
    /// End (RFC 3339 UTC, exclusive).
    pub end: String,
    /// How busy.
    pub kind: BusyKind,
}

impl BusyPeriod {
    /// Merges overlapping and touching periods of the same kind, sorted by
    /// start; where kinds overlap, each keeps its own period (the UI draws
    /// the stronger one on top). Periods with `end <= start` are dropped.
    /// Times compare as text, so all must be RFC 3339 UTC with the same
    /// precision (`YYYY-MM-DDTHH:MM:SSZ`).
    pub fn merge(mut periods: Vec<BusyPeriod>) -> Vec<BusyPeriod> {
        periods.retain(|p| p.end > p.start);
        periods.sort_by(|a, b| (a.kind, &a.start, &a.end).cmp(&(b.kind, &b.start, &b.end)));
        let mut out: Vec<BusyPeriod> = Vec::with_capacity(periods.len());
        for period in periods {
            match out.last_mut() {
                Some(last) if last.kind == period.kind && period.start <= last.end => {
                    if period.end > last.end {
                        last.end = period.end;
                    }
                }
                _ => out.push(period),
            }
        }
        out.sort_by(|a, b| (&a.start, a.kind).cmp(&(&b.start, b.kind)));
        out
    }
}

/// Where an attendee's free/busy data came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FreeBusySource {
    /// The user's own calendars (the attendee is one of the user's
    /// identities).
    Own,
    /// A CalDAV server's schedule outbox (RFC 6638 `VFREEBUSY` request).
    Caldav {
        /// The source asked.
        source_id: PimSourceId,
    },
    /// An Exchange account (M3c: Graph `getSchedule` / EWS
    /// `GetUserAvailability`; rooms and resources too).
    Exchange {
        /// The account asked.
        account_id: AccountId,
    },
    /// No source knows the attendee.
    Unknown,
}

/// Free/busy of one attendee ([`FreeBusyResult::attendees`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AttendeeFreeBusy {
    /// The attendee (as asked).
    pub email: String,
    /// Where the data came from.
    pub source: FreeBusySource,
    /// Busy periods in the range (merged, sorted).
    pub periods: Vec<BusyPeriod>,
    /// Asking failed (the attendee's periods are then empty).
    #[serde(default)]
    pub error: Option<ErrorCode>,
    /// The user-facing text of the failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<Box<LocalizedMessage>>,
}

/// The answer to [`crate::Request::QueryFreeBusy`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FreeBusyResult {
    /// Range start (RFC 3339 UTC).
    pub from: String,
    /// Range end (RFC 3339 UTC).
    pub to: String,
    /// One entry per asked attendee, in request order.
    pub attendees: Vec<AttendeeFreeBusy>,
}

/// A free/busy question as the engines see it (Rust only; see
/// [`crate::engine::FreeBusyProvider`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeBusyQuery {
    /// The organizer's address (`ORGANIZER` of the `VFREEBUSY` request).
    pub organizer: String,
    /// Attendee addresses.
    pub attendees: Vec<String>,
    /// Range start (RFC 3339 UTC).
    pub from: String,
    /// Range end (RFC 3339 UTC).
    pub to: String,
}

/// A problem with a free/busy query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum FreeBusyIssue {
    /// No attendees, or more than [`MAX_FREE_BUSY_ATTENDEES`].
    AttendeeCount,
    /// An attendee is not an email address.
    InvalidAttendee,
    /// `from` / `to` are not RFC 3339 UTC, or `to <= from`.
    InvalidRange,
    /// The range is longer than [`MAX_FREE_BUSY_DAYS`].
    RangeTooLong,
}

impl FreeBusyIssue {
    /// Every issue.
    pub const ALL: [FreeBusyIssue; 4] = [
        Self::AttendeeCount,
        Self::InvalidAttendee,
        Self::InvalidRange,
        Self::RangeTooLong,
    ];

    /// The Fluent id (`freebusy-issue-*`, in `core-scheduling.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::AttendeeCount => "freebusy-issue-attendee-count",
            Self::InvalidAttendee => "freebusy-issue-invalid-attendee",
            Self::InvalidRange => "freebusy-issue-invalid-range",
            Self::RangeTooLong => "freebusy-issue-range-too-long",
        }
    }
}

impl FreeBusyQuery {
    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<FreeBusyIssue> {
        free_busy_issues(&self.attendees, &self.from, &self.to)
    }
}

/// Problems of a free/busy question (attendees, range).
pub fn free_busy_issues(attendees: &[String], from: &str, to: &str) -> Vec<FreeBusyIssue> {
    let mut out = Vec::new();
    if attendees.is_empty() || attendees.len() > MAX_FREE_BUSY_ATTENDEES {
        out.push(FreeBusyIssue::AttendeeCount);
    }
    if attendees.iter().any(|a| !is_plausible_email(a)) {
        out.push(FreeBusyIssue::InvalidAttendee);
    }
    match (utc_seconds(from), utc_seconds(to)) {
        (Some(start), Some(end)) if end > start => {
            if end - start > MAX_FREE_BUSY_DAYS * 86_400 {
                out.push(FreeBusyIssue::RangeTooLong);
            }
        }
        _ => out.push(FreeBusyIssue::InvalidRange),
    }
    out
}

/// Unix seconds of an RFC 3339 UTC instant (`YYYY-MM-DDTHH:MM:SS[.f]Z`),
/// `None` when it is not one.
pub fn utc_seconds(text: &str) -> Option<i64> {
    if !is_utc_instant(text) {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let days = nail_util::time::days_from_civil(y, u32::try_from(m).ok()?, u32::try_from(d).ok()?);
    Some(days * nail_util::time::SECS_PER_DAY + hh * 3600 + mm * 60 + ss)
}

/// Recipients of a forwarded invitation must be addresses.
pub fn are_valid_recipients(to: &[Address]) -> bool {
    !to.is_empty() && to.iter().all(|a| is_plausible_email(&a.email))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn methods_round_trip() {
        for method in ItipMethod::ALL {
            assert_eq!(ItipMethod::from_ical(method.as_ical()), Some(method));
        }
        assert_eq!(
            ItipMethod::from_ical(" declinecounter "),
            Some(ItipMethod::DeclineCounter)
        );
        assert_eq!(ItipMethod::from_ical("POLL"), None);
        assert!(ItipMethod::Request.from_organizer());
        assert!(!ItipMethod::Reply.from_organizer());
        assert!(!ItipMethod::Counter.from_organizer());
        assert_eq!(
            serde_json::to_value(ItipMethod::DeclineCounter).unwrap(),
            json!("decline-counter")
        );
        assert_eq!(
            InvitationResponse::Tentative.status(),
            ParticipationStatus::Tentative
        );
    }

    #[test]
    fn issues_have_ids() {
        for issue in ItipIssue::ALL {
            assert!(issue.message_id().starts_with("itip-issue-"));
        }
        assert!(ItipIssue::MissingUid.is_fatal());
        assert!(!ItipIssue::SenderNotOrganizer.is_fatal());
        for issue in [
            ItipIssue::SenderNotOrganizer,
            ItipIssue::SenderNotAttendee,
            ItipIssue::OrganizerChanged,
            ItipIssue::SentOnBehalf,
            ItipIssue::UninvitedAttendee,
            ItipIssue::NotAddressedToMe,
            ItipIssue::Unparsable,
        ] {
            assert!(issue.blocks_automatic(), "{issue:?}");
        }
        assert!(!ItipIssue::SeveralEvents.blocks_automatic());
        assert!(!ItipIssue::AttendeeCount.blocks_automatic());
        for issue in FreeBusyIssue::ALL {
            assert!(issue.message_id().starts_with("freebusy-issue-"));
        }
    }

    #[test]
    fn free_busy_query_validation() {
        let ok = FreeBusyQuery {
            organizer: "ada@example.org".into(),
            attendees: vec!["bob@example.org".into()],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-12T00:00:00Z".into(),
        };
        assert_eq!(ok.issues().len(), 0);
        let bad = FreeBusyQuery {
            attendees: vec!["bob".into()],
            from: "2026-10-12T00:00:00Z".into(),
            to: "2026-10-05T00:00:00Z".into(),
            ..ok.clone()
        };
        assert_eq!(
            bad.issues(),
            [FreeBusyIssue::InvalidAttendee, FreeBusyIssue::InvalidRange]
        );
        let long = FreeBusyQuery {
            to: "2027-03-01T00:00:00Z".into(),
            ..ok.clone()
        };
        assert_eq!(long.issues(), [FreeBusyIssue::RangeTooLong]);
        let none = FreeBusyQuery {
            attendees: vec![],
            ..ok
        };
        assert_eq!(none.issues(), [FreeBusyIssue::AttendeeCount]);
    }

    fn p(start: &str, end: &str, kind: BusyKind) -> BusyPeriod {
        BusyPeriod {
            start: format!("2026-10-05T{start}:00Z"),
            end: format!("2026-10-05T{end}:00Z"),
            kind,
        }
    }

    #[test]
    fn busy_periods_merge() {
        let merged = BusyPeriod::merge(vec![
            p("10:00", "11:00", BusyKind::Busy),
            p("09:00", "10:00", BusyKind::Busy),
            p("10:30", "12:00", BusyKind::Busy),
            p("13:00", "13:00", BusyKind::Busy),
            p("11:30", "14:00", BusyKind::Tentative),
            p("15:00", "16:00", BusyKind::Busy),
        ]);
        assert_eq!(
            merged,
            [
                p("09:00", "12:00", BusyKind::Busy),
                p("11:30", "14:00", BusyKind::Tentative),
                p("15:00", "16:00", BusyKind::Busy),
            ]
        );
    }

    #[test]
    fn utc_seconds_parses_instants() {
        assert_eq!(utc_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(utc_seconds("2026-10-01T12:00:00Z"), Some(1_790_856_000));
        assert_eq!(utc_seconds("2000-03-01T00:00:00.5Z"), Some(951_868_800));
        assert_eq!(utc_seconds("2026-10-01T12:00:00+02:00"), None);
        assert_eq!(utc_seconds("tomorrow"), None);
    }

    #[test]
    fn settings_and_comments() {
        let settings: SchedulingSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings, SchedulingSettings::default());
        assert!(settings.add_invitations && !settings.apply_cancellations);
        assert!(is_valid_comment("See you\nthere"));
        assert!(!is_valid_comment("bell\u{7}"));
        assert!(!is_valid_comment(&"x".repeat(MAX_COMMENT + 1)));
        assert!(are_valid_recipients(&[Address {
            name: None,
            email: "c@example.org".into()
        }]));
        assert!(!are_valid_recipients(&[]));
    }

    #[test]
    fn targets_and_sources_are_tagged() {
        let target = InvitationTarget::Event {
            event_id: "ev-1".into(),
            recurrence_id: None,
        };
        assert_eq!(
            serde_json::to_value(&target).unwrap(),
            json!({"type": "event", "event_id": "ev-1", "recurrence_id": null})
        );
        assert_eq!(
            serde_json::to_value(FreeBusySource::Caldav {
                source_id: "work".into()
            })
            .unwrap(),
            json!({"type": "caldav", "source_id": "work"})
        );
    }
}
