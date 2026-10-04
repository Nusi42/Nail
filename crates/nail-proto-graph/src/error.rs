//! Errors of the Microsoft Graph client, mapped to [`ErrorCode`]s, retry
//! hints and user-facing texts (`graph-*` in `locales/en/core-graph.ftl`).

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;

use crate::wire::ErrorBody;

/// A failed Graph call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// The server could not be reached, or the connection broke.
    #[error("network error: {0}")]
    Network(String),
    /// The server took too long.
    #[error("Microsoft Graph did not answer in time")]
    Timeout,
    /// The TLS handshake failed.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The access token was refused even after a refresh (`401`).
    #[error("Microsoft Graph refused the sign-in")]
    AuthFailed,
    /// Access denied (`403`): a missing permission or consent, or no
    /// access to another mailbox.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The item, folder or mailbox does not exist (`404`).
    #[error("not found: {0}")]
    NotFound(String),
    /// The mailbox is not reachable through Graph (on-premises or
    /// inactive: `MailboxNotEnabledForRESTAPI`).
    #[error("the mailbox is not available through Microsoft Graph: {0}")]
    MailboxUnavailable(String),
    /// A delta link the server no longer accepts (`410`,
    /// `syncStateNotFound`, `resyncRequired`): sync again from the start.
    #[error("the sync state expired: {0}")]
    SyncStateExpired(String),
    /// Throttled (`429`, or `503` / `504` with the throttling answer); wait
    /// the given seconds if the server said.
    #[error("Microsoft Graph asks to slow down")]
    Throttled(Option<u32>),
    /// The server failed (`5xx`) even after retrying.
    #[error("Microsoft Graph is unavailable (HTTP {status})")]
    Unavailable {
        /// The last status code.
        status: u16,
        /// The server's `Retry-After`, if any.
        retry_after: Option<u32>,
    },
    /// The request was refused as invalid (`400`).
    #[error("rejected: {0}")]
    Rejected(String),
    /// The item changed on the server since it was read (`409`, `412`).
    #[error("conflict: {0}")]
    Conflict(String),
    /// The upload is larger than Graph accepts, or the answer larger than
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
    /// The answer was not what Graph sends (malformed JSON, a paging link
    /// to another server, a broken `$batch` answer).
    #[error("invalid server answer: {0}")]
    Protocol(String),
    /// The API URL is not usable: not `https:` (or `http:` to a loopback
    /// host).
    #[error("insecure or invalid URL: {0}")]
    InsecureUrl(String),
    /// The account is not a Graph Exchange account signed in with OAuth2.
    #[error("not a Microsoft Graph account: {0}")]
    Config(String),
    /// Getting an OAuth2 access token failed.
    #[error("access token: {0}")]
    Token(Box<EngineError>),
    /// Graph cannot do this (e.g. a recipient address that cannot go into
    /// a header, a task list of another mailbox).
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl GraphError {
    /// The error class for the UI.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Network(_) | Self::Timeout | Self::Protocol(_) => ErrorCode::Network,
            Self::Tls(_) => ErrorCode::Tls,
            Self::AuthFailed => ErrorCode::AuthFailed,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::NotFound(_) | Self::MailboxUnavailable(_) => ErrorCode::NotFound,
            Self::SyncStateExpired(_) | Self::Conflict(_) => ErrorCode::Conflict,
            Self::Throttled(_) => ErrorCode::RateLimited,
            Self::Unavailable { .. } => ErrorCode::Unavailable,
            Self::Status { status, .. } if *status >= 500 => ErrorCode::Unavailable,
            Self::Rejected(_) | Self::Status { .. } => ErrorCode::Rejected,
            Self::TooLarge(_) => ErrorCode::TooLarge,
            Self::InsecureUrl(_) | Self::Config(_) => ErrorCode::Config,
            Self::Token(e) => e.code,
            Self::Unsupported(_) => ErrorCode::Unsupported,
        }
    }

    /// Whether trying again later may succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_)
            | Self::Timeout
            | Self::Throttled(_)
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
            Self::Throttled(secs) => *secs,
            Self::Unavailable { retry_after, .. } => *retry_after,
            Self::Token(e) => e.retry_after_secs,
            _ => None,
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::Network(_) => LocalizedMessage::new("graph-network"),
            Self::Timeout => LocalizedMessage::new("graph-timeout"),
            Self::Tls(_) => LocalizedMessage::new("graph-tls"),
            Self::AuthFailed => LocalizedMessage::new("graph-auth-failed"),
            Self::Forbidden(_) => LocalizedMessage::new("graph-forbidden"),
            Self::NotFound(_) => LocalizedMessage::new("graph-missing"),
            Self::MailboxUnavailable(_) => LocalizedMessage::new("graph-mailbox-unavailable"),
            Self::SyncStateExpired(_) => LocalizedMessage::new("graph-sync-expired"),
            Self::Throttled(_) => LocalizedMessage::new("graph-throttled"),
            Self::Unavailable { .. } => LocalizedMessage::new("graph-unavailable"),
            Self::Rejected(_) => LocalizedMessage::new("graph-rejected"),
            Self::Conflict(_) => LocalizedMessage::new("graph-conflict"),
            Self::TooLarge(_) => LocalizedMessage::new("graph-too-large"),
            Self::Status { status, .. } => {
                LocalizedMessage::new("graph-http-status").arg("status", i64::from(*status))
            }
            Self::Protocol(_) => LocalizedMessage::new("graph-bad-response"),
            Self::InsecureUrl(_) => LocalizedMessage::new("graph-insecure-url"),
            Self::Config(_) => LocalizedMessage::new("graph-config"),
            Self::Token(e) => e
                .localized
                .as_deref()
                .cloned()
                .unwrap_or_else(|| LocalizedMessage::new("graph-token")),
            Self::Unsupported(_) => LocalizedMessage::new("graph-unsupported"),
        }
    }

    /// Every Fluent id [`GraphError::localized`] can return (for the
    /// catalog coverage test).
    pub const MESSAGE_IDS: [&'static str; 19] = [
        "graph-network",
        "graph-timeout",
        "graph-tls",
        "graph-auth-failed",
        "graph-forbidden",
        "graph-missing",
        "graph-mailbox-unavailable",
        "graph-sync-expired",
        "graph-throttled",
        "graph-unavailable",
        "graph-rejected",
        "graph-conflict",
        "graph-too-large",
        "graph-http-status",
        "graph-bad-response",
        "graph-insecure-url",
        "graph-config",
        "graph-token",
        "graph-unsupported",
    ];

    /// Whether a failed answer is worth retrying inline (throttling and
    /// server errors).
    pub(crate) fn is_transient(status: u16) -> bool {
        matches!(status, 429 | 500 | 502 | 503 | 504)
    }

    /// Whether a status means "throttled, not processed" (safe to repeat
    /// even for non-idempotent calls).
    pub(crate) fn is_throttle(status: u16) -> bool {
        status == 429
    }

    /// The error for a failed answer of `what` (`body` = Graph's JSON
    /// error object, when it sent one).
    pub(crate) fn from_status(
        status: u16,
        retry_after: Option<u32>,
        body: Option<&ErrorBody>,
        what: &str,
    ) -> Self {
        let code = body.map(|b| b.error.code.as_str()).unwrap_or_default();
        let message = body
            .map(|b| b.error.message.clone())
            .filter(|m| !m.is_empty())
            .map_or_else(|| what.to_owned(), |m| format!("{what}: {m}"));
        if is_sync_state_code(code) {
            return Self::SyncStateExpired(message);
        }
        if code.eq_ignore_ascii_case("MailboxNotEnabledForRESTAPI")
            || code.eq_ignore_ascii_case("MailboxNotSupportedForRESTAPI")
        {
            return Self::MailboxUnavailable(message);
        }
        match status {
            401 => Self::AuthFailed,
            403 => Self::Forbidden(message),
            404 => Self::NotFound(message),
            410 => Self::SyncStateExpired(message),
            409 | 412 => Self::Conflict(message),
            413 => Self::TooLarge(message),
            429 | 503 | 504 => Self::Throttled(retry_after),
            400 if code.eq_ignore_ascii_case("ErrorMessageSizeExceeded") => Self::TooLarge(message),
            400 => Self::Rejected(message),
            500..=599 => Self::Unavailable {
                status,
                retry_after,
            },
            _ => Self::Status { status, message },
        }
    }
}

