//! M4c data portability (ADR 0094): imports, exports, backups, restores,
//! the diagnostics bundle and the desktop's file dialog.
//!
//! Contract stage (WP120): the file dialog goes to the desktop shell
//! ([`nail_api::DesktopHost::pick_path`]); everything else answers
//! `unsupported` (`m4c-not-ready`) until WP125 wires `nail-portability`
//! (WP121) in: jobs run in the core's runtime, report through
//! [`nail_api::Event::PortabilityJobProgress`] /
//! [`nail_api::Event::PortabilityJobFinished`], and write mail into
//! "Local Folders" ([`nail_api::portability::LOCAL_FOLDERS_ACCOUNT_ID`]) or
//! through an account's operation queue.

use nail_api::{ApiError, LocalizedMessage, Request, Response};

use crate::error::{CoreError, Result};
use crate::service::Service;

/// The answer of M4c requests the core does not serve yet.
pub(crate) fn not_ready(request: &Request) -> CoreError {
    ApiError::unsupported(format!(
        "{} is not available yet",
        crate::service::request_name(request)
    ))
    .with_localized(LocalizedMessage::new("m4c-not-ready"))
    .into()
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::PickPath { pick } => {
            let host = service.inner.desktop.clone().ok_or_else(|| {
                CoreError::from(
                    ApiError::unsupported("only the desktop app has file dialogs")
                        .with_localized(LocalizedMessage::new("desktop-unavailable")),
                )
            })?;
            let path = host.pick_path(&pick).await?;
            Ok(Response::PathPicked { path })
        }
        other => Err(not_ready(&other)),
    }
}
