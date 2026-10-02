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
    /// A problem Nail detects itself. It has its own user-facing text:
    /// Nail's English never reaches the UI as a `detail` argument
    /// (ADR 0054; M2d review fixes).
    #[error(transparent)]
    Problem(#[from] Problem),
    /// A smartcard operation failed (M4c WP122): card missing, PIN wrong /
    /// locked / not entered, mechanism not supported.
    #[error(transparent)]
    Smartcard(#[from] nail_pkcs11::CardError),
}

/// A problem `nail-pgp` detects itself (not a library error), each with
/// its own Fluent id in `locales/en/core-crypto.ftl`. The English
/// [`Display`](std::fmt::Display) text is for logs only.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    /// Not an e-mail address.
    #[error("not an address: {address:?}")]
    NotAnAddress {
        /// What was given.
        address: String,
    },
    /// A domain key discovery (WKD) cannot ask.
    #[error("unsupported domain {domain:?}")]
    UnsupportedDomain {
        /// The domain.
        domain: String,
    },
    /// A key lookup result holds no key.
    #[error("the candidate holds no key")]
    CandidateWithoutKey,
    /// An `Autocrypt:` attribute without `=`.
    #[error("bad Autocrypt attribute {attribute:?}")]
    AutocryptAttribute {
        /// The attribute as sent.
        attribute: String,
    },
    /// An unknown critical `Autocrypt:` attribute.
    #[error("unknown critical Autocrypt attribute {attribute:?}")]
    AutocryptCritical {
        /// Its name.
        attribute: String,
    },
    /// `keydata` is not base64 (`detail` = the decoder's text).
    #[error("bad Autocrypt keydata: {detail}")]
    AutocryptKeydata {
        /// The base64 decoder's reason.
        detail: String,
    },
    /// A required `Autocrypt:` attribute (`addr`, `keydata`) is missing.
    #[error("Autocrypt header without {attribute}")]
    AutocryptMissing {
        /// The attribute name (a protocol token).
        attribute: &'static str,
    },
    /// An `Autocrypt:` header carries a secret key.
    #[error("an Autocrypt header must not carry a secret key")]
    AutocryptSecretKey,
    /// `keydata` holds no key.
    #[error("no key in keydata")]
    AutocryptNoKey,
    /// An Autocrypt setup code is not 36 digits.
    #[error("the passcode must be 36 digits")]
    SetupCodeFormat,
    /// The message is not an Autocrypt setup message.
    #[error("not an Autocrypt setup message")]
    NotSetupMessage,
    /// The setup message has no encrypted payload.
    #[error("the setup message has no encrypted key")]
    SetupWithoutPayload,
    /// The decrypted setup message holds no secret key.
    #[error("the setup message holds no secret key")]
    SetupWithoutSecretKey,
    /// Neither signing nor encrypting was asked for.
    #[error("neither signing nor encrypting")]
    NothingToProtect,
    /// A key without a valid self-signature.
    #[error("the key has no valid self-signature")]
    NoSelfSignature,
    /// A key without a valid user id for an address.
    #[error("the key has no valid user id for {address}")]
    NoUserIdFor {
        /// The address.
        address: String,
    },
    /// A key expiry too far in the future.
    #[error("expiry of {days} days is too far")]
    ExpiryTooFar {
        /// The days asked for.
        days: u64,
    },
    /// A keyring entry that is not an OpenPGP key.
    #[error("{key} is not an OpenPGP key")]
    NotOpenPgpKey {
        /// The key id.
        key: String,
    },
    /// An empty passphrase.
    #[error("the passphrase is empty")]
    EmptyPassphrase,
    /// A key without a secret part.
    #[error("{key} has no secret key")]
    NoSecretKey {
        /// The key id.
        key: String,
    },
    /// OpenPGP secrets are never kept in the Windows certificate store.
    #[error("OpenPGP keys are never in the Windows store")]
    WindowsStore,
}

