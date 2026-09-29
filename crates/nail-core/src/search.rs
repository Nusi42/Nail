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
use nail_search::{SearchContext, SearchScope, VirtualAccountScope, run_request};
use nail_store::MessageFilter;

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::{MAX_PAGE_SIZE, Service};

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
    /// members), run.
    pub(crate) async fn search(&self, mut query: SearchQuery) -> Result<Response> {
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
        let index = Arc::clone(&self.inner.search);
        let store = self.inner.store.clone();
        let offset = query.offset;
        let outcome =
            tokio::task::spawn_blocking(move || run_request(&index, &store, &query, &scope, &ctx))
                .await??;
        Ok(match outcome {
            Ok(results) => Response::SearchResults {
                offset,
                total: results.total,
                hits: results.hits,
            },
            Err(error) => Response::InvalidQuery { error },
        })
    }
}

/// Commits staged index changes that are due, every second.
pub(crate) async fn run_committer(service: Service) {
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    loop {
        timer.tick().await;
        let index = Arc::clone(&service.inner.search);
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
            &self.inner.search,
        )?)
    }
}

pub(crate) async fn run_maintenance(service: Service) {
    let store = service.inner.store.clone();
    let blobs = service.inner.blobs.clone();
    let index = Arc::clone(&service.inner.search);
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
