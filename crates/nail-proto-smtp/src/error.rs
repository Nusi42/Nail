//! Error type of the SMTP client.

use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::engine::EngineError;

/// Result alias of this crate.
pub type Result<T, E = SmtpError> = std::result::Result<T, E>;

/// Errors talking to an SMTP server. Messages are English diagnostics for
/// logs (server replies included, secrets never); the UI shows the localized
/// text for [`SmtpError::code`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SmtpError {
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
    /// TLS setup or handshake failed (untrusted certificate, wrong host).
    #[error("TLS: {0}")]
    Tls(String),
    /// STARTTLS was required but the server does not offer it. Never falls
    /// back to plaintext.
    #[error("the server does not offer STARTTLS")]
    StartTlsUnavailable,
    /// The server rejected the credentials (SMTP 535 and friends).
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// No usable authentication mechanism.
    #[error("no supported authentication mechanism: {0}")]
    AuthUnsupported(String),
    /// The message exceeds the server's `SIZE` limit.
    #[error("message too large: {0}")]
    TooLarge(String),
    /// The server permanently refused the message or a recipient (5xx).
    #[error("rejected by the server ({code}): {message}")]
    Rejected {
        /// Reply code.
        code: u16,
        /// Server reply.
        message: String,
    },
    /// The server refused temporarily (4xx); try again later.
    #[error("temporarily refused by the server ({code}): {message}")]
    Transient {
        /// Reply code.
        code: u16,
        /// Server reply.
        message: String,
    },
    /// An address of the envelope is not a valid mailbox.
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    /// The message needs an extension the server lacks (`8BITMIME`,
    /// `SMTPUTF8`).
    #[error("not supported by the server: {0}")]
    Unsupported(String),
    /// An operation took longer than the configured timeout.
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    /// The connection broke.
    #[error("connection lost: {0}")]
    ConnectionLost(String),
    /// The server sent something unexpected.
    #[error("protocol error: {0}")]
    Protocol(String),
}

nail_net::line_client_codes!(SmtpError {
    Self::TooLarge(_) => ErrorCode::TooLarge,
    Self::Rejected { .. } => ErrorCode::Rejected,
    Self::InvalidAddress(_) => ErrorCode::InvalidRequest,
    Self::Transient { .. } | Self::ConnectionLost(_) => ErrorCode::Network,
});

impl SmtpError {
    /// Whether trying again later may succeed (the outbox keeps the message
    /// queued) — network trouble, TLS trouble and 4xx replies.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Connect { .. }
                | Self::Tls(_)
                | Self::Transient { .. }
                | Self::Timeout(_)
                | Self::ConnectionLost(_)
                | Self::Protocol(_)
        )
    }
}

impl From<SmtpError> for EngineError {
    fn from(err: SmtpError) -> Self {
        Self {
            code: err.code(),
            message: err.to_string(),
            retryable: err.is_retryable(),
            localized: None,
            retry_after_secs: None,
        }
    }
}

/// What the client was doing when an error happened; decides how a server
/// reply is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Connect,
    Auth,
    Envelope,
    Data,
    Other,
}

/// Classifies a `lettre` error.
pub(crate) fn map_lettre(
    err: &lettre::transport::smtp::Error,
    phase: Phase,
    config: &crate::SmtpConfig,
) -> SmtpError {
    let message = err.to_string();
    if let Some(code) = err.status() {
        let code = u16::from(code);
        return classify_reply(code, message, phase);
    }
    if err.is_tls() {
        return SmtpError::Tls(message);
    }
    if err.is_timeout() {
        return SmtpError::Timeout(config.timeout);
    }
    if err.is_client() || err.is_response() {
        return SmtpError::Protocol(message);
    }
    if phase == Phase::Connect {
        SmtpError::Connect {
            host: config.host.clone(),
            port: config.port,
            message,
        }
    } else {
        SmtpError::ConnectionLost(message)
    }
}

/// Classifies a negative reply.
pub(crate) fn classify_reply(code: u16, message: String, phase: Phase) -> SmtpError {
    let too_large = code == 552 || message.contains("5.3.4") || message.contains("5.2.3");
    match (code / 100, phase) {
        (5, Phase::Auth) if code == 504 => SmtpError::AuthUnsupported(message),
        (5, Phase::Auth) => SmtpError::AuthFailed(message),
        // "Authentication required": the account needs credentials.
        (5, Phase::Envelope) if code == 530 => SmtpError::AuthFailed(message),
        (5, Phase::Envelope | Phase::Data) if too_large => SmtpError::TooLarge(message),
        (5, _) => SmtpError::Rejected { code, message },
        _ => SmtpError::Transient { code, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_replies_by_phase() {
        assert!(matches!(
            classify_reply(535, "5.7.8 bad".into(), Phase::Auth),
            SmtpError::AuthFailed(_)
        ));
        assert!(matches!(
            classify_reply(504, "x".into(), Phase::Auth),
            SmtpError::AuthUnsupported(_)
        ));
        assert!(matches!(
            classify_reply(454, "4.7.0 later".into(), Phase::Auth),
            SmtpError::Transient { code: 454, .. }
        ));
        assert!(matches!(
            classify_reply(552, "x".into(), Phase::Data),
            SmtpError::TooLarge(_)
        ));
        assert!(matches!(
            classify_reply(554, "5.3.4 too big".into(), Phase::Envelope),
            SmtpError::TooLarge(_)
        ));
        assert!(matches!(
            classify_reply(530, "auth required".into(), Phase::Envelope),
            SmtpError::AuthFailed(_)
        ));
        assert!(matches!(
            classify_reply(550, "5.1.1 unknown".into(), Phase::Envelope),
            SmtpError::Rejected { code: 550, .. }
        ));
        assert!(matches!(
            classify_reply(554, "no".into(), Phase::Connect),
            SmtpError::Rejected { .. }
        ));
        assert!(matches!(
            classify_reply(421, "bye".into(), Phase::Other),
            SmtpError::Transient { .. }
        ));
    }

    #[test]
    fn codes_and_retry() {
        let rejected = SmtpError::Rejected {
            code: 550,
            message: "x".into(),
        };
        assert_eq!(rejected.code(), ErrorCode::Rejected);
        assert!(!rejected.is_retryable());
        let transient = SmtpError::Transient {
            code: 451,
            message: "x".into(),
        };
        assert_eq!(transient.code(), ErrorCode::Network);
        assert!(transient.is_retryable());
        let engine: EngineError = SmtpError::TooLarge("x".into()).into();
        assert_eq!(
            (engine.code, engine.retryable),
            (ErrorCode::TooLarge, false)
        );
        let engine: EngineError = SmtpError::AuthFailed("x".into()).into();
        assert_eq!(
            (engine.code, engine.retryable),
            (ErrorCode::AuthFailed, false)
        );
        let engine: EngineError = SmtpError::Tls("expired".into()).into();
        assert_eq!((engine.code, engine.retryable), (ErrorCode::Tls, true));
        assert_eq!(
            SmtpError::StartTlsUnavailable.code(),
            ErrorCode::Unsupported
        );
    }
}