impl Problem {
    /// Every kind once (tests: ids, arguments and catalog entries).
    #[cfg(test)]
    pub(crate) fn samples() -> Vec<Problem> {
        vec![
            Self::NotAnAddress {
                address: "@example.org".into(),
            },
            Self::UnsupportedDomain {
                domain: "münchen.de".into(),
            },
            Self::CandidateWithoutKey,
            Self::AutocryptAttribute {
                attribute: "x".into(),
            },
            Self::AutocryptCritical {
                attribute: "x".into(),
            },
            Self::AutocryptKeydata {
                detail: "Invalid symbol 42".into(),
            },
            Self::AutocryptMissing { attribute: "addr" },
            Self::AutocryptSecretKey,
            Self::AutocryptNoKey,
            Self::SetupCodeFormat,
            Self::NotSetupMessage,
            Self::SetupWithoutPayload,
            Self::SetupWithoutSecretKey,
            Self::NothingToProtect,
            Self::NoSelfSignature,
            Self::NoUserIdFor {
                address: "a@example.org".into(),
            },
            Self::ExpiryTooFar { days: 99999 },
            Self::NotOpenPgpKey { key: "k1".into() },
            Self::EmptyPassphrase,
            Self::NoSecretKey { key: "k1".into() },
            Self::WindowsStore,
        ]
    }

