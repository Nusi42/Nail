//! The crate's error type.

use nail_api::{ApiError, ErrorCode};

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the index and of running searches.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A Tantivy error.
    #[error("search index: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    /// An index directory problem.
    #[error("search index: {0}")]
    Index(String),
    /// A file-system error.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The store failed while filtering.
    #[error(transparent)]
    Store(#[from] nail_store::Error),
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        match err {
            Error::Store(e) => e.into(),
            other => ApiError::new(ErrorCode::Internal, other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_api_errors() {
        let api: ApiError = Error::Store(nail_store::Error::InvalidData("x".into())).into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        let api: ApiError = Error::Index("broken".into()).into();
        assert_eq!(api.code, ErrorCode::Internal);
        assert_eq!(api.message, "search index: broken");
    }
}
