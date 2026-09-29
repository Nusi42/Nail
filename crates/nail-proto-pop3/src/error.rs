//! Error type of the POP3 client.

use std::io;
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::engine::EngineError;

/// Result alias of this crate.
pub type Result<T, E = Pop3Error> = std::result::Result<T, E>;

/// Errors talking to a POP3 server. Messages are English diagnostics for
/// logs and never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Pop3Error {
    /// The TCP connection could not be established.
    #[error("connecting to {host}:{port}: {message}")]
    Connect {
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Cause.
        message: String,
    },
    /// The host is not a valid DNS name or IP address for TLS.
    #[error("invalid TLS server name {0:?}")]
    InvalidServerName(String),
    /// TLS setup or handshake failed.
    #[error("TLS: {0}")]
    Tls(String),
    /// STLS was required but the server does not offer or accept it.
    #[error("the server does not support STLS")]
    StartTlsUnavailable,
    /// The server rejected the credentials.
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// No usable authentication method.
    #[error("no supported authentication method: {0}")]
    AuthUnsupported(String),
    /// The mailbox is temporarily unavailable (`[IN-USE]`, `[LOGIN-DELAY]`,
    /// `[SYS/TEMP]`); try again later.
    #[error("temporarily unavailable: {0}")]
    Temporary(String),
    /// The server answered `-ERR` to a command.
    #[error("server refused {command}: {message}")]
    Refused {
        /// The command (without arguments).
        command: String,
        /// The server's text.
        message: String,
    },
    /// The server lacks a required extension (`UIDL`, `TOP`).
    #[error("not supported by the server: {0}")]
    Unsupported(String),
    /// The server closed the connection.
    #[error("connection closed by the server")]
    ConnectionLost,
    /// The server sent nothing for the configured timeout.
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    /// The server sent something unparsable.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Other I/O error.
    #[error("I/O: {0}")]
    Io(String),
}

impl Pop3Error {
    /// The API error code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::AuthFailed(_) => ErrorCode::AuthFailed,
            Self::AuthUnsupported(_) | Self::StartTlsUnavailable | Self::Unsupported(_) => {
                ErrorCode::Unsupported
            }
            Self::InvalidServerName(_) => ErrorCode::Config,
            Self::Tls(_) => ErrorCode::Tls,
            Self::Refused { .. } => ErrorCode::Rejected,
            Self::Connect { .. }
            | Self::Temporary(_)
            | Self::ConnectionLost
            | Self::Timeout(_)
            | Self::Protocol(_)
            | Self::Io(_) => ErrorCode::Network,
        }
    }

    /// Whether retrying later (after reconnecting) may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Connect { .. }
                | Self::Tls(_)
                | Self::Temporary(_)
                | Self::ConnectionLost
                | Self::Timeout(_)
                | Self::Protocol(_)
                | Self::Io(_)
        )
    }
}

impl From<Pop3Error> for EngineError {
    fn from(err: Pop3Error) -> Self {
        Self {
            code: err.code(),
            message: err.to_string(),
            retryable: err.is_retryable(),
            localized: None,
        }
    }
}

impl From<io::Error> for Pop3Error {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe => Self::ConnectionLost,
            _ => Self::Io(err.to_string()),
        }
    }
}

/// Classifies an `-ERR` answer to a login attempt using the RFC 2449 /
/// RFC 3206 response codes.
pub(crate) fn login_error(text: &str) -> Pop3Error {
    let upper = text.to_ascii_uppercase();
    if ["[IN-USE]", "[LOGIN-DELAY]", "[SYS/TEMP]"]
        .iter()
        .any(|c| upper.starts_with(c))
    {
        Pop3Error::Temporary(text.to_owned())
    } else {
        Pop3Error::AuthFailed(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_retry() {
        assert_eq!(login_error("[AUTH] bad").code(), ErrorCode::AuthFailed);
        assert_eq!(login_error("bad password").code(), ErrorCode::AuthFailed);
        assert!(login_error("[IN-USE] locked").is_retryable());
        assert!(login_error("[login-delay] wait").is_retryable());
        let refused = Pop3Error::Refused {
            command: "RETR".into(),
            message: "no such message".into(),
        };
        let engine: EngineError = refused.into();
        assert_eq!(
            (engine.code, engine.retryable),
            (ErrorCode::Rejected, false)
        );
        let lost: Pop3Error = io::Error::from(io::ErrorKind::UnexpectedEof).into();
        assert_eq!(lost, Pop3Error::ConnectionLost);
        assert!(lost.is_retryable());
        assert_eq!(Pop3Error::Tls("x".into()).code(), ErrorCode::Tls);
        assert_eq!(
            Pop3Error::StartTlsUnavailable.code(),
            ErrorCode::Unsupported
        );
    }
}
