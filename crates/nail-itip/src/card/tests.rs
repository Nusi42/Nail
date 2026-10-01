use super::*;
use crate::address::UserAddress;
use crate::object::parse_itip;
use nail_api::pim::calendar::Attendee;
use pretty_assertions::assert_eq;

use InvitationAction as A;

fn cal(method: &str, body: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nMETHOD:{method}\r\n{body}END:VCALENDAR\r\n"
    )
}

fn vevent(extra: &str) -> String {
    format!(
        "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTART:20261005T070000Z\r\nDTEND:20261005T080000Z\r\n\
SUMMARY:Planning\r\nORGANIZER;CN=Ada:mailto:ada@example.org\r\n{extra}END:VEVENT\r\n"
    )
}

fn request(seq: u32, stamp: &str) -> String {
    cal(
        "REQUEST",
        &vevent(&format!(
            "SEQUENCE:{seq}\r\nDTSTAMP:{stamp}\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.org\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:carol@example.org\r\n"
        )),
    )
}

fn bob() -> UserAddresses {
    UserAddresses::new([
        UserAddress {
            identity_id: Some("bob-work".into()),
            email: "bob@example.org".into(),
        },
        UserAddress {
            identity_id: Some("bob-home".into()),
            email: "bob@home.example".into(),
        },
    ])
}

fn ada() -> UserAddresses {
    UserAddresses::new([UserAddress {
        identity_id: Some("ada".into()),
        email: "ada@example.org".into(),
    }])
}

fn stored_event(text: &str, sequence: u32) -> CalendarEvent {
    let object = parse_itip(text, None);
    let mut event =
        object
            .event
            .unwrap()
            .into_event("ev-1".into(), "cal-1".into(), false, Default::default());
    event.sequence = sequence;
    event
}

fn card(
    text: &str,
    senders: &[&str],
    me: &UserAddresses,
    stored: Option<StoredCopy<'_>>,
) -> Invitation {
    let object = parse_itip(text, None);
    let senders: Vec<String> = senders.iter().map(|s| (*s).to_owned()).collect();
    build_card(CardInput {
        message_id: "msg-1".into(),
        object: &object,
        senders: &senders,
        me,
        stored,
        last_reply: None,
        applied: false,
        conflicts: Vec::new(),
        transport: SchedulingTransport::Imip,
    })
}

#[test]
fn a_new_invitation_for_an_attendee() {
    let text = request(0, "20261001T080000Z");
    let inv = card(&text, &["ada@example.org"], &bob(), None);
    assert_eq!(inv.method, ItipMethod::Request);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert_eq!(inv.state, InvitationState::New);
    assert_eq!(inv.my_address.as_deref(), Some("bob@example.org"));
    assert_eq!(inv.identity_id, Some("bob-work".into()));
    assert_eq!(inv.my_status, Some(ParticipationStatus::NeedsAction));
    assert_eq!(inv.uid, "m1@example.org");
    assert_eq!(inv.dtstamp.as_deref(), Some("2026-10-01T08:00:00Z"));
    assert_eq!(inv.attendees.len(), 2);
    assert_eq!(inv.fields.as_ref().unwrap().title, "Planning");
    assert_eq!(
        inv.actions,
        [
            A::Accept,
            A::Tentative,
            A::Decline,
            A::ProposeNewTime,
            A::AddToCalendar,
            A::Forward
        ]
    );
    assert!(inv.issues.is_empty());
    assert!(inv.event_id.is_none());
}

#[test]
fn the_identity_of_the_receiving_account_wins() {
    let text = cal(
        "REQUEST",
        &vevent(
            "DTSTAMP:20261001T080000Z\r\nATTENDEE:mailto:bob@home.example\r\n\
ATTENDEE:mailto:bob@example.org\r\n",
        ),
    );
    let inv = card(&text, &["ada@example.org"], &bob(), None);
    assert_eq!(inv.my_address.as_deref(), Some("bob@example.org"));
    let home_first = UserAddresses::new(bob().entries().iter().rev().cloned());
    let inv = card(&text, &["ada@example.org"], &home_first, None);
    assert_eq!(inv.identity_id, Some("bob-home".into()));
}

