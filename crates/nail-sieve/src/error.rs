//! Errors of the ManageSieve client, with their API error class and
//! user-facing text.

use std::io;
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;

use crate::reasons;

/// Result alias of this crate.
pub type Result<T, E = SieveError> = std::result::Result<T, E>;

/// Errors talking to a ManageSieve server. Messages are English
/// diagnostics for logs and never contain secrets; users see
/// [`SieveError::localized`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SieveError {
    /// The TCP connection could not be established.
    #[error("connecting to {host}:{port}: {message}")]
    Connect {
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Cause.
        message: String,
        /// The server actively refused the connection (nothing listens on
        /// the port).
        refused: bool,
    },
    /// The host is not a valid DNS name or IP address for TLS.
    #[error("invalid TLS server name {0:?}")]
    InvalidServerName(String),
    /// TLS setup or handshake failed.
    #[error("TLS with {host}: {message}")]
    Tls {
        /// Host.
        host: String,
        /// Cause.
        message: String,
    },
    /// STARTTLS was required but the server does not offer or accept it.
    #[error("the server does not support STARTTLS")]
    StartTlsUnavailable,
    /// The server wants an encrypted connection before logging in.
    #[error("the server requires encryption: {0}")]
    EncryptionNeeded(String),
    /// The server rejected the credentials.
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// No usable authentication mechanism.
    #[error("no supported authentication mechanism (server offers: {0})")]
    AuthUnsupported(String),
    /// The server did not greet with ManageSieve capabilities (no `SIEVE`).
    #[error("not a ManageSieve server: {0}")]
    NotManageSieve(String),
    /// The server rejected the script (`PUTSCRIPT` / `CHECKSCRIPT`).
    #[error("script rejected: {0}")]
    ScriptRejected(String),
    /// Over quota (`QUOTA`, `QUOTA/MAXSIZE`, `QUOTA/MAXSCRIPTS`).
    #[error("over quota: {0}")]
    Quota(String),
    /// The server asks to try again later (`TRYLATER`).
    #[error("temporarily unavailable: {0}")]
    TryLater(String),
    /// The server answered `NO` to a command.
    #[error("server refused {command}: {message}")]
    Refused {
        /// The command (without arguments).
        command: String,
        /// The server's response code, e.g. `NONEXISTENT`.
        code: Option<String>,
        /// The server's text.
        message: String,
    },
    /// A script with Nail's name exists but was not written by Nail; it is
    /// left alone.
    #[error("script {0:?} on the server was not written by Nail")]
    NotManaged(String),
    /// The server closed the connection (`BYE` or EOF).
    #[error("connection closed by the server{0}")]
    ConnectionLost(String),
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

impl SieveError {
    /// The API error code.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::AuthFailed(_) => ErrorCode::AuthFailed,
            Self::AuthUnsupported(_) | Self::StartTlsUnavailable | Self::NotManageSieve(_) => {
                ErrorCode::Unsupported
            }
            Self::EncryptionNeeded(_) => ErrorCode::Tls,
            Self::InvalidServerName(_) => ErrorCode::Config,
            Self::Tls { .. } => ErrorCode::Tls,
            Self::ScriptRejected(_) | Self::Refused { .. } => ErrorCode::Rejected,
            Self::Quota(_) => ErrorCode::TooLarge,
            Self::NotManaged(_) => ErrorCode::Conflict,
            Self::Connect { .. }
            | Self::TryLater(_)
            | Self::ConnectionLost(_)
            | Self::Timeout(_)
            | Self::Protocol(_)
            | Self::Io(_) => ErrorCode::Network,
        }
    }

    /// Whether retrying later may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Connect { .. }
                | Self::Tls { .. }
                | Self::TryLater(_)
                | Self::ConnectionLost(_)
                | Self::Timeout(_)
                | Self::Protocol(_)
                | Self::Io(_)
        )
    }

    /// Whether the account has no ManageSieve service at all (connection
    /// refused, not a ManageSieve server): the core shows the account as
    /// `unavailable` and runs every rule on the client.
    pub fn is_unavailable(&self) -> bool {
        match self {
            Self::NotManageSieve(_) => true,
            Self::Connect { refused, .. } => *refused,
            _ => false,
        }
    }

    /// The user-facing text (Fluent id + arguments, `core-sieve.ftl`).
    pub fn localized(&self) -> LocalizedMessage {
        let detail = |id: &str, text: &str| LocalizedMessage::new(id).arg("detail", text);
        match self {
            Self::Connect { host, port, .. } => LocalizedMessage::new(reasons::ERROR_CONNECT)
                .arg("host", host.as_str())
                .arg("port", *port),
            Self::InvalidServerName(host) => LocalizedMessage::new(reasons::ERROR_CONNECT)
                .arg("host", host.as_str())
                .arg("port", 0u16),
            Self::Tls { host, .. } => {
                LocalizedMessage::new(reasons::ERROR_TLS).arg("host", host.as_str())
            }
            Self::StartTlsUnavailable => LocalizedMessage::new(reasons::ERROR_STARTTLS),
            Self::EncryptionNeeded(_) => LocalizedMessage::new(reasons::ERROR_ENCRYPTION),
            Self::AuthFailed(_) => LocalizedMessage::new(reasons::ERROR_AUTH),
            Self::AuthUnsupported(offered) => {
                LocalizedMessage::new(reasons::ERROR_MECHANISM).arg("mechanisms", offered.as_str())
            }
            Self::NotManageSieve(_) => LocalizedMessage::new(reasons::ERROR_UNAVAILABLE),
            Self::ScriptRejected(text) => detail(reasons::ERROR_SCRIPT, text),
            Self::Quota(text) => detail(reasons::ERROR_QUOTA, text),
            Self::TryLater(text) => detail(reasons::ERROR_TRY_LATER, text),
            Self::Refused {
                command, message, ..
            } => detail(reasons::ERROR_REFUSED, message).arg("command", command.as_str()),
            Self::NotManaged(name) => {
                LocalizedMessage::new(reasons::ERROR_NOT_MANAGED).arg("script", name.as_str())
            }
            Self::Timeout(d) => {
                LocalizedMessage::new(reasons::ERROR_TIMEOUT).arg("seconds", d.as_secs())
            }
            Self::ConnectionLost(_) | Self::Protocol(_) | Self::Io(_) => {
                LocalizedMessage::new(reasons::ERROR_PROTOCOL)
            }
        }
    }
}

