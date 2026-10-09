//! SMTP, POP3 and ManageSieve rules: what of a command or response line is
//! logged. (IMAP has its own module; HTTP is in `http.rs`.)

use crate::Part;
use crate::imap::{CHALLENGE, VALUE};
use crate::lexer::{Piece, atoms, bytes_placeholder, pieces, render, render_masked};
use crate::redact::{REDACTED, mask_user};

/// The text of plain parts with payloads as their byte count.
fn plain(parts: &[Part<'_>]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Text(text) => out.push_str(text.trim_end_matches(['\r', '\n'])),
            Part::Bytes(n) => out.push_str(&bytes_placeholder(*n)),
        }
    }
    out
}

/// The first word, upper-cased.
fn verb(line: &str) -> String {
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
}

/// `VERB MECHANISM`, plus a redacted initial response when there is one.
fn auth_line(line: &str) -> String {
    let mut words = line.split_whitespace();
    let verb = words.next().unwrap_or_default();
    let mechanism = words.next().unwrap_or_default();
    if words.next().is_some() {
        format!("{verb} {mechanism} {REDACTED}")
    } else {
        format!("{verb} {mechanism}")
    }
}

/// SMTP: a running `AUTH` exchange hides every client line and server
/// challenge (`334`).
#[derive(Debug, Default)]
pub(crate) struct SmtpState {
    in_auth: bool,
}

