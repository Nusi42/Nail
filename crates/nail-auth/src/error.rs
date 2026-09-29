//! Error type of the auth crate.

use nail_api::ErrorCode;
use nail_api::engine::EngineError;

/// Errors of OAuth2 flows, token refreshes and secret storage. Messages
/// never contain tokens, codes or passwords.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// The provider is unknown or incompletely configured (no client id,
    /// missing endpoints, client secret not in the keychain).
    #[error("OAuth2 configuration: {0}")]
    Config(String),
    /// The user declined, closed the sign-in or the flow was cancelled.
    #[error("sign-in cancelled")]
    Cancelled,
    /// The user did not finish signing in in time.
    #[error("sign-in timed out")]
    Timeout,
    /// The token endpoint could not be reached.
    #[error("network: {0}")]
    Network(String),
    /// The refresh token or authorization code was rejected
    /// (`invalid_grant`): the user must sign in again.
    #[error("the grant was rejected: {0}")]
    InvalidGrant(String),
    /// Another OAuth2 error answer (`invalid_client`, `invalid_scope`, …).
    #[error("the authorization server refused: {0}")]
    Refused(String),
    /// The server answered something unparsable.
    #[error("protocol: {0}")]
    Protocol(String),
    /// Reading or writing the keychain failed.
    #[error("keychain: {0}")]
    Secret(EngineError),
}

impl AuthError {
    /// The API error code.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Config(_) => ErrorCode::Config,
            Self::Cancelled | Self::Timeout => ErrorCode::Cancelled,
            Self::Network(_) | Self::Protocol(_) => ErrorCode::Network,
            Self::InvalidGrant(_) | Self::Refused(_) => ErrorCode::AuthFailed,
            Self::Secret(e) => e.code,
        }
    }

    /// Whether trying again later may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::Protocol(_) => true,
            Self::Secret(e) => e.retryable,
            _ => false,
        }
    }
}

impl From<AuthError> for EngineError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Secret(e) => e,
            other => Self {
                code: other.code(),
                message: other.to_string(),
                retryable: other.is_retryable(),
            },
        }
    }
}

impl From<EngineError> for AuthError {
    fn from(err: EngineError) -> Self {
        Self::Secret(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(AuthError::Cancelled.code(), ErrorCode::Cancelled);
        assert_eq!(
            AuthError::InvalidGrant("x".into()).code(),
            ErrorCode::AuthFailed
        );
        assert!(!AuthError::InvalidGrant("x".into()).is_retryable());
        assert!(AuthError::Network("x".into()).is_retryable());
        let engine: EngineError = AuthError::Config("no client id".into()).into();
        assert_eq!((engine.code, engine.retryable), (ErrorCode::Config, false));
        let inner = EngineError::temporary(ErrorCode::Internal, "locked");
        let engine: EngineError = AuthError::from(inner.clone()).into();
        assert_eq!(engine, inner);
    }
}
