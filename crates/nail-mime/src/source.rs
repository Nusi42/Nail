//! "Show source" (ADR 0194): a raw message as text, and decoded for
//! reading — the message's header fields (folded lines joined, encoded
//! words decoded), then each MIME part with its header fields and its content
//! without transfer encoding, in its charset's characters. Parts that are
//! not text are named, not shown.

use std::borrow::Cow;
use std::fmt::Write as _;

use mail_parser::{Message, MimeHeaders, PartType};
use nail_api::message_source::{MAX_SOURCE_NESTING, SourceChunk};

use mail_parser::Header;

use crate::{guarded, parse, part_filename, part_mime, unfold, written_value};

/// The message as received, as text, cut at `max` bytes:
/// `(text, truncated)`. Bytes that are not UTF-8 become U+FFFD; the line
/// endings stay as they are.
pub fn original_source(raw: &[u8], max: usize) -> (String, bool) {
    let mut end = raw.len().min(max);
    // Do not cut a UTF-8 sequence in two (at most three continuation bytes).
    let mut back = 0;
    while end < raw.len() && back < 3 && raw.get(end).is_some_and(|b| b & 0xC0 == 0x80) {
        end -= 1;
        back += 1;
    }
    let text = String::from_utf8_lossy(raw.get(..end).unwrap_or_default());
    let (text, cut) = cut_text(&text, max);
    (text.to_owned(), cut || end < raw.len())
}

/// The decoded view of a message, at most `max` bytes of text:
/// `(chunks, truncated)`. Line endings are `\n`. A message the parser does
/// not understand is shown as it is.
pub fn decoded_source(raw: &[u8], max: usize) -> (Vec<SourceChunk>, bool) {
    guarded(|| {
        let mut out = Chunks::new(max);
        match parse(raw) {
            Some(message) => write_part(&mut out, raw, &message, 0, 0),
            None => out.text(&String::from_utf8_lossy(raw)),
        }
        (out.list, out.truncated)
    })
    .unwrap_or_default()
}

/// Writes part `id` of `message` (`0` = the message itself) and its
/// children; `depth` counts the nested parts above it. `raw` is the whole
/// message: the offsets of an attached message's parts count from its
/// start too.
fn write_part(out: &mut Chunks, raw: &[u8], message: &Message<'_>, id: u32, depth: usize) {
    let Some(part) = message.part(id) else {
        return;
    };
    let mut header = String::new();
    for h in &part.headers {
        let _ = writeln!(header, "{}: {}", h.name.as_str(), decoded_value(h, raw));
    }
    header.push('\n');
    out.text(&header);
    let size = u64::from(part.offset_end.saturating_sub(part.offset_body));
    match &part.body {
        PartType::Text(text) | PartType::Html(text) => {
            out.text(&format!("{}\n", text.trim_end()));
        }
        PartType::Binary(data) | PartType::InlineBinary(data) => {
            out.omitted(part_mime(part), part_filename(part), data.len() as u64);
        }
        PartType::Message(_) | PartType::Multipart(_) if depth >= MAX_SOURCE_NESTING => {
            out.omitted(part_mime(part), part_filename(part), size);
        }
        PartType::Message(nested) => write_part(out, raw, nested, 0, depth + 1),
        PartType::Multipart(children) => {
            let boundary = part.content_type().and_then(|ct| ct.attribute("boundary"));
            for &child in children {
                if let Some(b) = boundary {
                    out.text(&format!("--{b}\n"));
                }
                write_part(out, raw, message, child, depth + 1);
            }
            if let Some(b) = boundary {
                out.text(&format!("--{b}--\n"));
            }
        }
    }
}

/// A header field's value for the decoded view: as written (unfolded),
/// unless it has encoded words (`=?charset?q?…?=`) the parser decoded to
/// text. IDs, addresses and parameters stay as written.
fn decoded_value(header: &Header<'_>, raw: &[u8]) -> String {
    let written = written_value(header, raw);
    match header.value.as_text() {
        Some(text) if written.contains("=?") => unfold(text),
        _ => written,
    }
}

/// `text` cut at `max` bytes at a character boundary: `(text, cut)`.
fn cut_text(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text.get(..end).unwrap_or_default(), true)
}

/// The chunks written so far, with the byte budget.
struct Chunks {
    list: Vec<SourceChunk>,
    left: usize,
    truncated: bool,
}

impl Chunks {
    fn new(max: usize) -> Self {
        Self {
            list: Vec::new(),
            left: max,
            truncated: false,
        }
    }

