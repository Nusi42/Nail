//! Collection sync: `sync-collection` (RFC 6578) when the server offers
//! it, else the CalendarServer `getctag` plus an ETag listing; changed
//! members are fetched with multiget. Invalid sync tokens restart from
//! an initial sync, reporting members that disappeared as removed.

use std::collections::{BTreeMap, BTreeSet};

use url::Url;

use crate::client::DavClient;
use crate::error::DavError;
use crate::ops::{CollectionKind, DavObject};

/// What the client knows about a collection after the last sync (the
/// core stores it with the collection: sync token / ctag in
/// `pim_collections`, ETags with the raw objects).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectionState {
    /// RFC 6578 token of the last sync, if that used `sync-collection`.
    pub sync_token: Option<String>,
    /// `getctag` of the last ctag / ETag sync.
    pub ctag: Option<String>,
    /// Member href (path) → ETag.
    pub etags: BTreeMap<String, String>,
}

/// How a sync ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMethod {
    /// `sync-collection` reports.
    SyncCollection,
    /// A full ETag listing (PROPFIND depth 1).
    Etags,
    /// The ctag had not changed: nothing was listed.
    Unchanged,
}

/// Options of [`sync_collection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncOptions {
    /// Try `sync-collection` first (when the collection advertises it).
    pub use_sync_collection: bool,
    /// `limit` per sync report (`None` = the server's choice).
    pub page_limit: Option<u32>,
    /// Most report pages fetched in one sync (truncated answers).
    pub max_pages: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            use_sync_collection: true,
            page_limit: None,
            max_pages: 100,
        }
    }
}

/// The changes since the previous state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// New or changed members, with data and ETag.
    pub changed: Vec<DavObject>,
    /// Hrefs of removed members.
    pub removed: Vec<String>,
    /// The state to store for the next sync.
    pub state: CollectionState,
    /// How the sync ran.
    pub method: SyncMethod,
    /// The server had forgotten the previous sync token; the collection
    /// was listed again from scratch.
    pub token_reset: bool,
}

/// Errors that mean "this server cannot do `sync-collection` here".
fn sync_unsupported(error: &DavError) -> bool {
    match error {
        DavError::Forbidden(_) => true,
        DavError::Status { status, .. } => matches!(status, 400 | 405 | 415 | 422 | 501),
        _ => false,
    }
}

/// Syncs one collection from `previous`.
pub async fn sync_collection(
    client: &DavClient,
    collection: &Url,
    kind: CollectionKind,
    previous: &CollectionState,
    options: SyncOptions,
) -> Result<SyncOutcome, DavError> {
    if options.use_sync_collection {
        let mut token_reset = false;
        let mut token = previous.sync_token.clone();
        loop {
            match by_report(
                client,
                collection,
                kind,
                previous,
                token.as_deref(),
                options,
            )
            .await
            {
                Ok(mut outcome) => {
                    outcome.token_reset = token_reset;
                    return Ok(outcome);
                }
                Err(DavError::InvalidSyncToken) if token.is_some() => {
                    tracing::info!(%collection, "sync token no longer valid; syncing from scratch");
                    token_reset = true;
                    token = None;
                }
                Err(e) if sync_unsupported(&e) => {
                    tracing::debug!(%collection, error = %e, "sync-collection unsupported; using ETags");
                    let mut outcome = by_etags(client, collection, kind, previous).await?;
                    outcome.token_reset = token_reset;
                    return Ok(outcome);
                }
                Err(e) => return Err(e),
            }
        }
    }
    by_etags(client, collection, kind, previous).await
}

