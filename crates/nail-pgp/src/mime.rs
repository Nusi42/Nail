//! Byte-exact MIME handling for PGP/MIME (RFC 3156).
//!
//! Signatures cover the exact bytes of a MIME entity, so the multipart
//! structure is split here by hand (boundaries, the CRLF that belongs to
//! each delimiter); header values are parsed with `mail-parser`, which
//! the rest of Nail uses too. Everything this module writes uses CRLF.

use mail_parser::{MessageParser, MimeHeaders};
use rand::Rng;

/// One header field as it appears in the entity: the name and the raw
/// field (name, colon, value and folded continuation lines) without the
/// final line break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderField {
    pub name: String,
    pub raw: Vec<u8>,
}

impl HeaderField {
    /// A new field `Name: value` (the value is written as given; fold
    /// long values before).
    pub(crate) fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            raw: format!("{name}: {value}").into_bytes(),
        }
    }

    /// Whether the field has this name (case-insensitive).
    pub(crate) fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// The unfolded value after the colon, trimmed (lossy UTF-8).
    pub(crate) fn value(&self) -> String {
        let text = String::from_utf8_lossy(&self.raw);
        let value = text.split_once(':').map_or("", |(_, v)| v);
        let mut out = String::with_capacity(value.len());
        for (i, line) in value.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if i > 0 {
                out.push(' ');
                out.push_str(line.trim_start());
            } else {
                out.push_str(line);
            }
        }
        out.trim().to_owned()
    }
}

/// A MIME entity split into its header fields and body.
#[derive(Debug, Clone)]
pub(crate) struct Entity<'a> {
    pub headers: Vec<HeaderField>,
    pub body: &'a [u8],
}

impl<'a> Entity<'a> {
    /// Splits `raw` at the first empty line. Without one the whole input
    /// is headers and the body is empty.
    pub(crate) fn parse(raw: &'a [u8]) -> Self {
        let (head, body) = split_header_block(raw);
        Self {
            headers: parse_fields(head),
            body,
        }
    }

    /// The first field with this name.
    pub(crate) fn header(&self, name: &str) -> Option<&HeaderField> {
        self.headers.iter().find(|h| h.is(name))
    }

    /// The unfolded value of the first field with this name.
    pub(crate) fn header_value(&self, name: &str) -> Option<String> {
        self.header(name).map(HeaderField::value)
    }

    /// The parsed `Content-Type` (default `text/plain`).
    pub(crate) fn content_type(&self) -> ContentType {
        self.header_value("Content-Type")
            .map_or_else(ContentType::default_text, |v| ContentType::parse(&v))
    }
}

fn split_header_block(raw: &[u8]) -> (&[u8], &[u8]) {
    // An entity may start with an empty line (no header fields).
    if raw.starts_with(b"\r\n") {
        return (&[], &raw[2..]);
    }
    if raw.starts_with(b"\n") {
        return (&[], &raw[1..]);
    }
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\n' {
            let rest = &raw[i + 1..];
            if rest.starts_with(b"\r\n") {
                return (&raw[..=i], &rest[2..]);
            }
            if rest.starts_with(b"\n") {
                return (&raw[..=i], &rest[1..]);
            }
        }
        i += 1;
    }
    (raw, &[])
}

fn parse_fields(head: &[u8]) -> Vec<HeaderField> {
    let mut fields: Vec<HeaderField> = Vec::new();
    for line in split_lines(head) {
        if line.is_empty() {
            continue;
        }
        if (line[0] == b' ' || line[0] == b'\t')
            && let Some(last) = fields.last_mut()
        {
            last.raw.extend_from_slice(b"\r\n");
            last.raw.extend_from_slice(line);
            continue;
        }
        let name = line
            .iter()
            .position(|b| *b == b':')
            .map(|p| String::from_utf8_lossy(&line[..p]).trim().to_owned());
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            fields.push(HeaderField {
                name,
                raw: line.to_vec(),
            });
        }
    }
    fields
}

/// Lines without their line breaks (CRLF or LF).
fn split_lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
}

/// Writes header fields, one per line, CRLF.
pub(crate) fn write_fields(out: &mut Vec<u8>, fields: &[HeaderField]) {
    for field in fields {
        out.extend_from_slice(&to_crlf(&field.raw));
        out.extend_from_slice(b"\r\n");
    }
}

/// A parsed `Content-Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentType {
    /// `type/subtype`, lowercase.
    pub mime: String,
    /// Parameters (names lowercase), decoded.
    pub params: Vec<(String, String)>,
}

impl ContentType {
    fn default_text() -> Self {
        Self {
            mime: "text/plain".into(),
            params: Vec::new(),
        }
    }