impl From<SieveError> for EngineError {
    fn from(err: SieveError) -> Self {
        let localized = err.localized();
        Self {
            code: err.code(),
            message: err.to_string(),
            retryable: err.is_retryable(),
            localized: None,
            retry_after_secs: None,
        }
        .with_localized(localized)
    }
}

impl From<io::Error> for SieveError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe => Self::ConnectionLost(String::new()),
            _ => Self::Io(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::MessageArg;

    #[test]
    fn codes_retry_and_texts() {
        let refused = SieveError::Connect {
            host: "h".into(),
            port: 4190,
            message: "connection refused".into(),
            refused: true,
        };
        assert_eq!(refused.code(), ErrorCode::Network);
        assert!(refused.is_retryable() && refused.is_unavailable());
        let l = refused.localized();
        assert_eq!(l.id, "sieve-error-connect");
        assert_eq!(l.args["host"], MessageArg::from("h"));
        assert_eq!(l.args["port"], MessageArg::Number(4190));

        let rejected = SieveError::ScriptRejected("line 3: unknown command".into());
        assert_eq!(rejected.code(), ErrorCode::Rejected);
        assert!(!rejected.is_retryable() && !rejected.is_unavailable());
        let engine = EngineError::from(rejected);
        assert_eq!(engine.code, ErrorCode::Rejected);
        let localized = engine.localized.unwrap();
        assert_eq!(localized.id, "sieve-error-script");
        assert_eq!(
            localized.args["detail"],
            MessageArg::from("line 3: unknown command")
        );

        assert_eq!(
            SieveError::AuthFailed("x".into()).code(),
            ErrorCode::AuthFailed
        );
        assert_eq!(SieveError::Quota("x".into()).code(), ErrorCode::TooLarge);
        assert_eq!(
            SieveError::NotManaged("nail".into()).code(),
            ErrorCode::Conflict
        );
        assert!(SieveError::NotManageSieve("x".into()).is_unavailable());
        assert_eq!(
            SieveError::Timeout(Duration::from_secs(30))
                .localized()
                .args["seconds"],
            MessageArg::Number(30)
        );
        for e in [
            SieveError::InvalidServerName("x y".into()),
            SieveError::Tls {
                host: "h".into(),
                message: "bad".into(),
            },
            SieveError::StartTlsUnavailable,
            SieveError::EncryptionNeeded("x".into()),
            SieveError::AuthUnsupported("CRAM-MD5".into()),
            SieveError::TryLater("x".into()),
            SieveError::Refused {
                command: "SETACTIVE".into(),
                code: None,
                message: "no".into(),
            },
            SieveError::ConnectionLost(String::new()),
            SieveError::Protocol("x".into()),
            SieveError::Io("x".into()),
        ] {
            assert!(reasons::ALL.contains(&e.localized().id.as_str()), "{e}");
        }
        let lost: SieveError = io::Error::from(io::ErrorKind::UnexpectedEof).into();
        assert_eq!(lost, SieveError::ConnectionLost(String::new()));
    }
}
