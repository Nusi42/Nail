//! The core's error type.

use nail_api::engine::EngineError;
use nail_api::{ApiError, ErrorCode};

/// Result alias of this crate.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Errors of the service and the engines. Converted to [`ApiError`] (and
/// so to `Response::Error` / `Event::SyncFailed`) at the API boundary.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// The local store failed.
    #[error(transparent)]
    Store(#[from] nail_store::Error),
    /// Talking to an IMAP server failed.
    #[error(transparent)]
    Imap(#[from] nail_proto_imap::ImapError),
    /// Talking to a POP3 server failed.
    #[error(transparent)]
    Pop3(#[from] nail_proto_pop3::Pop3Error),
    /// The search index failed.
    #[error(transparent)]
    Search(#[from] nail_search::Error),
    /// A configuration file could not be read, watched or written.
    #[error(transparent)]
    Config(#[from] nail_config::ConfigError),
    /// An engine seam (keychain, OAuth2, SMTP) failed.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// An error already expressed in API terms.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// A background task panicked or was cancelled.
    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    /// A file-system error outside the store (e.g. creating directories).
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// OpenPGP (M2a, `nail-pgp`).
    #[error(transparent)]
    Pgp(#[from] nail_pgp::Error),
    /// S/MIME (M2a, `nail-smime`).
    #[error(transparent)]
    Smime(#[from] nail_smime::Error),
    /// Encryption at rest (M2a, `nail-vault`).
    #[error(transparent)]
    Vault(#[from] nail_vault::VaultError),
}

impl From<nail_auth::AuthError> for CoreError {
    fn from(err: nail_auth::AuthError) -> Self {
        Self::Engine(err.into())
    }
}

impl CoreError {
    /// The API error code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Store(err) => store_error_code(err),
            Self::Imap(err) => err.code(),
            Self::Pop3(err) => err.code(),
            Self::Search(_) | Self::Task(_) | Self::Io(_) => ErrorCode::Internal,
            Self::Config(_) => ErrorCode::Config,
            Self::Engine(err) => err.code,
            Self::Api(err) => err.code,
            Self::Pgp(err) => err.code(),
            Self::Smime(err) => err.code(),
            Self::Vault(err) => err.code(),
        }
    }

    /// Whether trying again later (e.g. after reconnecting) may succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Imap(err) => err.is_transient(),
            Self::Pop3(err) => err.is_retryable(),
            Self::Engine(err) => err.retryable,
            Self::Api(err) => matches!(err.code, ErrorCode::Network | ErrorCode::Unavailable),
            _ => false,
        }
    }
}

/// The same mapping as `From<nail_store::Error> for ApiError`, by reference.
fn store_error_code(err: &nail_store::Error) -> ErrorCode {
    if err.is_not_found() {
        ErrorCode::NotFound
    } else if matches!(err, nail_store::Error::Conflict(_)) {
        ErrorCode::Conflict
    } else if matches!(
        err,
        nail_store::Error::InvalidData(_) | nail_store::Error::InvalidHash(_)
    ) {
        ErrorCode::InvalidRequest
    } else if matches!(err, nail_store::Error::Locked(_)) {
        ErrorCode::Locked
    } else if matches!(err, nail_store::Error::Crypto(_)) {
        ErrorCode::Crypto
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

impl From<CoreError> for EngineError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::Engine(e) => e,
            other => Self {
                code: other.code(),
                retryable: other.is_transient(),
                message: other.to_string(),
                localized: None,
            },
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

        let err: ApiError = CoreError::from(nail_store::Error::Conflict("busy".into())).into();
        assert_eq!(err.code, ErrorCode::Conflict);

        let err: ApiError = CoreError::from(nail_proto_imap::ImapError::AuthFailed(
            "bad password".into(),
        ))
        .into();
        assert_eq!(err.code, ErrorCode::AuthFailed);

        let err: ApiError = CoreError::from(ApiError::not_found("message m1")).into();
        assert_eq!(err, ApiError::not_found("message m1"));

        let err: ApiError = CoreError::from(std::io::Error::other("disk")).into();
        assert_eq!(err.code, ErrorCode::Internal);

        let err: ApiError =
            CoreError::from(EngineError::temporary(ErrorCode::Network, "down")).into();
        assert_eq!(err.code, ErrorCode::Network);
    }

    #[test]
    fn maps_m2a_errors() {
        let code = |err: CoreError| ApiError::from(err).code;
        assert_eq!(
            code(nail_store::Error::Locked("vault".into()).into()),
            ErrorCode::Locked
        );
        assert_eq!(
            code(nail_store::Error::Crypto("tag".into()).into()),
            ErrorCode::Crypto
        );
        assert_eq!(
            code(nail_pgp::Error::NoKey("bob".into()).into()),
            ErrorCode::NoKey
        );
        assert_eq!(
            code(nail_pgp::Error::BadPassphrase.into()),
            ErrorCode::AuthFailed
        );
        assert_eq!(
            code(nail_smime::Error::Locked("fp".into()).into()),
            ErrorCode::Locked
        );
        assert_eq!(
            code(nail_vault::VaultError::Crypto("check".into()).into()),
            ErrorCode::Crypto
        );
    }

    #[test]
    fn transient_errors() {
        assert!(CoreError::from(nail_proto_imap::ImapError::ConnectionLost).is_transient());
        assert!(
            !CoreError::from(nail_proto_imap::ImapError::AuthFailed("x".into())).is_transient()
        );
        assert!(CoreError::from(EngineError::temporary(ErrorCode::Network, "x")).is_transient());
        let engine: EngineError =
            CoreError::from(nail_proto_imap::ImapError::ConnectionLost).into();
        assert!(engine.retryable);
        assert_eq!(engine.code, ErrorCode::Network);
    }
}
