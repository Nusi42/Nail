//! Interop: synthesized iMIP mail in the shapes Google Calendar, Outlook /
//! Exchange, Apple Calendar and Thunderbird send (`tests/fixtures`, see its
//! README), taken through the whole flow — find the calendar part, build
//! the card, apply it, answer, read the answer back on the other side.
#![cfg(test)]

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use nail_api::localized::{LocalizedMessage, MessageArg};
use nail_api::pim::PimTime;
use nail_api::pim::calendar::{
    Attendee, AttendeeKind, CalendarEvent, EventInstance, EventStatus, ParticipationStatus,
    Transparency,
};
use nail_api::scheduling::{
    InvitationAction, InvitationRole, InvitationState, ItipIssue, ItipMethod, SchedulingTransport,
};
use nail_itip::apply::{self, Applied};
use nail_itip::build::{self, CounterOptions, MinimalOptions, ReplyOptions, RequestOptions};
use nail_itip::card::{CardInput, StoredCopy, build_card};
use nail_itip::conflicts;
use nail_itip::imip::{MailAddress, OutgoingImip, find_itip};
use nail_itip::server::{self, CalendarScheduling};
use nail_itip::texts::{Localizer, Summary, compose};
use nail_itip::{ItipObject, UserAddress, UserAddresses};
use pretty_assertions::assert_eq;

const GOOGLE_REQUEST: &[u8] = include_bytes!("fixtures/google-request.eml");
const GOOGLE_CANCEL: &[u8] = include_bytes!("fixtures/google-cancel-occurrence.eml");
const OUTLOOK_REQUEST: &[u8] = include_bytes!("fixtures/outlook-request.eml");
const OUTLOOK_COUNTER: &[u8] = include_bytes!("fixtures/outlook-counter.eml");
const APPLE_REPLY: &[u8] = include_bytes!("fixtures/apple-reply-occurrence.eml");
const THUNDERBIRD_REPLY: &[u8] = include_bytes!("fixtures/thunderbird-reply.eml");
const SPOOFED: &[u8] = include_bytes!("fixtures/spoofed-request.eml");

const GOOGLE_UID: &str = "5k1d9m3q7v0hb2c8e4f6g1j3l5@google.com";

struct English(FluentBundle<FluentResource>);

impl English {
    fn new() -> Self {
        let mut bundle = FluentBundle::new(vec!["en".parse().unwrap()]);
        bundle.set_use_isolating(false);
        let text = include_str!("../../../locales/en/itip.ftl").to_owned();
        bundle
            .add_resource(FluentResource::try_new(text).unwrap())
            .unwrap();
        Self(bundle)
    }
}

impl Localizer for English {
    fn render(&self, message: &LocalizedMessage) -> String {
        let pattern = self.0.get_message(&message.id).unwrap().value().unwrap();
        let mut args = FluentArgs::new();
        for (name, value) in &message.args {
            match value {
                MessageArg::Number(n) => args.set(name.as_str(), FluentValue::from(*n)),
                MessageArg::Text(t) => args.set(name.as_str(), FluentValue::from(t.as_str())),
            }
        }
        let mut errors = Vec::new();
        let out = self
            .0
            .format_pattern(pattern, Some(&args), &mut errors)
            .into_owned();
        assert!(errors.is_empty(), "{errors:?}");
        out
    }
}

fn me(email: &str) -> UserAddresses {
    UserAddresses::new([UserAddress {
        identity_id: Some(email.split('@').next().unwrap().into()),
        email: email.into(),
    }])
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap()
}

/// A calendar copy made from an iTIP object (what the core stores).
fn store(object: &ItipObject, id: &str) -> (CalendarEvent, String) {
    let new = apply::new_event(object).unwrap();
    let event = new.event.into_event(
        id.into(),
        "cal".into(),
        false,
        nail_api::pim::PimItemSync::default(),
    );
    (event, new.text)
}