    /// Parses a header value with `mail-parser` (RFC 2045 / 2231).
    pub(crate) fn parse(value: &str) -> Self {
        let synthetic = format!("Content-Type: {value}\r\n\r\n");
        let parsed = MessageParser::default().parse(synthetic.as_bytes());
        let Some(ct) = parsed.as_ref().and_then(|m| m.root_part().content_type()) else {
            return Self::default_text();
        };
        let mime = match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_owned(),
        }
        .to_ascii_lowercase();
        let params = ct
            .attributes()
            .map(|attrs| {
                attrs
                    .iter()
                    .map(|a| (a.name.to_ascii_lowercase(), a.value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Self { mime, params }
    }

    /// A parameter (case-insensitive name).
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether this is `mime` (lowercase).
    pub(crate) fn is(&self, mime: &str) -> bool {
        self.mime == mime
    }
}

/// The parts of a multipart body.
#[derive(Debug, Clone)]
pub(crate) struct Multipart<'a> {
    /// Text before the first delimiter.
    pub preamble: &'a [u8],
    /// Each part, exactly: from after the delimiter line to before the
    /// line break that precedes the next delimiter (RFC 2046 §5.1.1).
    pub parts: Vec<&'a [u8]>,
}

/// Splits a multipart body at `boundary`. `None` when no delimiter is
/// found.
pub(crate) fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Option<Multipart<'a>> {
    let delimiter = format!("--{boundary}");
    let delimiter = delimiter.as_bytes();
    // (start of the delimiter line, start of the line break before it,
    // end of the delimiter line incl. its break, is close delimiter)
    let mut marks: Vec<(usize, usize, bool)> = Vec::new();
    let mut line_start = 0;
    while line_start <= body.len() {
        let line_end = body[line_start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(body.len(), |p| line_start + p);
        let line = &body[line_start..line_end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(rest) = line.strip_prefix(delimiter) {
            let (close, rest) = match rest.strip_prefix(b"--") {
                Some(r) => (true, r),
                None => (false, rest),
            };
            if rest.iter().all(|b| *b == b' ' || *b == b'\t') {
                // The line break before the delimiter belongs to it.
                let before = if line_start >= 2 && &body[line_start - 2..line_start] == b"\r\n" {
                    line_start - 2
                } else if line_start >= 1 && body[line_start - 1] == b'\n' {
                    line_start - 1
                } else {
                    line_start
                };
                let after = (line_end + 1).min(body.len());
                marks.push((before, after, close));
                if close {
                    break;
                }
            }
        }
        if line_end >= body.len() {
            break;
        }
        line_start = line_end + 1;
    }
    let (first_before, _, _) = *marks.first()?;
    let mut parts = Vec::new();
    for pair in marks.windows(2) {
        let (_, start, close) = pair[0];
        if close {
            break;
        }
        let (end, _, _) = pair[1];
        parts.push(&body[start..end.max(start)]);
    }
    Some(Multipart {
        preamble: &body[..first_before],
        parts,
    })
}

/// Converts lone LF to CRLF (canonical form, RFC 3156 §5).
pub(crate) fn to_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 32);
    let mut prev = 0u8;
    for &b in data {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}

/// A random multipart boundary.
pub(crate) fn new_boundary() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..24)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("nail-{suffix}")
}

/// The address part of a mailbox header value (`Name <a@b>` → `a@b`),
/// lowercase. Uses `mail-parser`; `None` when there is no address.
pub(crate) fn addresses(field_value: &str, name: &str) -> Vec<String> {
    let synthetic = format!("{name}: {field_value}\r\n\r\n");
    let Some(message) = MessageParser::default().parse(synthetic.as_bytes()) else {
        return Vec::new();
    };
    let value = message.header(name);
    let mut out = Vec::new();
    if let Some(addr) = value.and_then(|v| v.as_address()) {
        for a in addr.iter() {
            if let Some(email) = a.address() {
                out.push(email.trim().to_ascii_lowercase());
            }
        }
    }
    out
}

/// The `Date` header as Unix seconds.
pub(crate) fn date(field_value: &str) -> Option<i64> {
    let synthetic = format!("Date: {field_value}\r\n\r\n");
    let message = MessageParser::default().parse(synthetic.as_bytes())?;
    message.date().map(mail_parser::DateTime::to_timestamp)
}

