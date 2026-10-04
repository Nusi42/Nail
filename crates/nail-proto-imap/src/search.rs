//! Server-side search (`UID SEARCH`; M2c WP53): the fallback for messages
//! whose body is not stored locally (sync policies "headers only" /
//! "recent N days"). The core turns a query of the search language into a
//! [`SearchKey`] (`nail_search::imap::search_key`), runs it per folder with
//! [`ImapClient::search_messages`] and merges the hits into the local
//! results.
//!
//! Strings are sent quoted when they are printable ASCII and as literals
//! otherwise, with `CHARSET UTF-8` (RFC 3501 6.4.4) whenever a literal
//! carries non-ASCII text. IMAP matches strings as case-insensitive
//! substrings; `TEXT` covers headers and body.

use std::borrow::Cow;
use std::fmt::Write as _;

use imap_proto::Response;
use imap_proto::types::MailboxDatum;

use crate::bodies::ImapDate;
use crate::client::{ImapClient, Part};
use crate::error::{ImapError, Result};

/// An IMAP search key (RFC 3501 6.4.4), as a tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearchKey {
    /// Every message.
    All,
    /// Header or body contains the text.
    Text(String),
    /// Body contains the text.
    Body(String),
    /// `Subject` contains the text.
    Subject(String),
    /// `From` contains the text.
    From(String),
    /// `To` contains the text.
    To(String),
    /// `Cc` contains the text.
    Cc(String),
    /// A header field contains the text (empty: the field exists).
    Header(String, String),
    /// The keyword (tag) is set.
    Keyword(String),
    /// The keyword is not set.
    Unkeyword(String),
    /// `\Seen`.
    Seen,
    /// Not `\Seen`.
    Unseen,
    /// `\Flagged`.
    Flagged,
    /// Not `\Flagged`.
    Unflagged,
    /// `\Answered`.
    Answered,
    /// Not `\Answered`.
    Unanswered,
    /// `\Draft`.
    Draft,
    /// Arrived (`INTERNALDATE`) on or after the day.
    Since(ImapDate),
    /// Arrived before the day.
    Before(ImapDate),
    /// `Date:` header on or after the day.
    SentSince(ImapDate),
    /// `Date:` header before the day.
    SentBefore(ImapDate),
    /// Larger than this many bytes.
    Larger(u32),
    /// Smaller than this many bytes.
    Smaller(u32),
    /// Not matching the inner key.
    Not(Box<SearchKey>),
    /// Matching either key.
    Or(Box<SearchKey>, Box<SearchKey>),
    /// Matching every key (an empty list matches everything).
    And(Vec<SearchKey>),
}

/// One piece of an encoded key: command text or a literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Literal(Vec<u8>),
}

impl SearchKey {
    /// `Or` over several keys (`All` for none, the key itself for one).
    pub fn any(keys: Vec<SearchKey>) -> Self {
        let mut keys = keys.into_iter().rev();
        match keys.next() {
            None => Self::Not(Box::new(Self::All)),
            Some(last) => keys.fold(last, |acc, key| Self::Or(Box::new(key), Box::new(acc))),
        }
    }

    /// Whether any string of the key needs `CHARSET UTF-8`.
    pub fn needs_utf8(&self) -> bool {
        match self {
            Self::Text(s)
            | Self::Body(s)
            | Self::Subject(s)
            | Self::From(s)
            | Self::To(s)
            | Self::Cc(s) => !s.is_ascii(),
            Self::Header(name, value) => !name.is_ascii() || !value.is_ascii(),
            Self::Not(inner) => inner.needs_utf8(),
            Self::Or(a, b) => a.needs_utf8() || b.needs_utf8(),
            Self::And(keys) => keys.iter().any(Self::needs_utf8),
            _ => false,
        }
    }

