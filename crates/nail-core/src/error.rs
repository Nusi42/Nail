//! The core's error type.

use nail_api::{ApiError, ErrorCode};

/// Result alias of this crate.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Errors of the service and the sync engine. Converted to [`ApiError`]
/// (and so to `Response::Error` / `Event::SyncFailed`) at the API boundary.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// The local store failed.
    #[error(transparent)]
    Store(#[from] nail_store::Error),
    /// Talking to an IMAP server failed.
    #[error(transparent)]
    Imap(#[from] nail_proto_imap::ImapError),
    /// The configuration could not be watched or loaded.
    #[error(transparent)]
    Config(#[from] nail_config::ConfigError),
    /// An error already expressed in API terms.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// A background task panicked or was cancelled.
    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    /// A file-system error outside the store (e.g. creating directories).
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

impl CoreError {
    /// The API error code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Store(err) => store_error_code(err),
            Self::Imap(err) => err.code(),
            Self::Config(_) => ErrorCode::Config,
            Self::Api(err) => err.code,
            Self::Task(_) | Self::Io(_) => ErrorCode::Internal,
        }
    }
}

/// The same mapping as `From<nail_store::Error> for ApiError`, by reference.
fn store_error_code(err: &nail_store::Error) -> ErrorCode {
    if err.is_not_found() {
        ErrorCode::NotFound
    } else if matches!(
        err,
        nail_store::Error::InvalidData(_) | nail_store::Error::InvalidHash(_)
    ) {
        ErrorCode::InvalidRequest
    } else {
        ErrorCode::Internal
    }
}

impl From<CoreError> for ApiError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::Api(api) => api,
            CoreError::Config(config) => config.into(),
            other => ApiError::new(other.code(), other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_api_codes() {
        let err: ApiError = CoreError::from(nail_store::Error::InvalidData("x".into())).into();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(err.message, "invalid data: x");

        let err: ApiError = CoreError::from(nail_proto_imap::ImapError::AuthFailed(
            "bad password".into(),
        ))
        .into();
        assert_eq!(err.code, ErrorCode::AuthFailed);

        let err: ApiError = CoreError::from(ApiError::not_found("message m1")).into();
        assert_eq!(err, ApiError::not_found("message m1"));

        let err: ApiError = CoreError::from(std::io::Error::other("disk")).into();
        assert_eq!(err.code, ErrorCode::Internal);
    }
}
