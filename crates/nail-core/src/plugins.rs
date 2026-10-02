//! Requests of wave M2d for plug-ins (ADR 0050; wired to the host in
//! WP64, ADR 0055).
//!
//! Administration is store-backed (installed plug-ins, grants, settings,
//! logs) and drives the host ([`crate::plugin_host`]): installing checks
//! the package again against the hash the user approved in the preview and
//! keeps it packed in `<data-dir>/plugins`; enabling starts it, disabling
//! stops it, grants and settings reach a running plug-in at once; dev
//! folders are loaded with hot reload. The runtime requests go to the
//! host: commands and UI actions answer `Done` as soon as they are handed
//! over (the plug-in runs on; a runaway one never blocks the UI), dialog
//! answers and panels are served directly.
//!
//! [`PluginInfo`] reports the host's run state (falling back to the
//! store), the stored error of a crashed plug-in, and manifest texts
//! resolved through the plug-in's own catalogs in the core's language.

use std::collections::BTreeMap;

use nail_api::customize::ConfigMergePreview;
use nail_api::plugins::admin::{PluginRunState, PluginStopReason};
use nail_api::plugins::{
    PluginInfo, PluginInstallPreview, PluginPackageSource, PluginUiTarget,
    is_supported_api_version, parse_plugin_command_id, parse_plugin_panel_id,
};
use nail_api::{
    ApiError, ConfigFile, ErrorCode, Event, LocalizedMessage, PluginId, Request, Response,
};
use nail_plugins::{PluginCatalogs, PluginPackage};
use nail_store::{PluginInstall, PluginRecord};

use crate::error::Result;
use crate::plugin_host::read_package_file;
use crate::service::Service;

/// Most log lines one `ListPluginLogs` returns.
const MAX_LOG_PAGE: u32 = 1000;

