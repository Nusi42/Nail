use super::*;
use crate::object::parse_itip;
use nail_api::pim::calendar::{Alarm, EventStatus};
use pretty_assertions::assert_eq;

fn cal(method: &str, body: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nMETHOD:{method}\r\n{body}END:VCALENDAR\r\n"
    )
}

const SERIES: &str = "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:1\r\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\r\nDTEND;TZID=Europe/Berlin:20261005T100000\r\n\
RRULE:FREQ=WEEKLY;COUNT=4\r\nSUMMARY:Weekly\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.org\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:carol@example.org\r\nEND:VEVENT\r\n";

fn stored(text: &str) -> CalendarEvent {
    let object = parse_itip(text, None);
    let mut event =
        object
            .event
            .unwrap()
            .into_event("ev".into(), "cal".into(), false, Default::default());
    event.fields.alarms = vec![Alarm::minutes_before(15)];
    event.fields.color = Some("#ff0000".into());
    event
}

fn berlin(local: &str) -> PimTime {
    PimTime::zoned(local, "Europe/Berlin")
}

fn status_of(fields: &EventFields, email: &str) -> Option<ParticipationStatus> {
    fields
        .attendees
        .iter()
        .find(|a| a.email == email)
        .map(|a| a.status)
}

#[test]
fn new_events_come_without_method() {
    let object = parse_itip(&cal("REQUEST", SERIES), None);
    let new = new_event(&object).unwrap();
    assert_eq!(new.event.uid, "s@x");
    assert!(!new.text.contains("METHOD"));
    let bare = parse_itip(
        &cal(
            "CANCEL",
            "BEGIN:VEVENT\r\nUID:s@x\r\nORGANIZER:mailto:ada@example.org\r\nEND:VEVENT\r\n",
        ),
        None,
    );
    assert_eq!(new_event(&bare).unwrap_err(), ApplyError::NoEventData);
}

#[test]
fn a_series_update_keeps_personal_settings() {
    let existing = stored(&cal("REQUEST", SERIES));
    let update = cal(
        "REQUEST",
        &SERIES
            .replace("SEQUENCE:1", "SEQUENCE:2")
            .replace("T090000", "T110000")
            .replace("T100000", "T120000")
            .replace("SUMMARY:Weekly", "SUMMARY:Weekly (later)"),
    );
    let object = parse_itip(&update, None);
    let updated = apply_request(&existing, &object).unwrap();
    assert_eq!(updated.id, existing.id);
    assert_eq!(updated.sequence, 2);
    assert_eq!(updated.fields.title, "Weekly (later)");
    assert_eq!(updated.fields.start, berlin("2026-10-05T11:00:00"));
    assert_eq!(updated.fields.alarms, existing.fields.alarms);
    assert_eq!(updated.fields.color.as_deref(), Some("#ff0000"));
    let other = parse_itip(
        &cal("REQUEST", &SERIES.replace("UID:s@x", "UID:other")),
        None,
    );
    assert_eq!(
        apply_request(&existing, &other).unwrap_err(),
        ApplyError::WrongEvent
    );
}