#[test]
fn spoofed_invitations_are_flagged() {
    let text = request(0, "20261001T080000Z");
    let inv = card(&text, &["mallory@evil.example"], &bob(), None);
    assert!(inv.issues.contains(&ItipIssue::SenderNotOrganizer));
    // Still answerable by hand.
    assert!(inv.allows(A::Accept));
    // The organizer's SENT-BY may send.
    let delegated = text.replace(
        "ORGANIZER;CN=Ada:",
        "ORGANIZER;CN=Ada;SENT-BY=\"mailto:assistant@example.org\":",
    );
    let inv = card(&delegated, &["assistant@example.org"], &bob(), None);
    assert!(inv.issues.is_empty());
    // The Sender header counts too (calendar services sending for the organizer).
    let inv = card(
        &text,
        &["calendar-notification@google.com", "ada@example.org"],
        &bob(),
        None,
    );
    assert!(inv.issues.is_empty());
    // No sender known: no check.
    let inv = card(&text, &[], &bob(), None);
    assert!(inv.issues.is_empty());
}

#[test]
fn forwarded_invitations_make_an_observer() {
    let text = request(0, "20261001T080000Z");
    let dan = UserAddresses::new([UserAddress {
        identity_id: Some("dan".into()),
        email: "dan@example.org".into(),
    }]);
    let inv = card(&text, &["bob@example.org"], &dan, None);
    assert_eq!(inv.role, InvitationRole::Observer);
    assert!(inv.issues.contains(&ItipIssue::NotAddressedToMe));
    assert!(inv.issues.contains(&ItipIssue::SenderNotOrganizer));
    assert_eq!(inv.identity_id, Some("dan".into()));
    assert_eq!(inv.my_status, None);
    assert_eq!(
        inv.actions,
        [A::Accept, A::Tentative, A::Decline, A::AddToCalendar]
    );
}

#[test]
fn versions_decide_update_current_outdated() {
    let v1 = request(1, "20261001T080000Z");
    let mut event = stored_event(&v1, 1);
    let mut me = Attendee::new("bob@example.org");
    me.status = ParticipationStatus::Accepted;
    event.fields.attendees[0] = me;
    let stored = |stamp: Option<&'static str>| StoredCopy {
        event: &event,
        dtstamp: stamp,
        read_only: false,
    };

    let same = card(
        &v1,
        &["ada@example.org"],
        &bob(),
        Some(stored(Some("2026-10-01T08:00:00Z"))),
    );
    assert_eq!(same.state, InvitationState::Current);
    assert_eq!(same.my_status, Some(ParticipationStatus::Accepted));
    assert_eq!(same.event_id, Some("ev-1".into()));
    assert!(same.allows(A::OpenEvent) && !same.allows(A::UpdateCalendar));

    // Same SEQUENCE, later DTSTAMP (Outlook's minor updates).
    let restamped = request(1, "20261002T080000Z");
    let inv = card(
        &restamped,
        &["ada@example.org"],
        &bob(),
        Some(stored(Some("2026-10-01T08:00:00Z"))),
    );
    assert_eq!(inv.state, InvitationState::Update);
    assert!(inv.allows(A::UpdateCalendar));

    let newer = request(2, "20261001T070000Z");
    let inv = card(
        &newer,
        &["ada@example.org"],
        &bob(),
        Some(stored(Some("2026-10-01T08:00:00Z"))),
    );
    assert_eq!(inv.state, InvitationState::Update);

    let stale = request(0, "20261003T080000Z");
    let inv = card(
        &stale,
        &["ada@example.org"],
        &bob(),
        Some(stored(Some("2026-10-01T08:00:00Z"))),
    );
    assert_eq!(inv.state, InvitationState::Outdated);
    assert_eq!(inv.actions, [A::Refresh, A::OpenEvent]);

    let earlier_stamp = request(1, "20260930T080000Z");
    let inv = card(
        &earlier_stamp,
        &["ada@example.org"],
        &bob(),
        Some(stored(Some("2026-10-01T08:00:00Z"))),
    );
    assert_eq!(inv.state, InvitationState::Outdated);

    // Unknown stored DTSTAMP: same sequence is newer when the message has one.
    let inv = card(&v1, &["ada@example.org"], &bob(), Some(stored(None)));
    assert_eq!(inv.state, InvitationState::Update);

    // Read-only calendars offer no writes.
    let ro = StoredCopy {
        event: &event,
        dtstamp: None,
        read_only: true,
    };
    let inv = card(&newer, &["ada@example.org"], &bob(), Some(ro));
    assert_eq!(inv.actions, [A::Forward, A::OpenEvent]);
}

