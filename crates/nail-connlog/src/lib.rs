//! The connection log (ADR 0190): the one redactor of the workspace, the
//! recorders protocol clients log through, the core's in-memory rings and
//! the rotating log files.
//!
//! * [`redact`]: text rules shared with the application log and the
//!   diagnostics bundle (secrets, addresses, subjects, user paths);
//! * protocol rules for IMAP, SMTP, POP3, ManageSieve (`imap.rs`,
//!   `lines.rs`) and HTTP ([`http`]) — what of a line stays;
//! * [`WireTap`] / [`ConnTap`] / [`HttpTap`]: what protocol clients call;
//!   they pass [`Part`]s (text and literal *sizes*, never literal bytes) and
//!   get the rules applied here;
//! * [`ConnectionLogs`]: the per-scope rings, the disk mirror and change
//!   notices; [`RotatingFile`] for `app.log` and the disk mirror.
//!
//! Safe by default: nothing is written to disk unless the user asks, and
//! nothing a protocol client hands in is recorded before the rules ran.

pub mod files;
pub mod http;
mod imap;
mod lexer;
mod lines;
pub mod redact;
mod store;
mod tap;
pub mod tls;

pub use files::RotatingFile;
pub use imap::{CHALLENGE, HEADER, SEARCH_TERM, VALUE};
pub use nail_api::connlog::{WireDirection, WireLevel, WireProtocol};
pub use store::{ConnectionLogs, export_name, file_name};
pub use tap::{ConnTap, HttpExchange, HttpTap, WireRecord, WireSink, WireTap};

/// A piece of a line handed to a recorder: text, or the size of a literal
/// / payload whose bytes are never handed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part<'a> {
    /// Protocol text (a command, response line, part of one).
    Text(&'a str),
    /// A literal or payload of this many bytes.
    Bytes(usize),
}

#[cfg(test)]
mod properties {
    //! No logged line keeps a registered secret, a subject, a name or a
    //! full address, whatever the protocol and wherever they appear.

    use std::sync::Arc;

    use proptest::prelude::*;

    use super::*;
    use crate::tap::tests::Collect;

    fn secret() -> impl Strategy<Value = String> {
        "[A-Za-z0-9!#%&*+=?^_~.-]{4,24}"
    }

    fn word() -> impl Strategy<Value = String> {
        "[A-Za-z][a-z]{3,10}"
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn secrets_never_survive(
            password in secret(),
            token in secret(),
            user in word(),
            subject in word(),
            name in word(),
            uid in 1u32..100_000,
        ) {
            let sink = Arc::new(Collect::default());
            let tap = WireTap::new(sink.clone());
            tap.add_secret(&password);
            tap.add_secret(&token);
            let address = format!("{user}@{name}.example");
            let imap = tap.connect(WireProtocol::Imap, "imap.example.org", 993);
            imap.sent(&[Part::Text(&format!("A0001 LOGIN \"{address}\" \"{password}\""))]);
            imap.sent(&[Part::Text(&format!("A0002 LOGIN {user} {password}"))]);
            imap.received(&[Part::Text(&format!("A0002 NO [AUTHENTICATIONFAILED] {address} {password} {token}"))]);
            imap.sent(&[Part::Text("A0003 AUTHENTICATE XOAUTH2")]);
            imap.sent(&[Part::Text(&format!("user={address}\u{1}auth=Bearer {token}\u{1}\u{1}"))]);
            imap.received(&[Part::Text("A0003 OK")]);
            imap.sent(&[Part::Text(&format!("A0004 UID SEARCH SUBJECT \"{subject}\" FROM {name}"))]);
            imap.received(&[
                Part::Text(&format!(
                    "* {uid} FETCH (UID {uid} ENVELOPE (NIL \"{subject}\" ((\"{name}\" NIL \"{user}\" \"{name}.example\")) NIL NIL NIL NIL NIL NIL NIL) BODY[] {{12}}\r\n"
                )),
                Part::Bytes(12),
                Part::Text(")\r\n"),
            ]);
            let smtp = tap.connect(WireProtocol::Smtp, "smtp.example.org", 465);
            smtp.sent(&[Part::Text("AUTH LOGIN")]);
            smtp.received(&[Part::Text("334 VXNlcm5hbWU6")]);
            smtp.sent(&[Part::Text(&user)]);
            smtp.sent(&[Part::Text(&password)]);
            smtp.received(&[Part::Text(&format!("535 {user} {password} rejected"))]);
            smtp.sent(&[Part::Text(&format!("MAIL FROM:<{address}>"))]);
            let pop = tap.connect(WireProtocol::Pop3, "pop.example.org", 995);
            pop.sent(&[Part::Text(&format!("USER {address}"))]);
            pop.sent(&[Part::Text(&format!("PASS {password}"))]);
            let http = tap.http(WireProtocol::Graph);
            let exchange = http.request(
                "GET",
                &format!("https://graph.example/v1.0/users/{address}/messages?$search=\"{subject}\"&access_token={token}"),
                true,
                None,
            );
            exchange.response(401, Some(10), Some(format!(r#"{{"error":"invalid_token","error_description":"{token} for {address}"}}"#).as_bytes()));
            drop((imap, smtp, pop));
            for text in sink.texts() {
                prop_assert!(!text.contains(&password), "password in {text}");
                prop_assert!(!text.contains(&token), "token in {text}");
                prop_assert!(!text.contains(&address), "address in {text}");
                prop_assert!(!text.contains(&format!("\"{subject}\"")), "subject in {text}");
                prop_assert!(!text.contains(&format!("\"{name}\" NIL")), "name in {text}");
            }
        }
    }
}
