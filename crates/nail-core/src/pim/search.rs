//! The PIM search index (`nail-search`, ADR 0061): kept **in memory** and
//! built from the store on first use (ADR 0063: PIM data is small, so
//! nothing of it is written to disk outside the sealed store, and the
//! vault migration has nothing to convert), then updated on every change.

use std::sync::Arc;

use nail_api::pim::{PimItemKind, PimItemRef, PimSearchHit};
use nail_search::{CommitPolicy, PimDocument, PimIndex};
use nail_store::Store;

use crate::error::Result;
use crate::service::{Service, lock};

/// Most hits of `SearchPim`.
const MAX_HITS: u32 = 200;

/// Every item of the store as index documents.
fn all_documents(store: &Store) -> nail_store::Result<Vec<PimDocument>> {
    let mut docs: Vec<PimDocument> = Vec::new();
    docs.extend(store.events(None)?.iter().map(PimDocument::event));
    let calendars: Vec<_> = store
        .calendars(None)?
        .into_iter()
        .filter(nail_api::Calendar::has_tasks)
        .map(|c| c.id)
        .collect();
    docs.extend(
        store
            .list_tasks(Some(&calendars), true)?
            .iter()
            .map(PimDocument::task),
    );
    docs.extend(store.contacts(None)?.iter().map(PimDocument::contact));
    docs.extend(store.contact_groups(None)?.iter().map(PimDocument::group));
    Ok(docs)
}

/// The document of one item, `None` when it is gone.
fn document(store: &Store, item: &PimItemRef) -> nail_store::Result<Option<PimDocument>> {
    Ok(match item {
        PimItemRef::Event { event_id } => store.event(event_id)?.as_ref().map(PimDocument::event),
        PimItemRef::Task { task_id } => store.task(task_id)?.as_ref().map(PimDocument::task),
        PimItemRef::Contact { contact_id } => store
            .contact(contact_id)?
            .as_ref()
            .map(PimDocument::contact),
        PimItemRef::ContactGroup { group_id } => store
            .contact_group(group_id)?
            .as_ref()
            .map(PimDocument::group),
    })
}

impl Service {
    /// The index, built on first use.
    pub(crate) async fn pim_index(&self) -> Result<Arc<PimIndex>> {
        if let Some(index) = lock(&self.inner.pim.index).clone() {
            return Ok(index);
        }
        let store = self.inner.store.clone();
        let index = tokio::task::spawn_blocking(move || -> Result<PimIndex> {
            let index = PimIndex::open_in_ram(CommitPolicy::default())?;
            index.add(&all_documents(&store)?)?;
            index.commit()?;
            Ok(index)
        })
        .await??;
        let index = Arc::new(index);
        let mut slot = lock(&self.inner.pim.index);
        // Another request may have built it meanwhile; keep the first.
        Ok(Arc::clone(slot.get_or_insert(index)))
    }

    /// Drops the index (rebuilt on next use).
    pub(crate) fn reindex_pim(&self) {
        *lock(&self.inner.pim.index) = None;
    }

    /// Re-indexes changed items and drops removed ones (when the index is
    /// built; otherwise its build reads them).
    pub(crate) async fn pim_index_apply(&self, items: &[PimItemRef], removed: &[PimItemRef]) {
        let Some(index) = lock(&self.inner.pim.index).clone() else {
            return;
        };
        let store = self.inner.store.clone();
        let items = items.to_vec();
        let removed = removed.to_vec();
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let mut docs = Vec::new();
            let mut gone = removed;
            for item in &items {
                match document(&store, item)? {
                    Some(doc) => docs.push(doc),
                    None => gone.push(item.clone()),
                }
            }
            index.remove(&gone)?;
            index.add(&docs)?;
            Ok(())
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(%err, "cannot update the PIM index"),
            Err(err) => tracing::warn!(%err, "PIM index task failed"),
        }
    }

    /// Re-indexes one item (or drops it when it is gone).
    pub(crate) async fn pim_index_update(&self, item: &PimItemRef) {
        self.pim_index_apply(std::slice::from_ref(item), &[]).await;
    }

    /// Drops a deleted collection's items.
    pub(crate) fn pim_index_remove_collection(&self, collection: &str) {
        if let Some(index) = lock(&self.inner.pim.index).clone()
            && let Err(err) = index.remove_collection(collection)
        {
            tracing::warn!(%err, "cannot update the PIM index");
        }
    }

    /// `SearchPim`.
    pub(crate) async fn search_pim(
        &self,
        query: String,
        kinds: Vec<PimItemKind>,
        limit: u32,
    ) -> Result<Vec<PimSearchHit>> {
        let limit = limit.clamp(1, MAX_HITS) as usize;
        let index = self.pim_index().await?;
        let found = tokio::task::spawn_blocking(move || index.search(&query, &kinds, None, limit))
            .await??;
        let items: Vec<PimItemRef> = found.into_iter().map(|(item, _)| item).collect();
        Ok(self
            .inner
            .store
            .call(move |s| s.pim_search_hits(&items))
            .await?)
    }
}
