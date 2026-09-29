//! Errors of `nail-pgp`, mapped onto the protocol's [`ErrorCode`]s.

use nail_api::engine::EngineError;
use nail_api::{ApiError, ErrorCode, KeyId};

/// A `nail-pgp` error. Messages are English diagnostics for logs; the UI
/// shows the [`ErrorCode`] ([`Error::code`]).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A secret key is protected by a passphrase that was not entered
    /// (`locked`; the UI asks for it and sends `UnlockKey`).
    #[error("the secret key {key_id} is locked")]
    Locked {
        /// The own key that needs its passphrase.
        key_id: KeyId,
    },
    /// A passphrase or passcode is wrong (`auth_failed`).
    #[error("wrong passphrase")]
    BadPassphrase,
    /// A needed key is missing or unusable (`no_key`).
    #[error("no usable key: {0}")]
    NoKey(String),
    /// Signing, encryption or verification failed, or key data is
    /// malformed (`crypto`).
    #[error("openpgp: {0}")]
    Crypto(String),
    /// The input is not what the call expects (`invalid_request`).
    #[error("invalid input: {0}")]
    Invalid(String),
    /// Not supported (`unsupported`).
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The keyring store failed.
    #[error(transparent)]
    Store(#[from] nail_store::Error),
    /// The OS keychain failed.
    #[error(transparent)]
    Secrets(#[from] EngineError),
}

impl Error {
    /// The protocol error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Locked { .. } => ErrorCode::Locked,
            Self::BadPassphrase => ErrorCode::AuthFailed,
            Self::NoKey(_) => ErrorCode::NoKey,
            Self::Crypto(_) => ErrorCode::Crypto,
            Self::Invalid(_) => ErrorCode::InvalidRequest,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::Store(e) => store_code(e),
            Self::Secrets(e) => e.code,
        }
    }

    pub(crate) fn crypto(err: impl std::fmt::Display) -> Self {
        Self::Crypto(err.to_string())
    }
}

fn store_code(e: &nail_store::Error) -> ErrorCode {
    match e {
        nail_store::Error::NotFound { .. } => ErrorCode::NotFound,
        nail_store::Error::InvalidData(_) => ErrorCode::InvalidRequest,
        nail_store::Error::Conflict(_) => ErrorCode::Conflict,
        nail_store::Error::Locked(_) => ErrorCode::Locked,
        nail_store::Error::Crypto(_) => ErrorCode::Crypto,
        _ => ErrorCode::Internal,
    }
}

impl From<pgp::errors::Error> for Error {
    fn from(e: pgp::errors::Error) -> Self {
        Self::crypto(e)
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::Store(store) => store.into(),
            Error::Secrets(engine) => ApiError::new(engine.code, engine.message),
            other => ApiError::new(other.code(), other.to_string()),
        }
    }
}

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_protocol_codes() {
        let locked = Error::Locked {
            key_id: KeyId("key-1".into()),
        };
        assert_eq!(locked.code(), ErrorCode::Locked);
        assert_eq!(Error::BadPassphrase.code(), ErrorCode::AuthFailed);
        assert_eq!(Error::NoKey("x".into()).code(), ErrorCode::NoKey);
        assert_eq!(Error::Crypto("x".into()).code(), ErrorCode::Crypto);
        assert_eq!(Error::Invalid("x".into()).code(), ErrorCode::InvalidRequest);
        assert_eq!(
            Error::Unsupported("x".into()).code(),
            ErrorCode::Unsupported
        );
        let conflict = Error::Store(nail_store::Error::Conflict("own key".into()));
        assert_eq!(conflict.code(), ErrorCode::Conflict);
        let api: ApiError = conflict.into();
        assert_eq!(api.code, ErrorCode::Conflict);
        let api: ApiError = locked.into();
        assert_eq!(api.code, ErrorCode::Locked);
        assert!(api.message.contains("key-1"));
        let engine = EngineError::permanent(ErrorCode::Unavailable, "keychain");
        assert_eq!(Error::Secrets(engine).code(), ErrorCode::Unavailable);
    }
}
