//! The API error type.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Machine-readable error classes. The UI maps each code to the Fluent
/// message `error-<code with "_" replaced by "-">`, e.g. `not_found` →
/// `error-not-found`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request was malformed or had invalid arguments.
    InvalidRequest,
    /// The addressed account, folder or message does not exist.
    NotFound,
    /// The request is valid but not supported (yet) by this core or account.
    Unsupported,
    /// A server rejected the credentials.
    AuthFailed,
    /// A network or protocol error talking to a server.
    Network,
    /// The configuration is invalid.
    Config,
    /// The core is busy or shutting down; retry later.
    Unavailable,
    /// An unexpected internal error.
    Internal,
    /// The request conflicts with the current state (an account id that
    /// already exists, cancelling a message that is already being sent).
    Conflict,
    /// The TLS handshake failed (untrusted or expired certificate, wrong
    /// host name, no common protocol version).
    Tls,
    /// The message is larger than the server accepts.
    TooLarge,
    /// The server permanently refused the operation (e.g. SMTP 5xx: unknown
    /// recipient, policy rejection).
    Rejected,
    /// The operation was cancelled (e.g. the user closed the OAuth2 sign-in).
    Cancelled,
    /// A secret key is protected by a passphrase that was not entered
    /// ([`crate::Request::UnlockKey`]), or the data vault's key is not
    /// available (M2a).
    Locked,
    /// A needed key or certificate is missing or unusable: a recipient
    /// without a key for encryption, no own key for signing (M2a).
    NoKey,
    /// Signing, encryption, decryption or verification failed, or key data
    /// is malformed (M2a).
    Crypto,
    /// The caller is not allowed to do this: a remote session's grant
    /// lacks the scope or `admin`, or the action is locked (internet mode)
    /// (M2b).
    Forbidden,
    /// A sensitive action needs a fresh re-authentication of the remote
    /// session (M2b).
    ReauthRequired,
    /// Too many attempts; try again later (M2b).
    RateLimited,
}

impl ErrorCode {
    /// The code's wire name, e.g. `"not_found"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::NotFound => "not_found",
            Self::Unsupported => "unsupported",
            Self::AuthFailed => "auth_failed",
            Self::Network => "network",
            Self::Config => "config",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
            Self::Conflict => "conflict",
            Self::Tls => "tls",
            Self::TooLarge => "too_large",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Locked => "locked",
            Self::NoKey => "no_key",
            Self::Crypto => "crypto",
            Self::Forbidden => "forbidden",
            Self::ReauthRequired => "reauth_required",
            Self::RateLimited => "rate_limited",
        }
    }

    /// The Fluent message id the UI shows for this code, e.g. `"error-not-found"`.
    pub fn message_id(self) -> String {
        format!("error-{}", self.as_str().replace('_', "-"))
    }

    /// All codes, for exhaustive checks (e.g. catalog completeness tests).
    pub const ALL: [ErrorCode; 19] = [
        Self::InvalidRequest,
        Self::NotFound,
        Self::Unsupported,
        Self::AuthFailed,
        Self::Network,
        Self::Config,
        Self::Unavailable,
        Self::Internal,
        Self::Conflict,
        Self::Tls,
        Self::TooLarge,
        Self::Rejected,
        Self::Cancelled,
        Self::Locked,
        Self::NoKey,
        Self::Crypto,
        Self::Forbidden,
        Self::ReauthRequired,
        Self::RateLimited,
    ];
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error returned by the API: a code plus a diagnostic message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, TS)]
#[error("{code}: {message}")]
pub struct ApiError {
    /// Machine-readable error class.
    pub code: ErrorCode,
    /// Diagnostic detail (English, for logs and developer tools).
    pub message: String,
}

impl ApiError {
    /// Creates an error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Shorthand for [`ErrorCode::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    /// Shorthand for [`ErrorCode::InvalidRequest`].
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    /// Shorthand for [`ErrorCode::Unsupported`].
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }

    /// Shorthand for [`ErrorCode::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_names_match_serde() {
        for code in ErrorCode::ALL {
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                serde_json::Value::from(code.as_str())
            );
        }
        assert_eq!(ErrorCode::AuthFailed.message_id(), "error-auth-failed");
        assert_eq!(ErrorCode::TooLarge.message_id(), "error-too-large");
        let unique: std::collections::HashSet<_> = ErrorCode::ALL.iter().collect();
        assert_eq!(unique.len(), ErrorCode::ALL.len());
    }

    #[test]
    fn display() {
        assert_eq!(
            ApiError::not_found("message m1").to_string(),
            "not_found: message m1"
        );
    }
}
