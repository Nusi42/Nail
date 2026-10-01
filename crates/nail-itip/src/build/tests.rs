use super::*;
use crate::object::ScheduleAgent;
use chrono::TimeZone;
use nail_api::pim::calendar::ParticipationStatus;
use pretty_assertions::assert_eq;

const STORED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Nail PIM//EN\r\n\
BEGIN:VTIMEZONE\r\nTZID:W. Europe Standard Time\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\n\
TZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:1\r\n\
DTSTART;TZID=W. Europe Standard Time:20261005T090000\r\nDTEND;TZID=W. Europe Standard Time:20261005T100000\r\n\
RRULE:FREQ=WEEKLY;COUNT=4\r\nSUMMARY:Weekly\r\nLOCATION:Room 1\r\n\
ORGANIZER;CN=Ada;SCHEDULE-AGENT=CLIENT:mailto:ada@example.org\r\n\
ATTENDEE;CN=Bob;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;SCHEDULE-STATUS=1.2:mailto:bob@example.org\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:carol@example.org\r\nX-CUSTOM:kept\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s@x\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:1\r\n\
RECURRENCE-ID;TZID=W. Europe Standard Time:20261012T090000\r\n\
DTSTART;TZID=W. Europe Standard Time:20261012T140000\r\nDTEND;TZID=W. Europe Standard Time:20261012T150000\r\n\
SUMMARY:Weekly (moved)\r\nORGANIZER;CN=Ada:mailto:ada@example.org\r\n\
ATTENDEE;CN=Bob;PARTSTAT=NEEDS-ACTION:mailto:bob@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 3, 12, 0, 0).unwrap()
}

fn berlin(local: &str) -> PimTime {
    PimTime::zoned(local, "Europe/Berlin")
}

fn parse(text: &str) -> ItipObject {
    let object = parse_itip(text, None);
    assert!(object.issues.is_empty(), "{:?}\n{text}", object.issues);
    object
}

#[test]
fn requests_drop_alarms_and_scheduling_params() {
    let text = build_request(
        STORED,
        &RequestOptions {
            method: ItipMethod::Request,
            uid: "s@x",
            recurrence_id: None,
            sequence: Some(2),
            now: now(),
        },
    )
    .unwrap();
    assert!(text.contains("METHOD:REQUEST\r\n"));
    assert!(text.contains("PRODID:-//Nail//Nail PIM//EN"));
    assert!(!text.contains("VALARM"));
    assert!(!text.contains("SCHEDULE-"));
    assert!(text.contains("X-CUSTOM:kept"));
    // The source's own VTIMEZONE travels with it.
    assert!(text.contains("TZID:W. Europe Standard Time"));
    let object = parse(&text);
    assert_eq!(object.method, Some(ItipMethod::Request));
    assert_eq!(object.components.len(), 2);
    for c in &object.components {
        assert_eq!(c.sequence, 2);
        assert_eq!(c.dtstamp.as_deref(), Some("2026-10-03T12:00:00Z"));
        assert_eq!(c.organizer_agent, ScheduleAgent::Server);
    }
    let master = object.master().unwrap();
    assert_eq!(
        master.fields.as_ref().unwrap().start,
        berlin("2026-10-05T09:00:00")
    );

    // One occurrence only.
    let one = build_request(
        STORED,
        &RequestOptions {
            method: ItipMethod::Request,
            uid: "s@x",
            recurrence_id: Some(&berlin("2026-10-12T09:00:00")),
            sequence: None,
            now: now(),
        },
    )
    .unwrap();
    let object = parse(&one);
    assert_eq!(object.components.len(), 1);
    assert_eq!(object.components[0].sequence, 1);
    assert_eq!(
        object.components[0].recurrence_id,
        Some(berlin("2026-10-12T09:00:00"))
    );
    assert_eq!(
        build_request(
            STORED,
            &RequestOptions {
                method: ItipMethod::Request,
                uid: "s@x",
                recurrence_id: Some(&berlin("2026-10-19T09:00:00")),
                sequence: None,
                now: now(),
            },
        )
        .unwrap_err(),
        BuildError::NoOccurrence
    );
    assert_eq!(
        build_request(
            STORED,
            &RequestOptions {
                method: ItipMethod::Request,
                uid: "nope",
                recurrence_id: None,
                sequence: None,
                now: now(),
            },
        )
        .unwrap_err(),
        BuildError::NoEvent
    );
}

#[test]
fn requests_need_an_organizer_but_publish_does_not() {
    let plain = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\nBEGIN:VEVENT\r\nUID:p\r\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\r\nSUMMARY:Party\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let options = |method| RequestOptions {
        method,
        uid: "p",
        recurrence_id: None,
        sequence: None,
        now: now(),
    };
    assert_eq!(
        build_request(plain, &options(ItipMethod::Request)).unwrap_err(),
        BuildError::NoOrganizer
    );
    let published = build_request(plain, &options(ItipMethod::Publish)).unwrap();
    assert!(published.contains("METHOD:PUBLISH"));
    // No VTIMEZONE in the source: one is generated for the IANA zone.
    assert!(published.contains("BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin"));
    let object = parse(&published);
    assert_eq!(
        object.main().unwrap().fields.as_ref().unwrap().start,
        berlin("2026-10-05T09:00:00")
    );
}

