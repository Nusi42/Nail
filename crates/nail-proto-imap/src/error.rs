//! Error type of the IMAP client.

use std::io;

use nail_api::ErrorCode;
use nail_api::engine::EngineError;

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
    /// The server does not support what was asked (e.g. `IDLE` or
    /// `UNCHANGEDSINCE` without the extension).
    #[error("not supported by the server: {0}")]
    Unsupported(String),
    /// A caller-supplied value cannot be sent (e.g. a flag with spaces).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The message is larger than the server accepts (`APPENDLIMIT`,
    /// `[TOOBIG]`).
    #[error("message too large: {0}")]
    TooLarge(String),
    /// Other I/O error.
    #[error("I/O: {0}")]
    Io(#[source] io::Error),
}

impl ImapError {
    /// The API error code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::AuthFailed(_) => ErrorCode::AuthFailed,
            Self::AuthUnsupported(_) | Self::StartTlsUnavailable | Self::Unsupported(_) => {
                ErrorCode::Unsupported
            }
            Self::InvalidServerName(_) => ErrorCode::Config,
            Self::InvalidArgument(_) => ErrorCode::InvalidRequest,
            Self::TooLarge(_) => ErrorCode::TooLarge,
            Self::Tls(_) => ErrorCode::Tls,
            Self::No(text) => no_code(text),
            Self::Connect { .. }
            | Self::ConnectionLost
            | Self::Timeout(_)
            | Self::Protocol(_)
            | Self::Io(_) => ErrorCode::Network,
        }
    }

    /// The response code of a `NO` (RFC 5530), e.g. `"ALREADYEXISTS"`.
    pub fn response_code(&self) -> Option<&str> {
        match self {
            Self::No(text) => response_code(text),
            _ => None,
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

impl From<ImapError> for EngineError {
    fn from(err: ImapError) -> Self {
        Self {
            code: err.code(),
            retryable: err.is_transient(),
            message: err.to_string(),
            localized: None,
            retry_after_secs: None,
        }
    }
}

/// The bracketed response code at the start of a response text:
/// `"[TRYCREATE] no such mailbox"` → `"TRYCREATE"`.
pub(crate) fn response_code(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix('[')?;
    let end = rest.find(']')?;
    let code = &rest[..end];
    Some(code.split(' ').next().unwrap_or(code))
}

/// Maps the RFC 5530 code of a `NO` to an API error code.
fn no_code(text: &str) -> ErrorCode {
    match response_code(text).map(str::to_ascii_uppercase).as_deref() {
        Some("ALREADYEXISTS" | "INUSE" | "EXPUNGEISSUED") => ErrorCode::Conflict,
        Some("TOOBIG" | "LIMIT" | "OVERQUOTA") => ErrorCode::TooLarge,
        Some("AUTHENTICATIONFAILED" | "AUTHORIZATIONFAILED" | "EXPIRED") => ErrorCode::AuthFailed,
        Some("UNAVAILABLE" | "SERVERBUG") => ErrorCode::Unavailable,
        Some("CANNOT" | "PRIVACYREQUIRED" | "CONTACTADMIN") => ErrorCode::Rejected,
        // NONEXISTENT, TRYCREATE, NOPERM (a folder we may not open is as
        // good as missing) and plain NOs.
        _ => ErrorCode::NotFound,
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
        let err: ImapError = io::Error::from(io::ErrorKind::PermissionDenied).into();
        assert!(matches!(err, ImapError::Io(_)));
    }

    #[test]
    fn no_codes_map_to_api_codes() {
        let no = |text: &str| ImapError::No(text.to_owned());
        assert_eq!(no("[ALREADYEXISTS] exists").code(), ErrorCode::Conflict);
        assert_eq!(no("[TOOBIG] big").code(), ErrorCode::TooLarge);
        assert_eq!(no("[NONEXISTENT] nope").code(), ErrorCode::NotFound);
        assert_eq!(no("[TRYCREATE] nope").code(), ErrorCode::NotFound);
        assert_eq!(no("[NOPERM] denied").code(), ErrorCode::NotFound);
        assert_eq!(no("[CANNOT] reserved name").code(), ErrorCode::Rejected);
        assert_eq!(no("[SERVERBUG] oops").code(), ErrorCode::Unavailable);
        assert_eq!(no("plain text").code(), ErrorCode::NotFound);
        assert_eq!(no("[COPYUID 1 2 3] x").response_code(), Some("COPYUID"));
        assert_eq!(no("text").response_code(), None);
        assert_eq!(ImapError::Tls("x".into()).code(), ErrorCode::Tls);
        assert_eq!(ImapError::TooLarge("x".into()).code(), ErrorCode::TooLarge);
        assert_eq!(
            ImapError::InvalidArgument("x".into()).code(),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn engine_errors_keep_code_and_retryability() {
        let engine: EngineError = ImapError::ConnectionLost.into();
        assert_eq!(engine.code, ErrorCode::Network);
        assert!(engine.retryable);
        let engine: EngineError = ImapError::AuthFailed("bad token".into()).into();
        assert_eq!(engine.code, ErrorCode::AuthFailed);
        assert!(!engine.retryable);
    }
}
