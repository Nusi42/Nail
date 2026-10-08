//! Percent-decoding (RFC 3986 §2.1) of URL paths, `mid:` URLs and folder
//! URIs: one definition for the DAV client, `nail-ical`, the portability
//! importers and the mock API servers (wave Q2, ADR 0171; before, five
//! copies).

/// Decodes `%XX` escapes (two hex digits; anything else stays as it is)
/// and reads the bytes as UTF-8 (invalid sequences become U+FFFD).
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2)))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of one hex digit.
fn hex(byte: Option<&u8>) -> Option<u8> {
    let digit = char::from(*byte?).to_digit(16)?;
    u8::try_from(digit).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_escapes() {
        assert_eq!(decode("a%20b%2Fc%"), "a b/c%");
        assert_eq!(decode("a%2Fb%zz"), "a/b%zz");
        assert_eq!(decode("a%2"), "a%2");
        assert_eq!(decode("%41b"), "Ab");
        assert_eq!(decode("%c3%a4%C3%A4"), "ää");
        assert_eq!(decode("%+1"), "%+1");
        assert_eq!(decode("%ff"), "\u{fffd}");
        assert_eq!(decode(""), "");
    }
}
