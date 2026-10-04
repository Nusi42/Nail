//! The undo history of the in-app editors (M5d, ADR 0131): every write of
//! `layout.kdl`, `theme.kdl` or a user theme in `themes/` that
//! [`ConfigManager`] makes is recorded as an entry with the files' text
//! before and after; [`ConfigManager::undo_edit`] / [`ConfigManager::redo_edit`]
//! put the text back.
//!
//! * Two scopes ([`HistoryScope`]): `layout` (`layout.kdl`) and
//!   `appearance` (`theme.kdl` and `themes/<id>.kdl`). Each keeps at most
//!   [`MAX_EDIT_HISTORY`] undo and redo entries, in memory for the running
//!   core.
//! * A new entry clears the scope's redo list.
//! * **Changes on disk.** Undo writes an entry's `before` text only when
//!   every file still holds its `after` text (redo: `before`); otherwise it
//!   answers `conflict` (`edit-issue-conflict`) and changes nothing, unless
//!   `force` — then the current `theme.kdl` / `layout.kdl` is backed up
//!   first (ADR 0022 backups). Edits carrying a `base_revision` are refused
//!   the same way when the file's [`config_revision`] differs.

use std::time::{SystemTime, UNIX_EPOCH};

use nail_api::editing::{
    EditEntry, EditHistory, EditIssue, EditKind, HistoryScope, MAX_EDIT_HISTORY, config_revision,
};
use nail_api::{ApiError, ConfigFile, ErrorCode, LocalizedMessage};

use crate::backup;
use crate::manager::{ConfigKind, ConfigManager, ConfigUpdate};

/// One file an entry changed: its path relative to the config directory
/// and its text before and after (`None` = no file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileChange {
    pub(crate) file: String,
    pub(crate) before: Option<String>,
    pub(crate) after: Option<String>,
}

impl FileChange {
    pub(crate) fn new(
        file: impl Into<String>,
        before: Option<String>,
        after: Option<String>,
    ) -> Self {
        Self {
            file: file.into(),
            before,
            after,
        }
    }
}

#[derive(Debug, Clone)]
struct Recorded {
    entry: EditEntry,
    changes: Vec<FileChange>,
}

#[derive(Debug, Default)]
struct Stacks {
    undo: Vec<Recorded>,
    redo: Vec<Recorded>,
}

/// Both scopes' entries (in [`ConfigManager`]'s shared state).
#[derive(Debug, Default)]
pub(crate) struct Histories {
    next_id: u64,
    layout: Stacks,
    appearance: Stacks,
}

impl Histories {
    fn stacks(&mut self, scope: HistoryScope) -> &mut Stacks {
        match scope {
            HistoryScope::Layout => &mut self.layout,
            HistoryScope::Appearance => &mut self.appearance,
        }
    }
}

/// The scope a file belongs to (`layout.kdl` → layout, everything else
/// the editors write → appearance).
pub(crate) fn scope_of(file: &str) -> HistoryScope {
    if file == ConfigFile::Layout.file_name() {
        HistoryScope::Layout
    } else {
        HistoryScope::Appearance
    }
}

/// The main file of a scope (its revision is [`EditHistory::revision`]).
pub(crate) fn main_file(scope: HistoryScope) -> ConfigFile {
    match scope {
        HistoryScope::Layout => ConfigFile::Layout,
        HistoryScope::Appearance => ConfigFile::Theme,
    }
}

fn now_rfc3339() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    backup::rfc3339(millis)
}

fn api_error(code: ErrorCode, localized: LocalizedMessage) -> ApiError {
    ApiError::new(code, crate::l10n::english(&localized)).with_localized(localized)
}

/// `conflict` with `edit-issue-conflict` naming `file`.
pub(crate) fn conflict(file: &str) -> ApiError {
    api_error(
        ErrorCode::Conflict,
        LocalizedMessage::new(EditIssue::Conflict.message_id()).arg("file", file),
    )
}

impl ConfigManager {
    fn histories(&self) -> std::sync::MutexGuard<'_, Histories> {
        self.inner
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reads a file of the config directory (`None` = absent).
    pub(crate) fn read_relative(&self, file: &str) -> Result<Option<String>, ApiError> {
        backup::read_optional(&self.dir().join(file))
            .map_err(|e| crate::error::ConfigError::io(file, &e).into())
    }

