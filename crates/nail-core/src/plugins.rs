//! Requests of wave M2d for plug-ins (ADR 0050).
//!
//! **Contract stage (WP60).** The store-backed administration is served
//! now: listing, enabling / disabling (the flag only), uninstalling (the
//! store rows), grants, logs and settings. Everything that needs the
//! plug-in host (`nail-plugins`, WP61) — reading packages, installing,
//! dev folders, placements, running commands, dialogs, panels — answers
//! `unsupported` until the integration (WP64) wires the host in. Until
//! then an enabled plug-in is reported as `stopped` (or `crashed` when an
//! error was recorded).

use std::collections::BTreeMap;

use nail_api::plugins::admin::PluginRunState;
use nail_api::plugins::{PluginInfo, is_supported_api_version};
use nail_api::{ApiError, Event, LocalizedMessage, PluginId, Request, Response};
use nail_store::{PluginRecord, Store};

use crate::error::Result;
use crate::service::Service;

/// Most log lines one `ListPluginLogs` returns.
const MAX_LOG_PAGE: u32 = 1000;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListPlugins => {
            let plugins = service
                .store()
                .call(|s| {
                    s.plugins()?
                        .into_iter()
                        .map(|record| info(s, record))
                        .collect::<nail_store::Result<Vec<_>>>()
                })
                .await?;
            Response::Plugins { plugins }
        }
        Request::GetPlugin { plugin_id } => plugin_response(service, plugin_id).await?,
        Request::EnablePlugin { plugin_id } => {
            let id = plugin_id.clone();
            service
                .store()
                .call(move |s| s.set_plugin_enabled(&id, true))
                .await?;
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, plugin_id).await?
        }
        Request::DisablePlugin { plugin_id } => {
            let id = plugin_id.clone();
            service
                .store()
                .call(move |s| s.set_plugin_enabled(&id, false))
                .await?;
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, plugin_id).await?
        }
        Request::UninstallPlugin { plugin_id } => {
            service
                .store()
                .call(move |s| s.remove_plugin(&plugin_id))
                .await?;
            service.broadcast(Event::PluginsChanged);
            Response::Done
        }
        Request::UpdatePluginGrants { plugin_id, grants } => {
            let id = plugin_id.clone();
            service
                .store()
                .call(move |s| s.set_plugin_grants(&id, &grants))
                .await?;
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, plugin_id).await?
        }
        Request::ListPluginLogs {
            plugin_id,
            after_seq,
            limit,
        } => {
            let limit = limit.clamp(1, MAX_LOG_PAGE);
            let entries = service
                .store()
                .call(move |s| {
                    if let Some(id) = &plugin_id {
                        s.plugin(id)?;
                    }
                    s.plugin_logs(plugin_id.as_ref(), after_seq, limit)
                })
                .await?;
            Response::PluginLogs { entries }
        }
        Request::ClearPluginLogs { plugin_id } => {
            service
                .store()
                .call(move |s| {
                    s.plugin(&plugin_id)?;
                    s.clear_plugin_logs(&plugin_id)
                })
                .await?;
            Response::Done
        }
        Request::GetPluginSettings { plugin_id } => {
            let id = plugin_id.clone();
            let (record, values) = service
                .store()
                .call(move |s| Ok((s.plugin(&id)?, s.plugin_settings(&id)?)))
                .await?;
            settings_response(record, values)
        }
        Request::UpdatePluginSettings { plugin_id, values } => {
            let id = plugin_id.clone();
            let record = service.store().call(move |s| s.plugin(&id)).await?;
            let invalid = record.manifest.invalid_settings(&values);
            if !invalid.is_empty() {
                let keys = invalid.join(", ");
                return Err(
                    ApiError::invalid_request(format!("invalid plug-in settings: {keys}"))
                        .with_localized(
                            LocalizedMessage::new("plugin-settings-invalid").arg("keys", keys),
                        )
                        .into(),
                );
            }
            let values = service
                .store()
                .call(move |s| s.update_plugin_settings(&plugin_id, &values))
                .await?;
            settings_response(record, values)
        }
        other @ (Request::PreviewPluginInstall { .. }
        | Request::InstallPlugin { .. }
        | Request::LoadDevPlugin { .. }
        | Request::PreviewPluginPlacements { .. }
        | Request::RunPluginCommand { .. }
        | Request::RespondPluginDialog { .. }
        | Request::PluginUiAction { .. }
        | Request::GetPluginPanel { .. }) => {
            return Err(ApiError::unsupported(format!(
                "{} needs the plug-in host (M2d WP61 / WP64)",
                crate::service::request_name(&other)
            ))
            .into());
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a plug-in request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

async fn plugin_response(service: &Service, id: PluginId) -> Result<Response> {
    let plugin = service
        .store()
        .call(move |s| {
            let record = s.plugin(&id)?;
            info(s, record)
        })
        .await?;
    Ok(Response::Plugin {
        plugin: Box::new(plugin),
    })
}

fn settings_response(
    record: PluginRecord,
    values: BTreeMap<String, serde_json::Value>,
) -> Response {
    Response::PluginSettings {
        plugin_id: record.id,
        schema: record.manifest.contributes.settings,
        values,
    }
}

/// The API view of a stored plug-in. Without a host, an enabled plug-in
/// is `stopped` (`crashed` after a recorded error).
fn info(store: &Store, record: PluginRecord) -> nail_store::Result<PluginInfo> {
    let grants = store.plugin_grants(&record.id)?;
    let storage = store.plugin_storage_usage(&record.id)?;
    let state = if !is_supported_api_version(&record.manifest.api_version) {
        PluginRunState::Incompatible
    } else if !record.enabled {
        PluginRunState::Disabled
    } else if record.last_error.is_some() {
        PluginRunState::Crashed
    } else {
        PluginRunState::Stopped
    };
    let (error, localized) = match record.last_error {
        Some(err) => (Some(err.message), err.localized),
        None => (None, None),
    };
    Ok(PluginInfo {
        id: record.id,
        manifest: record.manifest,
        source: record.source,
        enabled: record.enabled,
        state,
        grants,
        storage,
        package_sha256: record.package_sha256,
        installed_at: record.installed_at,
        updated_at: record.updated_at,
        error,
        localized,
    })
}
