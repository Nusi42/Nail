//! Requests of M1b WP24 (customization, ADR 0026): `UpdateLayout`,
//! `PreviewConfigMerge`, `ResolveConfigMerge`, `ApplyConfigMerge`,
//! `ListConfigBackups`, `RevertConfig`, `OpenConfigFile`.
//!
//! The work is done by [`nail_config::ConfigManager`]; every write returns
//! the resulting [`nail_config::ConfigUpdate`]s, which are broadcast at
//! once (the file watcher then sees unchanged content and stays quiet).
//! `accounts.kdl` and `rules.kdl` are re-read by the core's own watchers
//! after a revert.

use nail_api::{ApiError, ConfigFile, Request, Response};
use nail_config::ConfigUpdate;

use crate::error::Result;
use crate::service::Service;

fn broadcast(service: &Service, updates: &[ConfigUpdate]) {
    for update in updates {
        service.broadcast(update.to_event());
    }
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let config = service.config().clone();
    // The config manager does blocking file I/O.
    let service = service.clone();
    tokio::task::spawn_blocking(move || handle_blocking(&service, &config, request)).await?
}

fn handle_blocking(
    service: &Service,
    config: &nail_config::ConfigManager,
    request: Request,
) -> Result<Response> {
    Ok(match request {
        Request::UpdateLayout { patches } => {
            let updates = config.update_layout(&patches)?;
            broadcast(service, &updates);
            Response::Done
        }
        Request::PreviewConfigMerge { file, incoming } => Response::ConfigMergePreview {
            preview: Box::new(config.preview_merge(file, &incoming)?),
        },
        Request::ResolveConfigMerge {
            file,
            incoming,
            choices,
            default,
        } => {
            let outcome = config.resolve_merge(file, &incoming, &choices, default)?;
            Response::ConfigMergeResolved {
                theme: outcome.theme.map(Box::new),
                layout: outcome.layout.map(Box::new),
            }
        }
        Request::ApplyConfigMerge {
            file,
            incoming,
            choices,
            default,
        } => {
            let (backup, updates) = config.apply_merge(file, &incoming, &choices, default)?;
            broadcast(service, &updates);
            Response::ConfigMergeApplied { backup }
        }
        Request::ListConfigBackups { file } => Response::ConfigBackups {
            backups: config.list_backups(file)?,
        },
        Request::RevertConfig { file, backup_id } => {
            let updates = config.revert(file, backup_id.as_ref())?;
            broadcast(service, &updates);
            if file == ConfigFile::Accounts {
                // Without a watcher (tests, `--no-watch`) apply it here.
                service.reload_accounts_file();
            }
            if file == ConfigFile::Rules {
                service.reload_rules_file();
            }
            Response::Done
        }
        Request::OpenConfigFile { file, line, column } => {
            if !service.inner.open_editor {
                return Err(ApiError::unsupported(
                    "opening configuration files is disabled for this session",
                )
                .into());
            }
            let mut command = config.open_file_command(file, line, column)?;
            command
                .spawn()
                .map_err(|e| ApiError::internal(format!("cannot start the editor: {e}")))?;
            Response::Done
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the customization module",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}
