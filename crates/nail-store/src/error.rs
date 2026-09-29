//! The store's error type.

use nail_api::{ApiError, ErrorCode};

use crate::blob::BlobHash;

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by [`crate::Store`] and [`crate::BlobStore`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An SQLite error.
    #[error("sqlite: {0}")]
    Sqlite(rusqlite::Error),
    /// A file-system error.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// A (de)serialization error of a stored JSON column.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// The addressed row or blob does not exist.
    #[error("{kind} not found: {id}")]
    NotFound {
        /// What was looked up, e.g. `"message"`.
        kind: &'static str,
        /// The id or hash that was looked up.
        id: String,
    },
    /// The database was written by a newer Nail version.
    #[error("database schema version {found} is newer than the supported version {supported}")]
    SchemaTooNew {
        /// Version found in the database.
        found: u32,
        /// Highest version this build knows.
        supported: u32,
    },
    /// A string is not a valid blob hash (64 lowercase hex characters).
    #[error("invalid blob hash {0:?}")]
    InvalidHash(String),
    /// A blob file failed decompression or its content does not match its hash.
    #[error("blob {0} is corrupt")]
    BlobCorrupt(BlobHash),
    /// Input or stored data violates an invariant (e.g. a message whose
    /// account differs from its folder's account).
    #[error("invalid data: {0}")]
    InvalidData(String),
    /// The request conflicts with the current state (e.g. cancelling a
    /// message that is already being sent).
    #[error("conflict: {0}")]
    Conflict(String),
    /// Sealed data (encryption at rest) cannot be read or written because
    /// the data key is not available.
    #[error("locked: {0}")]
    Locked(String),
    /// Sealed data could not be opened (damaged, another key) or sealing
    /// failed.
    #[error("crypto: {0}")]
    Crypto(String),
    /// A blocking store task panicked or was cancelled.
    #[error("store task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        // Errors of the sealing SQL functions arrive as SQLite errors.
        crate::seal::map_sqlite_error(err)
    }
}

impl Error {
    pub(crate) fn not_found(kind: &'static str, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            kind,
            id: id.to_string(),
        }
    }

    /// Whether this is [`Error::NotFound`].
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        let code = match &err {
            Error::NotFound { .. } => ErrorCode::NotFound,
            Error::InvalidData(_) | Error::InvalidHash(_) => ErrorCode::InvalidRequest,
            Error::Conflict(_) => ErrorCode::Conflict,
            Error::Locked(_) => ErrorCode::Locked,
            Error::Crypto(_) => ErrorCode::Crypto,
            _ => ErrorCode::Internal,
        };
        ApiError::new(code, err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_api_error_codes() {
        let api: ApiError = Error::not_found("message", "m1").into();
        assert_eq!(api.code, ErrorCode::NotFound);
        assert_eq!(api.message, "message not found: m1");

        let api: ApiError = Error::InvalidData("x".into()).into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);

        let api: ApiError = Error::Conflict("sending".into()).into();
        assert_eq!(api.code, ErrorCode::Conflict);

        let api: ApiError = Error::Locked("vault".into()).into();
        assert_eq!(api.code, ErrorCode::Locked);
        let api: ApiError = Error::Crypto("tag".into()).into();
        assert_eq!(api.code, ErrorCode::Crypto);

        let api: ApiError = Error::Io(std::io::Error::other("disk")).into();
        assert_eq!(api.code, ErrorCode::Internal);
    }
}