impl SmtpState {
    pub(crate) fn sent(&mut self, parts: &[Part<'_>]) -> String {
        if parts.iter().any(|p| matches!(p, Part::Bytes(_))) {
            return format!("<message {}>", plain(parts));
        }
        let line = plain(parts);
        if self.in_auth {
            return REDACTED.to_owned();
        }
        if verb(&line) == "AUTH" {
            self.in_auth = true;
            return auth_line(&line);
        }
        line
    }

    pub(crate) fn received(&mut self, parts: &[Part<'_>]) -> String {
        let line = plain(parts);
        if self.in_auth {
            if line.starts_with("334") {
                return format!("334 {CHALLENGE}");
            }
            self.in_auth = false;
        }
        line
    }
}

/// POP3: `PASS`, `APOP` and `AUTH` exchanges.
#[derive(Debug, Default)]
pub(crate) struct Pop3State {
    in_auth: bool,
}

impl Pop3State {
    pub(crate) fn sent(&mut self, parts: &[Part<'_>]) -> String {
        let line = plain(parts);
        if self.in_auth {
            return REDACTED.to_owned();
        }
        let mut words = line.split_whitespace();
        let verb = words.next().unwrap_or_default().to_ascii_uppercase();
        match verb.as_str() {
            "USER" => format!("USER {}", mask_user(words.next().unwrap_or_default())),
            "PASS" => format!("PASS {REDACTED}"),
            "APOP" => format!(
                "APOP {} {REDACTED}",
                mask_user(words.next().unwrap_or_default())
            ),
            "AUTH" => {
                self.in_auth = true;
                auth_line(&line)
            }
            _ => line,
        }
    }

    pub(crate) fn received(&mut self, parts: &[Part<'_>]) -> String {
        let line = plain(parts);
        if self.in_auth {
            if line.starts_with("+ ") || line == "+" {
                return format!("+ {CHALLENGE}");
            }
            if line.starts_with("+OK") || line.starts_with("-ERR") {
                self.in_auth = false;
            }
        }
        line
    }
}

/// ManageSieve commands whose strings are script names or capability
/// values (kept).
const SIEVE_PLAIN: &[&str] = &[
    "CAPABILITY",
    "CHECKSCRIPT",
    "DELETESCRIPT",
    "GETSCRIPT",
    "HAVESPACE",
    "LISTSCRIPTS",
    "LOGOUT",
    "NOOP",
    "PUTSCRIPT",
    "RENAMESCRIPT",
    "SETACTIVE",
    "STARTTLS",
    "UNAUTHENTICATE",
];

/// ManageSieve: `AUTHENTICATE` exchanges; scripts are literals.
#[derive(Debug, Default)]
pub(crate) struct SieveState {
    in_auth: bool,
}

impl SieveState {
    pub(crate) fn sent(&mut self, parts: &[Part<'_>]) -> String {
        if self.in_auth {
            return REDACTED.to_owned();
        }
        let pieces = pieces(parts);
        let words = atoms(&pieces);
        let command = words.first().cloned().unwrap_or_default();
        if command == "AUTHENTICATE" {
            self.in_auth = true;
            let strings: Vec<&Piece<'_>> = pieces
                .iter()
                .filter(|p| !matches!(p, Piece::Gap(_)))
                .collect();
            let mechanism = match strings.get(1) {
                Some(Piece::Quoted { raw, .. }) => (*raw).to_owned(),
                Some(Piece::Atom(atom)) => (*atom).to_owned(),
                _ => String::new(),
            };
            return if strings.len() > 2 {
                format!("AUTHENTICATE {mechanism} {REDACTED}")
            } else {
                format!("AUTHENTICATE {mechanism}")
            };
        }
        if SIEVE_PLAIN.contains(&command.as_str()) {
            return render(&pieces, |_, _| None);
        }
        render_masked(&pieces, VALUE)
    }

    pub(crate) fn received(&mut self, parts: &[Part<'_>]) -> String {
        let pieces = pieces(parts);
        let words = atoms(&pieces);
        let first = words.first().map(String::as_str).unwrap_or_default();
        let status = matches!(first, "OK" | "NO" | "BYE");
        if self.in_auth {
            if !status {
                return CHALLENGE.to_owned();
            }
            self.in_auth = false;
        }
        render(&pieces, |_, _| None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn smtp_auth_login_hides_both_lines() {
        let mut state = SmtpState::default();
        assert_eq!(
            state.sent(&[Part::Text("EHLO [127.0.0.1]\r\n")]),
            "EHLO [127.0.0.1]"
        );
        assert_eq!(
            state.received(&[Part::Text("250-smtp.example.net")]),
            "250-smtp.example.net"
        );
        assert_eq!(state.sent(&[Part::Text("AUTH LOGIN\r\n")]), "AUTH LOGIN");
        assert_eq!(
            state.received(&[Part::Text("334 VXNlcm5hbWU6")]),
            "334 <challenge>"
        );
        assert_eq!(
            state.sent(&[Part::Text("c2FtQGV4YW1wbGUubmV0")]),
            "<redacted>"
        );
        assert_eq!(
            state.received(&[Part::Text("334 UGFzc3dvcmQ6")]),
            "334 <challenge>"
        );
        assert_eq!(state.sent(&[Part::Text("aHVudGVyMg==")]), "<redacted>");
        assert_eq!(
            state.received(&[Part::Text("235 2.7.0 Authentication successful")]),
            "235 2.7.0 Authentication successful"
        );
        assert_eq!(
            state.sent(&[Part::Text("MAIL FROM:<sam@example.net> SIZE=4211")]),
            "MAIL FROM:<sam@example.net> SIZE=4211",
            "addresses are masked by the final scrub"
        );
        assert_eq!(state.sent(&[Part::Bytes(4211)]), "<message {4211 bytes}>");
    }

    #[test]
    fn smtp_auth_plain_and_xoauth2() {
        let mut state = SmtpState::default();
        assert_eq!(
            state.sent(&[Part::Text("AUTH PLAIN AHNhbQBodW50ZXIy")]), // spellchecker:disable-line
            "AUTH PLAIN <redacted>"
        );
        assert_eq!(
            state.received(&[Part::Text("535 5.7.8 Authentication credentials invalid")]),
            "535 5.7.8 Authentication credentials invalid"
        );
        assert_eq!(
            state.sent(&[Part::Text("AUTH XOAUTH2 dXNlcj1hAWF1dGg9QmVhcmVyIHgBAQ==")]),
            "AUTH XOAUTH2 <redacted>"
        );
        assert_eq!(
            state.received(&[Part::Text("334 eyJzdGF0dXMiOiI0MDEifQ==")]),
            "334 <challenge>"
        );
        assert_eq!(state.sent(&[Part::Text("")]), "<redacted>");
    }

    #[test]
    fn pop3_credentials() {
        let mut state = Pop3State::default();
        assert_eq!(
            state.sent(&[Part::Text("USER sam@example.net")]),
            "USER s***@example.net"
        );
        assert_eq!(state.sent(&[Part::Text("PASS hunter2")]), "PASS <redacted>");
        assert_eq!(
            state.sent(&[Part::Text("APOP sam c4c9334bac560ecc979e58001b3e22fb")]),
            "APOP s*** <redacted>"
        );
        assert_eq!(state.sent(&[Part::Text("AUTH PLAIN")]), "AUTH PLAIN");
        assert_eq!(state.received(&[Part::Text("+ ")]), "+ <challenge>");
        assert_eq!(state.sent(&[Part::Text("AHNhbQBodW50ZXIy")]), "<redacted>"); // spellchecker:disable-line
        assert_eq!(
            state.received(&[Part::Text("+OK logged in")]),
            "+OK logged in"
        );
        assert_eq!(state.sent(&[Part::Text("RETR 3")]), "RETR 3");
        assert_eq!(state.received(&[Part::Bytes(812)]), "{812 bytes}");
    }

    #[test]
    fn sieve_auth_and_scripts() {
        let mut state = SieveState::default();
        assert_eq!(
            state.sent(&[Part::Text("AUTHENTICATE \"PLAIN\" \"AHNhbQBodW50ZXIy\"")]), // spellchecker:disable-line
            "AUTHENTICATE \"PLAIN\" <redacted>"
        );
        assert_eq!(
            state.received(&[Part::Text("OK \"Logged in.\"")]),
            "OK \"Logged in.\""
        );
        assert_eq!(
            state.sent(&[
                Part::Text("PUTSCRIPT \"nail\" {812+}\r\n"),
                Part::Bytes(812),
                Part::Text("\r\n")
            ]),
            "PUTSCRIPT \"nail\" {812 bytes}"
        );
        assert_eq!(
            state.received(&[Part::Text("\"IMPLEMENTATION\" \"Dovecot Pigeonhole\"")]),
            "\"IMPLEMENTATION\" \"Dovecot Pigeonhole\""
        );
        assert_eq!(
            state.sent(&[Part::Text("XSECRET \"abc\"")]),
            "XSECRET <value>"
        );
        assert_eq!(
            state.sent(&[Part::Text("AUTHENTICATE \"XOAUTH2\"")]),
            "AUTHENTICATE \"XOAUTH2\""
        );
        assert_eq!(state.received(&[Part::Text("\"\"")]), "<challenge>");
        assert_eq!(state.sent(&[Part::Text("\"dG9rZW4=\"")]), "<redacted>");
        assert_eq!(
            state.received(&[Part::Text("NO \"Bad token\"")]),
            "NO \"Bad token\""
        );
    }
}
