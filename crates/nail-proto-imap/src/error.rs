//! Error type of the IMAP client.

use std::io;

use nail_api::ErrorCode;

/// Result alias of this crate.
pub type Result<T, E = ImapError> = std::result::Result<T, E>;

/// Errors talking to an IMAP server. Messages are for logs; the UI shows the
/// localized text for [`ImapError::code`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImapError {
    /// The TCP connection could not be established.
    #[error("connecting to {host}:{port}: {source}")]
    Connect {
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Cause.
        #[source]
        source: io::Error,
    },
    /// The host is not a valid DNS name or IP address for TLS.
    #[error("invalid TLS server name {0:?}")]
    InvalidServerName(String),
    /// TLS setup or handshake failed (including certificate errors).
    #[error("TLS: {0}")]
    Tls(String),
    /// `STARTTLS` was required but the server does not offer or accept it.
    #[error("the server does not support STARTTLS")]
    StartTlsUnavailable,
    /// The server rejected the credentials.
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// No usable authentication mechanism (e.g. `LOGINDISABLED` without `AUTH=PLAIN`).
    #[error("no supported authentication mechanism: {0}")]
    AuthUnsupported(String),
    /// The server closed the connection.
    #[error("connection closed by the server")]
    ConnectionLost,
    /// An operation took longer than the configured timeout.
    #[error("timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// The server answered `NO` (e.g. unknown mailbox).
    #[error("server refused: {0}")]
    No(String),
    /// The server answered `BAD` or sent something unparsable.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Other I/O error.
    #[error("I/O: {0}")]
    Io(#[source] io::Error),
}

impl ImapError {
    /// The API error code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::AuthFailed(_) => ErrorCode::AuthFailed,
            Self::AuthUnsupported(_) | Self::StartTlsUnavailable => ErrorCode::Unsupported,
            Self::InvalidServerName(_) => ErrorCode::Config,
            Self::No(_) => ErrorCode::NotFound,
            Self::Connect { .. }
            | Self::Tls(_)
            | Self::ConnectionLost
            | Self::Timeout(_)
            | Self::Protocol(_)
            | Self::Io(_) => ErrorCode::Network,
        }
    }

    /// Whether retrying later (e.g. after reconnecting) may succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Connect { .. } | Self::ConnectionLost | Self::Timeout(_) | Self::Io(_)
        )
    }
}

impl From<io::Error> for ImapError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe => Self::ConnectionLost,
            _ => Self::Io(err),
        }
    }
}

impl From<async_imap::error::Error> for ImapError {
    fn from(err: async_imap::error::Error) -> Self {
        use async_imap::error::Error as E;
        match err {
            E::Io(io) => io.into(),
            E::ConnectionLost => Self::ConnectionLost,
            E::No(text) => Self::No(text),
            E::Bad(text) => Self::Protocol(format!("BAD {text}")),
            other => Self::Protocol(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(
            ImapError::AuthFailed("x".into()).code(),
            ErrorCode::AuthFailed
        );
        assert_eq!(ImapError::ConnectionLost.code(), ErrorCode::Network);
        assert_eq!(
            ImapError::StartTlsUnavailable.code(),
            ErrorCode::Unsupported
        );
        assert!(ImapError::ConnectionLost.is_transient());
        assert!(!ImapError::AuthFailed("x".into()).is_transient());
    }

    #[test]
    fn io_eof_is_connection_lost() {
        let err: ImapError = io::Error::from(io::ErrorKind::UnexpectedEof).into();
        assert!(matches!(err, ImapError::ConnectionLost));
        let err: ImapError = async_imap::error::Error::ConnectionLost.into();
        assert!(matches!(err, ImapError::ConnectionLost));
    }
}
