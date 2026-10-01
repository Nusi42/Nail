//! iMIP (RFC 6047): iTIP over email.
//!
//! **Receiving** — [`find_itip`] picks the calendar data of a received
//! message: a `text/calendar` part (with `method=…` preferred, inline
//! before attached), else an `.ics` attachment (`application/ics`,
//! `application/octet-stream; name=*.ics`). Parts of attached messages
//! (`message/rfc822`) are not looked at: a forwarded mail is not an
//! invitation. The `METHOD` property wins over the `method` parameter
//! ([`crate::object::parse_itip`]).
//!
//! **Sending** — [`OutgoingImip::to_mime`] writes
//!
//! ```text
//! multipart/mixed
//! ├── multipart/alternative
//! │   ├── text/plain            (readable summary, crate::texts)
//! │   ├── text/html             (the same)
//! │   └── text/calendar; method=REQUEST; charset=utf-8
//! └── application/ics; name=invite.ics   (attachment, optional)
//! ```
//!
//! The `text/calendar` part inside `multipart/alternative` is what RFC
//! 6047 §2.4 and Outlook / Google / Apple expect; the `.ics` attachment
//! (Google does the same) helps clients that only show attachments.
//! Without the attachment the top level is the `multipart/alternative`.

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as MailBuilderAddress;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::raw::Raw;
use mail_builder::mime::MimePart;
use mail_parser::{MessageParser, MimeHeaders};
use nail_api::scheduling::ItipMethod;

use crate::address::normalize;
use crate::object::{ItipObject, parse_itip};

/// Largest message searched for calendar data.
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

/// The calendar data found in a received message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedImip {
    /// The iCalendar text.
    pub text: String,
    /// The `method` parameter of the part, if any.
    pub method_param: Option<String>,
    /// It came from an attachment (not an inline `text/calendar` part).
    pub attachment: bool,
    /// `From` address.
    pub from: Option<String>,
    /// `Sender` address.
    pub sender: Option<String>,
    /// `Message-ID` (without angle brackets).
    pub message_id: Option<String>,
    /// `Subject`.
    pub subject: Option<String>,
}

impl ReceivedImip {
    /// The sender addresses to check against the organizer / attendee
    /// (`From`, then `Sender`), normalized.
    pub fn senders(&self) -> Vec<String> {
        let mut out = Vec::new();
        for a in [&self.from, &self.sender].into_iter().flatten() {
            let a = normalize(a);
            if !a.is_empty() && !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// The iTIP object.
    pub fn parse(&self) -> ItipObject {
        parse_itip(&self.text, self.method_param.as_deref())
    }
}

fn has_ics_name(part: &mail_parser::MessagePart<'_>) -> bool {
    part.attachment_name()
        .or_else(|| part.content_type().and_then(|ct| ct.attribute("name")))
        .is_some_and(|n| n.trim().to_ascii_lowercase().ends_with(".ics"))
}

/// Finds the calendar data of a received message (raw RFC 5322 bytes).
pub fn find_itip(raw: &[u8]) -> Option<ReceivedImip> {
    if raw.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    let message = MessageParser::default().parse(raw)?;
    // (score, part index): higher is better.
    let mut best: Option<(u8, usize)> = None;
    for (index, part) in message.parts.iter().enumerate() {
        let Some(ct) = part.content_type() else {
            continue;
        };
        let ctype = ct.ctype().to_ascii_lowercase();
        let subtype = ct.subtype().map(str::to_ascii_lowercase);
        let attached = part
            .content_disposition()
            .is_some_and(|d| d.is_attachment());
        let score = match (ctype.as_str(), subtype.as_deref()) {
            ("text", Some("calendar")) => {
                let method = ct.attribute("method").is_some();
                match (method, attached) {
                    (true, false) => 6,
                    (true, true) => 5,
                    (false, false) => 4,
                    (false, true) => 3,
                }
            }
            ("application", Some("ics")) => 2,
            ("application", Some("octet-stream")) if has_ics_name(part) => 1,
            _ => continue,
        };
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, index));
        }
    }
    let (score, index) = best?;
    let part = &message.parts[index];
    let ct = part.content_type();
    let text = match part.text_contents() {
        Some(text) if part.is_text() => text.to_owned(),
        _ => nail_ical::content::decode_charset(
            part.contents(),
            ct.and_then(|c| c.attribute("charset")),
        ),
    };
    if !text.to_ascii_uppercase().contains("BEGIN:VCALENDAR") {
        return None;
    }
    let first = |a: Option<&mail_parser::Address<'_>>| {
        a.and_then(|a| a.first())
            .and_then(|a| a.address())
            .map(str::to_owned)
    };
    Some(ReceivedImip {
        text,
        method_param: ct
            .and_then(|c| c.attribute("method"))
            .map(|m| m.trim().to_owned()),
        attachment: matches!(score, 1 | 2 | 3 | 5),
        from: first(message.from()),
        sender: first(message.sender()),
        message_id: message.message_id().map(str::to_owned),
        subject: message.subject().map(str::to_owned),
    })
}