async fn by_report(
    client: &DavClient,
    collection: &Url,
    kind: CollectionKind,
    previous: &CollectionState,
    token: Option<&str>,
    options: SyncOptions,
) -> Result<SyncOutcome, DavError> {
    let full = token.is_none();
    let mut token = token.map(str::to_owned);
    let mut changed: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut pages = 0;
    loop {
        pages += 1;
        let report = client
            .sync_collection(collection, token.as_deref(), options.page_limit)
            .await?;
        for (href, etag) in report.changed {
            removed.remove(&href);
            changed.insert(href, etag);
        }
        for href in report.removed {
            changed.remove(&href);
            removed.insert(href);
        }
        let next = report.token.ok_or_else(|| {
            DavError::Protocol("sync-collection answer without a sync-token".into())
        })?;
        let advanced = token.as_deref() != Some(next.as_str());
        token = Some(next);
        if !report.truncated {
            break;
        }
        if pages >= options.max_pages.max(1) || !advanced {
            return Err(DavError::Protocol(
                "sync-collection kept answering truncated results".into(),
            ));
        }
    }
    let mut etags = if full {
        // An initial sync lists every member: whatever is not listed is gone.
        removed.extend(
            previous
                .etags
                .keys()
                .filter(|h| !changed.contains_key(*h))
                .cloned(),
        );
        BTreeMap::new()
    } else {
        previous.etags.clone()
    };
    for href in &removed {
        etags.remove(href);
    }
    let mut fetch = Vec::new();
    for (href, etag) in &changed {
        match etag {
            Some(etag) if previous.etags.get(href) == Some(etag) => {
                etags.insert(href.clone(), etag.clone());
            }
            _ => fetch.push(href.clone()),
        }
    }
    let (objects, missing) = fetch_members(client, collection, kind, &fetch).await?;
    for object in &objects {
        match &object.etag {
            Some(etag) => {
                etags.insert(object.href.clone(), etag.clone());
            }
            None => {
                etags.remove(&object.href);
            }
        }
    }
    for href in missing {
        etags.remove(&href);
        if previous.etags.contains_key(&href) {
            removed.insert(href);
        }
    }
    Ok(SyncOutcome {
        changed: objects,
        removed: removed
            .into_iter()
            .filter(|h| previous.etags.contains_key(h))
            .collect(),
        state: CollectionState {
            sync_token: token,
            ctag: None,
            etags,
        },
        method: SyncMethod::SyncCollection,
        token_reset: false,
    })
}

async fn by_etags(
    client: &DavClient,
    collection: &Url,
    kind: CollectionKind,
    previous: &CollectionState,
) -> Result<SyncOutcome, DavError> {
    let (ctag, _) = client.ctag(collection).await?;
    if ctag.is_some() && ctag == previous.ctag {
        return Ok(SyncOutcome {
            changed: Vec::new(),
            removed: Vec::new(),
            state: CollectionState {
                sync_token: None,
                ..previous.clone()
            },
            method: SyncMethod::Unchanged,
            token_reset: false,
        });
    }
    let listing: BTreeMap<String, String> =
        client.list_etags(collection).await?.into_iter().collect();
    let removed: Vec<String> = previous
        .etags
        .keys()
        .filter(|h| !listing.contains_key(*h))
        .cloned()
        .collect();
    let fetch: Vec<String> = listing
        .iter()
        .filter(|(href, etag)| previous.etags.get(*href) != Some(etag))
        .map(|(href, _)| href.clone())
        .collect();
    let (objects, missing) = fetch_members(client, collection, kind, &fetch).await?;
    let mut etags = listing;
    for object in &objects {
        if let Some(etag) = &object.etag {
            etags.insert(object.href.clone(), etag.clone());
        }
    }
    let mut removed = removed;
    for href in missing {
        etags.remove(&href);
        if previous.etags.contains_key(&href) {
            removed.push(href);
        }
    }
    Ok(SyncOutcome {
        changed: objects,
        removed,
        state: CollectionState {
            sync_token: None,
            ctag,
            etags,
        },
        method: SyncMethod::Etags,
        token_reset: false,
    })
}

async fn fetch_members(
    client: &DavClient,
    collection: &Url,
    kind: CollectionKind,
    hrefs: &[String],
) -> Result<(Vec<DavObject>, Vec<String>), DavError> {
    if hrefs.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let result = client.multiget(collection, kind, hrefs).await?;
    Ok((result.objects, result.missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_errors() {
        assert!(sync_unsupported(&DavError::Forbidden("x".into())));
        assert!(sync_unsupported(&DavError::Status {
            status: 501,
            message: String::new()
        }));
        assert!(!sync_unsupported(&DavError::AuthFailed));
        assert!(!sync_unsupported(&DavError::Network("x".into())));
        let options = SyncOptions::default();
        assert!(options.use_sync_collection && options.max_pages > 1);
    }
}
