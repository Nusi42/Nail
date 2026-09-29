//! MIME parsing of raw RFC 5322 messages into `nail_api` summaries and
//! bodies, with HTML sanitizing.
//!
//! [`parse_message`] never fails and never panics: malformed input yields a
//! best-effort [`ParsedMessage`] (in the worst case an empty one). From it,
//! [`ParsedMessage::to_summary`] builds a message-list row and
//! [`ParsedMessage::into_body`] the reader view. Parsing is done with
//! `mail-parser` (all common charsets, including legacy 8-bit and CJK ones)
//! and HTML is cleaned with [`sanitize_html`].
//!
//! ```
//! let raw = b"From: Ada <ada@example.org>\r\nSubject: Hi\r\n\r\nHello Sam";
//! let parsed = nail_mime::parse_message(raw);
//! assert_eq!(parsed.headers.subject, "Hi");
//! assert_eq!(parsed.preview, "Hello Sam");
//! ```

mod sanitize;
mod text;

use std::panic::{AssertUnwindSafe, catch_unwind};

use mail_parser::decoders::html::html_to_text;
use mail_parser::{
    Address as MpAddress, HeaderValue, Message, MessageParser, MessagePart, MimeHeaders, PartType,
};
use nail_api::{
    AccountId, Address, AttachmentInfo, Flags, FolderId, MessageBody, MessageHeaders, MessageId,
    MessageSummary,
};

pub use sanitize::{SanitizedHtml, sanitize_html};
pub use text::{PREVIEW_CHARS, preview_text};

/// Date used for [`MessageSummary::date`] when neither the `Date` header nor
/// a fallback date is available.
pub const UNKNOWN_DATE: &str = "1970-01-01T00:00:00Z";

/// Everything Nail needs from one raw message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedMessage {
    /// Display headers (decoded).
    pub headers: MessageHeaders,
    /// `In-Reply-To` message ids (without angle brackets).
    pub in_reply_to: Vec<String>,
    /// `References` message ids (without angle brackets), oldest first.
    pub references: Vec<String>,
    /// Plain-text body (derived from HTML when there is no text part).
    pub text: String,
    /// Sanitized HTML, `None` for text-only mail.
    pub sanitized_html: Option<String>,
    /// Whether the sanitizer removed remote content.
    pub remote_content_blocked: bool,
    /// Attachments (including inline parts) in MIME order.
    pub attachments: Vec<AttachmentInfo>,
    /// List preview (see [`preview_text`]).
    pub preview: String,
}

impl ParsedMessage {
    /// Whether the message has at least one non-inline attachment.
    pub fn has_attachments(&self) -> bool {
        self.attachments.iter().any(|a| !a.inline)
    }

    /// Builds a message-list row. `fallback_date` (RFC 3339, typically the
    /// server's internal date) is used when the `Date` header is missing or
    /// invalid; without either, [`UNKNOWN_DATE`] is used.
    pub fn to_summary(
        &self,
        ids: SummaryIds,
        flags: Flags,
        fallback_date: Option<&str>,
    ) -> MessageSummary {
        MessageSummary {
            id: ids.id,
            folder_id: ids.folder_id,
            account_id: ids.account_id,
            subject: self.headers.subject.clone(),
            from: self.headers.from.first().cloned(),
            to: self.headers.to.clone(),
            date: self
                .headers
                .date
                .clone()
                .or_else(|| fallback_date.map(str::to_owned))
                .unwrap_or_else(|| UNKNOWN_DATE.to_owned()),
            flags,
            has_attachments: self.has_attachments(),
            preview: self.preview.clone(),
        }
    }

    /// Builds the reader view.
    pub fn into_body(self, id: MessageId) -> MessageBody {
        MessageBody {
            id,
            headers: self.headers,
            text: self.text,
            sanitized_html: self.sanitized_html,
            attachments: self.attachments,
            remote_content_blocked: self.remote_content_blocked,
        }
    }
}

/// Identifiers for [`ParsedMessage::to_summary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryIds {
    /// Message id.
    pub id: MessageId,
    /// Folder id.
    pub folder_id: FolderId,
    /// Account id.
    pub account_id: AccountId,
}