    /// The [`config_revision`] of a file of the config directory.
    pub fn file_revision(&self, file: &str) -> Result<String, ApiError> {
        Ok(config_revision(self.read_relative(file)?.as_deref()))
    }

    /// `conflict` when `base` is given and `current` (the file's text now)
    /// has another revision.
    pub(crate) fn check_base_revision(
        file: &str,
        base: Option<&str>,
        current: Option<&str>,
    ) -> Result<(), ApiError> {
        match base {
            Some(base) if base != config_revision(current) => Err(conflict(file)),
            _ => Ok(()),
        }
    }

    /// The undo history of a scope, with the main file's current revision.
    pub fn edit_history(&self, scope: HistoryScope) -> EditHistory {
        let file = main_file(scope).file_name();
        let revision = self
            .file_revision(file)
            .unwrap_or_else(|_| config_revision(None));
        let mut histories = self.histories();
        let stacks = histories.stacks(scope);
        EditHistory {
            scope,
            revision,
            undo: stacks.undo.iter().map(|r| r.entry.clone()).collect(),
            redo: stacks.redo.iter().map(|r| r.entry.clone()).collect(),
        }
    }

    /// Records a write (called with the write lock held, after the files
    /// were written). Unchanged files are dropped; nothing is recorded when
    /// no file changed. Clears the scope's redo list.
    pub(crate) fn record_edit(
        &self,
        scope: HistoryScope,
        kind: EditKind,
        label: LocalizedMessage,
        changes: Vec<FileChange>,
    ) {
        let changes: Vec<FileChange> = changes
            .into_iter()
            .filter(|c| c.before != c.after)
            .collect();
        if changes.is_empty() {
            return;
        }
        let mut histories = self.histories();
        histories.next_id += 1;
        let entry = EditEntry {
            id: format!("e{}", histories.next_id),
            kind,
            label,
            at: now_rfc3339(),
            files: changes.iter().map(|c| c.file.clone()).collect(),
        };
        let stacks = histories.stacks(scope);
        stacks.redo.clear();
        stacks.undo.push(Recorded { entry, changes });
        if stacks.undo.len() > MAX_EDIT_HISTORY {
            stacks.undo.remove(0);
        }
    }

    /// The label of an entry of `kind` without arguments.
    pub(crate) fn plain_label(kind: EditKind) -> LocalizedMessage {
        LocalizedMessage::new(kind.message_id())
    }

    /// Undoes the scope's last entry. Returns the history afterwards and
    /// the config updates to broadcast. `invalid_request` with
    /// `edit-issue-nothing-to-undo`, `conflict` with `edit-issue-conflict`
    /// when a file changed on disk since (unless `force`).
    pub fn undo_edit(
        &self,
        scope: HistoryScope,
        force: bool,
    ) -> Result<(EditHistory, Vec<ConfigUpdate>), ApiError> {
        self.step(scope, force, true)
    }

    /// Redoes the scope's last undone entry (see [`Self::undo_edit`];
    /// `edit-issue-nothing-to-redo`).
    pub fn redo_edit(
        &self,
        scope: HistoryScope,
        force: bool,
    ) -> Result<(EditHistory, Vec<ConfigUpdate>), ApiError> {
        self.step(scope, force, false)
    }