/// Longest download of a package from a URL.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListPlugins => {
            let records = service.store().call(|s| s.plugins()).await?;
            let mut plugins = Vec::with_capacity(records.len());
            for record in records {
                plugins.push(info(service, record).await?);
            }
            Response::Plugins { plugins }
        }
        Request::GetPlugin { plugin_id } => plugin_response(service, plugin_id).await?,
        Request::PreviewPluginInstall { source } => {
            let bytes = package_bytes(service, &source).await?;
            let package = parse_package(bytes).await?;
            let manifest = match package.valid_manifest() {
                Some(manifest) => {
                    let catalogs = PluginCatalogs::load(manifest, &package.files);
                    Some(catalogs.localize_manifest(manifest, service.inner.locale.as_deref()))
                }
                None => None,
            };
            let installed_version = match package.valid_manifest() {
                Some(manifest) => {
                    let id = manifest.id.clone();
                    service
                        .store()
                        .call(move |s| match s.plugin(&id) {
                            Ok(record) => Ok(Some(record.manifest.version)),
                            Err(e) if e.is_not_found() => Ok(None),
                            Err(e) => Err(e),
                        })
                        .await?
                }
                None => None,
            };
            Response::PluginInstallPreview {
                preview: Box::new(PluginInstallPreview {
                    source,
                    manifest,
                    issues: package.issues().to_vec(),
                    package_sha256: package.sha256.clone(),
                    size: package.size,
                    installed_version,
                }),
            }
        }
        Request::InstallPlugin {
            source,
            package_sha256,
            grants,
            enable,
        } => {
            let bytes = package_bytes(service, &source).await?;
            // The package must be exactly the previewed one before
            // anything is written.
            if !nail_plugins::package::sha256_hex(&bytes).eq_ignore_ascii_case(&package_sha256) {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "the package changed since the preview",
                )
                .with_localized(LocalizedMessage::new(
                    nail_plugins::l10n::PACKAGE_HASH_MISMATCH,
                ))
                .into());
            }
            let package = parse_package(bytes.clone()).await?;
            let Some(manifest) = package.valid_manifest().cloned() else {
                let first = package.issues().first().map(|i| i.localized());
                let mut error = ApiError::invalid_request("the package's manifest has problems");
                if let Some(localized) = first {
                    error = error.with_localized(localized);
                }
                return Err(error.into());
            };
            nail_store::check_plugin_grants(&manifest, &grants)?;
            let packages = service.inner.plugins.packages()?.clone();
            let sha = package_sha256.to_ascii_lowercase();
            let installed = {
                let sha = sha.clone();
                tokio::task::spawn_blocking(move || packages.install(&bytes, &sha)).await?
            };
            let package = installed.map_err(|e| e.to_api_error())?;
            service.inner.plugins.set_catalogs(
                &manifest.id,
                Some(package.sha256.clone()),
                PluginCatalogs::load(&manifest, &package.files),
            );
            let record = {
                let manifest = manifest.clone();
                service
                    .store()
                    .call(move |s| {
                        s.install_plugin(&PluginInstall {
                            manifest: &manifest,
                            source: &source,
                            package_sha256: Some(&sha),
                            grants: &grants,
                            enabled: enable,
                        })
                    })
                    .await?
            };
            if enable {
                // A plug-in that fails to start is installed and shown as
                // crashed; the install itself succeeded.
                if let Err(err) = service.start_plugin(&record).await {
                    tracing::warn!(plugin = %record.id, %err, "installed plug-in did not start");
                }
            } else if let Some(host) = service.inner.plugins.existing_host() {
                host.stop(&record.id, PluginStopReason::Disabled);
            }
            service.refresh_installed_plugins().await;
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, record.id).await?
        }
        Request::LoadDevPlugin { folder, grants } => {
            let path = std::path::PathBuf::from(&folder);
            if !path.is_absolute() {
                return Err(ApiError::invalid_request(format!(
                    "the plug-in folder must be an absolute path: {folder}"
                ))
                .into());
            }
            let read = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || nail_plugins::package::load_folder(&path))
                    .await?
            };
            let manifest = match read {
                Ok((Ok(manifest), _)) => manifest,
                Ok((Err(issues), _)) => {
                    return Err(nail_plugins::DevLoadError::Manifest(issues)
                        .to_api_error()
                        .into());
                }
                Err(err) => return Err(err.to_api_error().into()),
            };
            nail_store::check_plugin_grants(&manifest, &grants)?;
            let source = PluginPackageSource::Dev {
                folder: folder.clone(),
            };
            let record = {
                let manifest = manifest.clone();
                service
                    .store()
                    .call(move |s| {
                        s.install_plugin(&PluginInstall {
                            manifest: &manifest,
                            source: &source,
                            package_sha256: None,
                            grants: &grants,
                            enabled: true,
                        })
                    })
                    .await?
            };
            if let Err(err) = service.start_plugin(&record).await {
                tracing::warn!(plugin = %record.id, %err, "dev plug-in did not start");
            }
            service.refresh_installed_plugins().await;
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, record.id).await?
        }
        Request::EnablePlugin { plugin_id } => {
            let id = plugin_id.clone();
            let record = service
                .store()
                .call(move |s| {
                    s.set_plugin_enabled(&id, true)?;
                    s.set_plugin_error(&id, None)?;
                    s.plugin(&id)
                })
                .await?;
            if is_supported_api_version(&record.manifest.api_version)
                && let Err(err) = service.start_plugin(&record).await
            {
                tracing::warn!(plugin = %record.id, %err, "plug-in did not start");
            }
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, plugin_id).await?
        }
        Request::DisablePlugin { plugin_id } => {
            let id = plugin_id.clone();
            service
                .store()
                .call(move |s| s.set_plugin_enabled(&id, false))
                .await?;
            if let Some(host) = service.inner.plugins.existing_host() {
                host.stop(&plugin_id, PluginStopReason::Disabled);
            }
            service.broadcast(Event::PluginsChanged);
            plugin_response(service, plugin_id).await?
        }
        Request::UninstallPlugin { plugin_id } => {
            let id = plugin_id.clone();
            service.store().call(move |s| s.plugin(&id)).await?;
            if let Some(host) = service.inner.plugins.existing_host() {
                host.stop(&plugin_id, PluginStopReason::Uninstalled);
            }
            let id = plugin_id.clone();
            service.store().call(move |s| s.remove_plugin(&id)).await?;
            if let Ok(packages) = service.inner.plugins.packages() {
                let packages = packages.clone();
                let id = plugin_id.clone();
                if let Err(err) = tokio::task::spawn_blocking(move || packages.remove(&id)).await? {
                    tracing::warn!(%err, "cannot remove the plug-in package");
                }
            }
            service.inner.plugins.forget(&plugin_id);
            // M4a: its pending confirmations and policies go with it.
            service
                .forget_actor(&nail_api::agents::ActionActor::Plugin {
                    plugin_id: plugin_id.clone(),
                })
                .await;
            service.refresh_installed_plugins().await;
            service.broadcast(Event::PluginsChanged);
            Response::Done
        }
        Request::UpdatePluginGrants { plugin_id, grants } => {
            let id = plugin_id.clone();
            let saved = grants.clone();
            service
                .store()
                .call(move |s| s.set_plugin_grants(&id, &saved))
                .await?;
            if let Some(host) = service.inner.plugins.existing_host() {
                host.update_grants(&plugin_id, &grants);
            }
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
            settings_response(service, record, values).await
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
            let id = plugin_id.clone();
            let values = service
                .store()
                .call(move |s| s.update_plugin_settings(&id, &values))
                .await?;
            if let Some(host) = service.inner.plugins.existing_host() {
                host.update_settings(&plugin_id, values.clone());
            }
            settings_response(service, record, values).await
        }
        Request::PreviewPluginPlacements { plugin_id } => {
            let id = plugin_id.clone();
            let record = service.store().call(move |s| s.plugin(&id)).await?;
            let config = service.config().clone();
            let id = plugin_id.clone();
            let placed = tokio::task::spawn_blocking(move || {
                config.preview_placements(&id, &record.manifest)
            })
            .await??;
            match placed {
                Some((incoming, preview)) => Response::PluginPlacements {
                    plugin_id,
                    incoming,
                    preview: Box::new(preview),
                },
                None => Response::PluginPlacements {
                    plugin_id,
                    incoming: String::new(),
                    preview: Box::new(ConfigMergePreview {
                        file: ConfigFile::Layout,
                        sections: Vec::new(),
                        errors: Vec::new(),
                    }),
                },
            }
        }
        Request::RunPluginCommand {
            command,
            surface,
            context,
            client_id,
        } => {
            let (plugin, name) = parse_plugin_command_id(&command).ok_or_else(|| {
                ApiError::not_found(format!("`{command}` is not a plug-in command"))
            })?;
            let manifest = running_manifest(service, &plugin)?;
            if manifest.command(&name).is_none() {
                return Err(ApiError::not_found(format!(
                    "`{command}` is not declared by the plug-in"
                ))
                .into());
            }
            // Handed over; the plug-in runs on its own thread and reports
            // through events (a runaway command never blocks the UI).
            let host = service.plugin_host().clone();
            tokio::spawn(async move {
                if let Err(err) = host
                    .run_command(&command, surface, &context, client_id)
                    .await
                {
                    tracing::debug!(%command, %err, "plug-in command failed");
                }
            });
            Response::Done
        }
        Request::RespondPluginDialog {
            dialog_id,
            button,
            values,
        } => {
            service
                .plugin_host()
                .respond_dialog(&dialog_id, button, values)?;
            Response::Done
        }
        Request::PluginUiAction {
            target,
            component_id,
            values,
            client_id,
        } => {
            if let PluginUiTarget::Panel { panel } = &target {
                let (plugin, _) = parse_plugin_panel_id(panel)
                    .ok_or_else(|| ApiError::not_found(format!("`{panel}` is not a panel")))?;
                running_manifest(service, &plugin)?;
            }
            let host = service.plugin_host().clone();
            tokio::spawn(async move {
                if let Err(err) = host
                    .ui_action(&target, &component_id, values, client_id)
                    .await
                {
                    tracing::debug!(%component_id, %err, "plug-in UI action failed");
                }
            });
            Response::Done
        }
        Request::GetPluginPanel {
            panel,
            context,
            client_id,
        } => {
            let tree = service
                .plugin_host()
                .panel(&panel, &context, client_id)
                .await?;
            Response::PluginPanel { panel, tree }
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

/// The manifest of a running plug-in; `not_found` when it is not
/// installed, `unavailable` when it is not running.
fn running_manifest(
    service: &Service,
    plugin: &PluginId,
) -> Result<nail_api::plugins::PluginManifest> {
    let host = service.plugin_host();
    match host.state(plugin) {
        Some(PluginRunState::Running) => host
            .manifest(plugin)
            .ok_or_else(|| ApiError::not_found(format!("plug-in `{plugin}` is not loaded")).into()),
        Some(_) => Err(ApiError::new(
            ErrorCode::Unavailable,
            format!("plug-in `{plugin}` is not running"),
        )
        .into()),
        None => Err(ApiError::not_found(format!("plug-in `{plugin}` is not loaded")).into()),
    }
}

/// The bytes of a package to preview or install.
async fn package_bytes(service: &Service, source: &PluginPackageSource) -> Result<Vec<u8>> {
    match source {
        PluginPackageSource::Dev { .. } => Err(ApiError::invalid_request(
            "development folders are loaded with LoadDevPlugin",
        )
        .into()),
        PluginPackageSource::File { path } => {
            let path = std::path::PathBuf::from(path);
            Ok(tokio::task::spawn_blocking(move || read_package_file(&path)).await??)
        }
        PluginPackageSource::Url { url } => {
            let parsed = url::Url::parse(url)
                .map_err(|e| ApiError::invalid_request(format!("invalid URL {url}: {e}")))?;
            let host = parsed
                .host_str()
                .ok_or_else(|| ApiError::invalid_request(format!("{url} has no host")))?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_ascii_lowercase();
            let response = service
                .inner
                .plugins
                .http
                .fetch(nail_plugins::HttpRequest {
                    method: "GET".to_owned(),
                    url: url.clone(),
                    headers: Vec::new(),
                    body: None,
                    allowed_hosts: vec![host],
                    max_response_bytes: nail_api::plugins::admin::MAX_PACKAGE_BYTES,
                    timeout: DOWNLOAD_TIMEOUT,
                })
                .await?;
            if !(200..300).contains(&response.status) {
                return Err(ApiError::new(
                    ErrorCode::Network,
                    format!("{url} answered {}", response.status),
                )
                .into());
            }
            Ok(response.body)
        }
    }
}

async fn parse_package(bytes: Vec<u8>) -> Result<PluginPackage> {
    Ok(
        tokio::task::spawn_blocking(move || PluginPackage::read(&bytes))
            .await?
            .map_err(|e| e.to_api_error())?,
    )
}

async fn plugin_response(service: &Service, id: PluginId) -> Result<Response> {
    let record = service.store().call(move |s| s.plugin(&id)).await?;
    Ok(Response::Plugin {
        plugin: Box::new(info(service, record).await?),
    })
}

async fn settings_response(
    service: &Service,
    record: PluginRecord,
    values: BTreeMap<String, serde_json::Value>,
) -> Response {
    let catalogs = service.plugin_catalogs(&record).await;
    let manifest = catalogs.localize_manifest(&record.manifest, service.inner.locale.as_deref());
    Response::PluginSettings {
        plugin_id: record.id,
        schema: manifest.contributes.settings,
        values,
    }
}

/// The API view of a stored plug-in: the host's run state when it knows
/// the plug-in, else the store's (enabled but not started = `stopped`,
/// `crashed` after a recorded error); texts in the core's language.
async fn info(service: &Service, record: PluginRecord) -> Result<PluginInfo> {
    let id = record.id.clone();
    let (grants, storage) = service
        .store()
        .call(move |s| Ok((s.plugin_grants(&id)?, s.plugin_storage_usage(&id)?)))
        .await?;
    let catalogs = service.plugin_catalogs(&record).await;
    let host = service.inner.plugins.existing_host();
    let stored_state = if !is_supported_api_version(&record.manifest.api_version) {
        PluginRunState::Incompatible
    } else if !record.enabled {
        PluginRunState::Disabled
    } else if record.last_error.is_some() {
        PluginRunState::Crashed
    } else {
        PluginRunState::Stopped
    };
    let state = match host.and_then(|h| h.state(&record.id)) {
        // The store's flag wins over a host that has not caught up.
        Some(_) if !record.enabled => PluginRunState::Disabled,
        Some(state) => state,
        None => stored_state,
    };
    let error = host
        .and_then(|h| h.error(&record.id))
        .filter(|_| {
            matches!(
                state,
                PluginRunState::Crashed | PluginRunState::Incompatible
            )
        })
        .or_else(|| {
            record
                .last_error
                .clone()
                .filter(|_| state == PluginRunState::Crashed)
        });
    let (error, localized) = match error {
        Some(err) => {
            let mut localized = err.localized;
            if let Some(message) = localized.as_deref_mut() {
                service.inner.plugins.localize_plugin_arg(
                    &record.id,
                    service.inner.locale.as_deref(),
                    message,
                );
            }
            (Some(err.message), localized)
        }
        None => (None, None),
    };
    let manifest = catalogs.localize_manifest(&record.manifest, service.inner.locale.as_deref());
    Ok(PluginInfo {
        id: record.id,
        manifest,
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