/// A mail address with an optional display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailAddress {
    /// Display name.
    pub name: Option<String>,
    /// Address.
    pub email: String,
}

impl MailAddress {
    /// An address without a name.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            name: None,
            email: email.into(),
        }
    }

    fn to_builder(&self) -> MailBuilderAddress<'static> {
        match self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(name) => {
                MailBuilderAddress::new_address(Some(name.to_owned()), self.email.clone())
            }
            None => MailBuilderAddress::new_address(None::<String>, self.email.clone()),
        }
    }
}

/// An iMIP mail to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingImip {
    /// The method (the `method` parameter; must match the data's `METHOD`).
    pub method: ItipMethod,
    /// Sender (the organizer or answering attendee's identity).
    pub from: MailAddress,
    /// Recipients.
    pub to: Vec<MailAddress>,
    /// Copy recipients.
    pub cc: Vec<MailAddress>,
    /// Subject.
    pub subject: String,
    /// Plain text summary.
    pub text: String,
    /// HTML summary.
    pub html: String,
    /// The iTIP calendar text (from [`crate::build`]).
    pub calendar: String,
    /// Also attach the calendar as `invite.ics`.
    pub attach_ics: bool,
    /// `Message-ID` (without brackets); `None` = generated.
    pub message_id: Option<String>,
    /// `In-Reply-To` / `References`: the invitation answered.
    pub in_reply_to: Option<String>,
    /// `Date` (Unix seconds); `None` = now.
    pub date: Option<i64>,
}

impl OutgoingImip {
    /// A mail with the texts of [`crate::texts::compose`].
    pub fn new(
        method: ItipMethod,
        from: MailAddress,
        to: Vec<MailAddress>,
        texts: crate::texts::MailText,
        calendar: String,
    ) -> Self {
        Self {
            method,
            from,
            to,
            cc: Vec::new(),
            subject: texts.subject,
            text: texts.text,
            html: texts.html,
            calendar,
            attach_ics: true,
            message_id: None,
            in_reply_to: None,
            date: None,
        }
    }

    /// The `text/calendar` part.
    fn calendar_part(&self) -> MimePart<'static> {
        MimePart::new(
            ContentType::new("text/calendar")
                .attribute("method", self.method.as_ical())
                .attribute("charset", "utf-8"),
            self.calendar.clone(),
        )
    }

    /// The MIME body (no headers), for transports that build the message
    /// themselves.
    pub fn body(&self) -> MimePart<'static> {
        let alternative = MimePart::new(
            "multipart/alternative",
            vec![
                MimePart::new(
                    ContentType::new("text/plain").attribute("charset", "utf-8"),
                    self.text.clone(),
                ),
                MimePart::new(
                    ContentType::new("text/html").attribute("charset", "utf-8"),
                    self.html.clone(),
                ),
                self.calendar_part(),
            ],
        );
        if !self.attach_ics {
            return alternative;
        }
        let attachment = MimePart::new(
            ContentType::new("application/ics").attribute("name", "invite.ics"),
            self.calendar.clone().into_bytes(),
        )
        .attachment("invite.ics");
        MimePart::new("multipart/mixed", vec![alternative, attachment])
    }

    /// The whole message (RFC 5322).
    pub fn to_mime(&self) -> Vec<u8> {
        let mut builder = MessageBuilder::new()
            .from(self.from.to_builder())
            .to(MailBuilderAddress::new_list(
                self.to.iter().map(MailAddress::to_builder).collect(),
            ))
            .subject(self.subject.clone())
            // Outlook files mail with this class as a meeting message.
            .header("Content-Class", Raw::new("urn:content-classes:calendarmessage"));
        if !self.cc.is_empty() {
            builder = builder.cc(MailBuilderAddress::new_list(
                self.cc.iter().map(MailAddress::to_builder).collect(),
            ));
        }
        if let Some(id) = &self.message_id {
            builder = builder.message_id(id.clone());
        }
        if let Some(id) = &self.in_reply_to {
            builder = builder.in_reply_to(id.clone()).references(id.clone());
        }
        if let Some(date) = self.date {
            builder = builder.date(date);
        }
        builder.body(self.body()).write_to_vec().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