#[test]
fn applied_messages_are_current() {
    let v1 = request(1, "20261001T080000Z");
    let event = stored_event(&request(0, "20261001T070000Z"), 0);
    let object = parse_itip(&v1, None);
    let senders = vec!["ada@example.org".to_owned()];
    let me = bob();
    let inv = build_card(CardInput {
        message_id: "m".into(),
        object: &object,
        senders: &senders,
        me: &me,
        stored: Some(StoredCopy {
            event: &event,
            dtstamp: None,
            read_only: false,
        }),
        last_reply: None,
        applied: true,
        conflicts: Vec::new(),
        transport: SchedulingTransport::Server,
    });
    assert_eq!(inv.state, InvitationState::Current);
    assert_eq!(inv.transport, SchedulingTransport::Server);
    // Applied, then deleted from the calendar.
    let inv = build_card(CardInput {
        message_id: "m".into(),
        object: &object,
        senders: &senders,
        me: &me,
        stored: None,
        last_reply: None,
        applied: true,
        conflicts: Vec::new(),
        transport: SchedulingTransport::Imip,
    });
    assert_eq!(inv.state, InvitationState::Cancelled);
}

#[test]
fn cancellations() {
    let text = cal(
        "CANCEL",
        &vevent(
            "SEQUENCE:2\r\nDTSTAMP:20261002T080000Z\r\nATTENDEE:mailto:bob@example.org\r\nSTATUS:CANCELLED\r\n",
        ),
    );
    let event = stored_event(&request(1, "20261001T080000Z"), 1);
    let stored = StoredCopy {
        event: &event,
        dtstamp: None,
        read_only: false,
    };
    let inv = card(&text, &["ada@example.org"], &bob(), Some(stored));
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.actions, [A::RemoveFromCalendar, A::OpenEvent]);
    let inv = card(&text, &["ada@example.org"], &bob(), None);
    assert_eq!(inv.state, InvitationState::Cancelled);
    assert!(inv.actions.is_empty());
    // Equal SEQUENCE still cancels; an older one does not.
    let same = text.replace("SEQUENCE:2", "SEQUENCE:1");
    assert_eq!(
        card(&same, &["ada@example.org"], &bob(), Some(stored)).state,
        InvitationState::Update
    );
    let old = text.replace("SEQUENCE:2", "SEQUENCE:0");
    assert_eq!(
        card(&old, &["ada@example.org"], &bob(), Some(stored)).state,
        InvitationState::Outdated
    );
    let spoofed = card(&text, &["mallory@evil.example"], &bob(), Some(stored));
    assert!(spoofed.issues.contains(&ItipIssue::SenderNotOrganizer));
    // A cancelled copy.
    let mut gone = event.clone();
    gone.fields.status = EventStatus::Cancelled;
    let inv = card(
        &text,
        &["ada@example.org"],
        &bob(),
        Some(StoredCopy {
            event: &gone,
            dtstamp: None,
            read_only: false,
        }),
    );
    assert_eq!(inv.state, InvitationState::Cancelled);
}

