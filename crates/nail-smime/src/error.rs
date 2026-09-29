//! Errors of the S/MIME crate and their protocol error codes.

use nail_api::{ApiError, ErrorCode};

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What went wrong. Messages are English diagnostics for logs and
/// `ApiError::message`; the UI shows the localized text of [`Error::code`].
/// They never contain key material or passphrases.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Certificate, CMS or MIME data could not be parsed.
    #[error("malformed S/MIME data: {0}")]
    Malformed(String),
    /// An algorithm or format this version does not implement.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A cryptographic operation failed (bad signature, integrity check,
    /// wrong key).
    #[error("cryptographic operation failed: {0}")]
    Crypto(String),
    /// No usable key or certificate (no recipient matches, a recipient has
    /// no certificate, the signer has no private key).
    #[error("no usable key: {0}")]
    NoKey(String),
    /// The private key is protected by a passphrase that was not entered.
    #[error("the private key is locked: {0}")]
    Locked(String),
    /// A PKCS#12 or key passphrase is wrong.
    #[error("wrong passphrase")]
    WrongPassphrase,
    /// The keyring store failed.
    #[error(transparent)]
    Store(#[from] nail_store::Error),
    /// The OS keychain failed.
    #[error("keychain: {0}")]
    Keychain(String),
    /// A Windows certificate store call failed.
    #[error("Windows certificate store: {0}")]
    Windows(String),
}

impl Error {
    /// The protocol error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Malformed(_) | Self::Crypto(_) => ErrorCode::Crypto,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::NoKey(_) => ErrorCode::NoKey,
            Self::Locked(_) => ErrorCode::Locked,
            Self::WrongPassphrase => ErrorCode::AuthFailed,
            Self::Store(nail_store::Error::NotFound { .. }) => ErrorCode::NotFound,
            Self::Store(nail_store::Error::Conflict(_)) => ErrorCode::Conflict,
            Self::Store(_) | Self::Keychain(_) | Self::Windows(_) => ErrorCode::Internal,
        }
    }

    pub(crate) fn malformed(what: impl std::fmt::Display) -> Self {
        Self::Malformed(what.to_string())
    }
}

impl From<der::Error> for Error {
    fn from(err: der::Error) -> Self {
        Self::Malformed(err.to_string())
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        ApiError::new(err.code(), err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_protocol_classes() {
        assert_eq!(Error::malformed("x").code(), ErrorCode::Crypto);
        assert_eq!(Error::Crypto("x".into()).code(), ErrorCode::Crypto);
        assert_eq!(
            Error::Unsupported("x".into()).code(),
            ErrorCode::Unsupported
        );
        assert_eq!(Error::NoKey("x".into()).code(), ErrorCode::NoKey);
        assert_eq!(Error::Locked("x".into()).code(), ErrorCode::Locked);
        assert_eq!(Error::WrongPassphrase.code(), ErrorCode::AuthFailed);
        assert_eq!(Error::Windows("x".into()).code(), ErrorCode::Internal);
        let api: ApiError = Error::NoKey("bob@example.net".into()).into();
        assert_eq!(api.code, ErrorCode::NoKey);
        assert!(api.message.contains("bob@example.net"));
    }
}
