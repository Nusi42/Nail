//! Facts about raw messages: the header block, the duplicate key, the
//! subject for issue labels, and flags stored in mbox headers (`Status`,
//! `X-Status`, `X-Mozilla-Status`, `X-Mozilla-Keys`).

use mail_parser::MessageParser;
use nail_api::Flags;

use crate::sink::DuplicateKey;

/// Where the header block ends: the index just after the empty line
/// (`\n\n` or `\r\n\r\n`), or the whole length when there is none.
pub fn header_end(raw: &[u8]) -> usize {
    let mut i = 0;
    let mut line_start = 0;
    while i < raw.len() {
        if raw[i] == b'\n' {
            let line = &raw[line_start..i];
            if line.is_empty() || line == b"\r" {
                return i + 1;
            }
            line_start = i + 1;
        }
        i += 1;
    }
    raw.len()
}

/// The header fields of a message, unfolded, in order (names as written;
/// values decoded as UTF-8, lossy; no RFC 2047 decoding).
pub fn header_fields(raw: &[u8]) -> Vec<(String, String)> {
    let block = &raw[..header_end(raw)];
    let text = String::from_utf8_lossy(block);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':')
            && is_field_name(name)
        {
            fields.push((name.to_owned(), value.trim().to_owned()));
        }
    }
    fields
}

fn is_field_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| (33..=126).contains(&b) && b != b':')
}

/// The first value of a header field (case-insensitive name).
pub fn header_value<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Whether the bytes start like a message: a header field before the
/// first empty line.
pub fn looks_like_message(raw: &[u8]) -> bool {
    let start = raw
        .iter()
        .position(|b| !matches!(b, b'\r' | b'\n' | b' ' | b'\t'))
        .unwrap_or(raw.len());
    let rest = &raw[start..];
    let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
    let line = &rest[..line_end];
    match line.iter().position(|&b| b == b':') {
        Some(colon) if colon > 0 => line[..colon]
            .iter()
            .all(|&b| (33..=126).contains(&b) && b != b':'),
        _ => false,
    }
}

/// What issues and duplicate checks need from a message's headers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeaderFacts {
    /// `Message-ID` without angle brackets.
    pub message_id: Option<String>,
    /// `Date` as Unix seconds.
    pub date: Option<i64>,
    /// `Subject`, decoded.
    pub subject: Option<String>,
}

/// Reads the facts from the header block.
pub fn header_facts(raw: &[u8]) -> HeaderFacts {
    let block = &raw[..header_end(raw)];
    let Some(message) = MessageParser::default().parse_headers(block) else {
        return HeaderFacts::default();
    };
    HeaderFacts {
        message_id: message
            .message_id()
            .map(|id| id.trim().trim_matches(['<', '>']).to_owned())
            .filter(|id| !id.is_empty()),
        date: message.date().map(mail_parser::DateTime::to_timestamp),
        subject: message.subject().map(str::to_owned),
    }
}

/// The duplicate key of a raw message.
pub fn duplicate_key(raw: &[u8], facts: &HeaderFacts) -> DuplicateKey {
    DuplicateKey {
        message_id: facts.message_id.clone(),
        date: facts.date,
        size: raw.len() as u64,
    }
}

/// Flags and keywords found in a message's own headers (mbox files).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeaderFlags {
    /// System flags.
    pub flags: Flags,
    /// Keywords (`X-Mozilla-Keys`).
    pub keywords: Vec<String>,
    /// Deleted but not yet compacted (Thunderbird `X-Mozilla-Status`
    /// expunged bit, `X-Status: D`).
    pub deleted: bool,
    /// Whether any flag header was present.
    pub present: bool,
}

/// Thunderbird's `X-Mozilla-Status` bits.
mod mozilla {
    pub(super) const READ: u32 = 0x0001;
    pub(super) const REPLIED: u32 = 0x0002;
    pub(super) const MARKED: u32 = 0x0004;
    pub(super) const EXPUNGED: u32 = 0x0008;
}

/// Reads `Status` (`R` read), `X-Status` (`A` answered, `F` flagged, `T`
/// draft, `D` deleted), `X-Mozilla-Status` (hex bits) and
/// `X-Mozilla-Keys` (space-separated keywords).
pub fn header_flags(fields: &[(String, String)]) -> HeaderFlags {
    let mut out = HeaderFlags::default();
    if let Some(status) = header_value(fields, "Status") {
        out.present = true;
        out.flags.seen |= status.contains('R');
    }
    if let Some(x_status) = header_value(fields, "X-Status") {
        out.present = true;
        out.flags.answered |= x_status.contains('A');
        out.flags.flagged |= x_status.contains('F');
        out.flags.draft |= x_status.contains('T');
        out.deleted |= x_status.contains('D');
    }
    if let Some(bits) = header_value(fields, "X-Mozilla-Status")
        .and_then(|v| u32::from_str_radix(v.trim(), 16).ok())
    {
        out.present = true;
        out.flags.seen |= bits & mozilla::READ != 0;
        out.flags.answered |= bits & mozilla::REPLIED != 0;
        out.flags.flagged |= bits & mozilla::MARKED != 0;
        out.deleted |= bits & mozilla::EXPUNGED != 0;
    }
    if let Some(keys) = header_value(fields, "X-Mozilla-Keys") {
        out.present = true;
        for key in keys.split_whitespace() {
            push_keyword(&mut out.keywords, key);
        }
    }
    out
}

