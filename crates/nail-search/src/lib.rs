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

#![warn(missing_docs)]

mod date;
mod document;
mod error;
mod exec;
mod index;
mod policy;
pub mod query;

pub use document::IndexDocument;
pub use error::{Error, Result};
pub use exec::{
    Compiled, SearchContext, SearchResults, SearchScope, VirtualAccountScope, compile,
    matching_ids, run_request, search,
};
pub use index::{CommitPolicy, DirectoryWrapper, INDEX_DIR_NAME, SearchIndex, VERSION_FILE_NAME};
pub use policy::IndexPolicy;
pub use query::{Query, parse_query};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