#[test]
fn replies_name_one_attendee() {
    let mut bob = Attendee::new("bob@example.org");
    bob.status = ParticipationStatus::Accepted;
    let text = build_reply(
        STORED,
        &ReplyOptions {
            uid: "s@x",
            recurrence_id: None,
            attendee: &bob,
            delegates: &[],
            comment: Some("Looking forward"),
            now: now(),
        },
    )
    .unwrap();
    assert!(text.contains("METHOD:REPLY"));
    assert!(text.contains("ATTENDEE;CN=Bob;PARTSTAT=ACCEPTED:mailto:bob@example.org"));
    assert!(!text.contains("carol"));
    assert!(!text.contains("RSVP"));
    assert!(!text.contains("RRULE:FREQ=WEEKLY"));
    assert!(!text.contains("SCHEDULE-"));
    let object = parse(&text);
    let main = object.main().unwrap();
    assert_eq!(main.sequence, 1);
    assert_eq!(main.attendees.len(), 1);
    assert_eq!(
        main.attendees[0].attendee.status,
        ParticipationStatus::Accepted
    );
    assert_eq!(main.comment.as_deref(), Some("Looking forward"));
    assert_eq!(main.organizer.as_ref().unwrap().email, "ada@example.org");

    // An occurrence with an override uses its RECURRENCE-ID and times.
    let mut declined = bob.clone();
    declined.status = ParticipationStatus::Declined;
    let moved = build_reply(
        STORED,
        &ReplyOptions {
            uid: "s@x",
            recurrence_id: Some(&berlin("2026-10-12T09:00:00")),
            attendee: &declined,
            delegates: &[],
            comment: None,
            now: now(),
        },
    )
    .unwrap();
    let object = parse(&moved);
    let c = object.main().unwrap();
    assert_eq!(c.recurrence_id, Some(berlin("2026-10-12T09:00:00")));
    assert_eq!(
        c.fields.as_ref().unwrap().start,
        berlin("2026-10-12T14:00:00")
    );
    // An occurrence without an override: RECURRENCE-ID written like DTSTART.
    let plain = build_reply(
        STORED,
        &ReplyOptions {
            uid: "s@x",
            recurrence_id: Some(&PimTime::utc("2026-10-19T07:00:00")),
            attendee: &declined,
            delegates: &[],
            comment: None,
            now: now(),
        },
    )
    .unwrap();
    assert!(plain.contains("RECURRENCE-ID;TZID=W. Europe Standard Time:20261019T090000"));
    assert!(!plain.contains("DTSTART;"));
    let object = parse(&plain);
    assert_eq!(
        object.main().unwrap().recurrence_id,
        Some(berlin("2026-10-19T09:00:00"))
    );
}

#[test]
fn delegation_reply_and_request() {
    let mut bob = Attendee::new("bob@example.org");
    bob.status = ParticipationStatus::Delegated;
    bob.delegated_to = vec!["dan@example.org".into()];
    let dan = Attendee::new("dan@example.org");
    let reply = build_reply(
        STORED,
        &ReplyOptions {
            uid: "s@x",
            recurrence_id: None,
            attendee: &bob,
            delegates: std::slice::from_ref(&dan),
            comment: None,
            now: now(),
        },
    )
    .unwrap();
    let object = parse(&reply);
    let main = object.main().unwrap();
    assert_eq!(main.attendees.len(), 2);
    assert_eq!(main.attendees[0].attendee.delegated_to, ["dan@example.org"]);
    assert_eq!(
        main.attendees[1].attendee.delegated_from,
        ["bob@example.org"]
    );

    let request = build_delegated_request(STORED, "s@x", "bob@example.org", &dan, now()).unwrap();
    let object = parse(&request);
    for c in &object.components {
        let b = c.attendee("bob@example.org").unwrap();
        assert_eq!(b.attendee.status, ParticipationStatus::Delegated);
        assert_eq!(b.attendee.delegated_to, ["dan@example.org"]);
        let d = c.attendee("dan@example.org").unwrap();
        assert!(d.attendee.rsvp);
        assert_eq!(d.attendee.delegated_from, ["bob@example.org"]);
        assert_eq!(c.sequence, 1);
    }
    assert!(!request.contains("VALARM"));
}

