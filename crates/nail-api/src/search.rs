//! Search requests and results (design.md, "Threading and search").
//!
//! The query language is the same everywhere (search bar, saved searches,
//! rules, MCP): `from: to: subject: has:attachment is:unread is:flagged
//! tag: account: folder: before: after: size>` plus free text, quotes,
//! `AND` / `OR` / `NOT` and parentheses. `nail-search` parses it; a query
//! that does not parse is answered with [`crate::Response::InvalidQuery`]
//! carrying a [`SearchError`] with the exact position.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::MessageSummary;
use crate::ids::{AccountId, FolderId, SavedSearchId};

/// A search request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SearchQuery {
    /// The query text in the query language.
    pub text: String,
    /// Restrict to an account (real or virtual); `null` = all accounts.
    #[serde(default)]
    pub account_id: Option<AccountId>,
    /// Restrict to a folder (real or virtual); `null` = all folders.
    #[serde(default)]
    pub folder_id: Option<FolderId>,
    /// Index of the first hit to return (0 = best).
    #[serde(default)]
    pub offset: u32,
    /// Maximum number of hits to return.
    pub limit: u32,
    /// Result order.
    #[serde(default)]
    pub sort: SearchSort,
}

/// The server-side part of a search (M2c): messages whose body is not
/// stored locally ([`crate::MessageSummary::headers_only`]) are searched on
/// the server (IMAP `SEARCH`) and the hits merged into the result.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct ServerSearch {
    /// Accounts asked.
    pub accounts: Vec<AccountId>,
    /// Hits that came only from the server (marked `from_server`, on top of
    /// the page, at most the page size; included in `total`).
    pub hits: u32,
    /// Accounts that could not be asked (offline, error); their
    /// headers-only messages were matched on headers only.
    pub failed: Vec<AccountId>,
}

/// Order of search hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    /// Newest first.
    #[default]
    Date,
    /// Best match first.
    Relevance,
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SearchHit {
    /// The matching message.
    pub message: MessageSummary,
    /// Relevance score (higher is better; only comparable within one result).
    pub score: f32,
    /// Found only by the server-side search (M2c, first page only). Such
    /// hits come on top of the page: `offset` / `limit` count the local
    /// hits only, so the next page starts at `offset` + the local hits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub from_server: bool,
}

/// Why a query does not parse. Offsets count UTF-16 code units, as
/// JavaScript indexes strings, so the UI can underline `start..end` directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SearchError {
    /// Error class; the UI shows the Fluent message
    /// [`SearchErrorKind::message_id`].
    pub kind: SearchErrorKind,
    /// Diagnostic detail (English).
    pub message: String,
    /// Start of the offending part (inclusive).
    pub start: u32,
    /// End of the offending part (exclusive); equals `start` for "missing
    /// something here".
    pub end: u32,
}

/// Classes of query errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SearchErrorKind {
    /// A token that cannot appear here (e.g. `AND` at the start).
    UnexpectedToken,
    /// The query ends where something is still needed (e.g. `from:`).
    UnexpectedEnd,
    /// A `"` without its closing quote.
    UnterminatedQuote,
    /// A `(` without `)` or a `)` without `(`.
    UnbalancedParenthesis,
    /// An unknown `field:`.
    UnknownField,
    /// A value that does not fit its field (a bad date, size or `is:` flag).
    InvalidValue,
    /// The query is too long or nested too deeply (parentheses, `NOT`).
    TooComplex,
}

impl SearchErrorKind {
    /// All kinds.
    pub const ALL: [SearchErrorKind; 7] = [
        Self::UnexpectedToken,
        Self::UnexpectedEnd,
        Self::UnterminatedQuote,
        Self::UnbalancedParenthesis,
        Self::UnknownField,
        Self::InvalidValue,
        Self::TooComplex,
    ];

    /// Wire name, e.g. `unknown-field`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnexpectedToken => "unexpected-token",
            Self::UnexpectedEnd => "unexpected-end",
            Self::UnterminatedQuote => "unterminated-quote",
            Self::UnbalancedParenthesis => "unbalanced-parenthesis",
            Self::UnknownField => "unknown-field",
            Self::InvalidValue => "invalid-value",
            Self::TooComplex => "too-complex",
        }
    }

    /// Fluent message id the UI shows, e.g. `search-error-unknown-field`.
    pub fn message_id(self) -> String {
        format!("search-error-{}", self.as_str())
    }
}

/// A saved search (smart folder): a named query, optionally scoped. The
/// layout can place saved searches like folders (M1b WP24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SavedSearch {
    /// Stable id; `null` in [`crate::Request::SaveSavedSearch`] creates a
    /// new one.
    #[serde(default)]
    pub id: Option<SavedSearchId>,
    /// Display name.
    pub name: String,
    /// The query in the query language.
    pub query: String,
    /// Restrict to an account (real or virtual); `null` = all accounts.
    #[serde(default)]
    pub account_id: Option<AccountId>,
    /// Restrict to a folder (real or virtual); `null` = all folders.
    #[serde(default)]
    pub folder_id: Option<FolderId>,
    /// Result order.
    #[serde(default)]
    pub sort: SearchSort,
    /// Display position (ascending); new searches go last.
    #[serde(default)]
    pub position: u32,
}

/// Converts a byte offset into `text` to a UTF-16 offset for
/// [`SearchError`]. Offsets inside a character round down to its start.
pub fn utf16_offset(text: &str, byte_offset: usize) -> u32 {
    let mut end = byte_offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    u32::try_from(text[..end].encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_have_wire_names_and_message_ids() {
        for kind in SearchErrorKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
        assert_eq!(
            SearchErrorKind::UnknownField.message_id(),
            "search-error-unknown-field"
        );
    }

    #[test]
    fn query_defaults() {
        let q: SearchQuery =
            serde_json::from_value(json!({"text": "from:ada", "limit": 50})).unwrap();
        assert_eq!(q.offset, 0);
        assert_eq!(q.sort, SearchSort::Date);
        assert_eq!(q.account_id, None);
    }

    #[test]
    fn saved_search_defaults() {
        let s: SavedSearch =
            serde_json::from_value(json!({"name": "Unread", "query": "is:unread"})).unwrap();
        assert_eq!(s.id, None);
        assert_eq!(s.sort, SearchSort::Date);
        assert_eq!(s.position, 0);
    }

    #[test]
    fn utf16_offsets() {
        let text = "é😀x";
        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, 2), 1); // after é
        assert_eq!(utf16_offset(text, 6), 3); // after the emoji (2 units)
        assert_eq!(utf16_offset(text, 3), 1); // inside the emoji
        assert_eq!(utf16_offset(text, 100), 4);
    }
}
