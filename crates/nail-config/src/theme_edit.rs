//! The theme editor's operations (M5d, ADR 0131): a theme's tokens and part
//! rules ([`ThemeDetails`]), token and part-override edits ([`ThemePatch`])
//! on the user's `theme.kdl` or a user theme, and duplicating / renaming /
//! deleting user themes.
//!
//! **Slot of WP184.** The contract fixes the [`ConfigManager`] methods the
//! core calls and checks which theme they name (unknown → `not_found` with
//! `edit-issue-theme-not-found`; a built-in theme where a user theme is
//! needed → `invalid_request` with `edit-issue-not-editable`); past those
//! checks each answers `unsupported` with `edit-issue-not-ready` until WP184
//! lands. WP184 implements them format and comment preserving (with golden
//! files), under the write lock (`ConfigManager::lock_writes`), checks
//! `base_revision` with `ConfigManager::check_base_revision`, records every
//! write in the `appearance` undo history (`ConfigManager::record_edit` with
//! the [`EditKind`] of the operation and a `FileChange` per file, e.g. the new
//! theme file plus `theme.kdl` when a deleted theme was selected) and returns
//! the updates of `ConfigManager::reload_themes` and of `theme.kdl`.

use nail_api::appearance::{Appearance, ThemeSource};
#[cfg(doc)]
use nail_api::editing::EditKind;
use nail_api::editing::{EditIssue, ThemeDetails, ThemeEditTarget, ThemePatch};
use nail_api::{ApiError, ErrorCode, LocalizedMessage};

use crate::manager::{ConfigManager, ConfigUpdate};

fn error(code: ErrorCode, issue: EditIssue, theme_id: &str) -> ApiError {
    let localized = LocalizedMessage::new(issue.message_id()).arg("theme", theme_id);
    ApiError::new(code, crate::l10n::english(&localized)).with_localized(localized)
}

/// `unsupported` with `edit-issue-not-ready` (until WP184).
fn not_ready(theme_id: &str) -> ApiError {
    error(ErrorCode::Unsupported, EditIssue::NotReady, theme_id)
}

impl ConfigManager {
    /// `not_found` unless the catalog lists `theme_id`; with `user_only`,
    /// `invalid_request` (`edit-issue-not-editable`) for a built-in theme.
    fn check_theme(&self, theme_id: &str, user_only: bool) -> Result<(), ApiError> {
        match self.theme_catalog().get(theme_id) {
            None => Err(error(
                ErrorCode::NotFound,
                EditIssue::ThemeNotFound,
                theme_id,
            )),
            Some(info) if user_only && info.source != ThemeSource::User => Err(error(
                ErrorCode::InvalidRequest,
                EditIssue::NotEditable,
                theme_id,
            )),
            Some(_) => Ok(()),
        }
    }

    /// `GetThemeDetails`: every token (base tokens included, schema order)
    /// and every part rule that applies to `theme_id`, with their origins,
    /// whether it can be edited and its file's revision. `not_found` with
    /// `edit-issue-theme-not-found`.
    pub fn theme_details(&self, theme_id: &str) -> Result<ThemeDetails, ApiError> {
        self.check_theme(theme_id, false)?;
        Err(not_ready(theme_id))
    }

    /// `EditTheme`: applies `patches` to the target file as one write (one
    /// undo entry, [`EditKind::EditTheme`]). Returns the new appearance and
    /// the updates to broadcast. `conflict` for a stale `base_revision`,
    /// `invalid_request` with `edit-issue-not-editable` /
    /// `edit-issue-invalid-token` / `edit-issue-invalid-part`.
    pub fn edit_theme(
        &self,
        target: &ThemeEditTarget,
        _patches: &[ThemePatch],
        _base_revision: Option<&str>,
    ) -> Result<(Appearance, Vec<ConfigUpdate>), ApiError> {
        match target {
            ThemeEditTarget::User => Err(not_ready("theme.kdl")),
            ThemeEditTarget::Theme { theme_id } => {
                self.check_theme(theme_id, true)?;
                Err(not_ready(theme_id))
            }
        }
    }

    /// `DuplicateTheme`: copies `theme_id` (built-in or user) into
    /// `themes/<new id>.kdl` named `name` (id from the name, kebab-case,
    /// unique). Returns the new id, the appearance and the updates.
    pub fn duplicate_theme(
        &self,
        theme_id: &str,
        _name: &str,
    ) -> Result<(String, Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, false)?;
        Err(not_ready(theme_id))
    }

    /// `RenameTheme`: a user theme's display name, and with `new_id` its
    /// file (the selection in `theme.kdl` follows, same undo entry).
    /// Returns the (new) id, the appearance and the updates.
    pub fn rename_theme(
        &self,
        theme_id: &str,
        _name: &str,
        _new_id: Option<&str>,
    ) -> Result<(String, Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, true)?;
        Err(not_ready(theme_id))
    }

    /// `DeleteTheme`: removes a user theme's file; a selection naming it
    /// falls back to the default theme of its kind (same undo entry).
    pub fn delete_theme(
        &self,
        theme_id: &str,
    ) -> Result<(Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, true)?;
        Err(not_ready(theme_id))
    }
}

#[cfg(test)]
mod tests {
    use nail_api::ErrorCode;
    use nail_api::editing::ThemeEditTarget;

    use crate::ConfigManager;

    fn id_of(error: &nail_api::ApiError) -> Option<&str> {
        error.localized.as_deref().map(|l| l.id.as_str())
    }

    #[test]
    fn theme_operations_check_the_theme_then_are_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        std::fs::write(
            dir.path().join("themes/paper.kdl"),
            "theme \"Paper\" extends=\"frost\" {\n}\n",
        )
        .unwrap();
        let manager = ConfigManager::load(dir.path());
        let not_ready = [
            manager.theme_details("frost").unwrap_err(),
            manager
                .edit_theme(&ThemeEditTarget::User, &[], None)
                .unwrap_err(),
            manager
                .edit_theme(
                    &ThemeEditTarget::Theme {
                        theme_id: "paper".into(),
                    },
                    &[],
                    None,
                )
                .unwrap_err(),
            manager.duplicate_theme("frost", "Mine").unwrap_err(),
            manager.rename_theme("paper", "Mine", None).unwrap_err(),
            manager.delete_theme("paper").unwrap_err(),
        ];
        for error in not_ready {
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert_eq!(id_of(&error), Some("edit-issue-not-ready"));
        }
        let unknown = manager.theme_details("nope").unwrap_err();
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert_eq!(id_of(&unknown), Some("edit-issue-theme-not-found"));
        let builtin = manager.delete_theme("frost").unwrap_err();
        assert_eq!(builtin.code, ErrorCode::InvalidRequest);
        assert_eq!(id_of(&builtin), Some("edit-issue-not-editable"));
    }
}
