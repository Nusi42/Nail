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
        }
    }

    /// The Fluent message id the UI shows for this code, e.g. `"error-not-found"`.
    pub fn message_id(self) -> String {
        format!("error-{}", self.as_str().replace('_', "-"))
    }

    /// All codes, for exhaustive checks (e.g. catalog completeness tests).
    pub const ALL: [ErrorCode; 8] = [
        Self::InvalidRequest,
        Self::NotFound,
        Self::Unsupported,
        Self::AuthFailed,
        Self::Network,
        Self::Config,
        Self::Unavailable,
        Self::Internal,
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
    }

    #[test]
    fn display() {
        assert_eq!(
            ApiError::not_found("message m1").to_string(),
            "not_found: message m1"
        );
    }
}
