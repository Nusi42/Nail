//! Errors of `nail-pgp`, mapped onto the protocol's [`ErrorCode`]s.

use nail_api::engine::EngineError;
use nail_api::{ApiError, ErrorCode, KeyId, LocalizedMessage};

/// A `nail-pgp` error. Messages are English diagnostics for logs; the UI
/// shows [`Error::localized`] (`pgp-*` in `locales/en/core-crypto.ftl`)
/// or, without one, the text of the [`ErrorCode`] ([`Error::code`]).
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
    /// `ImportKeys` data declared as base64 is not base64
    /// (`invalid_request`).
    #[error("not base64: {0}")]
    NotBase64(String),
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
            Self::Invalid(_) | Self::NotBase64(_) => ErrorCode::InvalidRequest,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::Store(e) => store_code(e),
            Self::Secrets(e) => e.code,
        }
    }

    /// The user-facing text (ADR 0043), when it says more than the text of
    /// [`Self::code`]. Library texts (rPGP, the base64 decoder) stay an
    /// English `detail` argument.
    pub fn localized(&self) -> Option<LocalizedMessage> {
        let with_detail = |id: &str, detail: &str| LocalizedMessage::new(id).arg("detail", detail);
        match self {
            Self::Locked { key_id } => {
                Some(LocalizedMessage::new("pgp-error-locked").arg("key", key_id.0.as_str()))
            }
            // `error-auth-failed` speaks of a server login.
            Self::BadPassphrase => Some(LocalizedMessage::new("pgp-error-bad-passphrase")),
            Self::Crypto(detail) => Some(with_detail("pgp-error-crypto", detail)),
            Self::Invalid(detail) => Some(with_detail("pgp-error-invalid", detail)),
            Self::Unsupported(detail) => Some(with_detail("pgp-error-unsupported", detail)),
            Self::NotBase64(detail) => Some(with_detail("pgp-import-not-base64", detail)),
            // The code's text says it all (`error-no-key`); store and
            // keychain errors keep their own.
            Self::NoKey(_) | Self::Store(_) | Self::Secrets(_) => None,
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
            Error::Secrets(engine) => engine.into(),
            other => {
                let api = ApiError::new(other.code(), other.to_string());
                match other.localized() {
                    Some(localized) => api.with_localized(localized),
                    None => api,
                }
            }
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
        assert_eq!(
            Error::NotBase64("x".into()).code(),
            ErrorCode::InvalidRequest
        );
    }

    type Expected = Option<(&'static str, &'static [&'static str])>;

    /// Every error kind with the Fluent id and argument names it builds.
    fn localized_cases() -> Vec<(Error, Expected)> {
        vec![
            (
                Error::Locked {
                    key_id: KeyId("key-1".into()),
                },
                Some(("pgp-error-locked", &["key"])),
            ),
            (
                Error::BadPassphrase,
                Some(("pgp-error-bad-passphrase", &[])),
            ),
            (Error::NoKey("bob".into()), None),
            (
                Error::Crypto("bad packet".into()),
                Some(("pgp-error-crypto", &["detail"])),
            ),
            (
                Error::Invalid("no user id".into()),
                Some(("pgp-error-invalid", &["detail"])),
            ),
            (
                Error::Unsupported("v6 keys".into()),
                Some(("pgp-error-unsupported", &["detail"])),
            ),
            (
                Error::NotBase64("Invalid symbol 42".into()),
                Some(("pgp-import-not-base64", &["detail"])),
            ),
            (
                Error::Secrets(EngineError::permanent(ErrorCode::Unavailable, "down")),
                None,
            ),
        ]
    }

    #[test]
    fn errors_carry_localized_texts_with_their_arguments() {
        for (error, expected) in localized_cases() {
            let localized = error.localized();
            assert_eq!(
                localized.as_ref().map(|l| l.id.as_str()),
                expected.map(|(id, _)| id),
                "{error:?}"
            );
            if let (Some(localized), Some((_, args))) = (&localized, expected) {
                let mut names: Vec<&str> = localized.args.keys().map(String::as_str).collect();
                names.sort_unstable();
                assert_eq!(names, args.to_vec(), "{error:?}");
            }
            // The protocol error carries the same text.
            let code = error.code();
            let api: ApiError = error.into();
            assert_eq!(api.code, code);
            assert_eq!(api.localized.as_deref(), localized.as_ref());
        }
        let api: ApiError = Error::Crypto("bad packet".into()).into();
        assert_eq!(api.message, "openpgp: bad packet", "English diagnostic");
        assert_eq!(
            api.localized.unwrap().args["detail"],
            nail_api::localized::MessageArg::Text("bad packet".into())
        );
    }

    #[test]
    fn error_texts_are_in_the_crypto_catalog() {
        let catalog = crate::testutil::crypto_catalog();
        for (error, expected) in localized_cases() {
            let Some((id, args)) = expected else { continue };
            let line = catalog
                .lines()
                .find(|l| l.starts_with(&format!("{id} =")))
                .unwrap_or_else(|| panic!("{id} ({error:?}) is not in core-crypto.ftl"));
            for arg in args {
                assert!(line.contains(&format!("{{ ${arg} }}")), "{id} lacks ${arg}");
            }
        }
    }
}