    /// The key as sent, with literals shown as `{n}` + their text (logs
    /// and tests).
    pub fn to_command(&self) -> Result<String> {
        let mut out = String::new();
        for piece in self.pieces()? {
            match piece {
                Piece::Text(text) => out.push_str(&text),
                Piece::Literal(bytes) => {
                    let _ = write!(out, "{{{}}}", bytes.len());
                    out.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
        }
        Ok(out)
    }

    /// The encoded key. The criteria of a command are a sequence of keys
    /// that must all match, so a top-level `And` needs no parentheses
    /// (some servers do not take a parenthesized list alone).
    pub(crate) fn pieces(&self) -> Result<Vec<Piece>> {
        let mut out = Vec::new();
        match self {
            Self::And(keys) if keys.len() > 1 => {
                for (i, key) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(Piece::Text(" ".to_owned()));
                    }
                    key.encode(&mut out)?;
                }
            }
            other => other.encode(&mut out)?,
        }
        // Merge adjacent text pieces.
        let mut merged: Vec<Piece> = Vec::with_capacity(out.len());
        for piece in out {
            match (merged.last_mut(), piece) {
                (Some(Piece::Text(last)), Piece::Text(text)) => last.push_str(&text),
                (_, piece) => merged.push(piece),
            }
        }
        Ok(merged)
    }

    fn encode(&self, out: &mut Vec<Piece>) -> Result<()> {
        let word = |out: &mut Vec<Piece>, w: &str| out.push(Piece::Text(w.to_owned()));
        let with_string = |out: &mut Vec<Piece>, key: &str, value: &str| -> Result<()> {
            out.push(Piece::Text(format!("{key} ")));
            string(out, value)
        };
        match self {
            Self::All => word(out, "ALL"),
            Self::Text(s) => with_string(out, "TEXT", s)?,
            Self::Body(s) => with_string(out, "BODY", s)?,
            Self::Subject(s) => with_string(out, "SUBJECT", s)?,
            Self::From(s) => with_string(out, "FROM", s)?,
            Self::To(s) => with_string(out, "TO", s)?,
            Self::Cc(s) => with_string(out, "CC", s)?,
            Self::Header(name, value) => {
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_graphic() && b != b':') {
                    return Err(ImapError::InvalidArgument(format!(
                        "{name:?} is not a header field name"
                    )));
                }
                out.push(Piece::Text("HEADER ".to_owned()));
                string(out, name)?;
                out.push(Piece::Text(" ".to_owned()));
                string(out, value)?;
            }
            Self::Keyword(k) => out.push(Piece::Text(format!("KEYWORD {}", atom(k)?))),
            Self::Unkeyword(k) => out.push(Piece::Text(format!("UNKEYWORD {}", atom(k)?))),
            Self::Seen => word(out, "SEEN"),
            Self::Unseen => word(out, "UNSEEN"),
            Self::Flagged => word(out, "FLAGGED"),
            Self::Unflagged => word(out, "UNFLAGGED"),
            Self::Answered => word(out, "ANSWERED"),
            Self::Unanswered => word(out, "UNANSWERED"),
            Self::Draft => word(out, "DRAFT"),
            Self::Since(d) => out.push(Piece::Text(format!("SINCE {d}"))),
            Self::Before(d) => out.push(Piece::Text(format!("BEFORE {d}"))),
            Self::SentSince(d) => out.push(Piece::Text(format!("SENTSINCE {d}"))),
            Self::SentBefore(d) => out.push(Piece::Text(format!("SENTBEFORE {d}"))),
            Self::Larger(n) => out.push(Piece::Text(format!("LARGER {n}"))),
            Self::Smaller(n) => out.push(Piece::Text(format!("SMALLER {n}"))),
            Self::Not(inner) => {
                word(out, "NOT ");
                inner.encode(out)?;
            }
            Self::Or(a, b) => {
                word(out, "OR ");
                a.encode(out)?;
                word(out, " ");
                b.encode(out)?;
            }
            Self::And(keys) => match keys.as_slice() {
                [] => word(out, "ALL"),
                [only] => only.encode(out)?,
                keys => {
                    word(out, "(");
                    for (i, key) in keys.iter().enumerate() {
                        if i > 0 {
                            word(out, " ");
                        }
                        key.encode(out)?;
                    }
                    word(out, ")");
                }
            },
        }
        Ok(())
    }
}

/// An `astring`: quoted when printable ASCII, else a literal.
fn string(out: &mut Vec<Piece>, value: &str) -> Result<()> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(ImapError::InvalidArgument(
            "search text contains line breaks or NUL".to_owned(),
        ));
    }
    if value.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        out.push(Piece::Text(crate::client::quote(value)?));
    } else {
        out.push(Piece::Literal(value.as_bytes().to_vec()));
    }
    Ok(())
}

