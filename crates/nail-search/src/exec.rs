//! Running a parsed query: full-text terms go to the index, everything
//! else becomes a store-side [`MessageFilter`].
//!
//! [`compile`] walks the query. Each full-text term (and each group of
//! full-text terms directly under the same `AND` / `OR`, merged into one
//! index query) is run against the index and becomes
//! [`MessageFilter::Ids`] of its hits; flags, tags, account, folder, dates
//! and size become the matching store filters. The store then evaluates the
//! whole boolean expression in one SQL query ([`nail_store::Store::filter_message_ids`]),
//! so `NOT`, `OR` and mixed terms behave exactly as written.
//!
//! Relevance scores are the sum of the index scores of the positive
//! (not negated) full-text terms a message matched.

use std::collections::HashMap;

use nail_api::search::{SearchError, SearchHit, SearchQuery, SearchSort};
use nail_api::{AccountId, FolderId, MessageId};
use nail_store::{MessageFilter, SizeCmp, Store};
use tantivy::query::Query as TantivyQuery;

use crate::date::resolve;
use crate::error::Result;
use crate::index::SearchIndex;
use crate::query::{FlagTerm, Query, SizeOp, Term, TermKind, parse_query};

/// A virtual account the `account:` term can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualAccountScope {
    /// Its id.
    pub id: AccountId,
    /// Its display name.
    pub name: String,
    /// Its member accounts.
    pub members: Vec<AccountId>,
}

/// Everything a query needs besides its text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchContext {
    /// Now, in Unix seconds (for relative dates like `7d`).
    pub now: i64,
    /// The user's UTC offset in seconds (east positive); calendar days in
    /// `before:` / `after:` start at local midnight.
    pub utc_offset_secs: i32,
    /// Virtual accounts `account:` may name (by id or name,
    /// case-insensitive). `account:all` always means every account.
    pub virtual_accounts: Vec<VirtualAccountScope>,
}

/// Where to search (the search bar's account / folder selection).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchScope {
    /// Only these accounts (a virtual account's members); `None` = all.
    pub accounts: Option<Vec<AccountId>>,
    /// Only these folders (a virtual folder's members); `None` = all.
    pub folders: Option<Vec<FolderId>>,
}

/// A page of search results.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResults {
    /// Number of matching messages.
    pub total: u32,
    /// The requested page.
    pub hits: Vec<SearchHit>,
}

/// A query compiled against an index snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The store-side filter (full-text terms resolved to id sets).
    pub filter: MessageFilter,
    /// Relevance per message (positive full-text matches only).
    pub scores: HashMap<MessageId, f32>,
}

struct Compiler<'a> {
    index: &'a SearchIndex,
    ctx: &'a SearchContext,
    scores: HashMap<MessageId, f32>,
}

impl Compiler<'_> {
    /// Runs full-text queries and turns their hits into an id filter.
    fn hits(&mut self, query: &dyn TantivyQuery, positive: bool) -> Result<MessageFilter> {
        let hits = self.index.run(query)?;
        let mut ids = Vec::with_capacity(hits.len());
        for (id, score) in hits {
            if positive {
                *self.scores.entry(id.clone()).or_default() += score;
            }
            ids.push(id);
        }
        Ok(MessageFilter::Ids(ids))
    }

    fn text_query(&self, term: &Term) -> Option<Option<Box<dyn TantivyQuery>>> {
        match &term.kind {
            TermKind::Text { field, value, .. } => Some(self.index.text_query(*field, value)),
            _ => None,
        }
    }

    fn node(&mut self, query: &Query, positive: bool) -> Result<MessageFilter> {
        Ok(match query {
            Query::All => MessageFilter::All,
            Query::Term(term) => match self.text_query(term) {
                // Text without searchable tokens ("!!!") is ignored.
                Some(None) => MessageFilter::All,
                Some(Some(q)) => self.hits(q.as_ref(), positive)?,
                None => self.term(term),
            },
            Query::Not(inner) => MessageFilter::Not(Box::new(self.node(inner, !positive)?)),
            Query::And(items) | Query::Or(items) => {
                let is_and = matches!(query, Query::And(_));
                // Direct full-text children run as one index query.
                let mut text = Vec::new();
                let mut rest = Vec::new();
                for item in items {
                    match item {
                        Query::Term(term) => match self.text_query(term) {
                            Some(Some(q)) => text.push(q),
                            Some(None) => rest.push(MessageFilter::All),
                            None => rest.push(self.term(term)),
                        },
                        other => rest.push(self.node(other, positive)?),
                    }
                }
                if !text.is_empty() {
                    let combined = if is_and {
                        SearchIndex::all_of(text)
                    } else {
                        SearchIndex::any_of(text)
                    };
                    rest.insert(0, self.hits(combined.as_ref(), positive)?);
                }
                if is_and {
                    MessageFilter::And(rest)
                } else {
                    MessageFilter::Or(rest)
                }
            }
        })
    }

    fn term(&self, term: &Term) -> MessageFilter {
        match &term.kind {
            TermKind::Text { .. } => MessageFilter::All, // handled by the caller
            TermKind::Is(flag) => match flag {
                FlagTerm::Unread => MessageFilter::Seen(false),
                FlagTerm::Read => MessageFilter::Seen(true),
                FlagTerm::Flagged => MessageFilter::Flagged(true),
                FlagTerm::Unflagged => MessageFilter::Flagged(false),
                FlagTerm::Answered => MessageFilter::Answered(true),
                FlagTerm::Unanswered => MessageFilter::Answered(false),
                FlagTerm::Draft => MessageFilter::Draft(true),
            },
            TermKind::HasAttachment => MessageFilter::HasAttachments(true),
            TermKind::Tag(v) => MessageFilter::Tag(v.text.clone()),
            TermKind::Account(v) => {
                if v.text.eq_ignore_ascii_case(AccountId::ALL) {
                    return MessageFilter::All;
                }
                let lower = v.text.to_lowercase();
                match self.ctx.virtual_accounts.iter().find(|va| {
                    va.id.as_str().to_lowercase() == lower || va.name.to_lowercase() == lower
                }) {
                    Some(va) => MessageFilter::AccountIn(va.members.clone()),
                    None => MessageFilter::AccountNamed(v.text.clone()),
                }
            }
            TermKind::Folder(v) => MessageFilter::FolderNamed(v.text.clone()),
            TermKind::Before(d) => {
                MessageFilter::DateBefore(resolve(*d, self.ctx.now, self.ctx.utc_offset_secs))
            }
            TermKind::After(d) => {
                MessageFilter::DateOnOrAfter(resolve(*d, self.ctx.now, self.ctx.utc_offset_secs))
            }
            TermKind::Size(op, n) => MessageFilter::Size(
                match op {
                    SizeOp::Greater => SizeCmp::Greater,
                    SizeOp::GreaterOrEqual => SizeCmp::GreaterOrEqual,
                    SizeOp::Less => SizeCmp::Less,
                    SizeOp::LessOrEqual => SizeCmp::LessOrEqual,
                },
                *n,
            ),
        }
    }
}

