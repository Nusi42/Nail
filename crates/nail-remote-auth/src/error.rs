//! Errors of the admin operations (the core maps them to `ApiError`).

use nail_api::{ApiError, ErrorCode};

/// An error of an operation of this crate.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The store failed or refused (not found, conflict, …).
    #[error("store: {0}")]
    Store(#[from] nail_store::Error),
    /// The request is not acceptable (weak password, wrong code, …).
    #[error("invalid request: {0}")]
    Invalid(String),
    /// The OS keychain failed.
    #[error("keychain: {0}")]
    Keychain(#[from] nail_api::engine::EngineError),
    /// Something that must not happen (a library refused valid input).
    #[error("internal: {0}")]
    Internal(String),
}

/// Result of this crate's operations.
pub type Result<T, E = AuthError> = std::result::Result<T, E>;

impl AuthError {
    /// The API error for this failure.
    pub fn to_api(&self) -> ApiError {
        match self {
            Self::Store(nail_store::Error::NotFound { kind, id }) => {
                ApiError::not_found(format!("{kind} {id}"))
            }
            Self::Store(nail_store::Error::Conflict(message)) => {
                ApiError::new(ErrorCode::Conflict, message.clone())
            }
            Self::Store(nail_store::Error::InvalidData(message)) | Self::Invalid(message) => {
                ApiError::invalid_request(message.clone())
            }
            Self::Keychain(error) => ApiError::new(error.code, error.message.clone()),
            Self::Store(other) => ApiError::internal(other.to_string()),
            Self::Internal(message) => ApiError::internal(message.clone()),
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        error.to_api()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_api_codes() {
        let not_found = AuthError::Store(nail_store::Error::NotFound {
            kind: "remote user",
            id: "ruser-9".into(),
        });
        assert_eq!(not_found.to_api().code, ErrorCode::NotFound);
        assert_eq!(
            ApiError::from(AuthError::Invalid("weak".into())).code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            AuthError::Store(nail_store::Error::Conflict("x".into()))
                .to_api()
                .code,
            ErrorCode::Conflict
        );
        let locked = nail_api::engine::EngineError {
            code: ErrorCode::Unavailable,
            message: "keychain locked".into(),
            retryable: true,
            localized: None,
        };
        assert_eq!(
            AuthError::from(locked).to_api().code,
            ErrorCode::Unavailable
        );
        assert_eq!(
            AuthError::Internal("bug".into()).to_api().code,
            ErrorCode::Internal
        );
    }
}
