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

pub mod auth_results;
mod domain;
pub mod links;
pub mod remote_content;
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

pub use remote_content::ProxyRewriter;
pub use sanitize::{SanitizedHtml, sanitize_html, sanitize_html_with};
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
    /// Remote references in the HTML body (removed, or rewritten by the
    /// [`ProxyRewriter`] of [`parse_message_with`]).
    pub remote_resources: u32,
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
            thread_id: None,
            tags: Vec::new(),
            duplicates: Vec::new(),
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
            // Filled by the core (M2a): decryption / verification, trusted
            // Authentication-Results, link and attachment warnings.
            security: None,
            auth_results: None,
            warnings: Vec::new(),
            // Filled by the core (M2b): allow-lists and proxied URLs.
            remote_content: None,
            render_url: None,
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
    parse_message_with(raw, None)
}

/// [`parse_message`], with remote content in the HTML body replaced
/// through `rewriter` instead of removed (see [`remote_content`]).
pub fn parse_message_with(raw: &[u8], rewriter: Option<ProxyRewriter>) -> ParsedMessage {
    guarded(|| {
        parse(raw)
            .map(|message| build(&message, rewriter))
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

/// The top-level header fields in message order as `(name, value)`:
/// unstructured values decoded (encoded words, charsets), structured ones
/// (addresses, `List-Id`, `Authentication-Results`, …) as written, with
/// folding removed. For filter rules, which match on arbitrary headers.
pub fn header_fields(raw: &[u8]) -> Vec<(String, String)> {
    guarded(|| {
        let Some(message) = parse(raw) else {
            return Vec::new();
        };
        let bytes = message.raw_message();
        message
            .headers()
            .iter()
            .map(|h| {
                let value = match h.value.as_text() {
                    Some(text) => text.to_owned(),
                    None => {
                        let start = usize::try_from(h.offset_start).unwrap_or(0);
                        let end = usize::try_from(h.offset_end).unwrap_or(0);
                        let raw = bytes.get(start..end.max(start)).unwrap_or_default();
                        unfold(&String::from_utf8_lossy(raw))
                    }
                };
                (h.name.as_str().to_owned(), value)
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Joins folded header lines and trims the value.
fn unfold(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, line) in value.split('\n').enumerate() {
        let line = line.trim_end_matches('\r');
        if i > 0 {
            out.push(' ');
            out.push_str(line.trim_start());
        } else {
            out.push_str(line);
        }
    }
    out.trim().to_owned()
}

/// Returns the decoded content of attachment `index` (as numbered in
/// [`ParsedMessage::attachments`]).
pub fn attachment_content(raw: &[u8], index: u32) -> Option<AttachmentContent> {
    guarded(|| {
        let message = parse(raw)?;
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
        let message = parse(raw)?;
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

/// Parses `raw` with `mail-parser`. Always go through this function: the
/// result must be dropped by [`Parsed`], never directly.
fn parse(raw: &[u8]) -> Option<Parsed<'_>> {
    MessageParser::default().parse(raw).map(|m| Parsed(Some(m)))
}

/// A parsed message that is torn down iteratively.
///
/// Nested `message/rfc822` parts form a tree of [`Message`] values, and the
/// compiler-generated `Drop` recurses once per level. Hostile mail can nest
/// tens of thousands of levels in well under a megabyte, and a stack overflow
/// aborts the process (`catch_unwind` cannot catch it). This wrapper
/// detaches nested messages onto a heap stack before dropping them, so no
/// level is dropped recursively. mail-parser itself parses nested messages
/// iteratively.
struct Parsed<'x>(Option<Message<'x>>);

impl<'x> std::ops::Deref for Parsed<'x> {
    type Target = Message<'x>;

    fn deref(&self) -> &Message<'x> {
        // Only `Drop` takes the value out.
        self.0
            .as_ref()
            .expect("a Parsed message is present until dropped")
    }
}

impl Drop for Parsed<'_> {
    fn drop(&mut self) {
        let mut pending: Vec<Message<'_>> = self.0.take().into_iter().collect();
        while let Some(mut message) = pending.pop() {
            for part in &mut message.parts {
                if matches!(part.body, PartType::Message(_))
                    && let PartType::Message(nested) = std::mem::take(&mut part.body)
                {
                    pending.push(nested);
                }
            }
            // `message` now has no nested messages; dropping it is shallow.
        }
    }
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

fn build(message: &Message<'_>, rewriter: Option<ProxyRewriter>) -> ParsedMessage {
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
    let html = body_html(message).map(|html| sanitize_html_with(&html, rewriter));
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
        remote_resources: html.as_ref().map_or(0, |h| h.remote_resources),
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
    fn header_fields_decode_text_and_keep_structured_values() {
        let raw = b"From: Ada <ada@example.org>\r\nSubject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\nList-Id: Rust users\r\n <rust.lists.example>\r\nAuthentication-Results: mx.example; dkim=pass\r\nX-Custom: one\r\nX-Custom: two\r\n\r\nbody";
        let fields = header_fields(raw);
        let get = |name: &str| -> Vec<&str> {
            fields
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
                .collect()
        };
        assert_eq!(get("Subject"), ["Grüße"]);
        assert_eq!(get("List-Id"), ["Rust users <rust.lists.example>"]);
        assert_eq!(get("Authentication-Results"), ["mx.example; dkim=pass"]);
        assert_eq!(get("X-Custom"), ["one", "two"]);
        assert!(header_fields(b"").is_empty());
    }

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

    /// Regression: dropping a deep `message/rfc822` chain used to recurse per
    /// level and overflow the stack (aborting the process) at a few thousand
    /// levels. 100 000 levels are about 3.3 MB of input.
    #[test]
    fn deeply_nested_messages_do_not_overflow_the_stack() {
        const LEVELS: usize = 100_000;
        let mut raw = String::from("Subject: outer\r\n");
        for _ in 0..LEVELS {
            raw.push_str("Content-Type: message/rfc822\r\n\r\n");
        }
        raw.push_str("Subject: inner\r\n\r\nhello\r\n");
        // A small stack (1 MiB, the Windows main-thread default) on purpose.
        let worker = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let parsed = parse_message(raw.as_bytes());
                let attachment = attachment_content(raw.as_bytes(), 0);
                let by_cid = inline_content_by_cid(raw.as_bytes(), "cid:none");
                (parsed, attachment, by_cid)
            })
            .unwrap();
        let (parsed, attachment, by_cid) = worker.join().expect("the parser thread survived");
        assert_eq!(parsed.headers.subject, "outer");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].mime, "message/rfc822");
        assert!(attachment.is_some());
        assert!(by_cid.is_none());
    }

    fn ids() -> SummaryIds {
        SummaryIds {
            id: "m".into(),
            folder_id: "f".into(),
            account_id: "a".into(),
        }
    }
}
