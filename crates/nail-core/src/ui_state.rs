//! UI state kept across restarts (M5b, ADR 0109): `GetUiState` /
//! `SetUiState` over `<config-dir>/ui-state.json`
//! ([`nail_config::ConfigManager::ui_state`]); every change is pushed as
//! `UiStateChanged` so other windows and sessions follow.

use nail_api::{ApiError, Event, Request, Response};
use nail_config::{ConfigManager, UiStateError};

use crate::error::Result;
use crate::service::Service;

/// `GetUiState`, `SetUiState`.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    crate::editing::on_config(service, request, answer).await
}

fn answer(service: &Service, config: &ConfigManager, request: Request) -> Result<Response> {
    let state = match request {
        Request::GetUiState => config.ui_state(),
        Request::SetUiState { key, value } => match config.set_ui_state(&key, value.clone()) {
            Ok((state, changed)) => {
                if changed {
                    service.broadcast(Event::UiStateChanged { key, value });
                }
                state
            }
            Err(UiStateError::Invalid(issue)) => {
                return Err(ApiError::invalid_localized(
                    format!("UI state `{key}`: {}", issue.as_str()),
                    issue.message_id(),
                )
                .into());
            }
            Err(err @ UiStateError::Io(_)) => {
                return Err(ApiError::internal(err.to_string()).into());
            }
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a UI-state request",
                other.wire_type()
            ))
            .into());
        }
    };
    Ok(Response::UiState { state })
}

#[cfg(test)]
mod tests {
    use nail_api::{ErrorCode, Event, Request, Response};
    use serde_json::json;

    fn state(response: Response) -> nail_api::ui_state::UiState {
        match response {
            Response::UiState { state } => state,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_set_and_events() {
        let t = crate::testing::TempService::quiet();
        let service = &t.service;
        let mut events = service.inner.events.subscribe();
        assert!(state(service.handle(Request::GetUiState).await).is_empty());
        let set = Request::SetUiState {
            key: "folder-pane.collapsed".into(),
            value: Some(json!(["favorites"])),
        };
        let now = state(service.handle(set.clone()).await);
        assert_eq!(now["folder-pane.collapsed"], json!(["favorites"]));
        let event = events.recv().await.unwrap();
        assert_eq!(
            event,
            Event::UiStateChanged {
                key: "folder-pane.collapsed".into(),
                value: Some(json!(["favorites"])),
            }
        );
        // Unchanged: no event (the next one is the removal).
        state(service.handle(set).await);
        state(
            service
                .handle(Request::SetUiState {
                    key: "folder-pane.collapsed".into(),
                    value: None,
                })
                .await,
        );
        let event = events.recv().await.unwrap();
        assert!(matches!(event, Event::UiStateChanged { value: None, .. }));
        match service
            .handle(Request::SetUiState {
                key: "Not Valid".into(),
                value: Some(json!(1)),
            })
            .await
        {
            Response::Error {
                code, localized, ..
            } => {
                assert_eq!(code, ErrorCode::InvalidRequest);
                assert_eq!(localized.unwrap().id, "ui-state-issue-invalid-key");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