/// Compiles a query: runs its full-text parts against `index` and returns
/// the store-side filter (full-text terms become id sets of their hits).
pub fn compile(index: &SearchIndex, query: &Query, ctx: &SearchContext) -> Result<Compiled> {
    let mut compiler = Compiler {
        index,
        ctx,
        scores: HashMap::new(),
    };
    let filter = compiler.node(query, true)?;
    Ok(Compiled {
        filter,
        scores: compiler.scores,
    })
}

/// Every message matching `query` within `scope`, newest first.
pub fn matching_ids(
    index: &SearchIndex,
    store: &Store,
    query: &Query,
    scope: &SearchScope,
    ctx: &SearchContext,
) -> Result<Vec<MessageId>> {
    let compiled = compile(index, query, ctx)?;
    Ok(store.filter_message_ids(&scoped(compiled.filter, scope))?)
}

fn scoped(filter: MessageFilter, scope: &SearchScope) -> MessageFilter {
    let mut filter = filter;
    if let Some(accounts) = &scope.accounts {
        filter = filter.and(MessageFilter::AccountIn(accounts.clone()));
    }
    if let Some(folders) = &scope.folders {
        filter = filter.and(MessageFilter::FolderIn(folders.clone()));
    }
    filter
}

/// Runs a parsed query and returns one page of hits, newest first or by
/// relevance (ties newest first).
#[allow(clippy::too_many_arguments)]
pub fn search(
    index: &SearchIndex,
    store: &Store,
    query: &Query,
    scope: &SearchScope,
    sort: SearchSort,
    offset: u32,
    limit: u32,
    ctx: &SearchContext,
) -> Result<SearchResults> {
    let compiled = compile(index, query, ctx)?;
    let mut ids = store.filter_message_ids(&scoped(compiled.filter, scope))?;
    if sort == SearchSort::Relevance {
        let score = |id: &MessageId| compiled.scores.get(id).copied().unwrap_or(0.0);
        ids.sort_by(|a, b| score(b).total_cmp(&score(a)));
    }
    let total = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    let start = (offset as usize).min(ids.len());
    let end = start.saturating_add(limit as usize).min(ids.len());
    let page = &ids[start..end];
    let hits = store
        .message_summaries(page)?
        .into_iter()
        .map(|message| {
            let score = compiled.scores.get(&message.id).copied().unwrap_or(0.0);
            SearchHit {
                message,
                score,
                from_server: false,
            }
        })
        .collect();
    Ok(SearchResults { total, hits })
}

/// Parses and runs a `Search` request. The outer error is an engine
/// failure; the inner one a query the user must fix.
pub fn run_request(
    index: &SearchIndex,
    store: &Store,
    request: &SearchQuery,
    scope: &SearchScope,
    ctx: &SearchContext,
) -> Result<std::result::Result<SearchResults, SearchError>> {
    let query = match parse_query(&request.text) {
        Ok(q) => q,
        Err(e) => return Ok(Err(e)),
    };
    search(
        index,
        store,
        &query,
        scope,
        request.sort,
        request.offset,
        request.limit,
        ctx,
    )
    .map(Ok)
}