    /// The protocol error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::CandidateWithoutKey
            | Self::AutocryptNoKey
            | Self::SetupWithoutSecretKey
            | Self::NoSelfSignature => ErrorCode::Crypto,
            Self::WindowsStore => ErrorCode::Unsupported,
            Self::NotAnAddress { .. }
            | Self::UnsupportedDomain { .. }
            | Self::AutocryptAttribute { .. }
            | Self::AutocryptCritical { .. }
            | Self::AutocryptKeydata { .. }
            | Self::AutocryptMissing { .. }
            | Self::AutocryptSecretKey
            | Self::SetupCodeFormat
            | Self::NotSetupMessage
            | Self::SetupWithoutPayload
            | Self::NothingToProtect
            | Self::NoUserIdFor { .. }
            | Self::ExpiryTooFar { .. }
            | Self::NotOpenPgpKey { .. }
            | Self::EmptyPassphrase
            | Self::NoSecretKey { .. } => ErrorCode::InvalidRequest,
        }
    }

    /// The Fluent id (`pgp-problem-*`).
    pub fn message_id(&self) -> &'static str {
        match self {
            Self::NotAnAddress { .. } => "pgp-problem-not-an-address",
            Self::UnsupportedDomain { .. } => "pgp-problem-unsupported-domain",
            Self::CandidateWithoutKey => "pgp-problem-candidate-without-key",
            Self::AutocryptAttribute { .. } => "pgp-problem-autocrypt-attribute",
            Self::AutocryptCritical { .. } => "pgp-problem-autocrypt-critical",
            Self::AutocryptKeydata { .. } => "pgp-problem-autocrypt-keydata",
            Self::AutocryptMissing { .. } => "pgp-problem-autocrypt-missing",
            Self::AutocryptSecretKey => "pgp-problem-autocrypt-secret-key",
            Self::AutocryptNoKey => "pgp-problem-autocrypt-no-key",
            Self::SetupCodeFormat => "pgp-problem-setup-code-format",
            Self::NotSetupMessage => "pgp-problem-not-setup-message",
            Self::SetupWithoutPayload => "pgp-problem-setup-without-payload",
            Self::SetupWithoutSecretKey => "pgp-problem-setup-without-secret-key",
            Self::NothingToProtect => "pgp-problem-nothing-to-protect",
            Self::NoSelfSignature => "pgp-problem-no-self-signature",
            Self::NoUserIdFor { .. } => "pgp-problem-no-user-id-for",
            Self::ExpiryTooFar { .. } => "pgp-problem-expiry-too-far",
            Self::NotOpenPgpKey { .. } => "pgp-problem-not-openpgp-key",
            Self::EmptyPassphrase => "pgp-problem-empty-passphrase",
            Self::NoSecretKey { .. } => "pgp-problem-no-secret-key",
            Self::WindowsStore => "pgp-problem-windows-store",
        }
    }

    /// The user-facing text: the id with the values it names (only
    /// `detail` is a library text).
    pub fn localized(&self) -> LocalizedMessage {
        let message = LocalizedMessage::new(self.message_id());
        match self {
            Self::NotAnAddress { address } | Self::NoUserIdFor { address } => {
                message.arg("address", address.as_str())
            }
            Self::UnsupportedDomain { domain } => message.arg("domain", domain.as_str()),
            Self::AutocryptAttribute { attribute } | Self::AutocryptCritical { attribute } => {
                message.arg("attribute", attribute.as_str())
            }
            Self::AutocryptMissing { attribute } => message.arg("attribute", *attribute),
            Self::AutocryptKeydata { detail } => message.arg("detail", detail.as_str()),
            Self::ExpiryTooFar { days } => message.arg("days", *days),
            Self::NotOpenPgpKey { key } | Self::NoSecretKey { key } => {
                message.arg("key", key.as_str())
            }
            Self::CandidateWithoutKey
            | Self::AutocryptSecretKey
            | Self::AutocryptNoKey
            | Self::SetupCodeFormat
            | Self::NotSetupMessage
            | Self::SetupWithoutPayload
            | Self::SetupWithoutSecretKey
            | Self::NothingToProtect
            | Self::NoSelfSignature
            | Self::EmptyPassphrase
            | Self::WindowsStore => message,
        }
    }
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
            Self::Problem(p) => p.code(),
            Self::Smartcard(e) => e.code(),
        }
    }

    /// The user-facing text (ADR 0043), when it says more than the text of
    /// [`Self::code`]. Library texts (rPGP, the base64 decoder) stay an
    /// English `detail` argument; what Nail detects itself is a
    /// [`Problem`] with its own text (`Crypto`, `Invalid` and
    /// `Unsupported` carry library texts only).
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
            Self::Problem(p) => Some(p.localized()),
            Self::Smartcard(e) => Some(LocalizedMessage::new(e.kind.message_id())),
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

    /// What Nail detects itself has its own id; only library texts are a
    /// `detail` (M2d review: "not an address" used to reach the UI as the
    /// English detail of `pgp-error-invalid`).
    #[test]
    fn problems_have_their_own_texts_in_the_catalog() {
        let catalog = crate::testutil::crypto_catalog();
        let mut ids = std::collections::HashSet::new();
        for problem in Problem::samples() {
            let id = problem.message_id();
            assert!(id.starts_with("pgp-problem-"), "{id}");
            assert!(ids.insert(id), "{id} is used twice");
            let error = Error::from(problem.clone());
            assert_eq!(error.code(), problem.code());
            let localized = error.localized().expect("a problem has a text");
            assert_eq!(localized.id, id);
            let line = catalog
                .lines()
                .find(|l| l.starts_with(&format!("{id} =")))
                .unwrap_or_else(|| panic!("{id} is not in core-crypto.ftl"));
            for name in localized.args.keys() {
                assert!(
                    line.contains(&format!("{{ ${name} }}")),
                    "{id} lacks ${name}"
                );
            }
            // Only the base64 decoder's reason is a `detail`.
            assert_eq!(
                localized.args.contains_key("detail"),
                matches!(problem, Problem::AutocryptKeydata { .. }),
                "{id}"
            );
            let api: ApiError = error.into();
            assert_eq!(api.localized.as_deref(), Some(&localized));
            assert_eq!(api.message, problem.to_string(), "English diagnostic");
        }
        let api: ApiError = Error::from(Problem::NotAnAddress {
            address: "@example.org".into(),
        })
        .into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        assert_eq!(
            api.localized.unwrap().args["address"],
            nail_api::localized::MessageArg::Text("@example.org".into())
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
