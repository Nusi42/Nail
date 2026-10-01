//! Errors of the DAV client, mapped to [`ErrorCode`]s, retry hints and
//! user-facing texts (`dav-*` in `locales/en/core-dav.ftl`).

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;

/// A failed DAV operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DavError {
    /// The server could not be reached, or the connection broke.
    #[error("network error: {0}")]
    Network(String),
    /// The server took too long.
    #[error("the server did not answer in time")]
    Timeout,
    /// The TLS handshake failed (untrusted certificate, wrong name, …).
    #[error("TLS error: {0}")]
    Tls(String),
    /// The server refused the login (`401`).
    #[error("the server refused the login")]
    AuthFailed,
    /// The login may not do this (`403`).
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The resource does not exist (`404` / `410`).
    #[error("not found: {0}")]
    NotFound(String),
    /// Discovery found no CalDAV / CardDAV service.
    #[error("no CalDAV / CardDAV service found: {0}")]
    NoService(String),
    /// A conditional write failed (`412`): someone else changed or
    /// deleted the resource, or it already exists.
    #[error("the resource changed on the server")]
    PreconditionFailed,
    /// The sync token is no longer valid (RFC 6578 `valid-sync-token`);
    /// the collection must be synced from scratch.
    #[error("the sync token is no longer valid")]
    InvalidSyncToken,
    /// The server is overloaded or rate limits (`429`, `503`); retry after
    /// the given seconds if it said.
    #[error("the server asks to retry later")]
    RateLimited(Option<u64>),
    /// The resource is too large for the server or for Nail (`413`, `507`,
    /// over the response limit).
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
    /// The answer was not valid WebDAV (malformed XML, missing parts).
    #[error("invalid server answer: {0}")]
    Protocol(String),
    /// The URL is not usable: not `https:` (or `http:` to a loopback
    /// host), or credentials would go over plain HTTP.
    #[error("insecure or invalid URL: {0}")]
    InsecureUrl(String),
    /// Getting an OAuth2 access token failed.
    #[error("access token: {0}")]
    Token(Box<EngineError>),
}

impl DavError {
    /// The error class for the UI.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Network(_) | Self::Timeout | Self::Protocol(_) => ErrorCode::Network,
            Self::Tls(_) => ErrorCode::Tls,
            Self::AuthFailed => ErrorCode::AuthFailed,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::NotFound(_) | Self::NoService(_) => ErrorCode::NotFound,
            Self::PreconditionFailed | Self::InvalidSyncToken => ErrorCode::Conflict,
            Self::RateLimited(_) => ErrorCode::RateLimited,
            Self::TooLarge(_) => ErrorCode::TooLarge,
            Self::Status { status, .. } if *status >= 500 => ErrorCode::Unavailable,
            Self::Status { .. } => ErrorCode::Rejected,
            Self::InsecureUrl(_) => ErrorCode::Config,
            Self::Token(e) => e.code,
        }
    }

    /// Whether trying again later may succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::Timeout | Self::RateLimited(_) => true,
            Self::Status { status, .. } => *status >= 500,
            Self::Token(e) => e.retryable,
            _ => false,
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::Network(_) => LocalizedMessage::new("dav-network"),
            Self::Timeout => LocalizedMessage::new("dav-timeout"),
            Self::Tls(_) => LocalizedMessage::new("dav-tls"),
            Self::AuthFailed => LocalizedMessage::new("dav-auth-failed"),
            Self::Forbidden(_) => LocalizedMessage::new("dav-forbidden"),
            Self::NotFound(_) => LocalizedMessage::new("dav-missing"),
            Self::NoService(_) => LocalizedMessage::new("dav-not-found"),
            Self::PreconditionFailed => LocalizedMessage::new("dav-conflict"),
            Self::InvalidSyncToken => LocalizedMessage::new("dav-sync-token-invalid"),
            Self::RateLimited(_) => LocalizedMessage::new("dav-rate-limited"),
            Self::TooLarge(_) => LocalizedMessage::new("dav-too-large"),
            Self::Status { status, .. } => {
                LocalizedMessage::new("dav-http-status").arg("status", i64::from(*status))
            }
            Self::Protocol(_) => LocalizedMessage::new("dav-bad-response"),
            Self::InsecureUrl(_) => LocalizedMessage::new("dav-insecure-url"),
            Self::Token(e) => e
                .localized
                .as_deref()
                .cloned()
                .unwrap_or_else(|| LocalizedMessage::new("dav-token")),
        }
    }

    /// Every Fluent id [`DavError::localized`] can return (for the catalog
    /// coverage test).
    pub const MESSAGE_IDS: [&'static str; 15] = [
        "dav-network",
        "dav-timeout",
        "dav-tls",
        "dav-auth-failed",
        "dav-forbidden",
        "dav-missing",
        "dav-not-found",
        "dav-conflict",
        "dav-sync-token-invalid",
        "dav-rate-limited",
        "dav-too-large",
        "dav-http-status",
        "dav-bad-response",
        "dav-insecure-url",
        "dav-token",
    ];

    /// The error for an unexpected HTTP status of `what`.
    pub(crate) fn from_status(status: u16, retry_after: Option<u64>, what: &str) -> Self {
        match status {
            401 => Self::AuthFailed,
            403 => Self::Forbidden(what.to_owned()),
            404 | 410 => Self::NotFound(what.to_owned()),
            412 => Self::PreconditionFailed,
            413 | 507 => Self::TooLarge(what.to_owned()),
            429 | 503 => Self::RateLimited(retry_after),
            _ => Self::Status {
                status,
                message: what.to_owned(),
            },
        }
    }
}

