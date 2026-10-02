//! Errors of the vault.

use nail_api::engine::EngineError;
use nail_api::recovery::RecoveryKeyIssue;
use nail_api::{ApiError, ErrorCode, LocalizedMessage};

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
    /// A recovery key needs an encrypted vault (M4c).
    #[error("conflict: encryption at rest is off")]
    NotEncrypted,
    /// No recovery key is configured for this data directory (M4c).
    #[error("conflict: no recovery key is configured")]
    NoRecoveryKey,
    /// The typed recovery key is not even a key (M4c).
    #[error("invalid recovery key: {}", .0.as_str())]
    RecoveryKeyInvalid(RecoveryKeyIssue),
    /// The recovery key is well-formed but does not open the data key
    /// (M4c).
    #[error("auth failed: the recovery key does not open the data key")]
    RecoveryKeyWrong,
    /// The vault file next to the database is unreadable (M4c).
    #[error("vault file: {0}")]
    VaultFile(String),
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
            Self::Io(_) | Self::VaultFile(_) => ErrorCode::Internal,
            Self::NotEncrypted | Self::NoRecoveryKey => ErrorCode::Conflict,
            Self::RecoveryKeyInvalid(_) => ErrorCode::InvalidRequest,
            Self::RecoveryKeyWrong => ErrorCode::AuthFailed,
        }
    }

    /// The user-facing text, when there is one more specific than the
    /// error class (`core-vault.ftl`).
    pub fn localized(&self) -> Option<LocalizedMessage> {
        let id = match self {
            Self::NotEncrypted => "vault-not-encrypted",
            Self::NoRecoveryKey => "vault-no-recovery-key",
            Self::RecoveryKeyInvalid(issue) => {
                return Some(LocalizedMessage::new(issue.message_id()));
            }
            Self::RecoveryKeyWrong => "recovery-key-wrong",
            Self::Locked(_) => "vault-key-missing",
            Self::Crypto(_) => "vault-key-mismatch",
            Self::Store(nail_store::Error::Locked(_)) => "vault-key-missing",
            Self::Store(nail_store::Error::Crypto(_)) => "vault-key-mismatch",
            Self::VaultFile(_) => "vault-file-damaged",
            _ => return None,
        };
        Some(LocalizedMessage::new(id))
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
        let api = ApiError::new(err.code(), err.to_string());
        match err.localized() {
            Some(localized) => api.with_localized(localized),
            None => api,
        }
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

    #[test]
    fn recovery_errors_have_codes_and_texts() {
        let cases = [
            (
                VaultError::NotEncrypted,
                ErrorCode::Conflict,
                "vault-not-encrypted",
            ),
            (
                VaultError::NoRecoveryKey,
                ErrorCode::Conflict,
                "vault-no-recovery-key",
            ),
            (
                VaultError::RecoveryKeyInvalid(RecoveryKeyIssue::ChecksumMismatch),
                ErrorCode::InvalidRequest,
                "recovery-key-issue-checksum-mismatch",
            ),
            (
                VaultError::RecoveryKeyWrong,
                ErrorCode::AuthFailed,
                "recovery-key-wrong",
            ),
            (
                VaultError::Locked("x".into()),
                ErrorCode::Locked,
                "vault-key-missing",
            ),
            (
                VaultError::Store(nail_store::Error::Locked("x".into())),
                ErrorCode::Locked,
                "vault-key-missing",
            ),
            (
                VaultError::Crypto("x".into()),
                ErrorCode::Crypto,
                "vault-key-mismatch",
            ),
            (
                VaultError::VaultFile("x".into()),
                ErrorCode::Internal,
                "vault-file-damaged",
            ),
        ];
        for (err, code, id) in cases {
            assert_eq!(err.code(), code, "{err}");
            let api: ApiError = err.into();
            assert_eq!(api.localized.unwrap().id, id);
        }
        let api: ApiError = VaultError::Io(std::io::Error::other("disk")).into();
        assert!(api.localized.is_none());
    }

    #[test]
    fn every_text_id_is_in_the_english_catalog() {
        let catalog = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../locales/en/core-vault.ftl"
        ))
        .unwrap();
        let defined = |id: &str| catalog.lines().any(|l| l.starts_with(&format!("{id} =")));
        for id in [
            "vault-not-encrypted",
            "vault-no-recovery-key",
            "recovery-key-wrong",
            "vault-key-missing",
            "vault-key-mismatch",
            "vault-file-damaged",
            "recovery-kit-title",
            "recovery-kit-intro",
            "recovery-kit-key",
            "recovery-kit-created",
            "recovery-kit-advice",
        ] {
            assert!(defined(id), "{id} is missing in locales/en/core-vault.ftl");
        }
        for issue in RecoveryKeyIssue::ALL {
            assert!(defined(&issue.message_id()), "{}", issue.message_id());
        }
    }
}
