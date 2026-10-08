//! Errors of the DAV client, mapped to [`ErrorCode`]s, retry hints and
//! user-facing texts (`dav-*` in `locales/en/core-dav.ftl`).

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;
use nail_net::status as http_status;

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
    /// A redirect led to another server (scheme, host or port), which
    /// asks for the login: Nail does not send it there on its own; the
    /// user can enter that server's address instead. Holds its host.
    #[error("redirected to {0}, which asks for the login")]
    RedirectNeedsLogin(String),
    /// The principal offers no CalDAV scheduling (RFC 6638: no
    /// `schedule-inbox-URL` / `schedule-outbox-URL`).
    #[error("the server does not offer CalDAV scheduling")]
    NoScheduling,
}

nail_net::client_error_classes!(DavError, "dav", Token,
    codes {
        Self::NoService(_) => ErrorCode::NotFound,
        Self::PreconditionFailed | Self::InvalidSyncToken => ErrorCode::Conflict,
        Self::RateLimited(_) => ErrorCode::RateLimited,
        Self::RedirectNeedsLogin(_) => ErrorCode::Config,
        Self::NoScheduling => ErrorCode::Unsupported,
    }
    texts {
        Self::NoService(_) => LocalizedMessage::new("dav-not-found"),
        Self::PreconditionFailed => LocalizedMessage::new("dav-conflict"),
        Self::InvalidSyncToken => LocalizedMessage::new("dav-sync-token-invalid"),
        Self::RateLimited(_) => LocalizedMessage::new("dav-rate-limited"),
        Self::RedirectNeedsLogin(host) => {
            LocalizedMessage::new("dav-redirect-login").arg("host", host.as_str())
        },
        Self::NoScheduling => LocalizedMessage::new("dav-no-scheduling"),
        Self::Token(e) => nail_net::http::token_text(e, "dav-token"),
    }
);

impl DavError {
    /// Whether trying again later may succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::Timeout | Self::RateLimited(_) => true,
            Self::Status { status, .. } => *status >= 500,
            Self::Token(e) => e.retryable,
            _ => false,
        }
    }

    /// Every Fluent id [`DavError::localized`] can return (for the catalog
    /// coverage test).
    pub const MESSAGE_IDS: [&'static str; 17] = [
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
        "dav-redirect-login",
        "dav-no-scheduling",
    ];

    /// The error for an unexpected HTTP status of `what`.
    pub(crate) fn from_status(status: u16, retry_after: Option<u64>, what: &str) -> Self {
        match status {
            401 => Self::AuthFailed,
            403 => Self::Forbidden(what.to_owned()),
            404 | http_status::GONE => Self::NotFound(what.to_owned()),
            412 => Self::PreconditionFailed,
            http_status::CONTENT_TOO_LARGE | 507 => Self::TooLarge(what.to_owned()),
            http_status::TOO_MANY_REQUESTS | 503 => Self::RateLimited(retry_after),
            _ => Self::Status {
                status,
                message: what.to_owned(),
            },
        }
    }
}

nail_net::transport_errors!(DavError);

nail_net::engine_error_from!(DavError, Token, |_: &DavError| None);

#[cfg(test)]
mod tests {
    use super::*;

    nail_testkit::error_texts_test!(DavError, "core-dav.ftl", crate::scheduling::MESSAGE_IDS);

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
            DavError::RedirectNeedsLogin("other.example".into()),
            DavError::NoScheduling,
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
}