/// Adds a keyword unless it is there (case-insensitive) or not a valid
/// IMAP atom.
pub fn push_keyword(keywords: &mut Vec<String>, keyword: &str) {
    let keyword = keyword.trim();
    let valid = !keyword.is_empty()
        && !keyword.starts_with('\\')
        && keyword
            .bytes()
            .all(|b| b > b' ' && b < 127 && !b"(){%*\"\\]".contains(&b));
    if valid && !keywords.iter().any(|k| k.eq_ignore_ascii_case(keyword)) {
        keywords.push(keyword.to_owned());
    }
}

/// A label for issues: the subject, else the `Message-ID`.
pub fn issue_label(facts: &HeaderFacts) -> Option<String> {
    facts
        .subject
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| facts.message_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const MESSAGE: &[u8] = b"From: Ada <ada@example.org>\r\nSubject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n  again\r\nMessage-ID: <abc@example.org>\r\nDate: Thu, 01 Oct 2026 10:00:00 +0000\r\nX-Mozilla-Status: 0005\r\nX-Mozilla-Keys: $label1 work   \r\n\r\nBody\r\nX: not a header\r\n";

    #[test]
    fn header_blocks_end_at_the_empty_line() {
        let end = header_end(MESSAGE);
        assert_eq!(&MESSAGE[end..end + 4], b"Body");
        assert_eq!(header_end(b"A: b\nC: d"), 9);
        assert_eq!(header_end(b"A: b\n\nbody"), 6);
    }

    #[test]
    fn fields_are_unfolded() {
        let fields = header_fields(MESSAGE);
        assert_eq!(
            header_value(&fields, "subject"),
            Some("=?utf-8?q?Gr=C3=BC=C3=9Fe?= again")
        );
        assert!(header_value(&fields, "X").is_none());
    }

    #[test]
    fn facts_and_keys() {
        let facts = header_facts(MESSAGE);
        assert_eq!(facts.message_id.as_deref(), Some("abc@example.org"));
        assert_eq!(facts.date, Some(1_790_848_800));
        assert!(facts.subject.as_deref().unwrap().starts_with("Grüße"));
        let key = duplicate_key(MESSAGE, &facts);
        assert_eq!(key.size, MESSAGE.len() as u64);
        assert_eq!(
            issue_label(&facts).as_deref().map(|s| &s[..6]),
            Some("Grüß")
        );
        assert_eq!(header_facts(b"no headers here").message_id, None);
    }

    #[test]
    fn mozilla_flags() {
        let flags = header_flags(&header_fields(MESSAGE));
        assert!(flags.present);
        assert!(flags.flags.seen && flags.flags.flagged && !flags.flags.answered);
        assert!(!flags.deleted);
        assert_eq!(flags.keywords, vec!["$label1", "work"]);
        let expunged = header_flags(&header_fields(b"X-Mozilla-Status: 0009\n\n"));
        assert!(expunged.deleted && expunged.flags.seen);
    }

    #[test]
    fn status_headers() {
        let flags = header_flags(&header_fields(b"Status: RO\nX-Status: AFT\n\n"));
        assert_eq!(
            flags.flags,
            Flags {
                seen: true,
                flagged: true,
                answered: true,
                draft: true
            }
        );
        let none = header_flags(&header_fields(b"Subject: x\n\n"));
        assert!(!none.present);
        assert_eq!(none.flags, Flags::default());
    }

    #[test]
    fn message_detection() {
        assert!(looks_like_message(b"Subject: hi\n\nbody"));
        assert!(looks_like_message(b"\r\nReceived: x\n"));
        assert!(!looks_like_message(b"Hello world\nSubject: x"));
        assert!(!looks_like_message(b": x"));
        assert!(!looks_like_message(b""));
    }

    #[test]
    fn keywords_are_atoms_without_duplicates() {
        let mut keywords = vec![];
        push_keyword(&mut keywords, "Work");
        push_keyword(&mut keywords, "work");
        push_keyword(&mut keywords, "\\Seen");
        push_keyword(&mut keywords, "a(b");
        push_keyword(&mut keywords, "");
        assert_eq!(keywords, vec!["Work"]);
    }
}