/// The decoded body of a leaf part (transfer encoding removed), with the
/// text decoded to UTF-8 for text parts.
pub(crate) fn decoded_text(entity_raw: &[u8]) -> Option<String> {
    let message = MessageParser::default().parse(entity_raw)?;
    let part = message.root_part();
    match &part.body {
        mail_parser::PartType::Text(t) | mail_parser::PartType::Html(t) => Some(t.to_string()),
        mail_parser::PartType::Binary(b) | mail_parser::PartType::InlineBinary(b) => {
            Some(String::from_utf8_lossy(b).into_owned())
        }
        _ => None,
    }
}

/// The decoded body bytes of a leaf part.
pub(crate) fn decoded_bytes(entity_raw: &[u8]) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(entity_raw)?;
    let part = message.root_part();
    match &part.body {
        mail_parser::PartType::Text(t) | mail_parser::PartType::Html(t) => {
            Some(t.as_bytes().to_vec())
        }
        mail_parser::PartType::Binary(b) | mail_parser::PartType::InlineBinary(b) => {
            Some(b.to_vec())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn entity_splits_headers_and_body_with_folding() {
        let raw = b"Subject: Hello\r\n World\r\nContent-Type: text/plain;\r\n charset=utf-8\r\n\r\nBody\r\n";
        let e = Entity::parse(raw);
        assert_eq!(e.headers.len(), 2);
        assert_eq!(e.header_value("subject").unwrap(), "Hello World");
        assert_eq!(e.body, b"Body\r\n");
        let ct = e.content_type();
        assert!(ct.is("text/plain"));
        assert_eq!(ct.param("charset"), Some("utf-8"));
        // LF-only input and an entity without headers.
        let e = Entity::parse(b"A: b\n\nx\n");
        assert_eq!(e.header_value("a").unwrap(), "b");
        assert_eq!(e.body, b"x\n");
        let e = Entity::parse(b"\r\nonly body");
        assert!(e.headers.is_empty(), "{:?}", e.headers);
        assert_eq!(e.body, b"only body");
        assert!(Entity::parse(b"").content_type().is("text/plain"));
    }

    #[test]
    fn multipart_parts_exclude_the_delimiter_line_break() {
        let body = b"preamble\r\n--b\r\nContent-Type: text/plain\r\n\r\none\r\n--b\r\n\r\ntwo\r\n\r\n--b--\r\nepilogue";
        let m = split_multipart(body, "b").unwrap();
        assert_eq!(m.preamble, b"preamble");
        assert_eq!(m.parts.len(), 2);
        assert_eq!(m.parts[0], b"Content-Type: text/plain\r\n\r\none");
        assert_eq!(m.parts[1], b"\r\ntwo\r\n");
        // Lookalike boundaries are not delimiters; no delimiter → None.
        let m = split_multipart(b"--bb\r\nx\r\n--b\r\ny\r\n--b--", "b").unwrap();
        assert_eq!(m.parts, vec![&b"y"[..]]);
        assert!(split_multipart(b"nothing here", "b").is_none());
        // Unclosed multipart keeps what it has.
        let m = split_multipart(b"--b\nA\n--b\nB", "b").unwrap();
        assert_eq!(m.parts, vec![&b"A"[..]]);
    }

    #[test]
    fn content_type_parameters() {
        let ct = ContentType::parse(
            "multipart/signed; micalg=pgp-sha256;\r\n protocol=\"application/pgp-signature\"; boundary=\"--x y\"",
        );
        assert!(ct.is("multipart/signed"));
        assert_eq!(ct.param("protocol"), Some("application/pgp-signature"));
        assert_eq!(ct.param("BOUNDARY"), Some("--x y"));
        assert_eq!(ct.param("micalg"), Some("pgp-sha256"));
    }

    #[test]
    fn crlf_boundary_and_addresses() {
        assert_eq!(to_crlf(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n");
        let b = new_boundary();
        assert!(b.starts_with("nail-") && b.len() == 29);
        assert_ne!(new_boundary(), b);
        assert_eq!(
            addresses("Ada <ADA@Example.org>, bob@example.net", "To"),
            vec!["ada@example.org", "bob@example.net"]
        );
        assert_eq!(addresses("", "From").len(), 0);
        assert_eq!(date("Thu, 01 Jan 2026 12:00:00 +0000"), Some(1_767_268_800));
        assert_eq!(
            decoded_text(b"Content-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n").unwrap(),
            "hello"
        );
    }

    #[test]
    fn header_field_value_and_writer() {
        let f = HeaderField::new("Subject", "Hi");
        assert!(f.is("subject"));
        assert_eq!(f.value(), "Hi");
        let mut out = Vec::new();
        write_fields(&mut out, &[f, HeaderField::new("X", "a\nb")]);
        assert_eq!(out, b"Subject: Hi\r\nX: a\r\nb\r\n");
    }
}