#[test]
fn occurrence_cancellation_of_an_excluded_occurrence() {
    let series = cal(
        "REQUEST",
        "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:20261005T070000Z\r\n\
DTEND:20261005T080000Z\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nEXDATE:20261012T070000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    );
    let event = stored_event(&series, 0);
    let cancel = |rid: &str| {
        cal(
            "CANCEL",
            &format!(
                "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:1\r\n\
RECURRENCE-ID:{rid}\r\nORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n"
            ),
        )
    };
    let stored = Some(StoredCopy {
        event: &event,
        dtstamp: None,
        read_only: false,
    });
    let inv = card(
        &cancel("20261012T070000Z"),
        &["ada@example.org"],
        &bob(),
        stored,
    );
    assert_eq!(inv.state, InvitationState::Cancelled);
    assert_eq!(inv.recurrence_id, Some(PimTime::utc("2026-10-12T07:00:00")));
    let inv = card(
        &cancel("20261019T070000Z"),
        &["ada@example.org"],
        &bob(),
        stored,
    );
    assert_eq!(inv.state, InvitationState::Update);
}

fn reply(status: &str, seq: u32, stamp: &str) -> String {
    cal(
        "REPLY",
        &vevent(&format!(
            "SEQUENCE:{seq}\r\nDTSTAMP:{stamp}\r\nATTENDEE;CN=Bob;PARTSTAT={status}:mailto:bob@example.org\r\n\
COMMENT:See you\r\n"
        )),
    )
}

#[test]
fn replies_on_the_organizer_side() {
    let event = stored_event(&request(1, "20261001T080000Z"), 1);
    let stored = Some(StoredCopy {
        event: &event,
        dtstamp: None,
        read_only: false,
    });
    let accepted = reply("ACCEPTED", 1, "20261001T090000Z");
    let inv = card(&accepted, &["bob@example.org"], &ada(), stored);
    assert_eq!(inv.role, InvitationRole::Organizer);
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.my_address.as_deref(), Some("ada@example.org"));
    assert_eq!(inv.my_status, None);
    let from = inv.from_attendee.as_ref().unwrap();
    assert_eq!(from.email, "bob@example.org");
    assert_eq!(from.status, ParticipationStatus::Accepted);
    assert_eq!(inv.comment.as_deref(), Some("See you"));
    assert_eq!(inv.actions, [A::ApplyReply, A::OpenEvent]);
    assert!(inv.issues.is_empty());

    // Already recorded: current.
    let needs_action = reply("NEEDS-ACTION", 1, "20261001T090000Z");
    assert_eq!(
        card(&needs_action, &["bob@example.org"], &ada(), stored).state,
        InvitationState::Current
    );
    // An answer to an older version is stale.
    let old = reply("DECLINED", 0, "20261003T090000Z");
    assert_eq!(
        card(&old, &["bob@example.org"], &ada(), stored).state,
        InvitationState::Outdated
    );
    // Older than the last answer applied.
    let object = parse_itip(&accepted, None);
    let senders = vec!["bob@example.org".to_owned()];
    let me = ada();
    let inv = build_card(CardInput {
        message_id: "m".into(),
        object: &object,
        senders: &senders,
        me: &me,
        stored,
        last_reply: Some(Version::new(1, Some("2026-10-01T10:00:00Z".into()))),
        applied: false,
        conflicts: Vec::new(),
        transport: SchedulingTransport::Imip,
    });
    assert_eq!(inv.state, InvitationState::Outdated);
    // From someone else: spoofing warning.
    let inv = card(&accepted, &["mallory@evil.example"], &ada(), stored);
    assert!(inv.issues.contains(&ItipIssue::SenderNotAttendee));
    // Not my event.
    let inv = card(&accepted, &["bob@example.org"], &bob(), None);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert!(inv.issues.contains(&ItipIssue::NotAddressedToMe));
    assert_eq!(inv.state, InvitationState::UnknownEvent);
    // Unknown event.
    let inv = card(&accepted, &["bob@example.org"], &ada(), None);
    assert_eq!(inv.state, InvitationState::UnknownEvent);
    assert!(inv.actions.is_empty());
}

#[test]
fn counters_and_refreshes() {
    let event = stored_event(&request(1, "20261001T080000Z"), 1);
    let stored = Some(StoredCopy {
        event: &event,
        dtstamp: None,
        read_only: false,
    });
    // Outlook lists every attendee in a COUNTER; the sender picks the proposer.
    let counter = cal(
        "COUNTER",
        "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261001T100000Z\r\nSEQUENCE:1\r\n\
DTSTART:20261005T120000Z\r\nDTEND:20261005T130000Z\r\nLOCATION:Room 2\r\nSUMMARY:Planning\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:carol@example.org\r\n\
ATTENDEE;PARTSTAT=TENTATIVE:mailto:bob@example.org\r\nCOMMENT:Later?\r\nEND:VEVENT\r\n",
    );
    let inv = card(&counter, &["bob@example.org"], &ada(), stored);
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.from_attendee.as_ref().unwrap().email, "bob@example.org");
    assert_eq!(
        inv.actions,
        [A::AcceptCounter, A::DeclineCounter, A::OpenEvent]
    );
    let proposal = inv.counter.as_ref().unwrap();
    assert_eq!(proposal.start, PimTime::utc("2026-10-05T12:00:00"));
    assert_eq!(proposal.location.as_deref(), Some("Room 2"));
    assert_eq!(proposal.comment.as_deref(), Some("Later?"));
    // Several attendees and no matching sender: unclear who proposes.
    let inv = card(&counter, &["mallory@evil.example"], &ada(), stored);
    assert!(inv.issues.contains(&ItipIssue::AttendeeCount));

    let refresh = cal(
        "REFRESH",
        "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261001T100000Z\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    );
    let inv = card(&refresh, &["bob@example.org"], &ada(), stored);
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.actions, [A::SendCurrent, A::OpenEvent]);
    assert!(inv.fields.is_none());

    let decline = cal(
        "DECLINECOUNTER",
        "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261001T110000Z\r\nSEQUENCE:1\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nCOMMENT:No\r\nEND:VEVENT\r\n",
    );
    let inv = card(&decline, &["ada@example.org"], &bob(), stored);
    assert_eq!(inv.role, InvitationRole::Attendee);
    assert_eq!(inv.state, InvitationState::Current);
    assert_eq!(inv.actions, [A::Refresh, A::OpenEvent]);
}

