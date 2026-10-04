use super::*;
use mail_parser::MimeHeaders;
use pretty_assertions::assert_eq;

const CAL: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:m1@example.org\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:20261005T070000Z\r\n\
SUMMARY:Planning\r\nORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

fn mail(body: &str) -> Vec<u8> {
    format!(
        "From: Ada <ada@example.org>\r\nSender: calendar@example.org\r\nTo: bob@example.org\r\n\
Subject: Invitation\r\nMessage-ID: <inv-1@example.org>\r\nMIME-Version: 1.0\r\n{body}"
    )
    .into_bytes()
}

#[test]
fn finds_an_inline_calendar_part() {
    let raw = mail(&format!(
        "Content-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nHi\r\n\
--b\r\nContent-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n{CAL}--b--\r\n"
    ));
    let found = find_itip(&raw).unwrap();
    assert_eq!(found.method_param.as_deref(), Some("REQUEST"));
    assert!(!found.attachment);
    assert_eq!(found.from.as_deref(), Some("ada@example.org"));
    assert_eq!(found.senders(), ["ada@example.org", "calendar@example.org"]);
    assert_eq!(found.message_id.as_deref(), Some("inv-1@example.org"));
    assert_eq!(found.subject.as_deref(), Some("Invitation"));
    let object = found.parse();
    assert_eq!(object.method, Some(ItipMethod::Request));
    assert_eq!(object.uid.as_deref(), Some("m1@example.org"));
}

#[test]
fn prefers_the_method_part_over_attachments() {
    let raw = mail(&format!(
        "Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
Content-Type: application/ics; name=invite.ics\r\nContent-Disposition: attachment; filename=invite.ics\r\n\
Content-Transfer-Encoding: base64\r\n\r\nQkVHSU46VkNBTEVOREFSDQpFTkQ6VkNBTEVOREFSDQo=\r\n\
--m\r\nContent-Type: text/calendar; method=CANCEL; charset=utf-8\r\n\r\n{}--m--\r\n",
        CAL.replace("METHOD:REQUEST", "METHOD:CANCEL")
    ));
    let found = find_itip(&raw).unwrap();
    assert_eq!(found.method_param.as_deref(), Some("CANCEL"));
    assert!(!found.attachment);
}

#[test]
fn falls_back_to_ics_attachments() {
    let encoded = "QkVHSU46VkNBTEVOREFSDQpWRVJTSU9OOjIuMA0KTUVUSE9EOlBVQkxJU0gNCkJFR0lOOlZFVkVOVA0KVUlEOnANCkRUU1RBUlQ6MjAyNjEwMDVUMDcwMDAwWg0KRU5EOlZFVkVOVA0KRU5EOlZDQUxFTkRBUg0K";
    for ct in [
        "application/ics",
        "application/octet-stream; name=\"Event.ICS\"",
    ] {
        let raw = mail(&format!(
            "Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
--m\r\nContent-Type: {ct}\r\nContent-Disposition: attachment; filename=event.ics\r\n\
Content-Transfer-Encoding: base64\r\n\r\n{encoded}\r\n--m--\r\n"
        ));
        let found = find_itip(&raw).unwrap_or_else(|| panic!("{ct}"));
        assert!(found.attachment);
        assert_eq!(found.method_param, None);
        let object = found.parse();
        assert_eq!(object.method, Some(ItipMethod::Publish), "{ct}");
    }
    // Other attachments and plain mail are no invitations.
    let raw = mail(
        "Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\nContent-Type: text/plain\r\n\r\nHi\r\n\
--m\r\nContent-Type: application/octet-stream; name=a.bin\r\n\r\nxyz\r\n--m--\r\n",
    );
    assert!(find_itip(&raw).is_none());
    let raw = mail("Content-Type: text/plain\r\n\r\nBEGIN:VCALENDAR\r\n");
    assert!(find_itip(&raw).is_none());
    // A text/calendar part that is not calendar data.
    let raw = mail("Content-Type: text/calendar; method=REQUEST\r\n\r\nnothing here\r\n");
    assert!(find_itip(&raw).is_none());
}

#[test]
fn forwarded_mail_with_an_invitation_inside_is_not_one() {
    let inner = String::from_utf8(mail(&format!(
        "Content-Type: text/calendar; method=REQUEST\r\n\r\n{CAL}"
    )))
    .unwrap();
    let raw = mail(&format!(
        "Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\nContent-Type: text/plain\r\n\r\nFYI\r\n\
--m\r\nContent-Type: message/rfc822\r\n\r\n{inner}\r\n--m--\r\n"
    ));
    assert!(find_itip(&raw).is_none());
}

