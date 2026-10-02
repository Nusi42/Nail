//! Customization operations of [`ConfigManager`] (M1b WP24): write-back of
//! UI edits, the merge dialog, backups and revert, "open file at line".
//!
//! Every write goes to disk atomically and is re-read right away, so the
//! returned [`ConfigUpdate`]s describe the new configuration (the file
//! watcher sees unchanged content afterwards and reports nothing twice).
//! The core turns the updates into events with [`ConfigUpdate::to_event`].

use std::path::PathBuf;
use std::process::Command;

use nail_api::customize::{ConfigBackup, ConfigMergePreview, MergeChoice, MergeTake};
use nail_api::{ApiError, ConfigBackupId, ConfigFile, ErrorCode, LayoutPatch};

use crate::backup;
use crate::error::ConfigError;
use crate::l10n::msg;
use crate::manager::{ConfigKind, ConfigManager, ConfigUpdate};
use crate::merge::{self, MergeOutcome};
use crate::open;
use crate::write_back;

fn io_error(file: ConfigFile, error: &std::io::Error) -> ApiError {
    ConfigError::io(file.file_name(), error).into()
}

fn mergeable(file: ConfigFile) -> Result<(), ApiError> {
    match file {
        ConfigFile::Theme | ConfigFile::Layout => Ok(()),
        other => {
            let localized = msg("config-merge-unsupported").arg("file", other.file_name());
            Err(ApiError::invalid_request(crate::l10n::english(&localized))
                .with_localized(localized))
        }
    }
}

impl ConfigManager {
    /// The path of a configuration file in the config directory.
    pub fn file_path(&self, file: ConfigFile) -> PathBuf {
        self.dir().join(file.file_name())
    }

    fn read_file(&self, file: ConfigFile) -> Result<Option<String>, ApiError> {
        backup::read_optional(&self.file_path(file)).map_err(|e| io_error(file, &e))
    }

