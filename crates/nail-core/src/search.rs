//! Search requests and index upkeep.
//!
//! The index (`<data dir>/search`, `nail-search`) is fed by ingest
//! (`ingest.rs`) and pruned when messages are deleted or expunged. A
//! committer task makes staged changes visible every second; a maintenance
//! task backfills threading headers of old rows and rebuilds the index
//! when it was recreated or is behind the store.

use std::sync::Arc;
use std::time::Duration;

use nail_api::{ApiError, Event, Response, SavedSearch, SavedSearchId, SearchQuery};
use nail_search::{SearchContext, SearchIndex, SearchScope, VirtualAccountScope, run_request};
use nail_store::MessageFilter;

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::{MAX_PAGE_SIZE, Service};

/// The search index, replaceable at run time: encryption at rest (M2a)
/// closes the on-disk index while it converts the index files and opens it
/// again with the new directory wrapper. Users take a short-lived
/// [`Arc`] with [`SearchSlot::get`] for each use and never keep it across
/// awaits of long work, so the old index is released soon after
/// [`SearchSlot::replace`].
pub(crate) struct SearchSlot {
    index: std::sync::RwLock<Arc<SearchIndex>>,
    closed: std::sync::atomic::AtomicBool,
}

impl SearchSlot {
    pub(crate) fn new(index: SearchIndex) -> Self {
        Self {
            index: std::sync::RwLock::new(Arc::new(index)),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The current index.
    pub(crate) fn get(&self) -> Arc<SearchIndex> {
        Arc::clone(
            &self
                .index
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Puts `index` in place and returns the previous one.
    pub(crate) fn replace(&self, index: SearchIndex) -> Arc<SearchIndex> {
        std::mem::replace(
            &mut *self
                .index
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Arc::new(index),
        )
    }

    /// Whether the real index is closed (a stand-in answers meanwhile;
    /// searches report `unavailable`).
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn set_closed(&self, closed: bool) {
        self.closed
            .store(closed, std::sync::atomic::Ordering::Release);
    }
}

/// Whether and where a search also asks the servers (IMAP `SEARCH` for
/// messages without a local body, first page only).
pub(crate) enum ServerSearchMode {
    /// Local index only (MCP, later pages).
    Off,
    /// Every folder in scope.
    All,
    /// Only folders this accepts (a remote session's grant): no server of
    /// an account outside the grant is contacted.
    Only(Box<dyn Fn(&nail_api::Folder) -> bool + Send + Sync>),
}

/// The local time zone's offset from UTC in seconds (east positive).
pub(crate) fn local_utc_offset() -> i32 {
    chrono::Local::now().offset().local_minus_utc()
}

impl Service {
    /// `SaveSavedSearch`: the query must parse.
    pub(crate) async fn save_saved_search(&self, search: SavedSearch) -> Result<SavedSearch> {
        if let Err(err) = nail_search::parse_query(&search.query) {
            return Err(ApiError::invalid_request(format!(
                "the saved search's query does not parse: {}",
                err.message
            ))
            .into());
        }
        let saved = self
            .inner
            .store
            .call(move |s| s.save_saved_search(&search))
            .await?;
        self.broadcast(Event::SavedSearchesChanged);
        Ok(saved)
    }

    /// `DeleteSavedSearch`.
    pub(crate) async fn delete_saved_search(&self, id: SavedSearchId) -> Result<()> {
        let key = id.clone();
        if !self
            .inner
            .store
            .call(move |s| s.delete_saved_search(&key))
            .await?
        {
            return Err(ApiError::not_found(format!("saved search {id}")).into());
        }
        self.broadcast(Event::SavedSearchesChanged);
        Ok(())
    }

    /// `Search`: parse, scope (a virtual account or folder means its
    /// members), run; on the first page also on the servers.
    pub(crate) async fn search(&self, query: SearchQuery) -> Result<Response> {
        self.search_with(query, ServerSearchMode::All).await
    }

    /// `Search` with a choice of server search. Server hits come on top of
    /// the first page (at most `limit` of them, marked `from_server`, in
    /// `total`); paging counts local hits only.
    pub(crate) async fn search_with(
        &self,
        mut query: SearchQuery,
        server_mode: ServerSearchMode,
    ) -> Result<Response> {
        if self.inner.search.is_closed() {
            return Err(ApiError::new(
                nail_api::ErrorCode::Unavailable,
                "the search index is being converted (encryption at rest); try again shortly",
            )
            .into());
        }
        let mut scope = SearchScope::default();
        if let Some(folder) = &query.folder_id {
            scope.folders = Some(self.resolve_folder(folder).await?.sources);
        }
        if let Some(account) = &query.account_id {
            scope.accounts = Some(if account.as_str() == nail_api::AccountId::ALL {
                self.all_members().await?
            } else {
                self.expand_account(account).await?
            });
        }
        let config = self.inner.accounts.config();
        let ctx = SearchContext {
            now: unix_now(),
            utc_offset_secs: local_utc_offset(),
            virtual_accounts: config
                .virtual_accounts
                .iter()
                .map(|v| VirtualAccountScope {
                    id: v.id.clone(),
                    name: v.name.clone(),
                    members: v.members.clone(),
                })
                .collect(),
        };
        query.limit = query.limit.min(MAX_PAGE_SIZE);
        let index = self.inner.search.get();
        let store = self.inner.store.clone();
        let offset = query.offset;
        let limit = query.limit;
        let text = query.text.clone();
        let sort = query.sort;
        let scope_for_server = scope.clone();
        let outcome =
            tokio::task::spawn_blocking(move || run_request(&index, &store, &query, &scope, &ctx))
                .await??;
        Ok(match outcome {
            Ok(mut results) => {
                // Messages without a local body are searched on their
                // server (first page only; M2c).
                let filter = match server_mode {
                    ServerSearchMode::Off => None,
                    ServerSearchMode::All => Some(None),
                    ServerSearchMode::Only(filter) => Some(Some(filter)),
                };
                let server = match (nail_search::parse_query(&text), filter) {
                    (Ok(parsed), Some(filter))
                        if offset == 0 && nail_search::imap::needs_server(&parsed) =>
                    {
                        self.server_search(
                            &parsed,
                            &scope_for_server,
                            filter.as_deref(),
                            limit as usize,
                            &mut results.hits,
                        )
                        .await
                    }
                    _ => None,
                };
                if let Some(server) = &server {
                    results.total += server.hits;
                    if sort == nail_api::search::SearchSort::Date {
                        results
                            .hits
                            .sort_by(|a, b| b.message.date.cmp(&a.message.date));
                    }
                }
                Response::SearchResults {
                    offset,
                    total: results.total,
                    hits: results.hits,
                    server,
                }
            }
            Err(error) => Response::InvalidQuery { error },
        })
    }

    /// The IMAP `SEARCH` fallback: in every folder in scope that holds
    /// messages without a local body, asks the server and adds the
    /// headers-only messages it finds (not found locally already).
    /// `None` when no folder in scope has such messages.
    async fn server_search(
        &self,
        query: &nail_search::Query,
        scope: &SearchScope,
        filter: Option<&(dyn Fn(&nail_api::Folder) -> bool + Send + Sync)>,
        cap: usize,
        hits: &mut Vec<nail_api::SearchHit>,
    ) -> Option<nail_api::search::ServerSearch> {
        let scope = scope.clone();
        let folders = self
            .inner
            .store
            .call(move |s| {
                let accounts = match &scope.accounts {
                    Some(accounts) => accounts.clone(),
                    None => s.list_accounts()?.into_iter().map(|a| a.id).collect(),
                };
                let mut out = Vec::new();
                for account in accounts {
                    for folder in s.list_folders(&account)? {
                        if scope
                            .folders
                            .as_ref()
                            .is_some_and(|f| !f.contains(&folder.id))
                        {
                            continue;
                        }
                        if s.headers_only_count(&folder.id)? > 0 {
                            out.push(folder);
                        }
                    }
                }
                Ok(out)
            })
            .await
            .ok()?;
        let folders: Vec<nail_api::Folder> = folders
            .into_iter()
            .filter(|f| filter.is_none_or(|allowed| allowed(f)))
            .collect();
        if folders.is_empty() || cap == 0 {
            return None;
        }
        let key = nail_search::imap::server_search(query, unix_now(), local_utc_offset());
        let mut info = nail_api::search::ServerSearch::default();
        let mut by_account: std::collections::BTreeMap<nail_api::AccountId, Vec<nail_api::Folder>> =
            std::collections::BTreeMap::new();
        for folder in folders {
            by_account
                .entry(folder.account_id.clone())
                .or_default()
                .push(folder);
        }
        let mut added = 0usize;
        for (account, folders) in by_account {
            info.accounts.push(account.clone());
            let Some(settings) = self.imap_settings(&account) else {
                info.failed.push(account);
                continue;
            };
            let mut client = match crate::sync::connect(&settings).await {
                Ok(client) => client,
                Err(err) => {
                    tracing::info!(%account, %err, "server search: cannot connect");
                    info.failed.push(account);
                    continue;
                }
            };
            let mut found: Vec<(nail_api::FolderId, u32)> = Vec::new();
            let mut failed = false;
            for folder in &folders {
                match client.search_messages(&folder.path, &key.key).await {
                    Ok(uids) => found.extend(uids.into_iter().map(|u| (folder.id.clone(), u))),
                    Err(err) => {
                        tracing::info!(%account, folder = %folder.path, %err, "server search failed");
                        failed = true;
                    }
                }
            }
            let _ = client.logout().await;
            if failed {
                info.failed.push(account.clone());
            }
            let known: std::collections::HashSet<nail_api::MessageId> =
                hits.iter().map(|h| h.message.id.clone()).collect();
            let room = cap.saturating_sub(added);
            let summaries = self
                .inner
                .store
                .call(move |s| {
                    let mut out = Vec::new();
                    for (folder, uid) in found {
                        if out.len() >= room {
                            break;
                        }
                        if let Some(m) = s.find_message_by_uid(&folder, uid)?
                            && m.summary.headers_only
                            && !known.contains(&m.summary.id)
                        {
                            out.push(m.summary);
                        }
                    }
                    Ok(out)
                })
                .await
                .unwrap_or_default();
            added += summaries.len();
            info.hits += u32::try_from(summaries.len()).unwrap_or(u32::MAX);
            hits.extend(summaries.into_iter().map(|message| nail_api::SearchHit {
                message,
                score: 0.0,
                from_server: true,
            }));
        }
        Some(info)
    }
}

/// Commits staged index changes that are due, every second.
pub(crate) async fn run_committer(service: Service) {
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    loop {
        timer.tick().await;
        let index = service.inner.search.get();
        let result = tokio::task::spawn_blocking(move || index.commit_if_due()).await;
        match result {
            Ok(Err(err)) => tracing::warn!(%err, "committing the search index failed"),
            Err(err) => tracing::warn!(%err, "search index committer failed"),
            Ok(Ok(_)) => {}
        }
    }
}

/// Backfills threading headers and rebuilds the index when needed.
impl Service {
    /// Rebuilds the search index from the store (blocking). For data put
    /// into the store directly, such as the demo seed. Returns the number
    /// of indexed messages.
    pub fn rebuild_search_index(&self) -> Result<usize> {
        Ok(crate::ingest::reindex_all(
            &self.inner.store,
            &self.inner.blobs,
            &self.inner.search.get(),
        )?)
    }
}

pub(crate) async fn run_maintenance(service: Service) {
    let store = service.inner.store.clone();
    let blobs = service.inner.blobs.clone();
    let index = service.inner.search.get();
    let settings = crate::security::security_settings(&service)
        .await
        .unwrap_or_default();
    let owner = service.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let backfilled = crate::ingest::backfill_meta(&store, &blobs)?;
        if backfilled > 0 {
            tracing::info!(backfilled, "threading headers of older messages backfilled");
        }
        let stored = u64::from(store.count_messages(&MessageFilter::All)?);
        let indexed = index.doc_count()?;
        if index.was_rebuilt() || indexed < stored {
            tracing::info!(stored, indexed, "rebuilding the search index");
            let count = crate::ingest::reindex_all(&store, &blobs, &index)?;
            tracing::info!(count, "search index rebuilt");
            // Accounts that opted in get their decrypted content back (M2a).
            let policy = nail_search::IndexPolicy::from_settings(&settings);
            owner.reindex_encrypted(&settings.index_decrypted, &policy)?;
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Err(err)) => tracing::warn!(%err, "search index maintenance failed"),
        Err(err) => tracing::warn!(%err, "search index maintenance task failed"),
        Ok(Ok(())) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_is_within_a_day() {
        assert!(local_utc_offset().abs() <= 14 * 3600);
    }
}
