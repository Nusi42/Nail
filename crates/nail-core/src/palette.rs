//! M4c command palette (ADR 0094): the recently used actions, kept in
//! `app_settings` ([`RECENT_ACTIONS_KEY`]) so they survive restarts and are
//! shared by the desktop and the local web UI.

use nail_api::palette::{RECENT_ACTIONS_KEY, RecentActions};
use nail_api::{ApiError, LocalizedMessage, Request, Response};

use crate::error::Result;
use crate::service::Service;

async fn load(service: &Service) -> Result<RecentActions> {
    Ok(service
        .store()
        .call(|s| s.setting(RECENT_ACTIONS_KEY))
        .await?
        .unwrap_or_default())
}

async fn save(service: &Service, recent: RecentActions) -> Result<Response> {
    let saved = recent.clone();
    service
        .store()
        .call(move |s| s.set_setting(RECENT_ACTIONS_KEY, &saved))
        .await?;
    Ok(Response::RecentActions { recent })
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetRecentActions => Ok(Response::RecentActions {
            recent: load(service).await?,
        }),
        Request::RecordActionUsed { action_id } => {
            let mut recent = load(service).await?;
            let now = crate::rules::rfc3339(crate::engine::unix_now());
            if !recent.record(&action_id, &now) {
                return Err(ApiError::invalid_request("not an action id")
                    .with_localized(LocalizedMessage::new("palette-invalid-action"))
                    .into());
            }
            save(service, recent).await
        }
        Request::ClearRecentActions => save(service, RecentActions::default()).await,
        other => Err(ApiError::internal(format!(
            "not a palette request: {}",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}
