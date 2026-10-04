//! The server-side search fallback (M2c WP53; ADR 0043
//! `SearchResults.server`): a query of the search language as an IMAP
//! [`SearchKey`], for messages whose body is not stored locally (sync
//! policies "headers only" / "recent N days"). The core runs the key in
//! the folders in scope ([`nail_proto_imap::ImapClient::search_messages`])
//! and merges the hits into the local results.
//!
//! IMAP matches case-insensitive substrings and has no attachment, account
//! or folder terms, so the key can be broader than the query:
//!
//! * `account:` / `folder:` are the caller's scope (which folders it
//!   searches), not part of the key;
//! * `has:attachment` and `filename:` become approximations
//!   (`Content-Type: multipart/mixed`, a body substring);
//! * relative dates (`before:3d`) are rounded to days.
//!
//! [`ServerSearch::exact`] says whether the key means exactly the query;
//! the core filters inexact hits with what it knows locally (flags, dates,
//! headers). A term the server cannot express at all widens its branch: it
//! is dropped from an `AND`, and makes an `OR` (or a `NOT` around it) match
//! everything, so a server search never misses a message the query finds.

use nail_proto_imap::{ImapDate, SearchKey};

use crate::date;
use crate::query::{DateValue, FlagTerm, Query, SizeOp, Term, TermKind, TextField};

/// A query as an IMAP search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSearch {
    /// The search key (`SearchKey::All` when nothing narrows it).
    pub key: SearchKey,
    /// Whether the key finds exactly what the query finds (no dropped or
    /// approximated terms).
    pub exact: bool,
}

/// Whether the query looks into message text that may not be stored
/// locally (free text, `body:`, `filename:`): only then is a server search
/// worth it — headers, flags and dates are known for every message.
pub fn needs_server(query: &Query) -> bool {
    query.terms().iter().any(|term| {
        matches!(
            term.kind,
            TermKind::Text {
                field: TextField::Any | TextField::Body | TextField::Filename,
                ..
            }
        )
    })
}

/// The IMAP search for `query`, with relative dates resolved at `now`
/// (Unix seconds) and calendar days at local midnight (`utc_offset_secs`
/// east of UTC), as the local search does.
pub fn server_search(query: &Query, now: i64, utc_offset_secs: i32) -> ServerSearch {
    let mut exact = true;
    let key = convert(query, now, utc_offset_secs, &mut exact).unwrap_or(SearchKey::All);
    ServerSearch { key, exact }
}

/// `None` = the server cannot narrow this part (matches everything).
fn convert(query: &Query, now: i64, offset: i32, exact: &mut bool) -> Option<SearchKey> {
    match query {
        Query::All => None,
        Query::Term(term) => term_key(term, now, offset, exact),
        Query::Not(inner) => {
            if let Some(key) = convert(inner, now, offset, exact) {
                Some(SearchKey::Not(Box::new(key)))
            } else {
                // NOT of "anything": the server cannot tell; ask for all.
                *exact = false;
                None
            }
        }
        Query::And(parts) => {
            let keys: Vec<SearchKey> = parts
                .iter()
                .filter_map(|p| convert(p, now, offset, exact))
                .collect();
            match keys.len() {
                0 => None,
                1 => keys.into_iter().next(),
                _ => Some(SearchKey::And(keys)),
            }
        }
        Query::Or(parts) => {
            let mut keys = Vec::with_capacity(parts.len());
            for part in parts {
                keys.push(convert(part, now, offset, exact)?);
            }
            Some(SearchKey::any(keys))
        }
    }
}

fn term_key(term: &Term, now: i64, offset: i32, exact: &mut bool) -> Option<SearchKey> {
    match &term.kind {
        TermKind::Text { field, value, .. } => Some(match field {
            TextField::Any => SearchKey::Text(value.clone()),
            TextField::From => SearchKey::From(value.clone()),
            TextField::To => SearchKey::To(value.clone()),
            TextField::Cc => SearchKey::Cc(value.clone()),
            TextField::Subject => SearchKey::Subject(value.clone()),
            TextField::Body => SearchKey::Body(value.clone()),
            TextField::Filename => {
                // Attachment names are in the MIME part headers, which IMAP
                // counts as body text.
                *exact = false;
                SearchKey::Body(value.clone())
            }
        }),
        TermKind::Is(flag) => Some(match flag {
            FlagTerm::Unread => SearchKey::Unseen,
            FlagTerm::Read => SearchKey::Seen,
            FlagTerm::Flagged => SearchKey::Flagged,
            FlagTerm::Unflagged => SearchKey::Unflagged,
            FlagTerm::Answered => SearchKey::Answered,
            FlagTerm::Unanswered => SearchKey::Unanswered,
            FlagTerm::Draft => SearchKey::Draft,
        }),
        TermKind::HasAttachment => {
            *exact = false;
            Some(SearchKey::Header(
                "Content-Type".into(),
                "multipart/mixed".into(),
            ))
        }
        TermKind::Tag(value) => {
            let keyword = SearchKey::Keyword(value.text.clone());
            if keyword.to_command().is_ok() {
                Some(keyword)
            } else {
                // Not a valid IMAP keyword: no message can carry it there.
                *exact = false;
                None
            }
        }
        // The caller's scope decides which folders it searches; IMAP has
        // no Gmail categories (they are Gmail API data, M3b).
        TermKind::Account(_) | TermKind::Folder(_) | TermKind::Category(_) | TermKind::Focus(_) => {
            *exact = false;
            None
        }
        TermKind::Before(value) => Some(SearchKey::SentBefore(day(*value, now, offset, exact))),
        TermKind::After(value) => Some(SearchKey::SentSince(day(*value, now, offset, exact))),
        TermKind::Size(op, bytes) => {
            let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
            Some(match op {
                SizeOp::Greater => SearchKey::Larger(clamp(*bytes)),
                SizeOp::GreaterOrEqual => SearchKey::Larger(clamp(bytes.saturating_sub(1))),
                SizeOp::Less => SearchKey::Smaller(clamp(*bytes)),
                SizeOp::LessOrEqual => SearchKey::Smaller(clamp(bytes.saturating_add(1))),
            })
        }
    }
}