fn copy(event: &CalendarEvent) -> StoredCopy<'_> {
    StoredCopy {
        event,
        dtstamp: None,
        read_only: false,
    }
}

fn card<'a>(
    object: &'a ItipObject,
    senders: &'a [String],
    me: &'a UserAddresses,
    stored: Option<StoredCopy<'a>>,
) -> nail_api::scheduling::Invitation {
    build_card(CardInput {
        message_id: "msg".into(),
        object,
        senders,
        me,
        stored,
        last_reply: None,
        applied: false,
        conflicts: Vec::new(),
        transport: SchedulingTransport::Imip,
    })
}

fn status(event: &CalendarEvent, email: &str) -> Option<ParticipationStatus> {
    event
        .fields
        .attendees
        .iter()
        .find(|a| a.email.eq_ignore_ascii_case(email))
        .map(|a| a.status)
}

fn berlin(local: &str) -> PimTime {
    PimTime::zoned(local, "Europe/Berlin")
}

#[test]
fn google_invitation_accepted_and_recorded_by_the_organizer() {
    let received = find_itip(GOOGLE_REQUEST).expect("calendar part");
    assert_eq!(received.method_param.as_deref(), Some("REQUEST"));
    assert!(!received.attachment, "the inline part wins over invite.ics");
    assert_eq!(
        received.senders(),
        ["ada@example.org", "calendar-notification@google.com"]
    );
    let object = received.parse();
    assert!(object.issues.is_empty(), "{:?}", object.issues);
    assert_eq!(object.uid.as_deref(), Some(GOOGLE_UID));

    // Bob's card.
    let bob = me("bob@example.org");
    let senders = received.senders();
    let inv = card(&object, &senders, &bob, None);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert_eq!(inv.state, InvitationState::New);
    assert!(inv.issues.is_empty(), "{:?}", inv.issues);
    assert_eq!(inv.my_status, Some(ParticipationStatus::NeedsAction));
    assert!(inv.allows(InvitationAction::Accept) && inv.allows(InvitationAction::ProposeNewTime));
    assert!(inv.recurrence.is_some());
    let fields = inv.fields.as_ref().unwrap();
    assert_eq!(fields.title, "Engine sync");
    assert_eq!(fields.start, berlin("2026-10-05T10:00:00"));
    assert!(
        fields
            .conferences
            .iter()
            .any(|c| c.uri == "https://meet.google.com/abc-defg-hij")
    );

    // Conflicts with Bob's dentist appointment on the second Monday.
    let (from, to) = conflicts::window(&object, now(), Tz::UTC);
    let spans = conflicts::invited_spans(&object, from, to, Tz::UTC);
    assert!(spans.len() >= 10);
    let dentist = EventInstance {
        event_id: "dentist".into(),
        calendar_id: "cal".into(),
        recurrence_id: None,
        start: PimTime::utc("2026-10-12T08:15:00"),
        end: PimTime::utc("2026-10-12T09:00:00"),
        start_utc: "2026-10-12T08:15:00Z".into(),
        end_utc: "2026-10-12T09:00:00Z".into(),
        all_day: false,
        title: "Dentist".into(),
        location: None,
        status: EventStatus::Confirmed,
        transparency: Transparency::Busy,
        color: None,
        recurring: false,
        overridden: false,
        has_attendees: false,
        has_alarms: false,
        read_only: false,
    };
    let found = conflicts::find_conflicts(&spans, &[dentist], None);
    assert_eq!(found.len(), 1);

    // Bob adds it and accepts.
    let (bob_copy, bob_text) = store(&object, "bob-ev");
    assert!(!bob_text.contains("METHOD:"));
    assert!(bob_copy.is_recurring());
    let accepted = apply::set_my_status(
        &bob_copy,
        "bob@example.org",
        None,
        ParticipationStatus::Accepted,
        None,
    )
    .unwrap();
    assert_eq!(
        status(&accepted, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
    let mut answer = Attendee::new("bob@example.org");
    answer.status = ParticipationStatus::Accepted;
    let reply_ics = build::build_reply(
        &bob_text,
        &ReplyOptions {
            uid: GOOGLE_UID,
            recurrence_id: None,
            attendee: &answer,
            delegates: &[],
            comment: Some("Count me in"),
            now: now(),
        },
    )
    .unwrap();
    let texts = compose(
        &Summary {
            method: Some(ItipMethod::Reply),
            fields: Some(&accepted.fields),
            attendee: Some(&answer),
            comment: Some("Count me in"),
            recurring: true,
            ..Summary::default()
        },
        &English::new(),
    );
    assert_eq!(texts.subject, "Accepted: Engine sync");
    let mut mail = OutgoingImip::new(
        ItipMethod::Reply,
        MailAddress::new("bob@example.org"),
        vec![MailAddress {
            name: Some("Ada Lovelace".into()),
            email: "ada@example.org".into(),
        }],
        texts,
        reply_ics,
    );
    mail.in_reply_to = received.message_id.clone();
    let raw = mail.to_mime();

    // Ada (the organizer) receives the reply.
    let incoming = find_itip(&raw).unwrap();
    let reply = incoming.parse();
    assert!(reply.issues.is_empty(), "{:?}", reply.issues);
    let ada = me("ada@example.org");
    let (ada_copy, _) = store(&object, "ada-ev");
    let senders = incoming.senders();
    let inv = card(&reply, &senders, &ada, Some(copy(&ada_copy)));
    assert_eq!(inv.role, InvitationRole::Organizer);
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.comment.as_deref(), Some("Count me in"));
    assert!(inv.allows(InvitationAction::ApplyReply));
    let recorded = apply::apply_reply(&ada_copy, &reply).unwrap();
    assert_eq!(
        status(&recorded, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
    assert_eq!(
        status(&recorded, "charles@example.net"),
        Some(ParticipationStatus::NeedsAction)
    );
    // Applying it again changes nothing: the card says current.
    let inv = card(&reply, &senders, &ada, Some(copy(&recorded)));
    assert_eq!(inv.state, InvitationState::Current);
}

#[test]
fn google_cancels_one_occurrence() {
    let request = find_itip(GOOGLE_REQUEST).unwrap().parse();
    let (bob_copy, _) = store(&request, "bob-ev");
    let received = find_itip(GOOGLE_CANCEL).unwrap();
    let cancel = received.parse();
    assert_eq!(cancel.method, Some(ItipMethod::Cancel));
    let bob = me("bob@example.org");
    let senders = received.senders();
    let inv = card(&cancel, &senders, &bob, Some(copy(&bob_copy)));
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.recurrence_id, Some(berlin("2026-10-12T10:00:00")));
    assert!(inv.allows(InvitationAction::RemoveFromCalendar));
    let Applied::Updated(updated) =
        apply::apply_cancel(&bob_copy, &cancel, Some("bob@example.org")).unwrap()
    else {
        panic!("one occurrence goes");
    };
    assert_eq!(
        updated.recurrence.as_ref().unwrap().exceptions,
        [berlin("2026-10-12T10:00:00")]
    );
    assert_eq!(updated.sequence, 1);
    // Afterwards the card shows it as cancelled.
    let inv = card(&cancel, &senders, &bob, Some(copy(&updated)));
    assert_eq!(inv.state, InvitationState::Cancelled);
    assert!(inv.actions.is_empty(), "{:?}", inv.actions);
}

#[test]
fn outlook_invitation_with_a_windows_zone() {
    let received = find_itip(OUTLOOK_REQUEST).unwrap();
    assert_eq!(received.method_param.as_deref(), Some("REQUEST"));
    let object = received.parse();
    assert!(object.issues.is_empty(), "{:?}", object.issues);
    let main = object.main().unwrap();
    let fields = main.fields.as_ref().unwrap();
    // `W. Europe Standard Time` is read as Europe/Berlin.
    assert_eq!(fields.start, berlin("2026-10-06T14:00:00"));
    assert_eq!(fields.title, "Compiler roadmap Q4");
    assert!(!main.disallow_counter);
    let room = main.attendee("room-ada@contoso.example").unwrap();
    assert_eq!(room.attendee.kind, AttendeeKind::Resource);

    // Bob's address is written `Bob@Example.org`.
    let bob = me("bob@example.org");
    let senders = received.senders();
    let inv = card(&object, &senders, &bob, None);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert_eq!(inv.my_address.as_deref(), Some("bob@example.org"));
    assert!(inv.issues.is_empty(), "{:?}", inv.issues);
    assert!(inv.allows(InvitationAction::ProposeNewTime));

    // Tentative with a comment: the reply keeps Outlook's CN and zone.
    let mut answer = Attendee::new("bob@example.org");
    answer.status = ParticipationStatus::Tentative;
    let reply = build::build_reply(
        &received.text,
        &ReplyOptions {
            uid: &object.uid.clone().unwrap(),
            recurrence_id: None,
            attendee: &answer,
            delegates: &[],
            comment: Some("Might be late"),
            now: now(),
        },
    )
    .unwrap();
    assert!(reply.contains("TZID:W. Europe Standard Time"));
    assert!(reply.contains("CN=Bob Builder"));
    let parsed = nail_itip::parse_itip(&reply, None);
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert_eq!(
        parsed.main().unwrap().attendees[0].attendee.status,
        ParticipationStatus::Tentative
    );
    // Outlook's long UID survives folding.
    assert_eq!(parsed.uid, object.uid);

    // Bob proposes Wednesday instead.
    let proposal = nail_api::scheduling::CounterProposal {
        start: berlin("2026-10-07T16:00:00"),
        end: berlin("2026-10-07T17:00:00"),
        location: None,
        comment: Some("Wednesday?".into()),
    };
    let counter = build::build_counter(
        &received.text,
        &CounterOptions {
            uid: &object.uid.clone().unwrap(),
            recurrence_id: None,
            attendee: "bob@example.org",
            proposal: &proposal,
            now: now(),
        },
    )
    .unwrap();
    let parsed = nail_itip::parse_itip(&counter, None);
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    assert_eq!(parsed.method, Some(ItipMethod::Counter));
    assert_eq!(
        parsed.main().unwrap().fields.as_ref().unwrap().start,
        berlin("2026-10-07T16:00:00")
    );
    // No auto-scheduling server: answers go by mail.
    assert_eq!(
        server::attendee_transport(
            ItipMethod::Reply,
            Some(&received.text),
            "x",
            CalendarScheduling::default()
        ),
        SchedulingTransport::Imip
    );
}

#[test]
fn outlook_counter_accepted_and_declined_by_the_organizer() {
    let request = find_itip(OUTLOOK_REQUEST).unwrap();
    let request_object = request.parse();
    let (grace_copy, grace_text) = store(&request_object, "grace-ev");
    let received = find_itip(OUTLOOK_COUNTER).unwrap();
    let counter = received.parse();
    assert!(counter.issues.is_empty(), "{:?}", counter.issues);
    let grace = me("grace@contoso.example");
    let senders = received.senders();
    let inv = card(&counter, &senders, &grace, Some(copy(&grace_copy)));
    assert_eq!(inv.role, InvitationRole::Organizer);
    assert_eq!(inv.state, InvitationState::Update);
    // Two attendees are listed; the sender is the proposer.
    assert_eq!(
        inv.from_attendee.as_ref().unwrap().email,
        "alan@contoso.example"
    );
    let proposal = inv.counter.clone().unwrap();
    assert_eq!(proposal.start, berlin("2026-10-07T16:00:00"));
    assert_eq!(
        proposal.comment.as_deref(),
        Some("Could we do Wednesday instead?\n")
    );
    assert!(inv.allows(InvitationAction::AcceptCounter));

    // Accept: the meeting moves, SEQUENCE 1, everyone asked again.
    let moved =
        apply::accept_counter(&grace_copy, &proposal, None, "grace@contoso.example").unwrap();
    assert_eq!(moved.sequence, 1);
    assert_eq!(moved.fields.start, berlin("2026-10-07T16:00:00"));
    let written = nail_ical::write::write_event(
        &moved,
        Some(&grace_text),
        &nail_ical::write::WriteOptions::at(now()),
    );
    let update = build::build_request(
        &written,
        &RequestOptions {
            method: ItipMethod::Request,
            uid: &moved.uid,
            recurrence_id: None,
            sequence: Some(moved.sequence),
            now: now(),
        },
    )
    .unwrap();
    let update_object = nail_itip::parse_itip(&update, None);
    assert!(
        update_object.issues.is_empty(),
        "{:?}",
        update_object.issues
    );
    // Bob still has version 0: this is an update for him.
    let (bob_copy, _) = store(&request_object, "bob-ev");
    let bob = me("bob@example.org");
    let grace_sender = vec!["grace@contoso.example".to_owned()];
    let inv = card(&update_object, &grace_sender, &bob, Some(copy(&bob_copy)));
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.my_status, Some(ParticipationStatus::NeedsAction));
    let bob_updated = apply::apply_request(&bob_copy, &update_object).unwrap();
    assert_eq!(bob_updated.fields.start, berlin("2026-10-07T16:00:00"));
    assert_eq!(bob_updated.sequence, 1);
    // The old invitation is now outdated for Bob.
    let inv = card(
        &request_object,
        &request.senders(),
        &bob,
        Some(copy(&bob_updated)),
    );
    assert_eq!(inv.state, InvitationState::Outdated);

    // Or decline it.
    let decline = build::build_decline_counter(
        &grace_text,
        &MinimalOptions {
            uid: &grace_copy.uid,
            recurrence_id: None,
            attendee: "alan@contoso.example",
            comment: Some("Tuesday stays"),
            sequence: Some(grace_copy.sequence),
            now: now(),
        },
    )
    .unwrap();
    let decline_object = nail_itip::parse_itip(&decline, None);
    let alan = me("alan@contoso.example");
    let inv = card(&decline_object, &grace_sender, &alan, Some(copy(&bob_copy)));
    assert_eq!(inv.method, ItipMethod::DeclineCounter);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert_eq!(inv.comment.as_deref(), Some("Tuesday stays"));
}

#[test]
fn apple_declines_one_occurrence() {
    let request = find_itip(GOOGLE_REQUEST).unwrap().parse();
    let (ada_copy, _) = store(&request, "ada-ev");
    let received = find_itip(APPLE_REPLY).unwrap();
    // Apple attaches the calendar part (text/calendar with a file name).
    assert!(received.attachment);
    assert_eq!(received.method_param.as_deref(), Some("REPLY"));
    let reply = received.parse();
    assert!(reply.issues.is_empty(), "{:?}", reply.issues);
    let ada = me("ada@example.org");
    let senders = received.senders();
    let inv = card(&reply, &senders, &ada, Some(copy(&ada_copy)));
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.recurrence_id, Some(berlin("2026-10-19T10:00:00")));
    assert_eq!(
        inv.from_attendee.as_ref().map(|a| a.status),
        Some(ParticipationStatus::Declined)
    );
    let recorded = apply::apply_reply(&ada_copy, &reply).unwrap();
    // Only that occurrence: an override with Bob declined.
    assert_eq!(
        status(&recorded, "bob@example.org"),
        Some(ParticipationStatus::NeedsAction)
    );
    let o = apply::override_for(&recorded, &berlin("2026-10-19T10:00:00")).unwrap();
    assert_eq!(
        o.fields
            .attendees
            .iter()
            .find(|a| a.email == "bob@example.org")
            .map(|a| a.status),
        Some(ParticipationStatus::Declined)
    );
    assert_eq!(o.fields.end, berlin("2026-10-19T10:30:00"));
}