#[test]
fn publish_add_and_invalid() {
    let publish = cal(
        "PUBLISH",
        "BEGIN:VEVENT\r\nUID:p@x\r\nDTSTAMP:20261001T100000Z\r\nDTSTART:20261005T120000Z\r\n\
SUMMARY:Webinar\r\nEND:VEVENT\r\n",
    );
    let inv = card(&publish, &["news@example.org"], &bob(), None);
    assert_eq!(inv.role, InvitationRole::Observer);
    assert_eq!(inv.state, InvitationState::New);
    assert_eq!(inv.actions, [A::AddToCalendar]);
    assert!(inv.issues.is_empty());

    let add = cal(
        "ADD",
        "BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261002T100000Z\r\nSEQUENCE:2\r\n\
DTSTART:20261030T070000Z\r\nDTEND:20261030T080000Z\r\nSUMMARY:Planning\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    );
    let inv = card(&add, &["ada@example.org"], &bob(), None);
    assert_eq!(inv.state, InvitationState::UnknownEvent);
    assert_eq!(inv.actions, [A::Refresh]);
    assert_eq!(inv.recurrence_id, Some(PimTime::utc("2026-10-30T07:00:00")));
    let event = stored_event(&request(1, "20261001T080000Z"), 1);
    let inv = card(
        &add,
        &["ada@example.org"],
        &bob(),
        Some(StoredCopy {
            event: &event,
            dtstamp: None,
            read_only: false,
        }),
    );
    assert_eq!(inv.state, InvitationState::Update);
    assert_eq!(inv.actions, [A::UpdateCalendar, A::OpenEvent]);

    let broken = card("not a calendar", &[], &bob(), None);
    assert_eq!(broken.state, InvitationState::Invalid);
    assert!(broken.actions.is_empty());
    assert!(broken.issues.contains(&ItipIssue::Unparsable));
}

#[test]
fn conflicts_only_where_they_matter() {
    let text = request(0, "20261001T080000Z");
    let object = parse_itip(&text, None);
    let conflict = InvitationConflict {
        event_id: "other".into(),
        calendar_id: "cal".into(),
        recurrence_id: None,
        title: "Dentist".into(),
        start_utc: "2026-10-05T07:30:00Z".into(),
        end_utc: "2026-10-05T08:30:00Z".into(),
        all_day: false,
        tentative: false,
    };
    let senders = vec!["ada@example.org".to_owned()];
    let me = bob();
    let input = CardInput {
        message_id: "m".into(),
        object: &object,
        senders: &senders,
        me: &me,
        stored: None,
        last_reply: None,
        applied: false,
        conflicts: vec![conflict.clone()],
        transport: SchedulingTransport::Imip,
    };
    assert_eq!(
        build_card(input.clone()).conflicts,
        std::slice::from_ref(&conflict)
    );
    let cancel = parse_itip(&text.replace("METHOD:REQUEST", "METHOD:CANCEL"), None);
    let inv = build_card(CardInput {
        object: &cancel,
        ..input
    });
    assert!(inv.conflicts.is_empty());
}

#[test]
fn disallowed_counters_and_my_entry() {
    let text = request(0, "20261001T080000Z").replace(
        "SUMMARY:Planning\r\n",
        "SUMMARY:Planning\r\nX-MICROSOFT-DISALLOW-COUNTER:TRUE\r\n",
    );
    let inv = card(&text, &["ada@example.org"], &bob(), None);
    assert!(!inv.allows(A::ProposeNewTime));
    let fields = inv.fields.as_ref().unwrap();
    assert_eq!(
        my_entry(fields, "MAILTO:BOB@example.org").map(|a| a.status),
        Some(ParticipationStatus::NeedsAction)
    );
    // The organizer looking at their own invitation.
    let own = card(&text, &["ada@example.org"], &ada(), None);
    assert_eq!(own.role, InvitationRole::Organizer);
    assert_eq!(own.actions, [A::AddToCalendar]);
}
