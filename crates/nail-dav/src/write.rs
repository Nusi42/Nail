//! Conflict-aware writes for the PIM operation queue.
//!
//! Every write is conditional: an update sends `If-Match` with the ETag
//! the local copy was based on, a new item `If-None-Match: *`. When the
//! server answers `412`, the **server wins**: the server's current
//! version (or its deletion) is returned as a conflict, and the core
//! shows the server's version and keeps the local one as a conflict copy
//! (`PimItemSync::Conflict`, ADR 0058).

use url::Url;

use crate::client::DavClient;
use crate::error::DavError;
use crate::ops::{CollectionKind, DavObject, Precondition};

/// The server's side of a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerVersion {
    /// The server's current version.
    Current(DavObject),
    /// The item was deleted on the server.
    Deleted,
}

/// The result of [`write_object`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Stored.
    Written {
        /// The new ETag, when known.
        etag: Option<String>,
        /// The server's text when it did not return an ETag and the item
        /// was read back (servers may rewrite what they store; RFC 4791
        /// §5.3.4).
        data: Option<String>,
    },
    /// Someone else changed (or deleted, or already created) the item.
    Conflict(ServerVersion),
}

/// The result of [`delete_object`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// Deleted.
    Deleted,
    /// It was already gone.
    AlreadyGone,
    /// It was changed on the server since the local copy was read; the
    /// server's version wins and stays.
    Conflict(DavObject),
}

/// Writes `data` to `url`: an update of the version with `known_etag`, or
/// (with `None`) a new item.
pub async fn write_object(
    client: &DavClient,
    url: &Url,
    kind: CollectionKind,
    data: &str,
    known_etag: Option<&str>,
) -> Result<WriteOutcome, DavError> {
    let precondition = match known_etag {
        Some(etag) => Precondition::IfMatch(etag.to_owned()),
        None => Precondition::Create,
    };
    match client.put(url, kind, data, &precondition).await {
        Ok(Some(etag)) => Ok(WriteOutcome::Written {
            etag: Some(etag),
            data: None,
        }),
        Ok(None) => match client.get(url).await {
            Ok(object) => Ok(WriteOutcome::Written {
                etag: object.etag,
                data: object.data,
            }),
            Err(DavError::NotFound(_)) => Ok(WriteOutcome::Written {
                etag: None,
                data: None,
            }),
            Err(e) => Err(e),
        },
        Err(DavError::PreconditionFailed) => {
            Ok(WriteOutcome::Conflict(server_version(client, url).await?))
        }
        Err(e) => Err(e),
    }
}

/// Deletes `url` if it still has `known_etag` (`None` = unconditionally).
pub async fn delete_object(
    client: &DavClient,
    url: &Url,
    known_etag: Option<&str>,
) -> Result<DeleteOutcome, DavError> {
    let precondition = match known_etag {
        Some(etag) => Precondition::IfMatch(etag.to_owned()),
        None => Precondition::None,
    };
    match client.delete(url, &precondition).await {
        Ok(()) => Ok(DeleteOutcome::Deleted),
        Err(DavError::NotFound(_)) => Ok(DeleteOutcome::AlreadyGone),
        Err(DavError::PreconditionFailed) => match server_version(client, url).await? {
            ServerVersion::Current(object) => Ok(DeleteOutcome::Conflict(object)),
            ServerVersion::Deleted => Ok(DeleteOutcome::AlreadyGone),
        },
        Err(e) => Err(e),
    }
}

async fn server_version(client: &DavClient, url: &Url) -> Result<ServerVersion, DavError> {
    match client.get(url).await {
        Ok(object) => Ok(ServerVersion::Current(object)),
        Err(DavError::NotFound(_)) => Ok(ServerVersion::Deleted),
        Err(e) => Err(e),
    }
}
