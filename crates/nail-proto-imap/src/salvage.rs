//! A lenient reader for `FETCH` responses the strict `imap-proto` parser
//! rejects (ADR 0191).
//!
//! `imap-proto` refuses a whole `FETCH` response when one item breaks the
//! grammar: 8-bit bytes in a quoted string (raw UTF-8 or Latin-1 header
//! values in `ENVELOPE` / `BODYSTRUCTURE`), `()` for an empty address list,
//! `NIL` where a date belongs (it even panics there), keywords with odd
//! characters. Before, such a message was silently missing from the
//! answer and the batch — the whole folder — failed with "server did not
//! return messages". This reader takes what the sync needs (`UID`, `FLAGS`,
//! `INTERNALDATE`, `RFC822.SIZE`, `MODSEQ`, the body or header section) and
//! skips everything else, whatever bytes it holds.

use std::borrow::Cow;

use imap_proto::Response;
use imap_proto::types::{AttributeValue, MessageSection, SectionPath};

/// One value of a `FETCH` item.
#[derive(Debug)]
enum Value<'a> {
    Nil,
    Atom(&'a [u8]),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

/// Deepest nesting followed (BODYSTRUCTURE of nested multiparts).
const MAX_DEPTH: usize = 64;

impl<'a> Reader<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.at += 1;
        }
    }

    fn expect(&mut self, text: &[u8]) -> Option<()> {
        let end = self.at.checked_add(text.len())?;
        let found = self.bytes.get(self.at..end)?;
        found.eq_ignore_ascii_case(text).then(|| self.at = end)
    }

    fn atom(&mut self) -> Option<&'a [u8]> {
        let start = self.at;
        let mut brackets = 0usize;
        while let Some(b) = self.peek() {
            match b {
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b' ' | b'(' | b')' if brackets == 0 => break,
                b'\r' | b'\n' => break,
                _ => {}
            }
            self.at += 1;
        }
        (self.at > start).then(|| &self.bytes[start..self.at])
    }

    fn number<T: std::str::FromStr>(&mut self) -> Option<T> {
        std::str::from_utf8(self.atom()?).ok()?.parse().ok()
    }

    fn value(&mut self) -> Option<Value<'a>> {
        self.skip_spaces();
        match self.peek()? {
            b'"' => {
                self.at += 1;
                let start = self.at;
                loop {
                    match self.peek()? {
                        b'\\' => self.at += 2,
                        b'"' => break,
                        _ => self.at += 1,
                    }
                }
                let raw = self.bytes.get(start..self.at)?;
                self.at += 1;
                Some(Value::Bytes(raw))
            }
            b'{' => {
                self.at += 1;
                let start = self.at;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.at += 1;
                }
                let len: usize = std::str::from_utf8(&self.bytes[start..self.at])
                    .ok()?
                    .parse()
                    .ok()?;
                if self.peek() == Some(b'+') {
                    self.at += 1;
                }
                self.expect(b"}")?;
                if self.peek() == Some(b'\r') {
                    self.at += 1;
                }
                self.expect(b"\n")?;
                let end = self.at.checked_add(len)?;
                let data = self.bytes.get(self.at..end)?;
                self.at = end;
                Some(Value::Bytes(data))
            }
            b'(' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return None;
                }
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_spaces();
                    match self.peek()? {
                        b')' => {
                            self.at += 1;
                            break;
                        }
                        _ => items.push(self.value()?),
                    }
                }
                self.depth -= 1;
                Some(Value::List(items))
            }
            _ => {
                let atom = self.atom()?;
                Some(if atom.eq_ignore_ascii_case(b"NIL") {
                    Value::Nil
                } else {
                    Value::Atom(atom)
                })
            }
        }
    }
}

fn text(bytes: &[u8]) -> Cow<'static, str> {
    Cow::Owned(String::from_utf8_lossy(bytes).into_owned())
}

fn number<T: std::str::FromStr>(value: &Value<'_>) -> Option<T> {
    match value {
        Value::Atom(atom) => std::str::from_utf8(atom).ok()?.parse().ok(),
        Value::List(items) => items.first().and_then(number),
        _ => None,
    }
}

