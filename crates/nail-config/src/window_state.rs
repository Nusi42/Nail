//! `<config-dir>/window-state.json` (M5c, ADR 0116): the remembered state of
//! every window kind — in-page geometry, pop-out geometry, whether it was
//! last popped out ([`nail_api::windows::WindowStateFile`]). Like
//! `ui-state.json` it is not configuration: neither watched nor backed up,
//! and `layout.kdl` keeps describing structure only.
//!
//! Reading never fails: a missing file is empty; an unreadable file, one of
//! another version or one that does not parse is logged and treated as
//! empty (the next write replaces it); single invalid entries are dropped.
//! Writes are atomic (temporary file + rename) and serialized by the
//! [`ConfigManager`]'s write lock.

use std::path::Path;

use nail_api::windows::{
    WINDOW_STATE_FILE, WindowState, WindowStateFile, WindowStateIssue, WindowStates,
    check_window_state_write,
};

use crate::backup::{read_optional, write_atomic};
use crate::manager::ConfigManager;

/// Why a window-state write failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowStateError {
    /// The window kind or the geometry is not allowed.
    Invalid(WindowStateIssue),
    /// The file could not be written (the OS message).
    Io(String),
}

impl std::fmt::Display for WindowStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(issue) => write!(f, "invalid window state: {}", issue.as_str()),
            Self::Io(err) => write!(f, "cannot write {WINDOW_STATE_FILE}: {err}"),
        }
    }
}

impl std::error::Error for WindowStateError {}

/// Reads the window states of a config directory (empty when missing or
/// invalid).
pub fn read_window_states(dir: &Path) -> WindowStates {
    let path = dir.join(WINDOW_STATE_FILE);
    match read_optional(&path) {
        Ok(None) => WindowStates::new(),
        Ok(Some(text)) => match WindowStateFile::parse(&text) {
            Some(file) => file.windows,
            None => {
                tracing::warn!(
                    "{WINDOW_STATE_FILE} is not a version-1 window-state file; starting empty"
                );
                WindowStates::new()
            }
        },
        Err(err) => {
            tracing::warn!(%err, "cannot read {WINDOW_STATE_FILE}; starting empty");
            WindowStates::new()
        }
    }
}

fn write_window_states(dir: &Path, windows: &WindowStates) -> Result<(), WindowStateError> {
    let file = WindowStateFile {
        windows: windows.clone(),
        ..WindowStateFile::default()
    };
    write_atomic(&dir.join(WINDOW_STATE_FILE), &file.to_text())
        .map_err(|e| WindowStateError::Io(e.to_string()))
}

impl ConfigManager {
    /// Every remembered window state (`window-state.json`).
    pub fn window_states(&self) -> WindowStates {
        read_window_states(self.dir())
    }

    /// Remembers the state of a window kind (`None`, or an empty state,
    /// forgets it) and rewrites the file. Returns every state and whether
    /// anything changed (nothing is written when it did not).
    pub fn set_window_state(
        &self,
        window: &str,
        state: Option<WindowState>,
    ) -> Result<(WindowStates, bool), WindowStateError> {
        let _guard = self
            .inner
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut windows = read_window_states(self.dir());
        if let Some(issue) = check_window_state_write(&windows, window, state.as_ref()) {
            return Err(WindowStateError::Invalid(issue));
        }
        let state = state.filter(|s| !s.is_empty());
        if windows.get(window) == state.as_ref() {
            return Ok((windows, false));
        }
        match state {
            Some(state) => {
                windows.insert(window.to_owned(), state);
            }
            None => {
                windows.remove(window);
            }
        }
        write_window_states(self.dir(), &windows)?;
        Ok((windows, true))
    }

    /// Changes the state of a window kind with `edit` (read, edit, write
    /// under the write lock) — for the core's pop-out bookkeeping. Returns
    /// the new state when it changed.
    pub fn update_window_state(
        &self,
        window: &str,
        edit: impl FnOnce(&mut WindowState),
    ) -> Result<Option<WindowState>, WindowStateError> {
        let _guard = self
            .inner
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut windows = read_window_states(self.dir());
        let before = windows.get(window).copied().unwrap_or_default();
        let mut after = before;
        edit(&mut after);
        if after == before {
            return Ok(None);
        }
        if let Some(issue) = check_window_state_write(&windows, window, Some(&after)) {
            return Err(WindowStateError::Invalid(issue));
        }
        if after.is_empty() {
            windows.remove(window);
        } else {
            windows.insert(window.to_owned(), after);
        }
        write_window_states(self.dir(), &windows)?;
        Ok(Some(after))
    }