    /// Appends text (line endings made `\n`), merged with a text chunk
    /// before it.
    fn text(&mut self, text: &str) {
        if self.truncated {
            return;
        }
        let text: Cow<'_, str> = if text.contains('\r') {
            Cow::Owned(text.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(text)
        };
        let (text, cut) = cut_text(&text, self.left);
        self.left -= text.len();
        self.truncated = cut;
        if let Some(SourceChunk::Text { text: last }) = self.list.last_mut() {
            last.push_str(text);
        } else if !text.is_empty() {
            self.list.push(SourceChunk::Text {
                text: text.to_owned(),
            });
        }
    }

    fn omitted(&mut self, mime: String, filename: Option<String>, size: u64) {
        if !self.truncated {
            self.list.push(SourceChunk::Omitted {
                mime,
                filename,
                size,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &[u8] = b"From: Ada <ada@example.org>\r\n\
Subject: Garden digest\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
PHA+R3LDvMOfZTwvcD4=\r\n\
--b1\r\n\
Content-Type: image/png; name=\"leaf.png\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--b1--\r\n";

    fn text_of(chunks: &[SourceChunk]) -> String {
        chunks
            .iter()
            .map(|c| match c {
                SourceChunk::Text { text } => text.clone(),
                SourceChunk::Omitted { mime, .. } => format!("<{mime}>\n"),
            })
            .collect()
    }

    #[test]
    fn decoded_shows_headers_boundaries_and_text_parts_decoded() {
        let (chunks, truncated) = decoded_source(MIXED, 1 << 20);
        assert!(!truncated);
        let text = text_of(&chunks);
        assert!(text.starts_with("From: Ada <ada@example.org>\nSubject: Garden digest\n"));
        assert!(text.contains("--b1\nContent-Type: text/html; charset=utf-8\n"));
        assert!(text.contains("<p>Grüße</p>\n"), "{text}");
        assert!(text.contains("<image/png>\n"));
        assert!(text.ends_with("--b1--\n"));
        assert!(!text.contains('\r'));
        assert_eq!(
            chunks.get(1),
            Some(&SourceChunk::Omitted {
                mime: "image/png".into(),
                filename: Some("leaf.png".into()),
                size: 8,
            })
        );
    }

    #[test]
    fn decoded_follows_an_attached_message_and_decodes_header_fields() {
        let raw = b"Subject: =?utf-8?q?Fwd:_K=C3=A4se?=\r\n\
Message-ID: <fwd@example.org>\r\n\
X-Long: one\r\n two\r\n\
Content-Type: message/rfc822\r\n\r\n\
Subject: Inner\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\r\nK=E4se\r\n";
        let text = text_of(&decoded_source(raw, 1 << 20).0);
        assert!(
            text.starts_with(
                "Subject: Fwd: Käse\nMessage-ID: <fwd@example.org>\nX-Long: one two\n"
            ),
            "{text:?}"
        );
        assert!(text.contains("Subject: Inner\n"), "{text:?}");
        assert!(text.contains("Käse\n"), "{text}");
    }

    #[test]
    fn deep_nesting_is_left_out_not_followed() {
        let mut raw = Vec::new();
        for _ in 0..200 {
            raw.extend_from_slice(b"Content-Type: message/rfc822\r\n\r\n");
        }
        raw.extend_from_slice(b"Subject: deep\r\n\r\nbottom\r\n");
        let (chunks, _) = decoded_source(&raw, 1 << 20);
        assert!(
            chunks.iter().any(
                |c| matches!(c, SourceChunk::Omitted { mime, .. } if mime == "message/rfc822")
            )
        );
        assert!(!text_of(&chunks).contains("bottom"));
    }

    #[test]
    fn decoded_is_cut_at_the_budget() {
        let (chunks, truncated) = decoded_source(MIXED, 30);
        assert!(truncated);
        assert_eq!(text_of(&chunks), "From: Ada <ada@example.org>\nSu");
    }

    #[test]
    fn original_is_the_bytes_as_text_cut_at_a_character() {
        assert_eq!(
            original_source(MIXED, 1 << 20),
            (String::from_utf8_lossy(MIXED).into_owned(), false)
        );
        let raw = "Subject: Grüße\r\n".as_bytes();
        // "ü" is two bytes at 11..13: a cut at 12 keeps it out.
        assert_eq!(original_source(raw, 12), ("Subject: Gr".to_owned(), true));
        assert_eq!(
            original_source(b"a\xffb", 10),
            ("a\u{fffd}b".to_owned(), false)
        );
    }
}