/// Error codes Graph answers for an expired or broken delta link.
fn is_sync_state_code(code: &str) -> bool {
    [
        "syncStateNotFound",
        "resyncRequired",
        "SyncStateNotFound",
        "SyncStateInvalid",
        "ErrorInvalidSyncStateData",
        "ErrorSyncStateNotFound",
    ]
    .iter()
    .any(|c| c.eq_ignore_ascii_case(code))
}

impl From<GraphError> for EngineError {
    fn from(err: GraphError) -> Self {
        if let GraphError::Token(inner) = err {
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

    fn all() -> Vec<GraphError> {
        vec![
            GraphError::Network("reset".into()),
            GraphError::Timeout,
            GraphError::Tls("bad cert".into()),
            GraphError::AuthFailed,
            GraphError::Forbidden("x".into()),
            GraphError::NotFound("x".into()),
            GraphError::MailboxUnavailable("x".into()),
            GraphError::SyncStateExpired("x".into()),
            GraphError::Throttled(Some(3)),
            GraphError::Unavailable {
                status: 500,
                retry_after: None,
            },
            GraphError::Rejected("x".into()),
            GraphError::Conflict("x".into()),
            GraphError::TooLarge("x".into()),
            GraphError::Status {
                status: 418,
                message: "x".into(),
            },
            GraphError::Protocol("x".into()),
            GraphError::InsecureUrl("http://x".into()),
            GraphError::Config("imap".into()),
            GraphError::Token(Box::new(EngineError::permanent(
                ErrorCode::AuthFailed,
                "revoked",
            ))),
            GraphError::Unsupported("x".into()),
        ]
    }

    fn body(code: &str, message: &str) -> ErrorBody {
        serde_json::from_value(serde_json::json!({
            "error": {"code": code, "message": message}
        }))
        .unwrap()
    }

    #[test]
    fn statuses_map_to_errors() {
        assert_eq!(
            GraphError::from_status(401, None, None, "x"),
            GraphError::AuthFailed
        );
        assert_eq!(
            GraphError::from_status(429, Some(7), None, "x"),
            GraphError::Throttled(Some(7))
        );
        assert_eq!(
            GraphError::from_status(503, Some(2), None, "x"),
            GraphError::Throttled(Some(2))
        );
        assert_eq!(
            GraphError::from_status(500, None, None, "x"),
            GraphError::Unavailable {
                status: 500,
                retry_after: None
            }
        );
        assert!(matches!(
            GraphError::from_status(410, None, None, "x"),
            GraphError::SyncStateExpired(_)
        ));
        assert!(matches!(
            GraphError::from_status(400, None, Some(&body("syncStateNotFound", "gone")), "x"),
            GraphError::SyncStateExpired(_)
        ));
        assert!(matches!(
            GraphError::from_status(
                404,
                None,
                Some(&body("MailboxNotEnabledForRESTAPI", "on-prem")),
                "x"
            ),
            GraphError::MailboxUnavailable(_)
        ));
        assert!(matches!(
            GraphError::from_status(412, None, None, "x"),
            GraphError::Conflict(_)
        ));
        assert!(matches!(
            GraphError::from_status(
                400,
                None,
                Some(&body("ErrorMessageSizeExceeded", "big")),
                "x"
            ),
            GraphError::TooLarge(_)
        ));
        let GraphError::Forbidden(message) = GraphError::from_status(
            403,
            None,
            Some(&body("ErrorAccessDenied", "Access is denied.")),
            "GET /users/x",
        ) else {
            panic!("expected forbidden");
        };
        assert_eq!(message, "GET /users/x: Access is denied.");
        assert!(matches!(
            GraphError::from_status(418, None, None, "x"),
            GraphError::Status { status: 418, .. }
        ));
        assert!(GraphError::is_transient(503) && !GraphError::is_transient(400));
        assert!(GraphError::is_throttle(429) && !GraphError::is_throttle(503));
    }

    #[test]
    fn engine_errors_carry_codes_retry_and_texts() {
        let err: EngineError = GraphError::Throttled(Some(30)).into();
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(err.retryable);
        assert_eq!(err.retry_after_secs, Some(30));
        assert_eq!(err.localized.unwrap().id, "graph-throttled");
        let err: EngineError = GraphError::Forbidden("no".into()).into();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(!err.retryable);
        let token = EngineError::permanent(ErrorCode::ReauthRequired, "sign in again");
        let passed: EngineError = GraphError::Token(Box::new(token.clone())).into();
        assert_eq!(passed, token);
    }

    #[test]
    fn every_error_has_a_listed_message() {
        for error in all() {
            let id = error.localized().id;
            assert!(GraphError::MESSAGE_IDS.contains(&id.as_str()), "{id}");
            assert!(!error.to_string().is_empty());
            let _ = (error.code(), error.retryable(), error.retry_after_secs());
        }
    }

    #[test]
    fn catalog_has_every_message() {
        let catalog = include_str!("../../../locales/en/core-graph.ftl");
        for id in GraphError::MESSAGE_IDS {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{id} ="))),
                "{id} missing in locales/en/core-graph.ftl"
            );
        }
        let german = include_str!("../../../locales/de/core-graph.ftl");
        assert!(
            german
                .lines()
                .all(|l| l.trim().is_empty() || l.starts_with('#')),
            "the German catalog stays empty"
        );
    }
}