    /// Forgets every window state. Returns the kinds that had one (nothing
    /// is written when there were none).
    pub fn reset_window_states(&self) -> Result<Vec<String>, WindowStateError> {
        let _guard = self
            .inner
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let windows = read_window_states(self.dir());
        let path = self.dir().join(WINDOW_STATE_FILE);
        if windows.is_empty() {
            // A broken or foreign file is replaced too.
            if path.exists() {
                write_window_states(self.dir(), &WindowStates::new())?;
            }
            return Ok(Vec::new());
        }
        write_window_states(self.dir(), &WindowStates::new())?;
        Ok(windows.into_keys().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::windows::{WindowGeometry, WindowRect, WindowSnap};
    use pretty_assertions::assert_eq;

    fn state(x: i32) -> WindowState {
        WindowState {
            page: Some(WindowGeometry::at(WindowRect {
                x,
                y: 10,
                width: 700,
                height: 500,
            })),
            ..WindowState::default()
        }
    }

    #[test]
    fn set_read_forget() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        assert!(manager.window_states().is_empty());
        let (all, changed) = manager
            .set_window_state("composer", Some(state(1)))
            .unwrap();
        assert!(changed);
        assert_eq!(all["composer"], state(1));
        let (_, changed) = manager
            .set_window_state("composer", Some(state(1)))
            .unwrap();
        assert!(!changed);
        manager
            .set_window_state("settings", Some(state(2)))
            .unwrap();
        // Another manager over the same directory reads it back.
        let other = ConfigManager::load(dir.path());
        assert_eq!(other.window_states()["settings"], state(2));
        // The file is the documented format.
        let text = std::fs::read_to_string(dir.path().join(WINDOW_STATE_FILE)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["windows"]["settings"]["page"]["rect"]["x"], 2);
        // An empty state forgets the kind, like `None`.
        let (all, changed) = manager
            .set_window_state("settings", Some(WindowState::default()))
            .unwrap();
        assert!(changed && !all.contains_key("settings"));
        let (all, changed) = manager.set_window_state("composer", None).unwrap();
        assert!(changed && all.is_empty());
        let (_, changed) = manager.set_window_state("composer", None).unwrap();
        assert!(!changed);
    }

    #[test]
    fn invalid_writes_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        assert_eq!(
            manager.set_window_state("composer:d1", Some(state(1))),
            Err(WindowStateError::Invalid(WindowStateIssue::InvalidWindow))
        );
        let mut bad = state(1);
        bad.page.as_mut().unwrap().maximized = true;
        bad.page.as_mut().unwrap().snap = Some(WindowSnap::Left);
        assert_eq!(
            manager.set_window_state("composer", Some(bad)),
            Err(WindowStateError::Invalid(WindowStateIssue::InvalidGeometry))
        );
        assert!(!dir.path().join(WINDOW_STATE_FILE).exists());
        // A broken or future file reads as empty and is replaced by the next
        // write.
        for text in ["[1, 2", r#"{"version": 9, "windows": {}}"#] {
            std::fs::write(dir.path().join(WINDOW_STATE_FILE), text).unwrap();
            assert!(manager.window_states().is_empty());
        }
        manager.set_window_state("rules", Some(state(3))).unwrap();
        assert_eq!(read_window_states(dir.path())["rules"], state(3));
        assert!(
            WindowStateError::Io("x".into())
                .to_string()
                .contains(WINDOW_STATE_FILE)
        );
    }

    #[test]
    fn update_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        assert_eq!(manager.reset_window_states().unwrap(), Vec::<String>::new());
        let changed = manager
            .update_window_state("composer", |s| s.popped_out = true)
            .unwrap();
        assert_eq!(
            changed,
            Some(WindowState {
                popped_out: true,
                ..WindowState::default()
            })
        );
        // No change: nothing returned.
        assert_eq!(
            manager
                .update_window_state("composer", |s| s.popped_out = true)
                .unwrap(),
            None
        );
        manager
            .set_window_state("settings", Some(state(1)))
            .unwrap();
        // Back to empty removes the kind.
        manager
            .update_window_state("composer", |s| s.popped_out = false)
            .unwrap();
        assert_eq!(
            manager.window_states().keys().collect::<Vec<_>>(),
            ["settings"]
        );
        assert_eq!(
            manager
                .update_window_state("Bad", |s| s.popped_out = true)
                .unwrap_err(),
            WindowStateError::Invalid(WindowStateIssue::InvalidWindow)
        );
        assert_eq!(manager.reset_window_states().unwrap(), ["settings"]);
        assert!(manager.window_states().is_empty());
        // A broken file is replaced by a reset.
        std::fs::write(dir.path().join(WINDOW_STATE_FILE), "nonsense").unwrap();
        assert_eq!(manager.reset_window_states().unwrap(), Vec::<String>::new());
        let text = std::fs::read_to_string(dir.path().join(WINDOW_STATE_FILE)).unwrap();
        assert_eq!(
            WindowStateFile::parse(&text),
            Some(WindowStateFile::default())
        );
    }
}
