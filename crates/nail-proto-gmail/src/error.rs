//! Errors of the Gmail API client, mapped to [`ErrorCode`]s, retry hints
//! and user-facing texts (`gmail-*` in `locales/en/core-gmail.ftl`).

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;

use crate::wire::ErrorBody;

/// A failed Gmail API call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GmailError {
    /// The server could not be reached, or the connection broke.
    #[error("network error: {0}")]
    Network(String),
    /// The server took too long.
    #[error("the Gmail API did not answer in time")]
    Timeout,
    /// The TLS handshake failed.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The access token was refused even after a refresh (`401`).
    #[error("the Gmail API refused the sign-in")]
    AuthFailed,
    /// The token lacks the Gmail scope (`403 insufficientPermissions`).
    #[error("the sign-in does not allow access to Gmail")]
    InsufficientScope,
    /// Another `403`.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The message, label, thread or history id does not exist (`404`,
    /// `410`).
    #[error("not found: {0}")]
    NotFound(String),
    /// Quota or rate limit (`429`, `403 rateLimitExceeded` /
    /// `userRateLimitExceeded` / `dailyLimitExceeded`); wait the given
    /// seconds if the server said.
    #[error("Gmail asks to slow down")]
    RateLimited(Option<u32>),
    /// The server failed (`5xx`) even after retrying.
    #[error("the Gmail API is unavailable (HTTP {status})")]
    Unavailable {
        /// The last status code.
        status: u16,
        /// The server's `Retry-After`, if any.
        retry_after: Option<u32>,
    },
    /// The request was refused as invalid (`400`): a bad label, an
    /// invalid recipient, a malformed message.
    #[error("rejected: {0}")]
    Rejected(String),
    /// A label colour is not in Gmail's palette (`400` on labels).
    #[error("label colour rejected: {0}")]
    ColorRejected(String),
    /// A label with this name already exists (`409`).
    #[error("conflict: {0}")]
    Conflict(String),
    /// The message is larger than Gmail accepts, or the answer larger than
    /// Nail reads (`413`).
    #[error("too large: {0}")]
    TooLarge(String),
    /// Another HTTP status.
    #[error("unexpected HTTP status {status}: {message}")]
    Status {
        /// The status code.
        status: u16,
        /// What was attempted.
        message: String,
    },
    /// The answer was not what the Gmail API sends (malformed JSON or
    /// base64, a broken batch answer).
    #[error("invalid server answer: {0}")]
    Protocol(String),
    /// The API URL is not usable: not `https:` (or `http:` to a loopback
    /// host).
    #[error("insecure or invalid URL: {0}")]
    InsecureUrl(String),
    /// The account is not a Gmail account signed in with OAuth2.
    #[error("not a Gmail API account: {0}")]
    Config(String),
    /// Getting an OAuth2 access token failed.
    #[error("access token: {0}")]
    Token(Box<EngineError>),
    /// Gmail has no such move (into Sent or Drafts, out of Drafts, Sent
    /// to the archive).
    #[error("Gmail cannot move a message from {from} to {to}")]
    MoveUnsupported {
        /// Source folder path.
        from: String,
        /// Target folder path.
        to: String,
    },
}