/// The decoded content of one attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentContent {
    /// Attachment metadata.
    pub info: AttachmentInfo,
    /// Decoded bytes. Text parts are returned as UTF-8 (converted from their
    /// charset); `message/rfc822` parts as the raw nested message.
    pub data: Vec<u8>,
}

/// Parses a raw RFC 5322 message. Never panics; malformed input yields a
/// best-effort result.
pub fn parse_message(raw: &[u8]) -> ParsedMessage {
    guarded(|| {
        MessageParser::default()
            .parse(raw)
            .map(|message| build(&message))
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// Returns the decoded content of attachment `index` (as numbered in
/// [`ParsedMessage::attachments`]).
pub fn attachment_content(raw: &[u8], index: u32) -> Option<AttachmentContent> {
    guarded(|| {
        let message = MessageParser::default().parse(raw)?;
        let part = message.attachment(index)?;
        Some(AttachmentContent {
            info: attachment_info(index, part),
            data: part.contents().to_vec(),
        })
    })
    .flatten()
}

/// Finds an inline part by `Content-ID` (with or without `cid:` prefix and
/// angle brackets), e.g. to serve `<img src="cid:...">`.
pub fn inline_content_by_cid(raw: &[u8], cid: &str) -> Option<AttachmentContent> {
    let wanted = normalize_cid(cid);
    guarded(|| {
        let message = MessageParser::default().parse(raw)?;
        let (index, part) = message.attachments().enumerate().find(|(_, part)| {
            part.content_id()
                .is_some_and(|id| normalize_cid(id) == wanted)
        })?;
        Some(AttachmentContent {
            info: attachment_info(u32::try_from(index).ok()?, part),
            data: part.contents().to_vec(),
        })
    })
    .flatten()
}

fn normalize_cid(cid: &str) -> String {
    let cid = cid.trim();
    let cid = cid.strip_prefix("cid:").unwrap_or(cid);
    cid.trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// Runs `f`, turning a panic in a third-party parser into `None`.
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::warn!("MIME parser panicked; returning an empty result");
            None
        }
    }
}

fn build(message: &Message<'_>) -> ParsedMessage {
    let headers = MessageHeaders {
        subject: message
            .subject()
            .map(|s| s.trim().to_owned())
            .unwrap_or_default(),
        from: addresses(message.from()),
        to: message.all_to().flat_map(|a| addresses(Some(a))).collect(),
        cc: message.all_cc().flat_map(|a| addresses(Some(a))).collect(),
        reply_to: addresses(message.reply_to()),
        date: message
            .date()
            .filter(|d| d.is_valid())
            .map(|d| d.to_rfc3339()),
        message_id: message.message_id().map(str::to_owned),
    };

    let text = body_text(message);
    let html = body_html(message).map(|html| sanitize_html(&html));
    let preview = preview_text(&text, PREVIEW_CHARS);
    let attachments = message
        .attachments()
        .enumerate()
        .map(|(index, part)| attachment_info(u32::try_from(index).unwrap_or(u32::MAX), part))
        .collect();

    ParsedMessage {
        headers,
        in_reply_to: id_list(message.in_reply_to()),
        references: id_list(message.references()),
        text,
        remote_content_blocked: html.as_ref().is_some_and(|h| h.remote_content_blocked),
        sanitized_html: html.map(|h| h.html),
        attachments,
        preview,
    }
}

fn addresses(address: Option<&MpAddress<'_>>) -> Vec<Address> {
    let Some(address) = address else {
        return Vec::new();
    };
    address
        .iter()
        .filter_map(|addr| {
            let email = addr.address()?.trim();
            if email.is_empty() {
                return None;
            }
            let name = addr
                .name()
                .map(str::trim)
                .filter(|n| !n.is_empty() && *n != email);
            Some(Address {
                name: name.map(str::to_owned),
                email: email.to_owned(),
            })
        })
        .collect()
}

fn id_list(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(id) => vec![id.to_string()],
        HeaderValue::TextList(ids) => ids.iter().map(|id| id.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Joins all text body parts. HTML parts (HTML-only mail) are converted to
/// text *after* sanitizing, so removed content (forms, scripts, frames) does
/// not reappear in the text view or preview.
fn body_text(message: &Message<'_>) -> String {
    let parts: Vec<String> = message
        .text_bodies()
        .filter_map(|part| match &part.body {
            PartType::Text(text) => Some(text.to_string()),
            PartType::Html(html) => Some(html_to_text(&sanitize_html(html).html)),
            _ => None,
        })
        .map(|text| text.trim_end().to_owned())
        .filter(|text| !text.is_empty())
        .collect();
    parts.join("\n\n")
}

/// The raw HTML to sanitize, or `None` when the message has no HTML part.
/// Text parts that sit between HTML parts (in `multipart/mixed`) are
/// converted to HTML so the reader shows them in order.
fn body_html(message: &Message<'_>) -> Option<String> {
    if !message
        .html_bodies()
        .any(|part| matches!(part.body, PartType::Html(_)))
    {
        return None;
    }
    let html: Vec<String> = (0..message.html_body_count())
        .filter_map(|i| message.body_html(i).map(|h| h.into_owned()))
        .collect();
    Some(html.join("\n"))
}

fn attachment_info(index: u32, part: &MessagePart<'_>) -> AttachmentInfo {
    let mime = part
        .content_type()
        .map(|ct| match &ct.c_subtype {
            Some(sub) => format!("{}/{}", ct.c_type, sub),
            None => ct.c_type.to_string(),
        })
        .map(|m| m.to_ascii_lowercase())
        .unwrap_or_else(|| {
            match part.body {
                PartType::Text(_) => "text/plain",
                PartType::Html(_) => "text/html",
                PartType::Message(_) => "message/rfc822",
                _ => "application/octet-stream",
            }
            .to_owned()
        });
    let disposition = part
        .content_disposition()
        .map(|d| d.c_type.to_ascii_lowercase());
    let inline = match disposition.as_deref() {
        Some("inline") => part.content_id().is_some() || mime.starts_with("image/"),
        Some(_) => false,
        None => part.content_id().is_some(),
    };
    AttachmentInfo {
        index,
        filename: part
            .attachment_name()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty()),
        mime,
        size: part.len() as u64,
        inline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_garbage_input_do_not_panic() {
        for raw in [
            &b""[..],
            b"\0\0\0",
            b"\r\n\r\n",
            b"Subject",
            b"Content-Type: multipart/mixed; boundary=\"\"\r\n\r\n--\r\n--",
        ] {
            let parsed = parse_message(raw);
            let _ = parsed.to_summary(ids(), Flags::default(), None);
        }
    }

    #[test]
    fn summary_uses_fallback_date() {
        let parsed = parse_message(b"Subject: x\r\nDate: nonsense\r\n\r\nbody");
        assert_eq!(parsed.headers.date, None);
        let summary = parsed.to_summary(ids(), Flags::default(), Some("2026-01-02T03:04:05Z"));
        assert_eq!(summary.date, "2026-01-02T03:04:05Z");
        assert_eq!(
            parsed.to_summary(ids(), Flags::default(), None).date,
            UNKNOWN_DATE
        );
    }

    #[test]
    fn text_only_mail_has_no_html() {
        let parsed = parse_message(b"From: a@b.example\r\nSubject: t\r\n\r\nJust <b>text</b>\r\n");
        assert_eq!(parsed.sanitized_html, None);
        assert_eq!(parsed.text, "Just <b>text</b>");
        assert!(!parsed.remote_content_blocked);
    }

    #[test]
    fn cid_normalization() {
        assert_eq!(normalize_cid("cid:abc@x"), "abc@x");
        assert_eq!(normalize_cid("<abc@x>"), "abc@x");
        assert_eq!(normalize_cid(" abc@x "), "abc@x");
    }

    fn ids() -> SummaryIds {
        SummaryIds {
            id: "m".into(),
            folder_id: "f".into(),
            account_id: "a".into(),
        }
    }
}