#[test]
fn an_occurrence_update_becomes_an_override() {
    let existing = stored(&cal("REQUEST", SERIES));
    let moved = cal(
        "REQUEST",
        "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:2\r\n\
RECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\r\nDTSTART;TZID=Europe/Berlin:20261012T140000\r\n\
DTEND;TZID=Europe/Berlin:20261012T150000\r\nSUMMARY:Weekly (moved)\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    );
    let updated = apply_request(&existing, &parse_itip(&moved, None)).unwrap();
    assert_eq!(updated.overrides.len(), 1);
    let o = &updated.overrides[0];
    assert_eq!(o.recurrence_id, berlin("2026-10-12T09:00:00"));
    assert_eq!(o.fields.start, berlin("2026-10-12T14:00:00"));
    assert_eq!(o.fields.alarms, existing.fields.alarms);
    assert_eq!(updated.sequence, 2);
    // The master is untouched.
    assert_eq!(updated.fields, existing.fields);

    // A date the rule does not generate is added as an RDATE.
    let extra = moved.replace(
        "RECURRENCE-ID;TZID=Europe/Berlin:20261012T090000",
        "RECURRENCE-ID;TZID=Europe/Berlin:20261013T090000",
    );
    let updated = apply_request(&existing, &parse_itip(&extra, None)).unwrap();
    assert_eq!(
        updated.recurrence.as_ref().unwrap().dates,
        [berlin("2026-10-13T09:00:00")]
    );
    assert_eq!(updated.overrides.len(), 1);
}

#[test]
fn add_adds_occurrences() {
    let single = stored(&cal(
        "REQUEST",
        "BEGIN:VEVENT\r\nUID:a@x\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:20261005T070000Z\r\n\
DTEND:20261005T080000Z\r\nSUMMARY:Review\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    ));
    assert!(!single.is_recurring());
    let add = cal(
        "ADD",
        "BEGIN:VEVENT\r\nUID:a@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:1\r\nDTSTART:20261012T070000Z\r\n\
DTEND:20261012T090000Z\r\nSUMMARY:Review (2)\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:1\r\nDTSTART:20261019T070000Z\r\n\
DTEND:20261019T080000Z\r\nSUMMARY:Review (3)\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    );
    let object = parse_itip(&add, None);
    assert_eq!(object.components.len(), 2);
    let updated = apply_add(&single, &object).unwrap();
    assert!(updated.is_recurring());
    assert_eq!(
        updated.recurrence.as_ref().unwrap().dates,
        [
            PimTime::utc("2026-10-12T07:00:00"),
            PimTime::utc("2026-10-19T07:00:00")
        ]
    );
    assert_eq!(updated.overrides.len(), 2);
    assert_eq!(updated.overrides[0].fields.title, "Review (2)");
    assert_eq!(
        updated.overrides[0].fields.end,
        PimTime::utc("2026-10-12T09:00:00")
    );
    assert_eq!(updated.sequence, 1);
    // The master stays.
    assert_eq!(updated.fields.title, "Review");
}

#[test]
fn cancel_meeting_occurrence_and_following() {
    let existing = stored(&cal("REQUEST", SERIES));
    let cancel = |extra: &str, attendee: &str| {
        cal(
            "CANCEL",
            &format!(
                "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:2\r\n{extra}\
ORGANIZER:mailto:ada@example.org\r\n{attendee}STATUS:CANCELLED\r\nEND:VEVENT\r\n"
            ),
        )
    };
    let bob = "ATTENDEE:mailto:bob@example.org\r\n";
    let whole = parse_itip(&cancel("", bob), None);
    assert_eq!(
        apply_cancel(&existing, &whole, Some("bob@example.org")).unwrap(),
        Applied::Delete
    );
    // A cancel for someone else.
    let carol = parse_itip(&cancel("", "ATTENDEE:mailto:carol@example.org\r\n"), None);
    assert_eq!(
        apply_cancel(&existing, &carol, Some("bob@example.org")).unwrap(),
        Applied::Unchanged
    );
    let one = parse_itip(
        &cancel("RECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\r\n", bob),
        None,
    );
    let Applied::Updated(updated) = apply_cancel(&existing, &one, Some("bob@example.org")).unwrap()
    else {
        panic!("expected an update");
    };
    assert_eq!(
        updated.recurrence.as_ref().unwrap().exceptions,
        [berlin("2026-10-12T09:00:00")]
    );
    assert_eq!(updated.sequence, 2);
    // A recurrence id in UTC names the same occurrence.
    let utc = parse_itip(&cancel("RECURRENCE-ID:20261012T070000Z\r\n", bob), None);
    assert!(matches!(
        apply_cancel(&existing, &utc, None).unwrap(),
        Applied::Updated(_)
    ));
    // Not an occurrence: nothing happens.
    let wrong = parse_itip(&cancel("RECURRENCE-ID:20261013T070000Z\r\n", bob), None);
    assert_eq!(
        apply_cancel(&existing, &wrong, None).unwrap(),
        Applied::Unchanged
    );
    // This and following ends the series; from the first occurrence it goes.
    let future = parse_itip(
        &cancel(
            "RECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/Berlin:20261019T090000\r\n",
            bob,
        ),
        None,
    );
    let Applied::Updated(ended) = apply_cancel(&existing, &future, None).unwrap() else {
        panic!("expected an update");
    };
    assert_ne!(ended.recurrence, existing.recurrence);
    let all = parse_itip(
        &cancel(
            "RECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/Berlin:20261005T090000\r\n",
            bob,
        ),
        None,
    );
    assert_eq!(
        apply_cancel(&existing, &all, None).unwrap(),
        Applied::Delete
    );
}

fn reply(extra: &str, attendee: &str) -> String {
    cal(
        "REPLY",
        &format!(
            "BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261002T080000Z\r\nSEQUENCE:1\r\n{extra}\
ORGANIZER:mailto:ada@example.org\r\n{attendee}END:VEVENT\r\n"
        ),
    )
}

#[test]
fn replies_set_partstat_on_the_series() {
    let existing = stored(&cal("REQUEST", SERIES));
    let object = parse_itip(
        &reply("", "ATTENDEE;PARTSTAT=ACCEPTED:mailto:BOB@example.org\r\n"),
        None,
    );
    let updated = apply_reply(&existing, &object).unwrap();
    assert_eq!(
        status_of(&updated.fields, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
    let bob = updated
        .fields
        .attendees
        .iter()
        .find(|a| a.email == "bob@example.org")
        .unwrap();
    assert!(!bob.rsvp);
    assert_eq!(updated.sequence, existing.sequence);
    // An uninvited attendee is added (party crashing).
    let crasher = parse_itip(
        &reply(
            "",
            "ATTENDEE;CN=Eve;PARTSTAT=TENTATIVE:mailto:eve@example.org\r\n",
        ),
        None,
    );
    let updated = apply_reply(&existing, &crasher).unwrap();
    assert_eq!(updated.fields.attendees.len(), 3);
    assert_eq!(
        status_of(&updated.fields, "eve@example.org"),
        Some(ParticipationStatus::Tentative)
    );
}

#[test]
fn occurrence_replies_create_overrides() {
    let existing = stored(&cal("REQUEST", SERIES));
    let object = parse_itip(
        &reply(
            "RECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\r\n",
            "ATTENDEE;PARTSTAT=DECLINED:mailto:bob@example.org\r\n",
        ),
        None,
    );
    let updated = apply_reply(&existing, &object).unwrap();
    assert_eq!(updated.overrides.len(), 1);
    let o = &updated.overrides[0];
    assert_eq!(o.recurrence_id, berlin("2026-10-12T09:00:00"));
    assert_eq!(o.fields.start, berlin("2026-10-12T09:00:00"));
    assert_eq!(o.fields.end, berlin("2026-10-12T10:00:00"));
    assert_eq!(
        status_of(&o.fields, "bob@example.org"),
        Some(ParticipationStatus::Declined)
    );
    // The series keeps its answer.
    assert_eq!(
        status_of(&updated.fields, "bob@example.org"),
        Some(ParticipationStatus::NeedsAction)
    );

    // A later series answer leaves the individually answered occurrence alone,
    // but moves occurrences that followed the series.
    let mut with_two = updated.clone();
    let carol_override = occurrence_fields(&with_two, &berlin("2026-10-19T09:00:00")).unwrap();
    edit::override_occurrence(
        &mut with_two,
        &berlin("2026-10-19T09:00:00"),
        carol_override,
    )
    .unwrap();
    let series = parse_itip(
        &reply("", "ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\n"),
        None,
    );
    let answered = apply_reply(&with_two, &series).unwrap();
    assert_eq!(
        status_of(&answered.fields, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
    let declined = override_for(&answered, &berlin("2026-10-12T09:00:00")).unwrap();
    assert_eq!(
        status_of(&declined.fields, "bob@example.org"),
        Some(ParticipationStatus::Declined)
    );
    let followed = override_for(&answered, &berlin("2026-10-19T09:00:00")).unwrap();
    assert_eq!(
        status_of(&followed.fields, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );

    // Not an occurrence.
    let wrong = parse_itip(
        &reply(
            "RECURRENCE-ID;TZID=Europe/Berlin:20261013T090000\r\n",
            "ATTENDEE;PARTSTAT=DECLINED:mailto:bob@example.org\r\n",
        ),
        None,
    );
    assert_eq!(
        apply_reply(&existing, &wrong).unwrap_err(),
        ApplyError::NotAnOccurrence
    );
    let nobody = parse_itip(&reply("", ""), None);
    assert_eq!(
        apply_reply(&existing, &nobody).unwrap_err(),
        ApplyError::NoAttendee
    );
}

#[test]
fn delegation_replies_add_the_delegate() {
    let existing = stored(&cal("REQUEST", SERIES));
    let object = parse_itip(
        &reply(
            "",
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:dan@example.org\":mailto:bob@example.org\r\n\
ATTENDEE;DELEGATED-FROM=\"mailto:bob@example.org\";PARTSTAT=NEEDS-ACTION:mailto:dan@example.org\r\n",
        ),
        None,
    );
    let updated = apply_reply(&existing, &object).unwrap();
    let bob = updated
        .fields
        .attendees
        .iter()
        .find(|a| a.email == "bob@example.org")
        .unwrap();
    assert_eq!(bob.status, ParticipationStatus::Delegated);
    assert_eq!(bob.delegated_to, ["dan@example.org"]);
    let dan = updated
        .fields
        .attendees
        .iter()
        .find(|a| a.email == "dan@example.org")
        .unwrap();
    assert_eq!(dan.delegated_from, ["bob@example.org"]);
    assert_eq!(updated.fields.attendees.len(), 3);
    // The delegate's own reply later.
    let dan_reply = parse_itip(
        &reply(
            "",
            "ATTENDEE;PARTSTAT=ACCEPTED;DELEGATED-FROM=\"mailto:bob@example.org\":mailto:dan@example.org\r\n",
        ),
        None,
    );
    let accepted = apply_reply(&updated, &dan_reply).unwrap();
    assert_eq!(
        status_of(&accepted.fields, "dan@example.org"),
        Some(ParticipationStatus::Accepted)
    );
}

#[test]
fn my_answers_and_delegation() {
    let existing = stored(&cal("REQUEST", SERIES));
    let mine = set_my_status(
        &existing,
        "Bob@Example.org",
        None,
        ParticipationStatus::Tentative,
        None,
    )
    .unwrap();
    assert_eq!(
        status_of(&mine.fields, "bob@example.org"),
        Some(ParticipationStatus::Tentative)
    );
    let occurrence = set_my_status(
        &existing,
        "bob@example.org",
        None,
        ParticipationStatus::Declined,
        Some(&PimTime::utc("2026-10-26T08:00:00")),
    )
    .unwrap();
    // 26 October is after the DST change: 09:00 Berlin = 08:00 UTC.
    assert_eq!(
        occurrence.overrides[0].recurrence_id,
        berlin("2026-10-26T09:00:00")
    );
    // A forwarded invitation: the user is added.
    let crashed = set_my_status(
        &existing,
        "dan@example.org",
        Some("Dan"),
        ParticipationStatus::Accepted,
        None,
    )
    .unwrap();
    let dan = crashed.fields.attendees.last().unwrap();
    assert_eq!(
        (dan.email.as_str(), dan.name.as_deref()),
        ("dan@example.org", Some("Dan"))
    );

    let mut delegate_to = Attendee::new("Dan@Example.org");
    delegate_to.name = Some("Dan".into());
    let delegated = delegate(&existing, "bob@example.org", &delegate_to, None).unwrap();
    let bob = delegated
        .fields
        .attendees
        .iter()
        .find(|a| a.email == "bob@example.org")
        .unwrap();
    assert_eq!(bob.status, ParticipationStatus::Delegated);
    assert_eq!(bob.delegated_to, ["dan@example.org"]);
    let dan = delegated
        .fields
        .attendees
        .iter()
        .find(|a| a.email == "dan@example.org")
        .unwrap();
    assert!(dan.rsvp);
    assert_eq!(dan.delegated_from, ["bob@example.org"]);
    assert_eq!(dan.status, ParticipationStatus::NeedsAction);
}

#[test]
fn accepting_a_counter_moves_the_event() {
    let existing = stored(&cal("REQUEST", SERIES));
    let proposal = CounterProposal {
        start: berlin("2026-10-05T13:00:00"),
        end: berlin("2026-10-05T14:00:00"),
        location: Some("Room 2".into()),
        comment: None,
    };
    let moved = accept_counter(&existing, &proposal, None, "ada@example.org").unwrap();
    assert_eq!(moved.fields.start, proposal.start);
    assert_eq!(moved.fields.location.as_deref(), Some("Room 2"));
    assert_eq!(moved.sequence, existing.sequence + 1);
    assert_eq!(
        status_of(&moved.fields, "carol@example.org"),
        Some(ParticipationStatus::NeedsAction)
    );
    let one = accept_counter(
        &existing,
        &proposal,
        Some(&berlin("2026-10-12T09:00:00")),
        "ada@example.org",
    )
    .unwrap();
    assert_eq!(one.overrides[0].fields.start, proposal.start);
    assert_eq!(one.fields.start, existing.fields.start);
}

#[test]
fn single_events_and_occurrence_ids() {
    let single = stored(&cal(
        "REQUEST",
        "BEGIN:VEVENT\r\nUID:a@x\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:20261005T070000Z\r\n\
DTEND:20261005T080000Z\r\nSUMMARY:Review\r\nSTATUS:CONFIRMED\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n",
    ));
    // A recurrence id equal to the start names the event itself.
    let r = set_my_status(
        &single,
        "bob@example.org",
        None,
        ParticipationStatus::Accepted,
        Some(&PimTime::utc("2026-10-05T07:00:00")),
    )
    .unwrap();
    assert_eq!(
        status_of(&r.fields, "bob@example.org"),
        Some(ParticipationStatus::Accepted)
    );
    assert_eq!(
        set_my_status(
            &single,
            "bob@example.org",
            None,
            ParticipationStatus::Accepted,
            Some(&PimTime::utc("2026-10-06T07:00:00"))
        )
        .unwrap_err(),
        ApplyError::NotAnOccurrence
    );
    let cancel = parse_itip(
        &cal(
            "CANCEL",
            "BEGIN:VEVENT\r\nUID:a@x\r\nRECURRENCE-ID:20261005T070000Z\r\nORGANIZER:mailto:ada@example.org\r\nEND:VEVENT\r\n",
        ),
        None,
    );
    assert_eq!(
        apply_cancel(&single, &cancel, None).unwrap(),
        Applied::Delete
    );
    assert_eq!(single.fields.status, EventStatus::Confirmed);
}