impl From<DavError> for EngineError {
    fn from(err: DavError) -> Self {
        if let DavError::Token(inner) = err {
            return *inner;
        }
        let localized = err.localized();
        let out = if err.retryable() {
            EngineError::temporary(err.code(), err.to_string())
        } else {
            EngineError::permanent(err.code(), err.to_string())
        };
        out.with_localized(localized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<DavError> {
        vec![
            DavError::Network("reset".into()),
            DavError::Timeout,
            DavError::Tls("bad cert".into()),
            DavError::AuthFailed,
            DavError::Forbidden("PUT".into()),
            DavError::NotFound("/x".into()),
            DavError::NoService("example.org".into()),
            DavError::PreconditionFailed,
            DavError::InvalidSyncToken,
            DavError::RateLimited(Some(3)),
            DavError::TooLarge("x".into()),
            DavError::Status {
                status: 500,
                message: "x".into(),
            },
            DavError::Protocol("x".into()),
            DavError::InsecureUrl("http://x".into()),
            DavError::Token(Box::new(EngineError::permanent(
                ErrorCode::AuthFailed,
                "revoked",
            ))),
        ]
    }

    #[test]
    fn statuses_map_to_errors() {
        assert_eq!(DavError::from_status(401, None, "x"), DavError::AuthFailed);
        assert_eq!(
            DavError::from_status(412, None, "x"),
            DavError::PreconditionFailed
        );
        assert_eq!(
            DavError::from_status(503, Some(5), "x"),
            DavError::RateLimited(Some(5))
        );
        assert_eq!(
            DavError::from_status(410, None, "x"),
            DavError::NotFound("x".into())
        );
        let server = DavError::from_status(502, None, "x");
        assert_eq!(server.code(), ErrorCode::Unavailable);
        assert!(server.retryable());
        let client = DavError::from_status(400, None, "x");
        assert_eq!(client.code(), ErrorCode::Rejected);
        assert!(!client.retryable());
    }

    #[test]
    fn engine_errors_carry_code_retry_and_text() {
        let network: EngineError = DavError::Network("down".into()).into();
        assert_eq!(network.code, ErrorCode::Network);
        assert!(network.retryable);
        assert_eq!(network.localized.unwrap().id, "dav-network");
        let auth: EngineError = DavError::AuthFailed.into();
        assert_eq!(auth.code, ErrorCode::AuthFailed);
        assert!(!auth.retryable);
        let status: EngineError = DavError::Status {
            status: 418,
            message: "PUT".into(),
        }
        .into();
        let text = status.localized.unwrap();
        assert_eq!(text.id, "dav-http-status");
        assert!(text.args.contains_key("status"));
        let token = EngineError::temporary(ErrorCode::Network, "token endpoint down");
        let passed: EngineError = DavError::Token(Box::new(token.clone())).into();
        assert_eq!(passed, token);
    }

    #[test]
    fn every_error_has_a_listed_message() {
        for error in all() {
            let id = error.localized().id;
            assert!(DavError::MESSAGE_IDS.contains(&id.as_str()), "{id}");
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn catalog_has_every_message() {
        let catalog = include_str!("../../../locales/en/core-dav.ftl");
        for id in DavError::MESSAGE_IDS {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{id} ="))),
                "{id} missing in locales/en/core-dav.ftl"
            );
        }
        let german = include_str!("../../../locales/de/core-dav.ftl");
        assert!(
            german
                .lines()
                .all(|l| l.trim().is_empty() || l.starts_with('#')),
            "the German catalog stays empty"
        );
    }
}