impl GmailError {
    /// The error class for the UI.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Network(_) | Self::Timeout | Self::Protocol(_) => ErrorCode::Network,
            Self::Tls(_) => ErrorCode::Tls,
            Self::AuthFailed | Self::InsufficientScope => ErrorCode::AuthFailed,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::RateLimited(_) => ErrorCode::RateLimited,
            Self::Unavailable { .. } => ErrorCode::Unavailable,
            Self::Status { status, .. } if *status >= 500 => ErrorCode::Unavailable,
            Self::Rejected(_) | Self::ColorRejected(_) | Self::Status { .. } => ErrorCode::Rejected,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::TooLarge(_) => ErrorCode::TooLarge,
            Self::InsecureUrl(_) | Self::Config(_) => ErrorCode::Config,
            Self::Token(e) => e.code,
            Self::MoveUnsupported { .. } => ErrorCode::Unsupported,
        }
    }

    /// Whether trying again later may succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_)
            | Self::Timeout
            | Self::RateLimited(_)
            | Self::Unavailable { .. }
            | Self::Protocol(_) => true,
            Self::Status { status, .. } => *status >= 500,
            Self::Token(e) => e.retryable,
            _ => false,
        }
    }

    /// How long the server asked to wait, in seconds.
    pub fn retry_after_secs(&self) -> Option<u32> {
        match self {
            Self::RateLimited(secs) => *secs,
            Self::Unavailable { retry_after, .. } => *retry_after,
            Self::Token(e) => e.retry_after_secs,
            _ => None,
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::Network(_) => LocalizedMessage::new("gmail-network"),
            Self::Timeout => LocalizedMessage::new("gmail-timeout"),
            Self::Tls(_) => LocalizedMessage::new("gmail-tls"),
            Self::AuthFailed => LocalizedMessage::new("gmail-auth-failed"),
            Self::InsufficientScope => LocalizedMessage::new("gmail-insufficient-scope"),
            Self::Forbidden(_) => LocalizedMessage::new("gmail-forbidden"),
            Self::NotFound(_) => LocalizedMessage::new("gmail-missing"),
            Self::RateLimited(_) => LocalizedMessage::new("gmail-rate-limited"),
            Self::Unavailable { .. } => LocalizedMessage::new("gmail-unavailable"),
            Self::Rejected(_) => LocalizedMessage::new("gmail-rejected"),
            Self::ColorRejected(_) => LocalizedMessage::new("gmail-label-color-rejected"),
            Self::Conflict(_) => LocalizedMessage::new("gmail-label-conflict"),
            Self::TooLarge(_) => LocalizedMessage::new("gmail-too-large"),
            Self::Status { status, .. } => {
                LocalizedMessage::new("gmail-http-status").arg("status", i64::from(*status))
            }
            Self::Protocol(_) => LocalizedMessage::new("gmail-bad-response"),
            Self::InsecureUrl(_) => LocalizedMessage::new("gmail-insecure-url"),
            Self::Config(_) => LocalizedMessage::new("gmail-config"),
            Self::Token(e) => e
                .localized
                .as_deref()
                .cloned()
                .unwrap_or_else(|| LocalizedMessage::new("gmail-token")),
            Self::MoveUnsupported { .. } => LocalizedMessage::new("gmail-move-unsupported"),
        }
    }

    /// Every Fluent id [`GmailError::localized`] can return (for the
    /// catalog coverage test).
    pub const MESSAGE_IDS: [&'static str; 19] = [
        "gmail-config",
        "gmail-network",
        "gmail-timeout",
        "gmail-tls",
        "gmail-auth-failed",
        "gmail-insufficient-scope",
        "gmail-forbidden",
        "gmail-missing",
        "gmail-rate-limited",
        "gmail-unavailable",
        "gmail-rejected",
        "gmail-label-color-rejected",
        "gmail-label-conflict",
        "gmail-too-large",
        "gmail-http-status",
        "gmail-bad-response",
        "gmail-insecure-url",
        "gmail-token",
        "gmail-move-unsupported",
    ];

    /// Whether a failed answer is worth retrying inline (quota / rate
    /// limits and server errors), given its status and error reason.
    pub(crate) fn is_transient(status: u16, reason: Option<&str>) -> bool {
        match status {
            429 | 500 | 502 | 503 | 504 => true,
            403 => reason.is_some_and(is_rate_reason),
            _ => false,
        }
    }

    /// The error for a failed answer of `what` (`body` = Gmail's JSON error
    /// object, when it sent one).
    pub(crate) fn from_status(
        status: u16,
        retry_after: Option<u32>,
        body: Option<&ErrorBody>,
        what: &str,
    ) -> Self {
        let reason = body.and_then(ErrorBody::reason);
        let message = body
            .map(|b| b.error.message.clone())
            .filter(|m| !m.is_empty())
            .map_or_else(|| what.to_owned(), |m| format!("{what}: {m}"));
        match status {
            401 => Self::AuthFailed,
            403 if reason.is_some_and(is_rate_reason) => Self::RateLimited(retry_after),
            403 if reason.is_some_and(|r| {
                r == "insufficientPermissions" || r == "ACCESS_TOKEN_SCOPE_INSUFFICIENT"
            }) =>
            {
                Self::InsufficientScope
            }
            403 => Self::Forbidden(message),
            404 | 410 => Self::NotFound(message),
            409 => Self::Conflict(message),
            413 => Self::TooLarge(message),
            429 => Self::RateLimited(retry_after),
            400 if message.to_ascii_lowercase().contains("color") => Self::ColorRejected(message),
            400 if message.to_ascii_lowercase().contains("too large")
                || message.to_ascii_lowercase().contains("size exceeds") =>
            {
                Self::TooLarge(message)
            }
            400 => Self::Rejected(message),
            500..=599 => Self::Unavailable {
                status,
                retry_after,
            },
            _ => Self::Status { status, message },
        }
    }
}

fn is_rate_reason(reason: &str) -> bool {
    matches!(
        reason,
        "rateLimitExceeded" | "userRateLimitExceeded" | "dailyLimitExceeded" | "quotaExceeded"
    )
}

