//! Errors of smartcard operations.

use nail_api::smartcard::SmartcardError;
use nail_api::{ApiError, ErrorCode, LocalizedMessage};

/// A smartcard operation failed: the contract's [`SmartcardError`] class
/// (with its `smartcard-error-*` text) plus an English detail.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("smartcard {}: {detail}", kind.as_str())]
pub struct CardError {
    /// What went wrong.
    pub kind: SmartcardError,
    /// Diagnostic detail (English; never a PIN).
    pub detail: String,
}

impl CardError {
    /// An error of a class with a detail.
    pub fn new(kind: SmartcardError, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// The API error class.
    pub fn code(&self) -> ErrorCode {
        match self.kind {
            SmartcardError::ModuleNotFound | SmartcardError::ModuleLoadFailed => ErrorCode::Config,
            SmartcardError::TokenNotPresent => ErrorCode::Unavailable,
            SmartcardError::PinIncorrect => ErrorCode::AuthFailed,
            SmartcardError::PinLocked => ErrorCode::Locked,
            SmartcardError::PinCancelled => ErrorCode::Cancelled,
            SmartcardError::KeyNotFound => ErrorCode::NoKey,
            SmartcardError::MechanismUnsupported => ErrorCode::Unsupported,
            SmartcardError::DeviceError => ErrorCode::Crypto,
        }
    }
}

impl From<CardError> for ApiError {
    fn from(err: CardError) -> Self {
        ApiError::new(err.code(), err.to_string())
            .with_localized(LocalizedMessage::new(err.kind.message_id()))
    }
}

/// Maps a `cryptoki` error to the contract's classes.
pub(crate) fn from_cryptoki(err: &cryptoki::error::Error) -> CardError {
    use cryptoki::error::{Error, RvError};
    let kind = match err {
        Error::LibraryLoading(_) | Error::MissingSymbol(_) | Error::NullFunctionPointer => {
            SmartcardError::ModuleLoadFailed
        }
        Error::Pkcs11(rv, _) => match rv {
            RvError::PinIncorrect | RvError::PinInvalid | RvError::PinLenRange => {
                SmartcardError::PinIncorrect
            }
            RvError::PinLocked | RvError::PinExpired => SmartcardError::PinLocked,
            RvError::TokenNotPresent
            | RvError::TokenNotRecognized
            | RvError::DeviceRemoved
            | RvError::SessionHandleInvalid
            | RvError::SessionClosed
            | RvError::SlotIdInvalid => SmartcardError::TokenNotPresent,
            RvError::MechanismInvalid
            | RvError::MechanismParamInvalid
            | RvError::KeyTypeInconsistent
            | RvError::KeyFunctionNotPermitted
            | RvError::FunctionNotSupported => SmartcardError::MechanismUnsupported,
            RvError::FunctionCanceled => SmartcardError::PinCancelled,
            RvError::KeyHandleInvalid | RvError::ObjectHandleInvalid => SmartcardError::KeyNotFound,
            _ => SmartcardError::DeviceError,
        },
        Error::NotSupported => SmartcardError::MechanismUnsupported,
        _ => SmartcardError::DeviceError,
    };
    CardError::new(kind, err.to_string())
}

/// Whether a `cryptoki` error means "log in first".
pub(crate) fn needs_login(err: &cryptoki::error::Error) -> bool {
    matches!(
        err,
        cryptoki::error::Error::Pkcs11(cryptoki::error::RvError::UserNotLoggedIn, _)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_codes_and_texts() {
        for kind in SmartcardError::ALL {
            let err = CardError::new(*kind, "detail");
            let api: ApiError = err.clone().into();
            assert_eq!(api.code, err.code());
            assert_eq!(api.localized.unwrap().id, kind.message_id());
        }
        assert_eq!(
            CardError::new(SmartcardError::PinIncorrect, "x").code(),
            ErrorCode::AuthFailed
        );
    }

    #[test]
    fn cryptoki_errors_map_to_classes() {
        use cryptoki::context::Function;
        use cryptoki::error::{Error, RvError};
        let cases = [
            (RvError::PinIncorrect, SmartcardError::PinIncorrect),
            (RvError::PinLocked, SmartcardError::PinLocked),
            (RvError::TokenNotPresent, SmartcardError::TokenNotPresent),
            (RvError::DeviceRemoved, SmartcardError::TokenNotPresent),
            (
                RvError::MechanismInvalid,
                SmartcardError::MechanismUnsupported,
            ),
            (RvError::KeyHandleInvalid, SmartcardError::KeyNotFound),
            (RvError::DeviceError, SmartcardError::DeviceError),
        ];
        for (rv, kind) in cases {
            assert_eq!(from_cryptoki(&Error::Pkcs11(rv, Function::Sign)).kind, kind);
        }
        assert!(needs_login(&Error::Pkcs11(
            RvError::UserNotLoggedIn,
            Function::Sign
        )));
        assert!(!needs_login(&Error::Pkcs11(
            RvError::PinIncorrect,
            Function::Login
        )));
    }
}
