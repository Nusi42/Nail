//! Requests of M1b WP24 (customization, ADR 0026): `UpdateLayout`,
//! `PreviewConfigMerge`, `ResolveConfigMerge`, `ApplyConfigMerge`,
//! `ListConfigBackups`, `RevertConfig`, `OpenConfigFile`.
//!
//! The work is done by [`nail_config::ConfigManager`]; every write returns
//! the resulting [`nail_config::ConfigUpdate`]s, which are broadcast at
//! once (the file watcher then sees unchanged content and stays quiet).
//! `accounts.kdl` and `rules.kdl` are re-read by the core's own watchers
//! after a revert.

use nail_api::editing::HistoryScope;
use nail_api::{ApiError, ConfigFile, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// The undo history of a theme / layout write (M5d).
fn broadcast_file_history(service: &Service, file: ConfigFile) {
    match file {
        ConfigFile::Layout => crate::editing::broadcast_history(service, HistoryScope::Layout),
        ConfigFile::Theme => crate::editing::broadcast_history(service, HistoryScope::Appearance),
        ConfigFile::Accounts | ConfigFile::Rules => {}
    }
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    crate::editing::on_config(service, request, handle_blocking).await
}

fn handle_blocking(
    service: &Service,
    config: &nail_config::ConfigManager,
    request: Request,
) -> Result<Response> {
    Ok(match request {
        Request::UpdateLayout {
            patches,
            base_revision,
        } => {
            let updates = config.update_layout_checked(&patches, base_revision.as_deref())?;
            service.broadcast_config(&updates);
            crate::editing::broadcast_history(service, HistoryScope::Layout);
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
            service.broadcast_config(&updates);
            broadcast_file_history(service, file);
            Response::ConfigMergeApplied { backup }
        }
        Request::ListConfigBackups { file } => Response::ConfigBackups {
            backups: config.list_backups(file)?,
        },
        Request::RevertConfig { file, backup_id } => {
            let updates = config.revert(file, backup_id.as_ref())?;
            service.broadcast_config(&updates);
            broadcast_file_history(service, file);
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
                other.wire_type()
            ))
            .into());
        }
    })
}