impl From<GmailError> for EngineError {
    fn from(err: GmailError) -> Self {
        if let GmailError::Token(inner) = err {
            return *inner;
        }
        let localized = err.localized();
        let retry_after = err.retry_after_secs();
        let mut out = if err.retryable() {
            EngineError::temporary(err.code(), err.to_string())
        } else {
            EngineError::permanent(err.code(), err.to_string())
        };
        out.retry_after_secs = retry_after;
        out.with_localized(localized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<GmailError> {
        vec![
            GmailError::Network("reset".into()),
            GmailError::Timeout,
            GmailError::Tls("bad cert".into()),
            GmailError::AuthFailed,
            GmailError::InsufficientScope,
            GmailError::Forbidden("x".into()),
            GmailError::NotFound("x".into()),
            GmailError::RateLimited(Some(3)),
            GmailError::Unavailable {
                status: 503,
                retry_after: None,
            },
            GmailError::Rejected("x".into()),
            GmailError::ColorRejected("x".into()),
            GmailError::Conflict("x".into()),
            GmailError::TooLarge("x".into()),
            GmailError::Status {
                status: 418,
                message: "x".into(),
            },
            GmailError::Protocol("x".into()),
            GmailError::InsecureUrl("http://x".into()),
            GmailError::Config("imap".into()),
            GmailError::Token(Box::new(EngineError::permanent(
                ErrorCode::AuthFailed,
                "revoked",
            ))),
            GmailError::MoveUnsupported {
                from: "INBOX".into(),
                to: "SENT".into(),
            },
        ]
    }

    fn body(reason: &str, message: &str) -> ErrorBody {
        serde_json::from_value(serde_json::json!({
            "error": {"code": 403, "message": message,
                      "errors": [{"domain": "usageLimits", "reason": reason, "message": message}]}
        }))
        .unwrap()
    }

    #[test]
    fn statuses_map_to_errors() {
        assert_eq!(
            GmailError::from_status(401, None, None, "x"),
            GmailError::AuthFailed
        );
        assert_eq!(
            GmailError::from_status(429, Some(7), None, "x"),
            GmailError::RateLimited(Some(7))
        );
        assert_eq!(
            GmailError::from_status(
                403,
                None,
                Some(&body("userRateLimitExceeded", "User Rate Limit Exceeded")),
                "x"
            ),
            GmailError::RateLimited(None)
        );
        assert_eq!(
            GmailError::from_status(
                403,
                None,
                Some(&body("insufficientPermissions", "Insufficient Permission")),
                "x"
            ),
            GmailError::InsufficientScope
        );
        assert!(matches!(
            GmailError::from_status(403, None, Some(&body("forbidden", "no")), "x"),
            GmailError::Forbidden(_)
        ));
        assert!(matches!(
            GmailError::from_status(404, None, None, "x"),
            GmailError::NotFound(_)
        ));
        assert!(matches!(
            GmailError::from_status(
                400,
                None,
                Some(&body(
                    "invalidArgument",
                    "Label color #123456 is not on the allowed color palette"
                )),
                "label"
            ),
            GmailError::ColorRejected(_)
        ));
        assert!(matches!(
            GmailError::from_status(
                400,
                None,
                Some(&body("invalidArgument", "Invalid To header")),
                "send"
            ),
            GmailError::Rejected(_)
        ));
        assert_eq!(
            GmailError::from_status(502, Some(2), None, "x"),
            GmailError::Unavailable {
                status: 502,
                retry_after: Some(2)
            }
        );
        assert!(GmailError::is_transient(503, None));
        assert!(GmailError::is_transient(403, Some("rateLimitExceeded")));
        assert!(!GmailError::is_transient(403, Some("forbidden")));
        assert!(!GmailError::is_transient(404, None));
    }

    #[test]
    fn engine_errors_carry_code_retry_and_text() {
        let rate: EngineError = GmailError::RateLimited(Some(30)).into();
        assert_eq!(rate.code, ErrorCode::RateLimited);
        assert!(rate.retryable);
        assert_eq!(rate.retry_after_secs, Some(30));
        assert_eq!(rate.localized.unwrap().id, "gmail-rate-limited");
        let auth: EngineError = GmailError::AuthFailed.into();
        assert_eq!(auth.code, ErrorCode::AuthFailed);
        assert!(!auth.retryable);
        let status: EngineError = GmailError::Status {
            status: 418,
            message: "x".into(),
        }
        .into();
        let text = status.localized.unwrap();
        assert_eq!(text.id, "gmail-http-status");
        assert!(text.args.contains_key("status"));
        let token = EngineError::temporary(ErrorCode::Network, "token endpoint down");
        let passed: EngineError = GmailError::Token(Box::new(token.clone())).into();
        assert_eq!(passed, token);
        let moved: EngineError = GmailError::MoveUnsupported {
            from: "INBOX".into(),
            to: "SENT".into(),
        }
        .into();
        assert_eq!(moved.code, ErrorCode::Unsupported);
        assert!(!moved.retryable);
    }

    #[test]
    fn every_error_has_a_listed_message() {
        for error in all() {
            let id = error.localized().id;
            assert!(GmailError::MESSAGE_IDS.contains(&id.as_str()), "{id}");
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn catalog_has_every_message() {
        let catalog = include_str!("../../../locales/en/core-gmail.ftl");
        for id in GmailError::MESSAGE_IDS {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{id} ="))),
                "{id} missing in locales/en/core-gmail.ftl"
            );
        }
        let german = include_str!("../../../locales/de/core-gmail.ftl");
        assert!(
            german
                .lines()
                .all(|l| l.trim().is_empty() || l.starts_with('#')),
            "locales/de/core-gmail.ftl stays empty for now"
        );
    }
}