    fn lock_writes(&self) -> std::sync::MutexGuard<'_, ()> {
        self.inner
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Re-reads a file after a write (theme and layout; the core's own
    /// watchers handle `accounts.kdl` and `rules.kdl`).
    fn updates_after_write(&self, file: ConfigFile) -> Vec<ConfigUpdate> {
        ConfigKind::from_file(file).map_or_else(Vec::new, |kind| self.reload_updates(kind))
    }

    /// Writes UI edits into `layout.kdl` (creating a sparse file when there
    /// is none) and returns the resulting updates (empty when nothing
    /// changed). `config` when the file on disk is invalid,
    /// `invalid_request` when a patch does not fit.
    pub fn update_layout(&self, patches: &[LayoutPatch]) -> Result<Vec<ConfigUpdate>, ApiError> {
        let _guard = self.lock_writes();
        let file = ConfigFile::Layout;
        let current = self.read_file(file)?;
        let text = write_back::apply_layout_patches(current.as_deref(), patches, file.file_name())?;
        if current.as_deref() == Some(text.as_str()) {
            return Ok(Vec::new());
        }
        backup::write_atomic(&self.file_path(file), &text).map_err(|e| io_error(file, &e))?;
        Ok(self.updates_after_write(file))
    }

    /// Compares an incoming theme or layout with the user's file.
    pub fn preview_merge(
        &self,
        file: ConfigFile,
        incoming: &str,
    ) -> Result<ConfigMergePreview, ApiError> {
        mergeable(file)?;
        let mine = self.read_file(file)?;
        let installed = self.installed_plugins();
        Ok(merge::preview_merge_with(
            file,
            mine.as_deref(),
            incoming,
            &installed,
        )?)
    }

    /// Tells the merge preview which plug-in commands and panels are
    /// installed (the core calls this whenever plug-ins change).
    pub fn set_installed_plugins(&self, installed: crate::plugins::InstalledPlugins) {
        *self
            .inner
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = installed;
    }

    /// What [`Self::set_installed_plugins`] last set.
    pub fn installed_plugins(&self) -> crate::plugins::InstalledPlugins {
        self.inner
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The user's `layout.kdl` with a plug-in's suggested placements added
    /// ([`crate::placement_layout`]) and its merge preview; `None` when
    /// nothing would change.
    pub fn preview_placements(
        &self,
        plugin: &nail_api::PluginId,
        manifest: &nail_api::plugins::PluginManifest,
    ) -> Result<Option<(String, ConfigMergePreview)>, ApiError> {
        let file = ConfigFile::Layout;
        let mine = self.read_file(file)?;
        let Some(incoming) = crate::plugins::placement_layout(mine.as_deref(), plugin, manifest)?
        else {
            return Ok(None);
        };
        let preview = self.preview_merge(file, &incoming)?;
        Ok(Some((incoming, preview)))
    }

    /// Resolves a merge for the live preview without writing it.
    pub fn resolve_merge(
        &self,
        file: ConfigFile,
        incoming: &str,
        choices: &[MergeChoice],
        default: MergeTake,
    ) -> Result<MergeOutcome, ApiError> {
        mergeable(file)?;
        let mine = self.read_file(file)?;
        Ok(merge::resolve_merge(
            file,
            mine.as_deref(),
            incoming,
            choices,
            default,
        )?)
    }

    /// Writes a merge result after backing the current file up. Returns the
    /// backup (for revert) and the resulting updates.
    pub fn apply_merge(
        &self,
        file: ConfigFile,
        incoming: &str,
        choices: &[MergeChoice],
        default: MergeTake,
    ) -> Result<(ConfigBackup, Vec<ConfigUpdate>), ApiError> {
        mergeable(file)?;
        let _guard = self.lock_writes();
        let mine = self.read_file(file)?;
        let outcome = merge::resolve_merge(file, mine.as_deref(), incoming, choices, default)?;
        let saved = backup::take(self.dir(), file).map_err(|e| io_error(file, &e))?;
        backup::write_atomic(&self.file_path(file), &outcome.text)
            .map_err(|e| io_error(file, &e))?;
        Ok((saved, self.updates_after_write(file)))
    }

    /// The current text of a configuration file (`None` = no file).
    pub fn file_text(&self, file: ConfigFile) -> Result<Option<String>, ApiError> {
        self.read_file(file)
    }

    /// Replaces a whole file (restoring a backup archive, M4c): the
    /// current file is backed up first (also when there is none, so the
    /// restore can be reverted), then `text` is written. Returns the backup
    /// and the resulting updates (theme / layout).
    pub fn replace_file(
        &self,
        file: ConfigFile,
        text: &str,
    ) -> Result<(ConfigBackup, Vec<ConfigUpdate>), ApiError> {
        let _guard = self.lock_writes();
        let saved = backup::take(self.dir(), file).map_err(|e| io_error(file, &e))?;
        backup::write_atomic(&self.file_path(file), text).map_err(|e| io_error(file, &e))?;
        Ok((saved, self.updates_after_write(file)))
    }

    /// Backups of a file, newest first.
    pub fn list_backups(&self, file: ConfigFile) -> Result<Vec<ConfigBackup>, ApiError> {
        backup::list(self.dir(), file).map_err(|e| io_error(file, &e))
    }

    /// Restores a backup (`None` = the newest); the current file is backed
    /// up first. `not_found` when there is no such backup.
    pub fn revert(
        &self,
        file: ConfigFile,
        backup_id: Option<&ConfigBackupId>,
    ) -> Result<Vec<ConfigUpdate>, ApiError> {
        let _guard = self.lock_writes();
        let restored =
            backup::restore(self.dir(), file, backup_id).map_err(|e| io_error(file, &e))?;
        if restored.is_none() {
            let localized = match backup_id {
                Some(id) => msg("config-backup-missing")
                    .arg("backup", id.to_string())
                    .arg("file", file.file_name()),
                None => msg("config-backup-none").arg("file", file.file_name()),
            };
            return Err(
                ApiError::new(ErrorCode::NotFound, crate::l10n::english(&localized))
                    .with_localized(localized),
            );
        }
        Ok(self.updates_after_write(file))
    }

    /// The command that opens a configuration file at a position in the
    /// editor from `NAIL_EDITOR` / `VISUAL` / `EDITOR` (else the system's
    /// text editor). The caller spawns it; a missing theme or layout file
    /// is created as a sparse file first so there is something to open.
    pub fn open_file_command(
        &self,
        file: ConfigFile,
        line: u32,
        column: u32,
    ) -> Result<Command, ApiError> {
        let path = self.file_path(file);
        if !path.exists() {
            let skeleton = match file {
                ConfigFile::Theme => Some(merge::NEW_THEME),
                ConfigFile::Layout => Some(write_back::NEW_LAYOUT),
                ConfigFile::Accounts | ConfigFile::Rules => None,
            };
            if let Some(text) = skeleton {
                let _guard = self.lock_writes();
                backup::write_atomic(&path, text).map_err(|e| io_error(file, &e))?;
            }
        }
        Ok(open::editor_command(
            &path,
            line.max(1),
            column.max(1),
            open::configured_editor().as_deref(),
        ))
    }
}