/// A keyword as an IMAP atom (no specials, no spaces).
fn atom(value: &str) -> Result<&str> {
    let ok = !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(b, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\' | b']')
        });
    if ok {
        Ok(value)
    } else {
        Err(ImapError::InvalidArgument(format!(
            "{value:?} cannot be an IMAP keyword"
        )))
    }
}

impl ImapClient {
    /// `UID SEARCH <key>` in `path` (examined unless selected), ascending.
    /// Adds `CHARSET UTF-8` when the key has non-ASCII text; a server
    /// without UTF-8 search answers `NO [BADCHARSET]`, an error.
    pub async fn search_messages(&mut self, path: &str, key: &SearchKey) -> Result<Vec<u32>> {
        let pieces = key.pieces()?;
        self.ensure_selected(path, false).await?;
        let mut parts: Vec<Part<'_>> = Vec::with_capacity(pieces.len() + 1);
        parts.push(Part::Text(Cow::Borrowed(if key.needs_utf8() {
            "UID SEARCH CHARSET UTF-8 "
        } else {
            "UID SEARCH "
        })));
        for piece in &pieces {
            parts.push(match piece {
                Piece::Text(text) => Part::Text(Cow::Borrowed(text)),
                Piece::Literal(bytes) => Part::Literal(bytes),
            });
        }
        let mut uids = Vec::new();
        let mut on = |response: &Response<'static>| {
            if let Response::MailboxData(MailboxDatum::Search(found)) = response {
                uids.extend(found.iter().copied());
                return Ok(true);
            }
            Ok(false)
        };
        self.run_parts(&parts, &mut on, true).await?;
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u8, day: u8) -> ImapDate {
        ImapDate { year, month, day }
    }

    #[test]
    fn keys_encode_as_imap_search_keys() {
        let key = SearchKey::And(vec![
            SearchKey::From("ada@example.org".into()),
            SearchKey::Not(Box::new(SearchKey::Seen)),
            SearchKey::any(vec![
                SearchKey::Subject("rust \"weekly\"".into()),
                SearchKey::Body("async".into()),
                SearchKey::Keyword("$Label1".into()),
            ]),
            SearchKey::Since(day(2026, 9, 1)),
            SearchKey::Larger(1024),
        ]);
        assert_eq!(
            key.to_command().unwrap(),
            "FROM \"ada@example.org\" NOT SEEN OR SUBJECT \"rust \\\"weekly\\\"\" OR BODY \"async\" KEYWORD $Label1 SINCE 1-Sep-2026 LARGER 1024"
        );
        assert!(!key.needs_utf8());
        assert_eq!(SearchKey::And(vec![]).to_command().unwrap(), "ALL");
        assert_eq!(
            SearchKey::any(vec![]).to_command().unwrap(),
            "NOT ALL",
            "no alternative matches nothing"
        );
        assert_eq!(
            SearchKey::Not(Box::new(SearchKey::And(vec![
                SearchKey::Flagged,
                SearchKey::Draft
            ])))
            .to_command()
            .unwrap(),
            "NOT (FLAGGED DRAFT)"
        );
    }

    #[test]
    fn non_ascii_text_is_a_literal_with_utf8() {
        let key = SearchKey::Text("Grüße".into());
        assert!(key.needs_utf8());
        assert_eq!(
            key.pieces().unwrap(),
            [
                Piece::Text("TEXT ".into()),
                Piece::Literal("Grüße".as_bytes().to_vec())
            ]
        );
        assert_eq!(key.to_command().unwrap(), "TEXT {7}Grüße");
        assert_eq!(
            SearchKey::Header("List-Id".into(), String::new())
                .to_command()
                .unwrap(),
            "HEADER \"List-Id\" \"\""
        );
    }

    #[test]
    fn invalid_keys_are_refused() {
        for key in [
            SearchKey::Keyword("two words".into()),
            SearchKey::Keyword(String::new()),
            SearchKey::Unkeyword("a)b".into()),
            SearchKey::Header("Bad:Name".into(), "x".into()),
            SearchKey::Text("line\r\nbreak".into()),
        ] {
            assert!(
                matches!(key.to_command(), Err(ImapError::InvalidArgument(_))),
                "{key:?}"
            );
        }
    }
}
