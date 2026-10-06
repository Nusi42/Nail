//! Errors of the EWS client, mapped to [`ErrorCode`]s, retry hints and
//! user-facing texts (`ews-*` in `locales/en/core-ews.ftl`).
//!
//! EWS reports problems three ways: HTTP statuses (`401`, `503`), SOAP
//! faults (`ErrorServerBusy`, `ErrorInvalidServerVersion`, schema errors)
//! and a `ResponseCode` per response message (`ErrorItemNotFound`,
//! `ErrorAccessDenied`, …). [`EwsError::from_response_code`] maps the
//! response codes.

use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::localized::LocalizedMessage;

/// A failed EWS call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EwsError {
    /// The server could not be reached, or the connection broke.
    #[error("network error: {0}")]
    Network(String),
    /// The server took too long.
    #[error("the Exchange server did not answer in time")]
    Timeout,
    /// The TLS handshake failed.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The server refused the sign-in (`401` after the handshake / one
    /// refresh), or the Negotiate handshake failed.
    #[error("the Exchange server refused the sign-in")]
    AuthFailed,
    /// The user may not do this (`ErrorAccessDenied`, `403`, no access to
    /// another mailbox, no free/busy permission).
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The item, folder or mailbox does not exist (`ErrorItemNotFound`,
    /// `ErrorFolderNotFound`, `ErrorNonExistentMailbox`, …).
    #[error("not found: {0}")]
    NotFound(String),
    /// Throttled (`ErrorServerBusy` with `BackOffMilliseconds`, HTTP
    /// `429`); wait the given seconds if the server said.
    #[error("the Exchange server asks to slow down")]
    RateLimited(Option<u32>),
    /// The server failed (HTTP `5xx`, a transient server error) even after
    /// retrying.
    #[error("the Exchange server is unavailable ({0})")]
    Unavailable(String, Option<u32>),
    /// The request was refused (an invalid recipient, a schema error, an
    /// unsupported property): the EWS response code and its message.
    #[error("rejected ({code}): {message}")]
    Rejected {
        /// The EWS response code (`ErrorInvalidRecipients`, …).
        code: String,
        /// The server's message text.
        message: String,
    },
    /// The item changed on the server since its change key was read
    /// (`ErrorIrresolvableConflict`), or a name is taken.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The message is larger than the server accepts, the mailbox is
    /// full, or the answer is larger than Nail reads.
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
    /// The answer was not EWS SOAP (malformed XML or base64, missing
    /// elements).
    #[error("invalid server answer: {0}")]
    Protocol(String),
    /// The EWS URL is not usable: not `https:` (or `http:` to a loopback
    /// host).
    #[error("insecure or invalid URL: {0}")]
    InsecureUrl(String),
    /// The account is not an EWS account with a supported sign-in.
    #[error("not an EWS account: {0}")]
    Config(String),
    /// Getting an OAuth2 access token or a stored password failed.
    #[error("credentials: {0}")]
    Credentials(Box<EngineError>),
    /// EWS cannot do this (classifying Focused Inbox on a server without
    /// it, a task recurrence EWS cannot store, …).
    #[error("not supported by EWS: {0}")]
    Unsupported(String),
    /// Changing inbox rules would delete the rules Outlook keeps only on
    /// the client (`ErrorOutlookRuleBlobExists`).
    #[error("Outlook client rules exist; changing server rules would delete them")]
    OutlookRuleBlob,
}

impl EwsError {
    /// Every Fluent id [`Self::localized`] can answer.
    pub const MESSAGE_IDS: [&'static str; 18] = [
        "ews-network",
        "ews-timeout",
        "ews-tls",
        "ews-auth-failed",
        "ews-forbidden",
        "ews-missing",
        "ews-rate-limited",
        "ews-unavailable",
        "ews-rejected",
        "ews-conflict",
        "ews-too-large",
        "ews-http-status",
        "ews-bad-response",
        "ews-insecure-url",
        "ews-config",
        "ews-credentials",
        "ews-unsupported",
        "ews-outlook-rule-blob",
    ];