#[test]
fn thunderbird_accepts_the_series() {
    let request = find_itip(GOOGLE_REQUEST).unwrap().parse();
    let (ada_copy, _) = store(&request, "ada-ev");
    let received = find_itip(THUNDERBIRD_REPLY).unwrap();
    assert!(!received.attachment);
    let reply = received.parse();
    assert!(reply.issues.is_empty(), "{:?}", reply.issues);
    let ada = me("ada@example.org");
    let senders = received.senders();
    let inv = card(&reply, &senders, &ada, Some(copy(&ada_copy)));
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.comment.as_deref(), Some("See you Monday."));
    let recorded = apply::apply_reply(&ada_copy, &reply).unwrap();
    assert_eq!(
        status(&recorded, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
}

#[test]
fn spoofed_invitations_are_flagged_and_refresh_works() {
    let received = find_itip(SPOOFED).unwrap();
    let object = received.parse();
    let bob = me("bob@example.org");
    let senders = received.senders();
    let inv = card(&object, &senders, &bob, None);
    assert_eq!(inv.issues, [ItipIssue::SenderNotOrganizer]);
    // Bob's real copy is older; the forged one claims SEQUENCE 5.
    let real = find_itip(GOOGLE_REQUEST).unwrap().parse();
    let (bob_copy, bob_text) = store(&real, "bob-ev");
    let inv = card(&object, &senders, &bob, Some(copy(&bob_copy)));
    assert_eq!(inv.state, InvitationState::Update);
    assert!(inv.issues.contains(&ItipIssue::SenderNotOrganizer));
    // Bob can ask the real organizer for the current version.
    let refresh = build::build_refresh(
        &bob_text,
        &MinimalOptions {
            uid: GOOGLE_UID,
            recurrence_id: None,
            attendee: "bob@example.org",
            comment: None,
            sequence: None,
            now: now(),
        },
    )
    .unwrap();
    let refresh = nail_itip::parse_itip(&refresh, None);
    assert!(refresh.issues.is_empty(), "{:?}", refresh.issues);
    let ada = me("ada@example.org");
    let (ada_copy, _) = store(&real, "ada-ev");
    let inv = card(
        &refresh,
        &["bob@example.org".to_owned()],
        &ada,
        Some(copy(&ada_copy)),
    );
    assert!(inv.allows(InvitationAction::SendCurrent));
}

#[test]
fn server_scheduling_means_no_imip() {
    let object = find_itip(GOOGLE_REQUEST).unwrap().parse();
    let (_, bob_text) = store(&object, "bob-ev");
    let caldav = CalendarScheduling {
        auto_schedule: true,
        writable: true,
    };
    assert_eq!(
        server::attendee_transport(ItipMethod::Reply, Some(&bob_text), GOOGLE_UID, caldav),
        SchedulingTransport::Server
    );
    // COUNTER and REFRESH are never implicit.
    assert_eq!(
        server::attendee_transport(ItipMethod::Counter, Some(&bob_text), GOOGLE_UID, caldav),
        SchedulingTransport::Imip
    );
    // Nail takes over delivery: mark the organizer CLIENT first.
    let marked = server::mark_client_scheduled(&bob_text, GOOGLE_UID, true, &[]);
    assert_eq!(
        server::attendee_transport(ItipMethod::Reply, Some(&marked), GOOGLE_UID, caldav),
        SchedulingTransport::Imip
    );
    // Organizer side: every attendee by the server.
    let plan = server::plan_organizer_delivery(
        ItipMethod::Request,
        &bob_text,
        GOOGLE_UID,
        &["bob@example.org".into(), "charles@example.net".into()],
        caldav,
    );
    assert_eq!(plan.transport(), SchedulingTransport::Server);
    assert!(plan.imip.is_empty(), "{:?}", plan.imip);
}
