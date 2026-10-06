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
mod css;
mod document;
mod domain;
pub mod links;
mod markdown;
pub mod message_file;
pub mod remote_content;
mod sanitize;
mod text;

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use mail_parser::decoders::html::html_to_text;
use mail_parser::{
    Address as MpAddress, HeaderValue, Message, MessageParser, MessagePart, MimeHeaders, PartType,
};
use nail_api::{
    AccountId, Address, AttachmentInfo, Flags, FolderId, MessageBody, MessageHeaders, MessageId,
    MessageSummary,
};

pub use message_file::{
    MAX_MESSAGE_FILE, MessageFile, MessageFileError, check_message_bytes, read_message_file,
};
pub use remote_content::ProxyRewriter;
use sanitize::normalize_cid;
pub use sanitize::{
    InlineImages, SanitizedHtml, sanitize_html, sanitize_html_with, sanitize_mail_html,
    sanitize_mail_html_portable,
};
pub use text::{PREVIEW_CHARS, preview_text};

/// The largest inline (`cid:`) image the reader shows, in decoded bytes;
/// larger ones stay attachments and the reader shows a placeholder.
pub const MAX_INLINE_IMAGE_BYTES: usize = 2 * 1024 * 1024;

/// The most inline image bytes resolved for one message.
pub const MAX_INLINE_IMAGES_BYTES: usize = 8 * 1024 * 1024;

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
    /// Sanitized HTML, `None` for text-only mail. Inline images stay
    /// `cid:` references (see `reader_html`).
    pub sanitized_html: Option<String>,
    /// The HTML the reader renders when it differs from `sanitized_html`:
    /// inline images embedded as `data:` URLs (P1). `None` = render
    /// `sanitized_html`.
    pub reader_html: Option<String>,
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
    /// Whether the message has at least one attachment the reader lists:
    /// one its HTML body does not show (ADR 0162).
    pub fn has_attachments(&self) -> bool {
        self.attachments.iter().any(|a| !a.shown_in_body)
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
            headers_only: false,
            gmail: None,
            exchange: None,
        }
    }

    /// Builds the reader view.
    pub fn into_body(self, id: MessageId) -> MessageBody {
        MessageBody {
            id,
            headers: self.headers,
            text: self.text,
            sanitized_html: self.sanitized_html,
            reader_html: self.reader_html,
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
            junk: None,
            invitation: None,
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
                let value = if let Some(text) = h.value.as_text() {
                    text.to_owned()
                } else {
                    let start = usize::try_from(h.offset_start).unwrap_or(0);
                    let end = usize::try_from(h.offset_end).unwrap_or(0);
                    let raw = bytes.get(start..end.max(start)).unwrap_or_default();
                    unfold(&String::from_utf8_lossy(raw))
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
            info: attachment_info(index, part, &shown_for(&message, part)),
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
            info: attachment_info(u32::try_from(index).ok()?, part, &shown_for(&message, part)),
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

    #[expect(
        clippy::expect_used,
        reason = "the message is Some from construction until `Drop` takes it"
    )]
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

/// Runs `f`, turning a panic in a third-party parser into `None`.
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    if let Ok(value) = catch_unwind(AssertUnwindSafe(f)) {
        Some(value)
    } else {
        tracing::warn!("MIME parser panicked; returning an empty result");
        None
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
            .map(mail_parser::DateTime::to_rfc3339),
        message_id: message.message_id().map(str::to_owned),
    };

    // `text/markdown` alternatives are bodies, not attachments (M2c).
    let markdown = markdown::alternatives(message);
    let markdown_source = markdown_source(&markdown);
    let has_plain = message
        .text_bodies()
        .any(|part| matches!(part.body, PartType::Text(_)));
    let text = match &markdown_source {
        Some(source) if !has_plain => source.clone(),
        _ => body_text(message),
    };
    // The reader gets inline images as `data:` URLs; everything else
    // (quoting, MCP, plug-ins) keeps the small `cid:` references.
    let (html, reader_html) =
        html_source(message, markdown_source.as_deref()).map_or((None, None), |html| {
            let inline = inline_images(message, &html);
            let reader = (!inline.is_empty())
                .then(|| sanitize_mail_html(&html, rewriter.clone(), inline.clone()).html);
            (
                Some(sanitize_mail_html_portable(&html, rewriter, inline)),
                reader,
            )
        });
    let preview = preview_text(&text, PREVIEW_CHARS);
    let shown = html
        .as_ref()
        .map(|h| h.inline_shown.clone())
        .unwrap_or_default();
    let attachments = message
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, id)| !markdown.iter().any(|(md, _)| md == *id))
        .filter_map(|(index, id)| {
            let part = message.parts.get(usize::try_from(*id).ok()?)?;
            Some(attachment_info(
                u32::try_from(index).unwrap_or(u32::MAX),
                part,
                &shown,
            ))
        })
        .collect();

    ParsedMessage {
        headers,
        in_reply_to: id_list(message.in_reply_to()),
        references: id_list(message.references()),
        text,
        remote_content_blocked: html.as_ref().is_some_and(|h| h.remote_content_blocked),
        remote_resources: html.as_ref().map_or(0, |h| h.remote_resources),
        sanitized_html: html.map(|h| h.html),
        reader_html,
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
        HeaderValue::TextList(ids) => ids.iter().map(ToString::to_string).collect(),
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
            PartType::Html(html) => Some(html_text(&sanitize_html(html).html)),
            _ => None,
        })
        .map(|text| text.trim_end().to_owned())
        .filter(|text| !text.is_empty())
        .collect();
    parts.join("\n\n")
}

