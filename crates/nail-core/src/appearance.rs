//! M5a appearance (ADR 0102): the theme list (built-ins + `<config-dir>/themes/`),
//! the selection and overrides in the user's `theme.kdl`, Follow Windows
//! through `ServiceOptions::system_appearance`, and `ThemeChanged`.
//!
//! Slot of WP152. Until it lands, the requests answer `unsupported`
//! (`m5a-not-ready`); the rules every implementation follows are in
//! `nail_api::appearance` (`active_theme`, `ThemeOverrides::issues`, …).

use nail_api::{ApiError, LocalizedMessage, Request, Response};

use crate::error::{CoreError, Result};
use crate::service::Service;

/// The answer of the appearance requests before WP152 serves them.
pub(crate) fn not_ready(request: &Request) -> CoreError {
    ApiError::unsupported(format!(
        "{} is not available yet",
        crate::service::request_name(request)
    ))
    .with_localized(LocalizedMessage::new("m5a-not-ready"))
    .into()
}

/// `ListThemes`, `SetTheme`, `SetThemePreferences`.
pub(crate) async fn handle(_service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::ListThemes | Request::SetTheme { .. } | Request::SetThemePreferences { .. } => {
            Err(not_ready(&request))
        }
        other => Err(ApiError::internal(format!(
            "{} is not an appearance request",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}
