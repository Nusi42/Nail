//! Byte and text helpers for raw messages that several readers and writers
//! need without a MIME parser: searching bytes, unfolding a header block,
//! undoing backslash escapes, adding a `Bcc` header (one definition for the
//! OpenPGP / S/MIME readers, the Graph / EWS uploads and the importers).

/// The position of the first `needle` in `haystack` (an empty needle is
/// found at 0).
#[must_use]
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The header section of a raw message, up to and including the empty line
/// (CRLF or LF ends), or everything when there is no body.
#[must_use]
pub fn header_section(raw: &[u8]) -> &[u8] {
    if let Some(i) = find(raw, b"\r\n\r\n") {
        return &raw[..i + 4];
    }
    if let Some(i) = find(raw, b"\n\n") {
        return &raw[..i + 2];
    }
    raw
}

/// The lines of `body` as (start, end) byte offsets, `end` at the line's
/// `\n` (exclusive) or at the end of `body`; a body ending in `\n` has a
/// last, empty line.
pub fn lines(body: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut next = Some(0);
    std::iter::from_fn(move || {
        let start = next?;
        let end = body[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(body.len(), |p| start + p);
        next = (end < body.len()).then_some(end + 1);
        Some((start, end))
    })
}

/// The headers of a header block as (lowercase name, unfolded value):
/// continuation lines (leading space or tab) join the previous value with
/// one space; empty lines and lines without a colon are skipped. CRLF and
/// LF both end a line (pass only the header block).
#[must_use]
pub fn unfold_headers(block: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(block);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    out
}

/// `text` with every backslash escape (`\x` → `x`) undone; a trailing lone
/// backslash is dropped.
#[must_use]
pub fn unescape_backslashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// ASCII `text` (base64) cut into lines of `width` characters joined by
/// CRLF (no line break at the end).
#[must_use]
pub fn wrap_ascii(text: &str, width: usize) -> String {
    text.as_bytes()
        .chunks(width.max(1))
        .map(|c| String::from_utf8_lossy(c))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// `raw` with a `Bcc` header for `addresses` in front (unchanged without
/// addresses): the envelope recipients a server takes from the headers.
#[must_use]
pub fn with_bcc(raw: &[u8], addresses: &[String]) -> Vec<u8> {
    if addresses.is_empty() {
        return raw.to_vec();
    }
    let mut out = format!("Bcc: {}\r\n", addresses.join(", ")).into_bytes();
    out.extend_from_slice(raw);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bytes() {
        assert_eq!(find(b"a\r\n\r\nb", b"\r\n\r\n"), Some(1));
        assert_eq!(find(b"abc", b"x"), None);
        assert_eq!(find(b"ab", b"abc"), None);
        assert_eq!(find(b"ab", b""), Some(0));
    }

    #[test]
    fn header_sections() {
        assert_eq!(header_section(b"A: 1\r\n\r\nbody"), b"A: 1\r\n\r\n");
        assert_eq!(header_section(b"A: 1\n\nbody"), b"A: 1\n\n");
        assert_eq!(header_section(b"A: 1\r\n"), b"A: 1\r\n");
    }

    #[test]
    fn splits_lines() {
        assert_eq!(
            lines(b"a\r\nbc\n").collect::<Vec<_>>(),
            [(0, 2), (3, 5), (6, 6)]
        );
        assert_eq!(lines(b"x").collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(lines(b"").collect::<Vec<_>>(), [(0, 0)]);
    }

    #[test]
    fn unfolds_headers() {
        let block = b"Subject: one\r\n two\r\nX-Empty:\r\nno colon\nTo:\ta@b\n\tc@d\r\n";
        assert_eq!(
            unfold_headers(block),
            [
                ("subject".to_owned(), "one two".to_owned()),
                ("x-empty".to_owned(), String::new()),
                ("to".to_owned(), "a@b c@d".to_owned()),
            ]
        );
        assert_eq!(unfold_headers(b" orphan continuation\r\n").len(), 0);
    }

    #[test]
    fn unescapes() {
        assert_eq!(unescape_backslashes(r#"a\"b\\c\"#), r#"a"b\c"#);
        assert_eq!(unescape_backslashes("plain"), "plain");
    }

    #[test]
    fn wraps_base64() {
        assert_eq!(wrap_ascii("abcdefg", 3), "abc\r\ndef\r\ng");
        assert_eq!(wrap_ascii("abc", 3), "abc");
        assert_eq!(wrap_ascii("", 3), "");
    }

    #[test]
    fn adds_bcc() {
        assert_eq!(with_bcc(b"Subject: x\r\n", &[]), b"Subject: x\r\n");
        assert_eq!(
            with_bcc(b"Subject: x\r\n", &["a@b".to_owned(), "c@d".to_owned()]),
            b"Bcc: a@b, c@d\r\nSubject: x\r\n"
        );
    }
}