#[test]
fn cancels() {
    let options = |what| CancelOptions {
        uid: "s@x",
        what,
        sequence: 2,
        comment: Some("Sorry"),
        now: now(),
    };
    let meeting = build_cancel(STORED, &options(CancelWhat::Meeting)).unwrap();
    let object = parse(&meeting);
    assert_eq!(object.method, Some(ItipMethod::Cancel));
    let c = object.main().unwrap();
    assert!(c.recurrence_id.is_none());
    assert_eq!(c.sequence, 2);
    assert_eq!(c.attendees.len(), 2);
    assert_eq!(c.comment.as_deref(), Some("Sorry"));
    assert!(meeting.contains("STATUS:CANCELLED"));

    let rid = berlin("2026-10-19T09:00:00");
    let occurrence = build_cancel(STORED, &options(CancelWhat::Occurrence(&rid))).unwrap();
    let object = parse(&occurrence);
    assert_eq!(object.main().unwrap().recurrence_id, Some(rid.clone()));

    let removed = ["carol@example.org".to_owned(), "zed@example.org".to_owned()];
    let some = build_cancel(STORED, &options(CancelWhat::Attendees(&removed))).unwrap();
    let object = parse(&some);
    let emails: Vec<&str> = object
        .main()
        .unwrap()
        .attendees
        .iter()
        .map(|a| a.attendee.email.as_str())
        .collect();
    assert_eq!(emails, ["carol@example.org", "zed@example.org"]);
}

#[test]
fn counters_refreshes_and_declines() {
    let proposal = CounterProposal {
        start: PimTime::utc("2026-10-05T13:00:00"),
        end: PimTime::utc("2026-10-05T14:00:00"),
        location: Some("Café".into()),
        comment: Some("Afternoon?".into()),
    };
    let text = build_counter(
        STORED,
        &CounterOptions {
            uid: "s@x",
            recurrence_id: None,
            attendee: "bob@example.org",
            proposal: &proposal,
            now: now(),
        },
    )
    .unwrap();
    let object = parse(&text);
    assert_eq!(object.method, Some(ItipMethod::Counter));
    let c = object.master().unwrap();
    let fields = c.fields.as_ref().unwrap();
    assert_eq!(fields.start, PimTime::utc("2026-10-05T13:00:00"));
    assert_eq!(fields.location.as_deref(), Some("Café"));
    assert_eq!(c.comment.as_deref(), Some("Afternoon?"));
    assert!(text.contains("RRULE:FREQ=WEEKLY"));
    assert!(!text.contains("VALARM"));
    // One occurrence: no recurrence, its RECURRENCE-ID.
    let one = build_counter(
        STORED,
        &CounterOptions {
            uid: "s@x",
            recurrence_id: Some(&berlin("2026-10-19T09:00:00")),
            attendee: "dan@example.org",
            proposal: &proposal,
            now: now(),
        },
    )
    .unwrap();
    assert!(!one.contains("RRULE:FREQ=WEEKLY"));
    let object = parse(&one);
    let c = object.main().unwrap();
    assert_eq!(c.recurrence_id, Some(berlin("2026-10-19T09:00:00")));
    assert!(c.attendee("dan@example.org").is_some());

    let minimal = |rid| MinimalOptions {
        uid: "s@x",
        recurrence_id: rid,
        attendee: "bob@example.org",
        comment: Some("Lost it"),
        sequence: Some(5),
        now: now(),
    };
    let refresh = build_refresh(STORED, &minimal(None)).unwrap();
    let object = parse(&refresh);
    assert_eq!(object.method, Some(ItipMethod::Refresh));
    let c = object.main().unwrap();
    assert_eq!(c.attendees.len(), 1);
    assert!(c.fields.is_none());
    assert!(!refresh.contains("SEQUENCE"));

    let rid = berlin("2026-10-12T09:00:00");
    let decline = build_decline_counter(STORED, &minimal(Some(&rid))).unwrap();
    let object = parse(&decline);
    assert_eq!(object.method, Some(ItipMethod::DeclineCounter));
    let c = object.main().unwrap();
    assert_eq!(c.sequence, 5);
    assert_eq!(c.recurrence_id, Some(rid));
    assert_eq!(c.comment.as_deref(), Some("Lost it"));
}

#[test]
fn time_and_attendee_properties() {
    let mut out = String::new();
    time_property("DTSTART", &PimTime::date("2026-10-05")).write(&mut out);
    time_property("DTSTART", &PimTime::floating("2026-10-05T09:00:00")).write(&mut out);
    time_property("DTSTART", &PimTime::utc("2026-10-05T09:00:00")).write(&mut out);
    time_property("DTSTART", &berlin("2026-10-05T09:00:00")).write(&mut out);
    assert_eq!(
        out,
        "DTSTART;VALUE=DATE:20261005\r\nDTSTART:20261005T090000\r\nDTSTART:20261005T090000Z\r\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\r\n"
    );
    let mut a = Attendee::new("room@example.org");
    a.kind = AttendeeKind::Room;
    a.role = AttendeeRole::NonParticipant;
    a.rsvp = true;
    a.name = Some("Room; 1".into());
    let mut out = String::new();
    attendee_property(&a).write(&mut out);
    let unfolded = out.replace("\r\n ", "");
    assert_eq!(
        unfolded,
        "ATTENDEE;CN=\"Room; 1\";ROLE=NON-PARTICIPANT;PARTSTAT=NEEDS-ACTION;CUTYPE=ROOM;RSVP=TRUE:mailto:room@example.org\r\n"
    );
}