/// The IMAP day of a date term: a calendar day as written; a relative date
/// resolved and rounded to its (local) day.
fn day(value: DateValue, now: i64, offset: i32, exact: &mut bool) -> ImapDate {
    match value {
        DateValue::Day { year, month, day } => ImapDate { year, month, day },
        DateValue::Ago { .. } => {
            *exact = false;
            let at = date::resolve(value, now, offset);
            ImapDate::from_unix(at + i64::from(offset))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_query;
    use pretty_assertions::assert_eq;

    const NOW: i64 = 1_790_769_600; // 2026-09-30T12:00:00Z

    fn search(text: &str) -> ServerSearch {
        server_search(&parse_query(text).unwrap(), NOW, 0)
    }

    fn command(text: &str) -> String {
        search(text).key.to_command().unwrap()
    }

    #[test]
    fn text_flags_sizes_and_days_map_exactly() {
        let s = search("from:ada subject:\"rust weekly\" is:unread size>=1024 before:2026-09-01");
        assert!(s.exact);
        assert_eq!(
            s.key.to_command().unwrap(),
            "FROM \"ada\" SUBJECT \"rust weekly\" UNSEEN LARGER 1023 SENTBEFORE 1-Sep-2026"
        );
        assert_eq!(command("async"), "TEXT \"async\"");
        assert_eq!(command("body:closure"), "BODY \"closure\"");
        assert_eq!(command("to:bob cc:carol"), "TO \"bob\" CC \"carol\"");
        assert_eq!(command("tag:$Label1"), "KEYWORD $Label1");
        assert_eq!(command("size<10"), "SMALLER 10");
        assert_eq!(command("size<=10"), "SMALLER 11");
        assert_eq!(command("after:2026-01-02"), "SENTSINCE 2-Jan-2026");
        assert_eq!(command("is:read is:draft"), "SEEN DRAFT");
    }

    #[test]
    fn boolean_structure_is_kept() {
        assert_eq!(command("rust OR go"), "OR TEXT \"rust\" TEXT \"go\"");
        assert_eq!(command("-is:flagged"), "NOT FLAGGED");
        assert_eq!(command("(a OR b) c"), "OR TEXT \"a\" TEXT \"b\" TEXT \"c\"");
        assert_eq!(command(""), "ALL");
    }

    #[test]
    fn what_the_server_cannot_express_widens_the_search() {
        let s = search("account:work async");
        assert!(!s.exact);
        assert_eq!(s.key.to_command().unwrap(), "TEXT \"async\"");
        // An OR with an unknown branch matches everything.
        let s = search("folder:Inbox OR async");
        assert_eq!(s.key, SearchKey::All);
        assert!(!s.exact);
        // So does a NOT around one.
        assert_eq!(search("-folder:Inbox").key, SearchKey::All);
        let s = search("has:attachment filename:report");
        assert!(!s.exact);
        assert_eq!(
            s.key.to_command().unwrap(),
            "HEADER \"Content-Type\" \"multipart/mixed\" BODY \"report\""
        );
        let s = search("after:7d");
        assert!(!s.exact);
        assert_eq!(s.key.to_command().unwrap(), "SENTSINCE 23-Sep-2026");
        let s = search("tag:\"two words\"");
        assert!(!s.exact);
        assert_eq!(s.key, SearchKey::All);
    }

    #[test]
    fn only_text_terms_need_the_server() {
        for (text, needed) in [
            ("async", true),
            ("body:x", true),
            ("filename:report", true),
            ("from:ada is:unread", false),
            ("subject:x -tag:y", false),
            ("", false),
        ] {
            assert_eq!(needs_server(&parse_query(text).unwrap()), needed, "{text}");
        }
    }
}
