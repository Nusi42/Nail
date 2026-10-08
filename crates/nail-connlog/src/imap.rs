//! IMAP rules: what of a command or response line is logged.

use std::borrow::Cow;

use crate::Part;
use crate::lexer::{Piece, atoms, pieces, render};
use crate::redact::{REDACTED, mask_user};

/// Per-connection state: a running `AUTHENTICATE` exchange.
#[derive(Debug, Default)]
pub(crate) struct ImapState {
    /// The tag of the running `AUTHENTICATE`.
    auth_tag: Option<String>,
}

/// Commands whose quoted strings are folder names, flags, dates or `ID`
/// values (kept).
const PLAIN_COMMANDS: &[&str] = &[
    "APPEND",
    "CAPABILITY",
    "CHECK",
    "CLOSE",
    "COMPRESS",
    "COPY",
    "CREATE",
    "DELETE",
    "ENABLE",
    "EXAMINE",
    "EXPUNGE",
    "FETCH",
    "GETQUOTA",
    "GETQUOTAROOT",
    "ID",
    "IDLE",
    "LIST",
    "LOGOUT",
    "LSUB",
    "MOVE",
    "NAMESPACE",
    "NOOP",
    "RENAME",
    "SELECT",
    "STARTTLS",
    "STATUS",
    "STORE",
    "SUBSCRIBE",
    "UNSELECT",
    "UNSUBSCRIBE",
    "XLIST",
];

/// Search keys and modifiers kept in `SEARCH` / `SORT` / `THREAD` commands;
/// every other atom, string or literal is a search term.
const SEARCH_KEYS: &[&str] = &[
    "ALL",
    "ANSWERED",
    "BCC",
    "BEFORE",
    "BODY",
    "CC",
    "CHARSET",
    "COUNT",
    "DELETED",
    "DRAFT",
    "FLAGGED",
    "FROM",
    "HEADER",
    "KEYWORD",
    "LARGER",
    "MAX",
    "MIN",
    "MODSEQ",
    "NEW",
    "NOT",
    "OLD",
    "ON",
    "OR",
    "RECENT",
    "RETURN",
    "SAVE",
    "SEEN",
    "SENTBEFORE",
    "SENTON",
    "SENTSINCE",
    "SINCE",
    "SMALLER",
    "SUBJECT",
    "TEXT",
    "TO",
    "UID",
    "UNANSWERED",
    "UNDELETED",
    "UNDRAFT",
    "UNFLAGGED",
    "UNKEYWORD",
    "UNSEEN",
    "US-ASCII",
    "UTF-8",
    "X-GM-RAW",
    "X-GM-MSGID",
    "X-GM-THRID",
    "$",
];

/// Untagged responses whose quoted strings are kept (status texts, folder
/// names, capabilities, server identity).
const PLAIN_RESPONSES: &[&str] = &[
    "BAD",
    "BYE",
    "CAPABILITY",
    "ENABLED",
    "ESEARCH",
    "EXISTS",
    "EXPUNGE",
    "FLAGS",
    "ID",
    "LIST",
    "LSUB",
    "NAMESPACE",
    "NO",
    "OK",
    "PREAUTH",
    "QUOTA",
    "QUOTAROOT",
    "RECENT",
    "SEARCH",
    "SORT",
    "STATUS",
    "THREAD",
    "VANISHED",
    "XLIST",
];

/// The placeholder of a search value.
pub const SEARCH_TERM: &str = "<search term>";
/// The placeholder of a header value inside a `FETCH` response.
pub const HEADER: &str = "<header>";
/// The placeholder of another value Nail does not know to be harmless.
pub const VALUE: &str = "<value>";
/// The placeholder of a SASL challenge.
pub const CHALLENGE: &str = "<challenge>";

fn is_sequence_set(atom: &str) -> bool {
    !atom.is_empty()
        && atom
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b':' | b',' | b'*'))
}

fn is_date(atom: &str) -> bool {
    let mut parts = atom.split('-');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(d), Some(m), Some(y), None)
            if d.bytes().all(|b| b.is_ascii_digit())
                && m.len() == 3
                && y.bytes().all(|b| b.is_ascii_digit())
    )
}