    /// The error class for the UI.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Network(_) | Self::Timeout | Self::Protocol(_) => ErrorCode::Network,
            Self::Tls(_) => ErrorCode::Tls,
            Self::AuthFailed => ErrorCode::AuthFailed,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::RateLimited(_) => ErrorCode::RateLimited,
            Self::Unavailable(..) => ErrorCode::Unavailable,
            Self::Status { status, .. } if *status >= 500 => ErrorCode::Unavailable,
            Self::Rejected { .. } | Self::Status { .. } => ErrorCode::Rejected,
            Self::Conflict(_) | Self::OutlookRuleBlob => ErrorCode::Conflict,
            Self::TooLarge(_) => ErrorCode::TooLarge,
            Self::InsecureUrl(_) | Self::Config(_) => ErrorCode::Config,
            Self::Credentials(e) => e.code,
            Self::Unsupported(_) => ErrorCode::Unsupported,
        }
    }

    /// Whether trying again later may succeed.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Network(_)
            | Self::Timeout
            | Self::RateLimited(_)
            | Self::Unavailable(..)
            | Self::Protocol(_) => true,
            Self::Status { status, .. } => *status >= 500,
            Self::Credentials(e) => e.retryable,
            _ => false,
        }
    }

    /// How long the server asked to wait, in seconds.
    pub fn retry_after_secs(&self) -> Option<u32> {
        match self {
            Self::RateLimited(secs) | Self::Unavailable(_, secs) => *secs,
            Self::Credentials(e) => e.retry_after_secs,
            _ => None,
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(match self {
            Self::Network(_) => "ews-network",
            Self::Timeout => "ews-timeout",
            Self::Tls(_) => "ews-tls",
            Self::AuthFailed => "ews-auth-failed",
            Self::Forbidden(_) => "ews-forbidden",
            Self::NotFound(_) => "ews-missing",
            Self::RateLimited(_) => "ews-rate-limited",
            Self::Unavailable(..) => "ews-unavailable",
            Self::Rejected { .. } => "ews-rejected",
            Self::Conflict(_) => "ews-conflict",
            Self::TooLarge(_) => "ews-too-large",
            Self::Status { status, .. } => {
                return LocalizedMessage::new("ews-http-status").arg("status", i64::from(*status));
            }
            Self::Protocol(_) => "ews-bad-response",
            Self::InsecureUrl(_) => "ews-insecure-url",
            Self::Config(_) => "ews-config",
            Self::Credentials(_) => "ews-credentials",
            Self::Unsupported(_) => "ews-unsupported",
            Self::OutlookRuleBlob => "ews-outlook-rule-blob",
        })
    }

    /// The error of an EWS `ResponseCode` (anything but `NoError`), with
    /// the server's message text and, for `ErrorServerBusy`, its
    /// `BackOffMilliseconds`.
    pub fn from_response_code(code: &str, message: &str, back_off_ms: Option<u64>) -> Self {
        let detail = || {
            if message.is_empty() {
                code.to_owned()
            } else {
                format!("{code}: {message}")
            }
        };
        match code {
            "ErrorServerBusy" => Self::RateLimited(Some(back_off_secs(back_off_ms.unwrap_or(0)))),
            "ErrorItemNotFound"
            | "ErrorFolderNotFound"
            | "ErrorNonExistentMailbox"
            | "ErrorMailRecipientNotFound"
            | "ErrorCalendarOccurrenceIndexIsOutOfRecurrenceRange"
            | "ErrorNameResolutionNoResults"
            | "ErrorSyncFolderNotFound"
            | "ErrorRuleNotFound"
            | "ErrorParentFolderNotFound" => Self::NotFound(detail()),
            "ErrorAccessDenied"
            | "ErrorNoFreeBusyAccess"
            | "ErrorInsufficientResources"
            | "ErrorDelegateNoUser"
            | "ErrorImpersonationDenied"
            | "ErrorAccountDisabled"
            | "ErrorInvalidLicense"
            | "ErrorNotDelegate"
            | "ErrorNoPermissionToSearchOrHoldMailbox"
            | "ErrorSendAsDenied" => Self::Forbidden(detail()),
            "ErrorIrresolvableConflict"
            | "ErrorChangeKeyRequiredForWriteOperations"
            | "ErrorStaleObject"
            | "ErrorDuplicateUserIdsSpecified"
            | "ErrorFolderExists" => Self::Conflict(detail()),
            "ErrorMessageSizeExceeded"
            | "ErrorQuotaExceeded"
            | "ErrorQuotaExceededOnDelete"
            | "ErrorRecipientSizeExceeded"
            | "ErrorExceededFindCountLimit" => Self::TooLarge(detail()),
            "ErrorOutlookRuleBlobExists" => Self::OutlookRuleBlob,
            "ErrorInternalServerTransientError"
            | "ErrorMailboxStoreUnavailable"
            | "ErrorMailboxMoveInProgress"
            | "ErrorConnectionFailed"
            | "ErrorTimeoutExpired"
            | "ErrorExceededConnectionCount"
            | "ErrorTooManyObjectsOpened"
            | "ErrorFreeBusyGenerationFailed"
            | "ErrorProxyRequestProcessingFailed"
            | "ErrorBatchProcessingStopped" => Self::Unavailable(detail(), None),
            _ => Self::Rejected {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        }
    }
}

/// Seconds to wait for a `BackOffMilliseconds` value (rounded up, at least
/// one).
pub fn back_off_secs(ms: u64) -> u32 {
    u32::try_from(ms.div_ceil(1000).max(1)).unwrap_or(u32::MAX)
}

impl From<EwsError> for EngineError {
    fn from(err: EwsError) -> Self {
        if let EwsError::Credentials(inner) = err {
            return *inner;
        }
        let localized = err.localized();
        let mut out = if err.retryable() {
            EngineError::temporary(err.code(), err.to_string())
        } else {
            EngineError::permanent(err.code(), err.to_string())
        }
        .with_localized(localized);
        out.retry_after_secs = err.retry_after_secs();
        out
    }
}

impl From<crate::xml::XmlError> for EwsError {
    fn from(err: crate::xml::XmlError) -> Self {
        Self::Protocol(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn all() -> Vec<EwsError> {
        vec![
            EwsError::Network("x".into()),
            EwsError::Timeout,
            EwsError::Tls("x".into()),
            EwsError::AuthFailed,
            EwsError::Forbidden("x".into()),
            EwsError::NotFound("x".into()),
            EwsError::RateLimited(Some(3)),
            EwsError::Unavailable("x".into(), None),
            EwsError::Rejected {
                code: "ErrorX".into(),
                message: "x".into(),
            },
            EwsError::Conflict("x".into()),
            EwsError::TooLarge("x".into()),
            EwsError::Status {
                status: 418,
                message: "x".into(),
            },
            EwsError::Protocol("x".into()),
            EwsError::InsecureUrl("x".into()),
            EwsError::Config("x".into()),
            EwsError::Credentials(Box::new(EngineError::temporary(ErrorCode::Locked, "x"))),
            EwsError::Unsupported("x".into()),
            EwsError::OutlookRuleBlob,
        ]
    }

    #[test]
    fn response_codes_map_to_classes() {
        let busy = EwsError::from_response_code("ErrorServerBusy", "busy", Some(29_219));
        assert_eq!(busy, EwsError::RateLimited(Some(30)));
        assert!(busy.retryable());
        assert_eq!(busy.retry_after_secs(), Some(30));
        assert_eq!(
            EwsError::from_response_code("ErrorItemNotFound", "", None).code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            EwsError::from_response_code("ErrorAccessDenied", "no", None).code(),
            ErrorCode::Forbidden
        );
        assert_eq!(
            EwsError::from_response_code("ErrorIrresolvableConflict", "", None).code(),
            ErrorCode::Conflict
        );
        assert_eq!(
            EwsError::from_response_code("ErrorMessageSizeExceeded", "", None).code(),
            ErrorCode::TooLarge
        );
        let transient = EwsError::from_response_code("ErrorMailboxStoreUnavailable", "", None);
        assert!(transient.retryable());
        let other = EwsError::from_response_code("ErrorInvalidRecipients", "bad", None);
        assert_eq!(other.code(), ErrorCode::Rejected);
        assert!(!other.retryable());
        assert!(other.to_string().contains("ErrorInvalidRecipients"));
        assert_eq!(back_off_secs(0), 1);
        assert_eq!(back_off_secs(1000), 1);
        assert_eq!(back_off_secs(1001), 2);
    }

    #[test]
    fn engine_errors_carry_code_retry_and_text() {
        let rate: EngineError = EwsError::RateLimited(Some(30)).into();
        assert_eq!(rate.code, ErrorCode::RateLimited);
        assert!(rate.retryable);
        assert_eq!(rate.retry_after_secs, Some(30));
        assert_eq!(rate.localized.unwrap().id, "ews-rate-limited");
        let auth: EngineError = EwsError::AuthFailed.into();
        assert_eq!(auth.code, ErrorCode::AuthFailed);
        assert!(!auth.retryable);
        let status: EngineError = EwsError::Status {
            status: 418,
            message: "x".into(),
        }
        .into();
        let text = status.localized.unwrap();
        assert_eq!(text.id, "ews-http-status");
        assert!(text.args.contains_key("status"));
        let inner = EngineError::temporary(ErrorCode::Network, "token endpoint down");
        let passed: EngineError = EwsError::Credentials(Box::new(inner.clone())).into();
        assert_eq!(passed, inner);
        let blob: EngineError = EwsError::OutlookRuleBlob.into();
        assert_eq!(blob.code, ErrorCode::Conflict);
    }

    #[test]
    fn every_error_has_a_listed_message() {
        for error in all() {
            let id = error.localized().id;
            assert!(EwsError::MESSAGE_IDS.contains(&id.as_str()), "{id}");
            assert_ne!(error.to_string().len(), 0);
        }
    }

    #[test]
    fn catalog_has_every_message() {
        let catalog = include_str!("../../../locales/en/core-ews.ftl");
        for id in EwsError::MESSAGE_IDS {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{id} ="))),
                "{id} missing in locales/en/core-ews.ftl"
            );
        }
        let german = include_str!("../../../locales/de/core-ews.ftl");
        assert!(
            german
                .lines()
                .all(|l| l.trim().is_empty() || l.starts_with('#')),
            "locales/de/core-ews.ftl stays empty for now"
        );
    }
}
