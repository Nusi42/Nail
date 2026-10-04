//! Smoke tests of the libraries WP81 builds on (WP80, ADR 0065): an iMIP
//! mail built with mail-builder, read back with mail-parser (the
//! `method` parameter of the `text/calendar` part), and its calendar data
//! read by nail-ical (`METHOD`, `UID`, `SEQUENCE`, attendees).
#![cfg(test)]

use mail_builder::MessageBuilder;
use mail_builder::headers::content_type::ContentType;
use mail_builder::mime::MimePart;
use mail_parser::{MessageParser, MimeHeaders};
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::scheduling::ItipMethod;

const REQUEST: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example//Test//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:meeting-1@example.org\r\n\
DTSTAMP:20261001T080000Z\r\n\
SEQUENCE:2\r\n\
DTSTART:20261005T090000Z\r\n\
DTEND:20261005T100000Z\r\n\
SUMMARY:Planning\r\n\
ORGANIZER;CN=Ada:mailto:ada@example.org\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.org\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn imip_request_round_trips_through_the_mail_libraries() {
    let calendar = MimePart::new(
        ContentType::new("text/calendar")
            .attribute("method", "REQUEST")
            .attribute("charset", "utf-8"),
        REQUEST,
    );
    let raw = MessageBuilder::new()
        .from(("Ada", "ada@example.org"))
        .to("bob@example.org")
        .subject("Invitation: Planning")
        .body(MimePart::new(
            "multipart/alternative",
            vec![MimePart::new("text/plain", "You are invited."), calendar],
        ))
        .write_to_vec()
        .unwrap();

    let message = MessageParser::default().parse(&raw).unwrap();
    let part = message
        .parts
        .iter()
        .find(|p| {
            p.content_type()
                .is_some_and(|ct| ct.ctype() == "text" && ct.subtype() == Some("calendar"))
        })
        .expect("a text/calendar part");
    let method = part
        .content_type()
        .and_then(|ct| ct.attribute("method"))
        .and_then(ItipMethod::from_ical);
    assert_eq!(method, Some(ItipMethod::Request));
    let text = part.text_contents().expect("calendar text");

    let parsed = nail_ical::parse_calendar(text, nail_ical::ParseOptions::default());
    assert_eq!(
        parsed.method.as_deref().and_then(ItipMethod::from_ical),
        method
    );
    let event = &parsed.events[0];
    assert_eq!(event.uid, "meeting-1@example.org");
    assert_eq!(event.sequence, 2);
    let organizer = event.fields.organizer.as_ref().unwrap();
    assert_eq!(organizer.email, "ada@example.org");
    assert_eq!(event.fields.attendees[0].email, "bob@example.org");
    assert_eq!(
        event.fields.attendees[0].status,
        ParticipationStatus::NeedsAction
    );
    assert!(event.fields.attendees[0].rsvp);
}