    fn step(
        &self,
        scope: HistoryScope,
        force: bool,
        undo: bool,
    ) -> Result<(EditHistory, Vec<ConfigUpdate>), ApiError> {
        let _guard = self.lock_writes();
        let recorded = {
            let mut histories = self.histories();
            let stacks = histories.stacks(scope);
            let from = if undo { &stacks.undo } else { &stacks.redo };
            let Some(recorded) = from.last() else {
                let issue = if undo {
                    EditIssue::NothingToUndo
                } else {
                    EditIssue::NothingToRedo
                };
                return Err(api_error(
                    ErrorCode::InvalidRequest,
                    LocalizedMessage::new(issue.message_id()),
                ));
            };
            recorded.clone()
        };
        // What each file must hold now, and what it gets.
        let plan: Vec<(&str, Option<&str>, Option<&str>)> = recorded
            .changes
            .iter()
            .map(|c| {
                let (expected, target) = if undo {
                    (c.after.as_deref(), c.before.as_deref())
                } else {
                    (c.before.as_deref(), c.after.as_deref())
                };
                (c.file.as_str(), expected, target)
            })
            .collect();
        for &(file, expected, _) in &plan {
            let current = self.read_relative(file)?;
            if current.as_deref() != expected {
                if !force {
                    return Err(conflict(file));
                }
                if let Some(config_file) = ConfigFile::from_file_name(file) {
                    backup::take(self.dir(), config_file)
                        .map_err(|e| crate::error::ConfigError::io(file, &e))?;
                }
            }
        }
        for &(file, _, target) in &plan {
            let path = self.dir().join(file);
            let result = match target {
                Some(text) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| crate::error::ConfigError::io(file, &e))?;
                    }
                    backup::write_atomic(&path, text)
                }
                None => match std::fs::remove_file(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => other,
                },
            };
            result.map_err(|e| crate::error::ConfigError::io(file, &e))?;
        }
        {
            let mut histories = self.histories();
            let stacks = histories.stacks(scope);
            if undo {
                if let Some(entry) = stacks.undo.pop() {
                    stacks.redo.push(entry);
                }
            } else if let Some(entry) = stacks.redo.pop() {
                stacks.undo.push(entry);
            }
        }
        let updates = self.updates_after_restore(scope);
        Ok((self.edit_history(scope), updates))
    }

    /// Re-reads a scope's files after undo / redo.
    fn updates_after_restore(&self, scope: HistoryScope) -> Vec<ConfigUpdate> {
        match scope {
            HistoryScope::Layout => self.reload_updates(ConfigKind::Layout),
            HistoryScope::Appearance => {
                let mut updates = self.reload_themes();
                updates.extend(self.reload_updates(ConfigKind::Theme));
                updates
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> (tempfile::TempDir, ConfigManager) {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        (dir, manager)
    }

    #[test]
    fn scopes_of_files() {
        assert_eq!(scope_of("layout.kdl"), HistoryScope::Layout);
        assert_eq!(scope_of("theme.kdl"), HistoryScope::Appearance);
        assert_eq!(scope_of("themes/paper.kdl"), HistoryScope::Appearance);
        assert_eq!(main_file(HistoryScope::Appearance), ConfigFile::Theme);
    }

    #[test]
    fn records_skip_unchanged_files_and_clear_redo() {
        let (_dir, manager) = manager();
        manager.record_edit(
            HistoryScope::Layout,
            EditKind::SplitSizes,
            ConfigManager::plain_label(EditKind::SplitSizes),
            vec![FileChange::new(
                "layout.kdl",
                Some("a".into()),
                Some("a".into()),
            )],
        );
        assert!(manager.edit_history(HistoryScope::Layout).undo.is_empty());
        for i in 0..(MAX_EDIT_HISTORY + 3) {
            manager.record_edit(
                HistoryScope::Layout,
                EditKind::SplitSizes,
                ConfigManager::plain_label(EditKind::SplitSizes),
                vec![FileChange::new("layout.kdl", None, Some(i.to_string()))],
            );
        }
        let history = manager.edit_history(HistoryScope::Layout);
        assert_eq!(history.undo.len(), MAX_EDIT_HISTORY);
        assert_eq!(history.undo[0].files, ["layout.kdl"]);
        assert_eq!(history.revision, config_revision(None));
        assert!(
            manager
                .edit_history(HistoryScope::Appearance)
                .undo
                .is_empty()
        );
    }

    #[test]
    fn nothing_to_undo_or_redo() {
        let (_dir, manager) = manager();
        let err = manager.undo_edit(HistoryScope::Layout, false).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(
            err.localized.as_deref().map(|l| l.id.as_str()),
            Some("edit-issue-nothing-to-undo")
        );
        let err = manager
            .redo_edit(HistoryScope::Appearance, false)
            .unwrap_err();
        assert_eq!(
            err.localized.as_deref().map(|l| l.id.as_str()),
            Some("edit-issue-nothing-to-redo")
        );
    }

    #[test]
    fn base_revisions() {
        assert!(ConfigManager::check_base_revision("layout.kdl", None, Some("x")).is_ok());
        let current = config_revision(Some("x"));
        assert!(
            ConfigManager::check_base_revision("layout.kdl", Some(&current), Some("x")).is_ok()
        );
        let err = ConfigManager::check_base_revision("layout.kdl", Some(&current), Some("y"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(
            err.localized.as_deref().map(|l| l.id.as_str()),
            Some("edit-issue-conflict")
        );
    }
}