/// Block elements whose end starts a new line in [`html_text`].
const BLOCK_ENDS: &[&str] = &[
    "</div>",
    "</h1>",
    "</h2>",
    "</h3>",
    "</h4>",
    "</h5>",
    "</h6>",
    "</li>",
    "</tr>",
    "</table>",
    "</blockquote>",
    "</section>",
    "</article>",
    "</header>",
    "</footer>",
    "</main>",
    "</ul>",
    "</ol>",
    "</pre>",
    "</center>",
    "</dt>",
    "</dd>",
];

/// The text of sanitized HTML: blocks on lines of their own (mail-parser
/// breaks only at `<br>` and `</p>`), table cells apart, at most one blank
/// line in a row.
fn html_text(sanitized: &str) -> String {
    let mut marked = String::with_capacity(sanitized.len() + 64);
    let mut rest = sanitized;
    while let Some(start) = rest.find("</") {
        marked.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find('>').map_or(rest.len(), |i| i + 1);
        let tag = &rest[..end];
        marked.push_str(tag);
        if BLOCK_ENDS.contains(&tag) {
            marked.push_str("<br>");
        } else if tag == "</td>" || tag == "</th>" {
            marked.push(' ');
        }
        rest = &rest[end..];
    }
    marked.push_str(rest);
    let text = html_to_text(&marked);
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line.trim_start_matches(' '));
        out.push('\n');
    }
    out.trim_end().to_owned()
}

/// The `text/markdown` alternatives joined, `None` when there are none.
fn markdown_source(markdown: &[(u32, String)]) -> Option<String> {
    (!markdown.is_empty()).then(|| {
        markdown
            .iter()
            .map(|(_, source)| source.trim_end())
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

/// The raw HTML body: the HTML parts, else the Markdown alternatives as
/// HTML; `None` for text-only mail.
fn html_source(message: &Message<'_>, markdown_source: Option<&str>) -> Option<String> {
    body_html(message).or_else(|| markdown_source.map(markdown::to_html))
}

/// The normalized Content-IDs of the inline images the message's HTML body
/// shows (as [`build`] finds them), computed only when `part` has a
/// Content-ID at all: for [`attachment_content`] / [`inline_content_by_cid`].
fn shown_for(message: &Message<'_>, part: &MessagePart<'_>) -> BTreeSet<String> {
    if part.content_id().is_none() {
        return BTreeSet::new();
    }
    let markdown_source = markdown_source(&markdown::alternatives(message));
    html_source(message, markdown_source.as_deref()).map_or_else(BTreeSet::new, |html| {
        let inline = inline_images(message, &html);
        if inline.is_empty() {
            return BTreeSet::new();
        }
        sanitize_mail_html_portable(&html, None, inline).inline_shown
    })
}

/// The `data:` URLs of the inline image parts `html` may refer to (by
/// Content-ID, ignoring case; the sanitizer checks the actual references),
/// up to [`MAX_INLINE_IMAGE_BYTES`] each and [`MAX_INLINE_IMAGES_BYTES`]
/// together. The image type comes from the bytes, never from the part's
/// header (no SVG: it can carry script).
fn inline_images(message: &Message<'_>, html: &str) -> InlineImages {
    use base64::Engine as _;
    let mut images = InlineImages::default();
    let lower = html.to_lowercase();
    if !lower.contains("cid:") {
        return images;
    }
    let mut total = 0usize;
    for part in message.attachments() {
        let Some(cid) = part.content_id().map(normalize_cid) else {
            continue;
        };
        // A `cid:` URL may percent-encode the `@` (RFC 2392).
        let mentioned = lower.contains(cid.as_str())
            || (cid.contains('@') && lower.contains(&cid.replace('@', "%40")));
        if cid.is_empty() || images.get(&cid).is_some() || !mentioned {
            continue;
        }
        let data = part.contents();
        if data.len() > MAX_INLINE_IMAGE_BYTES || total + data.len() > MAX_INLINE_IMAGES_BYTES {
            continue;
        }
        let Some(mime) = image_type(data) else {
            continue;
        };
        total += data.len();
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        images.insert(&cid, format!("data:{mime};base64,{encoded}"));
    }
    images
}

/// The type of a PNG, GIF, JPEG or WebP image from its first bytes.
fn image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
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
        .filter_map(|i| message.body_html(i).map(std::borrow::Cow::into_owned))
        .collect();
    Some(html.join("\n"))
}

/// The metadata of attachment `index`; `shown` = the normalized
/// Content-IDs of the inline images the HTML body shows.
fn attachment_info(index: u32, part: &MessagePart<'_>, shown: &BTreeSet<String>) -> AttachmentInfo {
    let mime = part
        .content_type()
        .map(|ct| match &ct.c_subtype {
            Some(sub) => format!("{}/{}", ct.c_type, sub),
            None => ct.c_type.to_string(),
        })
        .map_or_else(
            || {
                match part.body {
                    PartType::Text(_) => "text/plain",
                    PartType::Html(_) => "text/html",
                    PartType::Message(_) => "message/rfc822",
                    _ => "application/octet-stream",
                }
                .to_owned()
            },
            |m| m.to_ascii_lowercase(),
        );
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
        shown_in_body: part
            .content_id()
            .is_some_and(|cid| shown.contains(&normalize_cid(cid))),
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
        assert_eq!(header_fields(b"").len(), 0);
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
            drop(parsed.to_summary(ids(), Flags::default(), None));
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