fn data(value: &Value<'_>) -> Option<Cow<'static, [u8]>> {
    match value {
        Value::Bytes(bytes) => Some(Cow::Owned(bytes.to_vec())),
        _ => None,
    }
}

/// The item an attribute name stands for, if the sync uses it.
fn attribute(name: &[u8], value: &Value<'_>) -> Option<AttributeValue<'static>> {
    let name = String::from_utf8_lossy(name).to_ascii_uppercase();
    // `BODY[]<0>` partial origins are not asked for; ignore them.
    let name = name.split('<').next().unwrap_or_default();
    Some(match name {
        "UID" => AttributeValue::Uid(number(value)?),
        "RFC822.SIZE" => AttributeValue::Rfc822Size(number(value)?),
        "MODSEQ" => AttributeValue::ModSeq(number(value)?),
        "FLAGS" => match value {
            Value::List(items) => AttributeValue::Flags(
                items
                    .iter()
                    .filter_map(|item| match item {
                        Value::Atom(atom) | Value::Bytes(atom) => Some(text(atom)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => return None,
        },
        "INTERNALDATE" => match value {
            Value::Bytes(date) | Value::Atom(date) => AttributeValue::InternalDate(text(date)),
            _ => return None,
        },
        "BODY[]" | "BINARY[]" => AttributeValue::BodySection {
            section: None,
            index: None,
            data: data(value),
        },
        "BODY[HEADER]" => AttributeValue::BodySection {
            section: Some(SectionPath::Full(MessageSection::Header)),
            index: None,
            data: data(value),
        },
        "RFC822" => AttributeValue::Rfc822(data(value)),
        "RFC822.HEADER" => AttributeValue::Rfc822Header(data(value)),
        _ => return None,
    })
}

/// Reads `* <n> FETCH (...)` leniently; `None` when it is not a `FETCH`
/// response or has no `UID`.
pub(crate) fn fetch(bytes: &[u8]) -> Option<Response<'static>> {
    let mut reader = Reader {
        bytes,
        at: 0,
        depth: 0,
    };
    reader.expect(b"* ")?;
    let sequence: u32 = reader.number()?;
    reader.skip_spaces();
    reader.expect(b"FETCH")?;
    reader.skip_spaces();
    reader.expect(b"(")?;
    let mut attributes = Vec::new();
    loop {
        reader.skip_spaces();
        if reader.peek()? == b')' {
            break;
        }
        let name = reader.atom()?;
        let value = reader.value()?;
        attributes.extend(attribute(name, &value));
    }
    attributes
        .iter()
        .any(|a| matches!(a, AttributeValue::Uid(_)))
        .then_some(Response::Fetch(sequence, attributes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(bytes: &[u8]) -> Vec<AttributeValue<'static>> {
        match fetch(bytes) {
            Some(Response::Fetch(_, attributes)) => attributes,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn eight_bit_envelopes_and_empty_lists_are_skipped() {
        let mut line = b"* 3 FETCH (UID 151 FLAGS (\\Seen $Label1) INTERNALDATE \"07-Oct-2026 10:00:00 +0200\" RFC822.SIZE 11 ENVELOPE (NIL \"Gr\xfc\xdfe aus K\xf6ln\" () NIL NIL ((NIL NIL \"x\" \"y.de\")) NIL NIL NIL NIL) BODY[] {11}\r\n".to_vec();
        line.extend_from_slice(b"Hello world)\r\n");
        assert!(
            Response::from_bytes(&line).is_err(),
            "imap-proto refuses it"
        );
        let attributes = attributes(&line);
        assert!(attributes.contains(&AttributeValue::Uid(151)));
        assert!(attributes.contains(&AttributeValue::Rfc822Size(11)));
        assert!(attributes.contains(&AttributeValue::BodySection {
            section: None,
            index: None,
            data: Some(Cow::Owned(b"Hello world".to_vec())),
        }));
        assert!(attributes.iter().any(|a| matches!(
            a,
            AttributeValue::Flags(flags) if flags.len() == 2
        )));
        assert!(attributes.iter().any(|a| matches!(
            a,
            AttributeValue::InternalDate(date) if date.starts_with("07-Oct")
        )));
    }

    #[test]
    fn headers_modseq_and_literals_in_odd_places() {
        let line = b"* 9 FETCH (MODSEQ (77) BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"8BIT\" 3 1)(\"TEXT\" \"HTML\" NIL NIL NIL \"8BIT\" 3 1) \"ALTERNATIVE\") BODY[HEADER] {4}\r\nA: b UID 9 INTERNALDATE NIL)\r\n";
        let attributes = attributes(line);
        assert!(attributes.contains(&AttributeValue::ModSeq(77)));
        assert!(attributes.contains(&AttributeValue::Uid(9)));
        assert!(attributes.iter().any(|a| matches!(
            a,
            AttributeValue::BodySection {
                section: Some(SectionPath::Full(MessageSection::Header)),
                ..
            }
        )));
    }

    #[test]
    fn not_a_fetch_or_no_uid() {
        assert!(fetch(b"* 3 EXISTS\r\n").is_none());
        assert!(fetch(b"* 3 FETCH (FLAGS (\\Seen))\r\n").is_none());
        assert!(fetch(b"* 3 FETCH (UID 4 BODY[] {99}\r\nshort)\r\n").is_none());
        let deep = format!(
            "* 1 FETCH (UID 1 X {}{})\r\n",
            "(".repeat(100),
            ")".repeat(100)
        );
        assert!(fetch(deep.as_bytes()).is_none(), "nesting is bounded");
    }
}