impl ImapState {
    /// A line Nail sent: a command, a SASL answer or `DONE`.
    pub(crate) fn sent(&mut self, parts: &[Part<'_>]) -> String {
        let pieces = pieces(parts);
        if self.auth_tag.is_some() {
            return REDACTED.to_owned();
        }
        let words = atoms(&pieces);
        if words.first().map(String::as_str) == Some("DONE") && words.len() == 1 {
            return "DONE".to_owned();
        }
        let tag = words.first().cloned().unwrap_or_default();
        let mut command = words.get(1).cloned().unwrap_or_default();
        if command == "UID" {
            command = words.get(2).cloned().unwrap_or_default();
        }
        let tag_text = match pieces.first() {
            Some(Piece::Atom(tag)) => *tag,
            _ => "",
        };
        match command.as_str() {
            "LOGIN" => {
                let user = pieces
                    .iter()
                    .filter(|p| !matches!(p, Piece::Gap(_)))
                    .nth(2)
                    .map_or_else(String::new, |p| match p {
                        Piece::Atom(user) => mask_user(user),
                        Piece::Quoted { value, .. } => format!("\"{}\"", mask_user(value)),
                        _ => "<user>".to_owned(),
                    });
                format!("{tag_text} LOGIN {user} {REDACTED}")
            }
            "AUTHENTICATE" => {
                self.auth_tag = Some(tag);
                let mechanism = words.get(2).cloned().unwrap_or_default();
                let initial = pieces
                    .iter()
                    .filter(|p| !matches!(p, Piece::Gap(_)))
                    .count()
                    > 3;
                if initial {
                    format!("{tag_text} AUTHENTICATE {mechanism} {REDACTED}")
                } else {
                    format!("{tag_text} AUTHENTICATE {mechanism}")
                }
            }
            "SEARCH" | "SORT" | "THREAD" | "ESEARCH" => {
                let start = if words.get(1).is_some_and(|w| w == "UID") {
                    3
                } else {
                    2
                };
                let mut atom_index = 0usize;
                render(&pieces, |_, piece| match piece {
                    Piece::Atom(atom) => {
                        let keep = atom_index < start
                            || SEARCH_KEYS.contains(&atom.to_ascii_uppercase().as_str())
                            || is_sequence_set(atom)
                            || is_date(atom);
                        atom_index += 1;
                        (!keep).then_some(Cow::Borrowed(SEARCH_TERM))
                    }
                    Piece::Quoted { .. } | Piece::Literal(_) => Some(Cow::Borrowed(SEARCH_TERM)),
                    Piece::Gap(_) => None,
                })
            }
            plain if PLAIN_COMMANDS.contains(&plain) => render(&pieces, |_, _| None),
            _ => render(&pieces, |_, piece| match piece {
                Piece::Quoted { .. } => Some(Cow::Borrowed(VALUE)),
                _ => None,
            }),
        }
    }

    /// A response line (literals as [`Part::Bytes`]).
    pub(crate) fn received(&mut self, parts: &[Part<'_>]) -> String {
        let pieces = pieces(parts);
        let words = atoms(&pieces);
        let first = words.first().map(String::as_str).unwrap_or_default();
        if first == "+" {
            if self.auth_tag.is_some() && words.len() > 1 {
                return format!("+ {CHALLENGE}");
            }
            return render(&pieces, |_, _| None);
        }
        if first != "*" {
            // Tagged: the server's status text stays.
            if self
                .auth_tag
                .as_deref()
                .is_some_and(|tag| tag.eq_ignore_ascii_case(first))
            {
                self.auth_tag = None;
            }
            return render(&pieces, |_, _| None);
        }
        let kind = match words.get(1) {
            Some(n) if n.bytes().all(|b| b.is_ascii_digit()) => words.get(2),
            other => other,
        }
        .cloned()
        .unwrap_or_default();
        if kind == "FETCH" {
            let mut previous: Option<String> = None;
            return render(&pieces, |_, piece| {
                let replacement = match piece {
                    Piece::Quoted { .. } if previous.as_deref() != Some("INTERNALDATE") => {
                        Some(Cow::Borrowed(HEADER))
                    }
                    _ => None,
                };
                if let Some(atom) = piece.upper_atom() {
                    previous = Some(atom);
                } else if !matches!(piece, Piece::Gap(_)) {
                    previous = None;
                }
                replacement
            });
        }
        if PLAIN_RESPONSES.contains(&kind.as_str()) {
            return render(&pieces, |_, _| None);
        }
        render(&pieces, |_, piece| match piece {
            Piece::Quoted { .. } => Some(Cow::Borrowed(VALUE)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn sent(state: &mut ImapState, line: &str) -> String {
        state.sent(&[Part::Text(line)])
    }

    fn received(state: &mut ImapState, line: &str) -> String {
        state.received(&[Part::Text(line)])
    }

    #[test]
    fn login_with_quoted_strings_and_literals() {
        let mut state = ImapState::default();
        assert_eq!(
            sent(&mut state, "A0001 LOGIN \"sam@example.net\" \"hunter2\"\r\n"),
            "A0001 LOGIN \"s***@example.net\" <redacted>"
        );
        let literal = state.sent(&[
            Part::Text("A0002 LOGIN sam "),
            Part::Bytes(9),
            Part::Text("\r\n"),
        ]);
        assert_eq!(literal, "A0002 LOGIN s*** <redacted>");
    }

    #[test]
    fn authenticate_hides_every_answer() {
        let mut state = ImapState::default();
        assert_eq!(
            sent(&mut state, "A0001 AUTHENTICATE PLAIN AHNhbQBodW50ZXIy"),
            "A0001 AUTHENTICATE PLAIN <redacted>"
        );
        assert_eq!(
            received(&mut state, "+ eyJzdGF0dXMiOiI0MDEifQ=="),
            "+ <challenge>"
        );
        assert_eq!(sent(&mut state, ""), "<redacted>");
        assert_eq!(
            received(
                &mut state,
                "A0001 NO [AUTHENTICATIONFAILED] Invalid credentials"
            ),
            "A0001 NO [AUTHENTICATIONFAILED] Invalid credentials"
        );
        // XOAUTH2 without an initial response: the token goes in the answer.
        assert_eq!(
            sent(&mut state, "A0002 AUTHENTICATE XOAUTH2"),
            "A0002 AUTHENTICATE XOAUTH2"
        );
        assert_eq!(received(&mut state, "+ "), "+");
        assert_eq!(
            sent(&mut state, "dXNlcj1zYW0BYXV0aD1CZWFyZXIgeWEyOS5hAQE="),
            "<redacted>"
        );
        received(&mut state, "A0002 OK Logged in");
        assert_eq!(sent(&mut state, "A0003 NOOP"), "A0003 NOOP");
    }

    #[test]
    fn fetch_responses_keep_structure_only() {
        let mut state = ImapState::default();
        let line = state.received(&[
            Part::Text(
                "* 151 FETCH (UID 151 FLAGS (\\Seen $Forwarded) INTERNALDATE \"28-Sep-2026 08:15:00 +0200\" \
                 RFC822.SIZE 4211 ENVELOPE (\"Mon, 28 Sep 2026 08:15:00 +0200\" \"Quarterly numbers\" \
                 ((\"Ada Lovelace\" NIL \"ada\" \"example.org\")) NIL NIL ((NIL NIL \"sam\" \"example.net\")) \
                 NIL NIL NIL \"<abc@example.org>\") BODY[] {4211}\r\n",
            ),
            Part::Bytes(4211),
            Part::Text(")\r\n"),
        ]);
        assert!(
            line.starts_with("* 151 FETCH (UID 151 FLAGS (\\Seen $Forwarded)"),
            "{line}"
        );
        assert!(
            line.contains("INTERNALDATE \"28-Sep-2026 08:15:00 +0200\""),
            "{line}"
        );
        assert!(line.contains("BODY[] {4211 bytes})"), "{line}");
        for content in ["Quarterly", "Ada", "ada", "example.org", "example.net", "abc@"] {
            assert!(!line.contains(content), "{content} in {line}");
        }
        let bodystructure = received(
            &mut state,
            "* 3 FETCH (UID 9 BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\" \"NAME\" \"salary.txt\") NIL NIL \"7BIT\" 12 1))",
        );
        assert!(!bodystructure.contains("salary"), "{bodystructure}");
    }

    #[test]
    fn search_terms_are_hidden() {
        let mut state = ImapState::default();
        assert_eq!(
            sent(
                &mut state,
                "A0007 UID SEARCH HEADER Message-ID \"<abc@example.org>\""
            ),
            "A0007 UID SEARCH HEADER <search term> <search term>"
        );
        assert_eq!(
            sent(
                &mut state,
                "A0008 UID SEARCH UID 151:* SINCE 1-Oct-2026 NOT DELETED"
            ),
            "A0008 UID SEARCH UID 151:* SINCE 1-Oct-2026 NOT DELETED"
        );
        let literal = state.sent(&[
            Part::Text("A0009 UID SEARCH CHARSET UTF-8 SUBJECT {7+}\r\n"),
            Part::Bytes(7),
        ]);
        assert_eq!(
            literal,
            "A0009 UID SEARCH CHARSET UTF-8 SUBJECT <search term>"
        );
        assert_eq!(
            sent(&mut state, "A0010 SEARCH TEXT secretword"),
            "A0010 SEARCH TEXT <search term>"
        );
    }

    #[test]
    fn mailbox_commands_and_appends_keep_names() {
        let mut state = ImapState::default();
        assert_eq!(
            sent(&mut state, "A0004 SELECT \"INBOX\" (CONDSTORE)"),
            "A0004 SELECT \"INBOX\" (CONDSTORE)"
        );
        let append = state.sent(&[
            Part::Text("A0005 APPEND \"Sent Items\" (\\Seen) "),
            Part::Bytes(4211),
            Part::Text(""),
        ]);
        assert_eq!(append, "A0005 APPEND \"Sent Items\" (\\Seen) {4211 bytes}");
        assert_eq!(
            sent(&mut state, "A0006 XFOO \"private\""),
            "A0006 XFOO <value>"
        );
    }

    #[test]
    fn untagged_responses() {
        let mut state = ImapState::default();
        assert_eq!(
            received(&mut state, "* LIST (\\HasNoChildren) \"/\" \"Receipts\""),
            "* LIST (\\HasNoChildren) \"/\" \"Receipts\""
        );
        assert_eq!(
            received(&mut state, "* OK [UIDVALIDITY 1700000000] UIDs valid"),
            "* OK [UIDVALIDITY 1700000000] UIDs valid"
        );
        assert_eq!(
            received(&mut state, "* XUNKNOWN \"who knows\""),
            "* XUNKNOWN <value>"
        );
        assert_eq!(
            received(&mut state, "A0012 NO [SERVERBUG] Internal error occurred."),
            "A0012 NO [SERVERBUG] Internal error occurred."
        );
    }
}