#[test]
fn latin1_calendar_attachments_are_decoded() {
    let mut raw = mail(
        "Content-Type: application/ics; charset=iso-8859-1; name=a.ics\r\n\
Content-Disposition: attachment; filename=a.ics\r\n\r\n",
    );
    raw.extend_from_slice(
        b"BEGIN:VCALENDAR\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20261005T070000Z\r\n\
SUMMARY:Cr\xe8me br\xfbl\xe9e\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    );
    let found = find_itip(&raw).unwrap();
    assert!(
        found.text.contains("SUMMARY:Crème brûlée"),
        "{}",
        found.text
    );
}

fn outgoing(attach: bool) -> OutgoingImip {
    let mut out = OutgoingImip::new(
        ItipMethod::Reply,
        MailAddress {
            name: Some("Bob Builder".into()),
            email: "bob@example.org".into(),
        },
        vec![MailAddress::new("ada@example.org")],
        crate::texts::MailText {
            subject: "Accepted: Planning".into(),
            text: "Bob has accepted.\n".into(),
            html: "<p>Bob has accepted.</p>".into(),
        },
        CAL.replace("METHOD:REQUEST", "METHOD:REPLY"),
    );
    out.attach_ics = attach;
    out.cc = vec![MailAddress::new("carol@example.org")];
    out.message_id = Some("reply-1@example.org".into());
    out.in_reply_to = Some("inv-1@example.org".into());
    out.date = Some(1_790_856_000);
    out
}

#[test]
fn builds_mime_that_parses_back() {
    let raw = outgoing(true).to_mime();
    let message = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(message.subject(), Some("Accepted: Planning"));
    assert_eq!(message.message_id(), Some("reply-1@example.org"));
    assert_eq!(message.in_reply_to().as_text(), Some("inv-1@example.org"));
    assert_eq!(
        message
            .from()
            .and_then(|a| a.first())
            .and_then(|a| a.name()),
        Some("Bob Builder")
    );
    assert_eq!(
        message
            .cc()
            .and_then(|a| a.first())
            .and_then(|a| a.address()),
        Some("carol@example.org")
    );
    let text = String::from_utf8_lossy(&raw);
    assert!(text.contains("Content-Class: urn:content-classes:calendarmessage"));
    let root = message.root_part().content_type().unwrap();
    assert_eq!(root.subtype(), Some("mixed"));
    let types: Vec<String> = message
        .parts
        .iter()
        .filter_map(|p| p.content_type())
        .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")))
        .collect();
    assert_eq!(
        types,
        [
            "multipart/mixed",
            "multipart/alternative",
            "text/plain",
            "text/html",
            "text/calendar",
            "application/ics"
        ]
    );
    let calendar = message
        .parts
        .iter()
        .find(|p| {
            p.content_type()
                .is_some_and(|ct| ct.subtype() == Some("calendar"))
        })
        .unwrap();
    let ct = calendar.content_type().unwrap();
    assert_eq!(ct.attribute("method"), Some("REPLY"));
    assert_eq!(ct.attribute("charset"), Some("utf-8"));
    assert!(
        calendar
            .text_contents()
            .unwrap()
            .contains("METHOD:REPLY\r\n")
    );
    // mail-parser lists the alternative's calendar part as an attachment
    // too; the .ics file is the named one.
    assert!(
        message
            .attachments()
            .any(|a| a.attachment_name() == Some("invite.ics"))
    );

    // Our own mail is found again, by the inline part.
    let found = find_itip(&raw).unwrap();
    assert_eq!(found.method_param.as_deref(), Some("REPLY"));
    assert!(!found.attachment);
    assert_eq!(found.parse().method, Some(ItipMethod::Reply));
}

#[test]
fn without_attachment_the_top_level_is_alternative() {
    let raw = outgoing(false).to_mime();
    let message = MessageParser::default().parse(&raw).unwrap();
    assert_eq!(
        message.root_part().content_type().unwrap().subtype(),
        Some("alternative")
    );
    assert!(
        !message
            .attachments()
            .any(|a| a.attachment_name() == Some("invite.ics"))
    );
    assert!(find_itip(&raw).is_some());
    let too_big = vec![b'x'; MAX_MESSAGE_BYTES + 1];
    assert!(find_itip(&too_big).is_none());
}
