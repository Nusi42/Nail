//! Header reading for MIME uploads: the `Message-ID` of an appended
//! message (idempotent appends), the recipients of a message sent with
//! `sendMail` (Graph takes them from the headers), and a `Bcc` header for
//! envelope recipients that are missing there.

use crate::error::GraphError;

/// The header section of a raw message (up to the first empty line).
fn header_section(raw: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < raw.len() {
        if raw[i..].starts_with(b"\r\n\r\n") {
            return &raw[..i + 2];
        }
        if raw[i..].starts_with(b"\n\n") {
            return &raw[..=i];
        }
        i += 1;
    }
    raw
}

/// Every header as (lowercase name, unfolded value).
pub fn headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(header_section(raw));
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
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

/// The first value of header `name` (case-insensitive).
pub fn header(raw: &[u8], name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    headers(raw)
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v)
}

/// The `Message-ID` with its angle brackets (`<id@host>`).
pub fn message_id(raw: &[u8]) -> Option<String> {
    let value = header(raw, "message-id")?;
    let start = value.find('<')?;
    let end = value[start..].find('>')? + start;
    let id = &value[start..=end];
    (id.len() > 2 && !id.contains(['\'', ' '])).then(|| id.to_owned())
}

/// The addresses in an address list header value (lowercase).
pub fn addresses(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut part = String::new();
    let mut quoted = false;
    let mut depth = 0u32;
    let mut parts = Vec::new();
    for c in value.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' | ';' if !quoted && depth == 0 => {
                parts.push(std::mem::take(&mut part));
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    parts.push(part);
    for part in parts {
        let address = match (part.rfind('<'), part.rfind('>')) {
            (Some(s), Some(e)) if s < e => part[s + 1..e].trim().to_owned(),
            _ => part
                .split_whitespace()
                .find(|w| w.contains('@'))
                .unwrap_or_default()
                .trim_matches(['"', '(', ')'])
                .to_owned(),
        };
        if address.contains('@') {
            out.push(address.to_ascii_lowercase());
        }
    }
    out
}

/// The recipients in `To`, `Cc` and `Bcc` (lowercase).
pub fn header_recipients(raw: &[u8]) -> Vec<String> {
    headers(raw)
        .into_iter()
        .filter(|(n, _)| matches!(n.as_str(), "to" | "cc" | "bcc"))
        .flat_map(|(_, v)| addresses(&v))
        .collect()
}

/// Whether `address` can go into a header as it is (no header injection).
pub fn is_plain_address(address: &str) -> bool {
    !address.is_empty()
        && address.len() <= 320
        && address.contains('@')
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "<>,;:\"()[]\\".contains(c))
}

/// `raw` with a `Bcc` header naming the `envelope` recipients that are not
/// in its `To` / `Cc` / `Bcc` headers (Graph takes recipients from the
/// headers and does not deliver the `Bcc` header itself).
pub fn with_envelope_bcc(raw: &[u8], envelope: &[String]) -> Result<Vec<u8>, GraphError> {
    let present = header_recipients(raw);
    let mut missing: Vec<String> = Vec::new();
    for address in envelope {
        let lower = address.to_ascii_lowercase();
        if present.contains(&lower) || missing.contains(&lower) {
            continue;
        }
        if !is_plain_address(address) {
            return Err(GraphError::Unsupported(format!(
                "recipient address {address:?}"
            )));
        }
        missing.push(lower);
    }
    if missing.is_empty() {
        return Ok(raw.to_vec());
    }
    let mut out = format!("Bcc: {}\r\n", missing.join(", ")).into_bytes();
    out.extend_from_slice(raw);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = b"From: Ada <ada@contoso.example>\r\nTo: \"Lovelace, Bob\" <Bob@Contoso.example>,\r\n carol@contoso.example\r\nCc: (team) dave@contoso.example\r\nMessage-ID:\r\n <abc.123@contoso.example>\r\nSubject: Hi\r\n\r\nTo: not@header.example\r\n";

    #[test]
    fn reads_headers() {
        assert_eq!(header(RAW, "subject").as_deref(), Some("Hi"));
        assert_eq!(
            message_id(RAW).as_deref(),
            Some("<abc.123@contoso.example>")
        );
        assert_eq!(
            header_recipients(RAW),
            [
                "bob@contoso.example",
                "carol@contoso.example",
                "dave@contoso.example"
            ]
        );
        assert_eq!(message_id(b"Subject: x\n\nbody"), None);
        assert_eq!(message_id(b"Message-ID: <a' or 1=1>\n\n"), None);
        assert_eq!(
            header(b"Subject: unix\n\nx", "subject").as_deref(),
            Some("unix")
        );
    }

    #[test]
    fn envelope_recipients_become_bcc() {
        let out = with_envelope_bcc(
            RAW,
            &[
                "bob@contoso.example".into(),
                "Eve@Contoso.example".into(),
                "eve@contoso.example".into(),
            ],
        )
        .unwrap();
        assert!(out.starts_with(b"Bcc: eve@contoso.example\r\nFrom: Ada"));
        assert_eq!(
            with_envelope_bcc(RAW, &["carol@contoso.example".into()]).unwrap(),
            RAW
        );
        assert!(matches!(
            with_envelope_bcc(RAW, &["x@y.example\r\nBcc: z@q.example".into()]),
            Err(GraphError::Unsupported(_))
        ));
        assert!(!is_plain_address("a b@c"));
        assert!(is_plain_address("a+b@c.example"));
    }
}
