//! Search for Nail (design.md, "Threading and search"; ADR 0016).
//!
//! * [`SearchIndex`] — a Tantivy index over headers, decoded body text and
//!   attachment names, on disk under the data directory
//!   ([`INDEX_DIR_NAME`]) or in RAM, with incremental add / replace /
//!   remove and batched commits ([`CommitPolicy`]).
//! * [`parse_query`] — the hand-written parser of the query language
//!   (`from: to: cc: subject: body: filename: has:attachment is:unread
//!   is:flagged tag: account: folder: before: after: size>`, quotes,
//!   `AND` / `OR` / `NOT` / `-`, parentheses) into a public AST
//!   ([`Query`]), with precise error positions
//!   ([`nail_api::search::SearchError`], UTF-16 offsets).
//! * [`search`] / [`run_request`] — run a query: full-text terms against
//!   the index, flags / tags / account / folder / dates / size as
//!   store-side filters ([`nail_store::MessageFilter`]), paged and sorted.
//! * [`IndexPolicy`] — the decrypted-content indexing policy (M2a WP33):
//!   decrypted mail is indexed only for accounts that opted in; the index
//!   then counts as sensitive. [`SearchIndex::open_with`] takes the
//!   encryption-at-rest directory wrapper.
//! * [`imap`] — the server-side search fallback (M2c WP53): a query as an
//!   IMAP search key for messages without a local body
//!   ([`imap::server_search`], [`imap::needs_server`]).
//! * [`pim`] — PIM search (M3a WP73): [`PimIndex`], its own Tantivy index
//!   of events, tasks, contacts and groups ([`PIM_INDEX_DIR_NAME`]),
//!   type-ahead word-prefix queries, phone numbers by digits, sealed with
//!   the same directory wrapper as the mail index; [`contact_match`].

#![warn(missing_docs)]

mod date;
mod document;
mod error;
mod exec;
pub mod imap;
mod index;
pub mod pim;
mod policy;
pub mod query;

pub use document::IndexDocument;
pub use error::{Error, Result};
pub use exec::{
    Compiled, PageRequest, SearchContext, SearchResults, SearchScope, VirtualAccountScope, compile,
    matching_ids, run_request, search,
};
pub use index::{CommitPolicy, DirectoryWrapper, INDEX_DIR_NAME, SearchIndex, VERSION_FILE_NAME};
pub use pim::{PIM_INDEX_DIR_NAME, PimDocument, PimIndex, contact_match};
pub use policy::IndexPolicy;
pub use query::{Query, parse_query};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
