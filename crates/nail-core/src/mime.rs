//! Building outgoing plain-text messages (M1a; the full composer with HTML,
//! Markdown and attachments arrives in M1b with `nail-compose`).
//!
//! The output is a single `text/plain; charset=utf-8` part in
//! quoted-printable, with RFC 2047 encoded words for non-ASCII header text
//! and CRLF line ends throughout.

use base64::Engine as _;
use nail_api::engine::Envelope;
use nail_api::{Address, Draft, Identity};
use sha2::{Digest, Sha256};

/// Headers of the message being replied to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReplyContext {
    /// Its `Message-ID` (without angle brackets).
    pub message_id: Option<String>,
    /// Its `References` (without angle brackets), oldest first.
    pub references: Vec<String>,
}

/// A built message ready for submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutgoingMessage {
    /// SMTP envelope (sender and every recipient, Bcc included).
    pub envelope: Envelope,
    /// The RFC 5322 bytes (no `Bcc` header).
    pub raw: Vec<u8>,
    /// Its `Message-ID` (without angle brackets).
    pub message_id: String,
}

const MAX_LINE: usize = 76;

fn is_ascii_printable(text: &str) -> bool {
    text.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// RFC 2047 `B` encoded words, each at most 75 characters, split between
/// characters.
fn encoded_words(text: &str) -> String {
    // 45 bytes of UTF-8 → 60 base64 chars + 12 chars of framing = 72.
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in text.chars() {
        if chunk.len() + c.len_utf8() > 45 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(chunk);
    }
    words
        .iter()
        .map(|w| {
            format!(
                "=?utf-8?B?{}?=",
                base64::engine::general_purpose::STANDARD.encode(w.as_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join("\r\n ")
}

/// Unstructured header text (e.g. `Subject`).
fn header_text(text: &str) -> String {
    let text = text.replace(['\r', '\n'], " ");
    if is_ascii_printable(&text) && !text.contains("=?") {
        text
    } else {
        encoded_words(&text)
    }
}

/// A display name as a phrase: plain atoms, a quoted string, or encoded
/// words.
fn phrase(name: &str) -> String {
    let name = name.replace(['\r', '\n'], " ");
    if !is_ascii_printable(&name) || name.contains("=?") {
        return encoded_words(&name);
    }
    let special = |c: char| "()<>@,;:\\\".[]".contains(c);
    if name.chars().any(special) {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        name
    }
}

/// `Name <addr>` or `addr`.
pub(crate) fn format_address(name: Option<&str>, email: &str) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => format!("{} <{email}>", phrase(name)),
        None => email.to_owned(),
    }
}

/// An address-list header, folded between addresses.
fn address_header(field: &str, addresses: &[Address]) -> String {
    let mut out = format!("{field}: ");
    let mut line_len = out.len();
    for (i, a) in addresses.iter().enumerate() {
        let item = format_address(a.name.as_deref(), &a.email);
        if i > 0 {
            out.push(',');
            line_len += 1;
            if line_len + 1 + item.len() > MAX_LINE {
                out.push_str("\r\n ");
                line_len = 1;
            } else {
                out.push(' ');
                line_len += 1;
            }
        }
        line_len += item.rsplit("\r\n").next().map_or(item.len(), str::len);
        out.push_str(&item);
    }
    out.push_str("\r\n");
    out
}

/// Quoted-printable encoding (RFC 2045) of UTF-8 text with CRLF line ends.
pub(crate) fn quoted_printable(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len() + normalized.len() / 8);
    for (index, line) in normalized.split('\n').enumerate() {
        if index > 0 {
            out.push_str("\r\n");
        }
        let bytes = line.as_bytes();
        let mut current = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            let literal =
                (b == b' ' || b == b'\t') && !last || (0x21..=0x7e).contains(&b) && b != b'=';
            let token = if literal {
                (b as char).to_string()
            } else {
                format!("={b:02X}")
            };
            // Leave room for the soft line break "=".
            if current + token.len() > MAX_LINE - 1 {
                out.push_str("=\r\n");
                current = 0;
            }
            current += token.len();
            out.push_str(&token);
        }
    }
    out
}

fn angle(id: &str) -> String {
    format!("<{}>", id.trim_matches(['<', '>']))
}

/// A new `Message-ID` (without brackets) in the sender's domain.
pub(crate) fn new_message_id(sender: &str, seed: &[u8]) -> String {
    let domain = sender
        .rsplit_once('@')
        .map(|(_, d)| d)
        .filter(|d| !d.is_empty())
        .unwrap_or("nail.invalid");
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hasher.update(std::process::id().to_le_bytes());
    let mut hex = hex::encode(hasher.finalize());
    hex.truncate(32);
    format!("{hex}@{domain}")
}

/// Builds the message for `draft`, sent as `identity` at `date`
/// (RFC 2822 date text).
pub(crate) fn build_plain_text(
    draft: &Draft,
    identity: &Identity,
    reply: Option<&ReplyContext>,
    date: &str,
    message_id: &str,
) -> OutgoingMessage {
    let mut head = String::new();
    head.push_str(&format!("Date: {date}\r\n"));
    head.push_str(&format!(
        "From: {}\r\n",
        format_address(identity.name.as_deref(), &identity.email)
    ));
    if !draft.to.is_empty() {
        head.push_str(&address_header("To", &draft.to));
    }
    if !draft.cc.is_empty() {
        head.push_str(&address_header("Cc", &draft.cc));
    }
    if let Some(reply_to) = identity.reply_to.as_deref().filter(|r| !r.is_empty()) {
        head.push_str(&format!("Reply-To: {reply_to}\r\n"));
    }
    head.push_str(&format!("Subject: {}\r\n", header_text(&draft.subject)));
    head.push_str(&format!("Message-ID: {}\r\n", angle(message_id)));
    if let Some(reply) = reply
        && let Some(parent) = &reply.message_id
    {
        head.push_str(&format!("In-Reply-To: {}\r\n", angle(parent)));
        let mut refs: Vec<String> = reply.references.iter().map(|r| angle(r)).collect();
        refs.push(angle(parent));
        head.push_str(&format!("References: {}\r\n", refs.join("\r\n ")));
    }
    head.push_str("MIME-Version: 1.0\r\n");
    head.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    head.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
    head.push_str("\r\n");
    let mut raw = head.into_bytes();
    raw.extend_from_slice(quoted_printable(&draft.text).as_bytes());
    raw.extend_from_slice(b"\r\n");

    let mut to: Vec<String> = Vec::new();
    for address in draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .map(|a| a.email.trim().to_owned())
        .chain(identity.bcc.iter().map(|b| b.trim().to_owned()))
    {
        if !address.is_empty() && !to.iter().any(|t| t.eq_ignore_ascii_case(&address)) {
            to.push(address);
        }
    }
    OutgoingMessage {
        envelope: Envelope {
            from: identity.email.clone(),
            to,
        },
        raw,
        message_id: message_id.trim_matches(['<', '>']).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::ComposerMode;

    fn identity() -> Identity {
        Identity {
            id: "ada@work.example".into(),
            account_id: "work".into(),
            email: "ada@work.example".into(),
            name: Some("Ada Lovelace".into()),
            reply_to: None,
            bcc: vec!["archive@work.example".into()],
            signature: None,
            composer_mode: None,
        }
    }

    fn draft(subject: &str, text: &str) -> Draft {
        Draft {
            id: None,
            identity_id: "ada@work.example".into(),
            to: vec![Address {
                name: Some("Bob, the Builder".into()),
                email: "bob@example.org".into(),
            }],
            cc: vec![Address {
                name: None,
                email: "carol@example.org".into(),
            }],
            bcc: vec![Address {
                name: None,
                email: "dave@example.org".into(),
            }],
            subject: subject.into(),
            mode: ComposerMode::Plain,
            text: text.into(),
            html: None,
            markdown: None,
            attachments: vec![],
            in_reply_to: None,
        }
    }

    #[test]
    fn builds_a_parsable_message() {
        let d = draft("Grüße aus Wien", "Hallo Bob,\nschöne Grüße!\n\n-- \nAda");
        let out = build_plain_text(
            &d,
            &identity(),
            None,
            "Tue, 29 Sep 2026 10:00:00 +0200",
            "abc@work.example",
        );
        let text = String::from_utf8(out.raw.clone()).unwrap();
        assert!(!text.contains("Bcc"), "{text}");
        assert!(text.contains("To: \"Bob, the Builder\" <bob@example.org>\r\n"));
        assert!(text.contains("Message-ID: <abc@work.example>\r\n"));
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "bare LF in {text:?}"
        );
        for line in text.split("\r\n") {
            assert!(line.len() <= 998);
            assert!(line.is_ascii(), "{line}");
        }

        let parsed = nail_mime::parse_message(&out.raw);
        assert_eq!(parsed.headers.subject, "Grüße aus Wien");
        assert_eq!(parsed.headers.from[0].email, "ada@work.example");
        assert_eq!(parsed.headers.from[0].name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(
            parsed.headers.to[0].name.as_deref(),
            Some("Bob, the Builder")
        );
        assert_eq!(parsed.headers.cc[0].email, "carol@example.org");
        assert_eq!(
            parsed.text.trim_end().replace("\r\n", "\n"),
            "Hallo Bob,\nschöne Grüße!\n\n-- \nAda"
        );
        assert_eq!(
            out.envelope.to,
            [
                "bob@example.org",
                "carol@example.org",
                "dave@example.org",
                "archive@work.example"
            ]
        );
        assert_eq!(out.envelope.from, "ada@work.example");
    }

    #[test]
    fn replies_carry_references() {
        let reply = ReplyContext {
            message_id: Some("parent@x".into()),
            references: vec!["root@x".into()],
        };
        let out = build_plain_text(
            &draft("Re: Hi", "ok"),
            &identity(),
            Some(&reply),
            "Tue, 29 Sep 2026 10:00:00 +0000",
            "<c@work.example>",
        );
        let parsed = nail_mime::parse_message(&out.raw);
        assert_eq!(parsed.in_reply_to, ["parent@x"]);
        assert_eq!(parsed.references, ["root@x", "parent@x"]);
        assert_eq!(out.message_id, "c@work.example");
    }

    #[test]
    fn quoted_printable_wraps_and_escapes() {
        let long = "x".repeat(200);
        let qp = quoted_printable(&format!("{long}\na=b \nä"));
        for line in qp.split("\r\n") {
            assert!(line.len() <= MAX_LINE, "{line}");
        }
        assert!(qp.contains("a=3Db=20"));
        assert!(qp.ends_with("=C3=A4"));
    }

    #[test]
    fn long_subjects_become_several_encoded_words() {
        let subject = "ä".repeat(60);
        let encoded = header_text(&subject);
        assert!(encoded.split("\r\n ").all(|w| w.len() <= 75));
        let raw = format!("Subject: {encoded}\r\n\r\nx");
        assert_eq!(
            nail_mime::parse_message(raw.as_bytes()).headers.subject,
            subject
        );
        assert_eq!(header_text("plain"), "plain");
    }

    #[test]
    fn message_ids_use_the_sender_domain() {
        let id = new_message_id("ada@work.example", b"x");
        assert!(id.ends_with("@work.example"));
        assert_ne!(id, new_message_id("ada@work.example", b"x"));
        assert!(new_message_id("nobody", b"").ends_with("@nail.invalid"));
    }
}
