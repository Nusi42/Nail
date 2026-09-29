//! Errors of the vault.

use nail_api::engine::EngineError;
use nail_api::{ApiError, ErrorCode};

/// An error of encryption at rest.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VaultError {
    /// The data key is not available (missing from the keychain, or the
    /// keychain is locked). Encrypted data cannot be read.
    #[error("locked: {0}")]
    Locked(String),
    /// Sealed data failed authentication, has an unknown format, or the
    /// key does not match the key check value.
    #[error("crypto: {0}")]
    Crypto(String),
    /// The OS keychain failed.
    #[error("keychain: {0}")]
    Keychain(#[from] EngineError),
    /// The store failed.
    #[error("store: {0}")]
    Store(#[from] nail_store::Error),
    /// File-system error.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

impl VaultError {
    /// The API error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Locked(_) => ErrorCode::Locked,
            Self::Crypto(_) => ErrorCode::Crypto,
            Self::Keychain(err) => err.code,
            Self::Store(err) => match err {
                nail_store::Error::Locked(_) => ErrorCode::Locked,
                nail_store::Error::Crypto(_) => ErrorCode::Crypto,
                nail_store::Error::Conflict(_) => ErrorCode::Conflict,
                nail_store::Error::NotFound { .. } => ErrorCode::NotFound,
                _ => ErrorCode::Internal,
            },
            Self::Io(_) => ErrorCode::Internal,
        }
    }
}

impl From<nail_store::CipherError> for VaultError {
    fn from(err: nail_store::CipherError) -> Self {
        match err {
            nail_store::CipherError::Locked(m) => Self::Locked(m),
            nail_store::CipherError::Failed(m) => Self::Crypto(m),
        }
    }
}

impl From<VaultError> for nail_store::CipherError {
    fn from(err: VaultError) -> Self {
        match err {
            VaultError::Locked(m) => Self::Locked(m),
            VaultError::Crypto(m) => Self::Failed(m),
            other => Self::Failed(other.to_string()),
        }
    }
}

impl From<VaultError> for ApiError {
    fn from(err: VaultError) -> Self {
        ApiError::new(err.code(), err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(VaultError::Locked("x".into()).code(), ErrorCode::Locked);
        assert_eq!(VaultError::Crypto("x".into()).code(), ErrorCode::Crypto);
        assert_eq!(
            VaultError::Store(nail_store::Error::Locked("x".into())).code(),
            ErrorCode::Locked
        );
        assert_eq!(
            VaultError::Store(nail_store::Error::Io(std::io::Error::other("x"))).code(),
            ErrorCode::Internal
        );
        assert_eq!(
            VaultError::Keychain(EngineError::permanent(ErrorCode::Unavailable, "locked")).code(),
            ErrorCode::Unavailable
        );
        let api: ApiError = VaultError::Io(std::io::Error::other("disk")).into();
        assert_eq!(api.code, ErrorCode::Internal);
        let cipher: nail_store::CipherError = VaultError::Locked("k".into()).into();
        assert_eq!(cipher, nail_store::CipherError::Locked("k".into()));
    }
}
