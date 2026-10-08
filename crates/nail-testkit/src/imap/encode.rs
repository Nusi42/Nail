//! Encoding of server response data: strings and the `ENVELOPE` structure.

use mail_parser::{Address, HeaderName, Message, MessageParser};

/// Appends an IMAP `nstring`: `NIL`, a quoted string, or a literal for
/// anything that cannot be quoted safely.
pub(crate) fn push_nstring(out: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        None => out.extend_from_slice(b"NIL"),
        Some(bytes) => push_string(out, bytes),
    }
}

/// Appends an IMAP `string` (quoted or literal).
pub(crate) fn push_string(out: &mut Vec<u8>, bytes: &[u8]) {
    let quotable = bytes.len() < 1024 && bytes.iter().all(|&b| (0x20..0x7f).contains(&b));
    if quotable {
        out.push(b'"');
        for &b in bytes {
            if b == b'"' || b == b'\\' {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b'"');
    } else {
        push_literal(out, bytes);
    }
}

/// Appends a literal `{n}\r\n<bytes>`.
pub(crate) fn push_literal(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
    out.extend_from_slice(bytes);
}

/// Formats ascending numbers as an IMAP sequence set: `1:3,7,9:10`.
pub(crate) use nail_net::wire::sequence_set as number_set;

/// Formats numbers in the given order as a comma list (for `COPYUID`,
/// where source and destination sets must correspond position by position).
pub(crate) fn number_list(numbers: &[u32]) -> String {
    numbers
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Appends the `ENVELOPE` structure (RFC 3501 section 7.4.2) for a message.
pub(crate) fn push_envelope(out: &mut Vec<u8>, raw: &[u8]) {
    let parsed = MessageParser::default().parse_headers(raw);
    let message = parsed.as_ref();
    let raw_header = |name: HeaderName<'static>| -> Option<Vec<u8>> {
        let value = message?.header_raw(name)?;
        let unfolded: String = value.replace("\r\n", "").replace('\n', "");
        let trimmed = unfolded.trim();
        (!trimmed.is_empty()).then(|| trimmed.as_bytes().to_vec())
    };

    out.push(b'(');
    push_nstring(out, raw_header(HeaderName::Date).as_deref());
    out.push(b' ');
    push_nstring(out, raw_header(HeaderName::Subject).as_deref());
    let from = addresses(message, HeaderName::From);
    let sender = addresses(message, HeaderName::Sender);
    let reply_to = addresses(message, HeaderName::ReplyTo);
    for list in [
        &from,
        // RFC 3501: sender and reply-to default to from.
        if sender.is_empty() { &from } else { &sender },
        if reply_to.is_empty() {
            &from
        } else {
            &reply_to
        },
        &addresses(message, HeaderName::To),
        &addresses(message, HeaderName::Cc),
        &addresses(message, HeaderName::Bcc),
    ] {
        out.push(b' ');
        push_address_list(out, list);
    }
    out.push(b' ');
    push_nstring(out, raw_header(HeaderName::InReplyTo).as_deref());
    out.push(b' ');
    push_nstring(out, raw_header(HeaderName::MessageId).as_deref());
    out.push(b')');
}

/// `(name, mailbox, host)` triples of an address header.
fn addresses(
    message: Option<&Message<'_>>,
    name: HeaderName<'static>,
) -> Vec<(Option<String>, String, String)> {
    let Some(address) = message
        .and_then(|m| m.header(name))
        .and_then(|v| v.as_address())
    else {
        return Vec::new();
    };
    let addrs: Vec<_> = match address {
        Address::List(list) => list.iter().collect(),
        Address::Group(groups) => groups.iter().flat_map(|g| g.addresses.iter()).collect(),
    };
    addrs
        .into_iter()
        .filter_map(|addr| {
            let email = addr.address()?;
            let (mailbox, host) = email.rsplit_once('@').unwrap_or((email, ""));
            Some((
                addr.name().map(str::to_owned),
                mailbox.to_owned(),
                host.to_owned(),
            ))
        })
        .collect()
}

fn push_address_list(out: &mut Vec<u8>, list: &[(Option<String>, String, String)]) {
    if list.is_empty() {
        out.extend_from_slice(b"NIL");
        return;
    }
    out.push(b'(');
    for (name, mailbox, host) in list {
        out.push(b'(');
        push_nstring(out, name.as_deref().map(str::as_bytes));
        out.extend_from_slice(b" NIL ");
        push_string(out, mailbox.as_bytes());
        out.push(b' ');
        push_nstring(out, (!host.is_empty()).then_some(host.as_bytes()));
        out.push(b')');
    }
    out.push(b')');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn strings_are_quoted_or_literal() {
        let mut out = Vec::new();
        push_string(&mut out, b"say \"hi\" \\o/");
        assert_eq!(text(out), r#""say \"hi\" \\o/""#);
        let mut out = Vec::new();
        push_string(&mut out, "Grüße".as_bytes());
        assert_eq!(text(out), "{7}\r\nGrüße");
        let mut out = Vec::new();
        push_nstring(&mut out, None);
        assert_eq!(text(out), "NIL");
    }

    #[test]
    fn envelope_of_simple_message() {
        let raw = b"Date: Mon, 28 Sep 2026 08:15:00 +0200\r\nFrom: Ada Lovelace <ada@lovelace.example>\r\nTo: sam@nail.example, Bob <bob@x.example>\r\nSubject: Hello\r\n there\r\nMessage-ID: <m1@x>\r\n\r\nBody";
        let mut out = Vec::new();
        push_envelope(&mut out, raw);
        assert_eq!(
            text(out),
            "(\"Mon, 28 Sep 2026 08:15:00 +0200\" \"Hello there\" \
             ((\"Ada Lovelace\" NIL \"ada\" \"lovelace.example\")) \
             ((\"Ada Lovelace\" NIL \"ada\" \"lovelace.example\")) \
             ((\"Ada Lovelace\" NIL \"ada\" \"lovelace.example\")) \
             ((NIL NIL \"sam\" \"nail.example\")(\"Bob\" NIL \"bob\" \"x.example\")) \
             NIL NIL NIL \"<m1@x>\")"
        );
    }

    #[test]
    fn number_sets() {
        assert_eq!(number_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(number_set(&[]), "");
        assert_eq!(number_list(&[4, 2]), "4,2");
    }

    #[test]
    fn envelope_of_garbage_is_all_nil() {
        let mut out = Vec::new();
        push_envelope(&mut out, b"");
        assert_eq!(text(out), "(NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL)");
    }
}
