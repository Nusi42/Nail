//! Simple MAPI (M4b, ADR 0087; design.md "Windows installer &
//! distribution" → "Simple MAPI provider").
//!
//! The pipe protocol between `NailMapi.dll` and the app lives in the crate
//! `nail-mapi-protocol` (ADR 0215: the DLL builds without `nail-api`); this
//! module re-exports all of it and adds what needs the rest of the
//! contract. How the pieces fit together: the `nail-mapi-protocol` crate
//! docs.

pub use nail_mapi_protocol::*;

use crate::error::{ApiError, ErrorCode};

/// The result for a core error while preparing the draft: `too_large`
/// → attachment too large, `not_found` → attachment not found, `config`
/// → no account to write from (login failure), `forbidden` → access
/// denied, `cancelled` → user abort, `invalid_request` → invalid
/// recipients, the rest → failure.
pub fn result_for_error(err: &ApiError) -> MapiResult {
    match err.code {
        ErrorCode::TooLarge => MapiResult::AttachmentTooLarge,
        ErrorCode::NotFound => MapiResult::AttachmentNotFound,
        ErrorCode::Config => MapiResult::LoginFailure,
        ErrorCode::Forbidden => MapiResult::AccessDenied,
        ErrorCode::Cancelled => MapiResult::UserAbort,
        ErrorCode::InvalidRequest => MapiResult::InvalidRecips,
        _ => MapiResult::Failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn api_errors_map_to_mapi_codes() {
        let code = |c: ErrorCode| result_for_error(&ApiError::new(c, "x"));
        assert_eq!(code(ErrorCode::TooLarge), MapiResult::AttachmentTooLarge);
        assert_eq!(code(ErrorCode::NotFound), MapiResult::AttachmentNotFound);
        assert_eq!(code(ErrorCode::Config), MapiResult::LoginFailure);
        assert_eq!(code(ErrorCode::Internal), MapiResult::Failure);
    }
}
