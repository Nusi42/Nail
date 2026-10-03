//! `<config-dir>/ui-state.json` (M5b, ADR 0109): small UI state kept
//! across restarts — collapsed folder-pane sections, the last view and
//! settings section; M5c adds window geometry. One JSON object of keys
//! ([`nail_api::ui_state`]); not configuration, so it is neither watched nor
//! backed up, and `layout.kdl` keeps describing structure only.
//!
//! Reading never fails: a missing file is empty, an unreadable or invalid
//! one is logged and treated as empty (the next write replaces it). Writes
//! are atomic (temporary file + rename) and serialized by the
//! [`ConfigManager`]'s write lock.

use std::path::Path;

use nail_api::ui_state::{UI_STATE_FILE, UiState, UiStateIssue, check_ui_state_write};
use serde_json::Value;

use crate::backup::{read_optional, write_atomic};
use crate::manager::ConfigManager;

/// Why [`ConfigManager::set_ui_state`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiStateError {
    /// The key or value is not allowed.
    Invalid(UiStateIssue),
    /// The file could not be written (the OS message).
    Io(String),
}

impl std::fmt::Display for UiStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(issue) => write!(f, "invalid UI state write: {}", issue.as_str()),
            Self::Io(err) => write!(f, "cannot write {UI_STATE_FILE}: {err}"),
        }
    }
}

impl std::error::Error for UiStateError {}

/// Reads the UI state of a config directory (empty when missing or
/// invalid).
pub fn read_ui_state(dir: &Path) -> UiState {
    let path = dir.join(UI_STATE_FILE);
    match read_optional(&path) {
        Ok(None) => UiState::new(),
        Ok(Some(text)) => match serde_json::from_str::<UiState>(&text) {
            Ok(state) => state,
            Err(err) => {
                tracing::warn!(%err, "{UI_STATE_FILE} is not a JSON object; starting empty");
                UiState::new()
            }
        },
        Err(err) => {
            tracing::warn!(%err, "cannot read {UI_STATE_FILE}; starting empty");
            UiState::new()
        }
    }
}

impl ConfigManager {
    /// The UI state (`ui-state.json`).
    pub fn ui_state(&self) -> UiState {
        read_ui_state(self.dir())
    }

    /// Sets one key (`None` removes it) and rewrites the file. Returns the
    /// whole state and whether the value changed (nothing is written when
    /// it did not).
    pub fn set_ui_state(
        &self,
        key: &str,
        value: Option<Value>,
    ) -> Result<(UiState, bool), UiStateError> {
        let _guard = self
            .inner
            .writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = read_ui_state(self.dir());
        if let Some(issue) = check_ui_state_write(&state, key, value.as_ref()) {
            return Err(UiStateError::Invalid(issue));
        }
        if state.get(key) == value.as_ref() {
            return Ok((state, false));
        }
        match value {
            Some(value) => {
                state.insert(key.to_owned(), value);
            }
            None => {
                state.remove(key);
            }
        }
        let text =
            serde_json::to_string_pretty(&state).map_err(|e| UiStateError::Io(e.to_string()))?;
        write_atomic(&self.dir().join(UI_STATE_FILE), &text)
            .map_err(|e| UiStateError::Io(e.to_string()))?;
        Ok((state, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_read_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        assert!(manager.ui_state().is_empty());
        let (state, changed) = manager
            .set_ui_state("folder-pane.collapsed", Some(json!(["account:work"])))
            .unwrap();
        assert!(changed);
        assert_eq!(state["folder-pane.collapsed"], json!(["account:work"]));
        // The same value again changes nothing.
        let (_, changed) = manager
            .set_ui_state("folder-pane.collapsed", Some(json!(["account:work"])))
            .unwrap();
        assert!(!changed);
        manager
            .set_ui_state("shell.view", Some(json!("calendar")))
            .unwrap();
        // Another manager over the same directory reads it back.
        let other = ConfigManager::load(dir.path());
        assert_eq!(other.ui_state()["shell.view"], json!("calendar"));
        let (state, changed) = manager.set_ui_state("shell.view", None).unwrap();
        assert!(changed);
        assert!(!state.contains_key("shell.view"));
        assert!(dir.path().join(UI_STATE_FILE).is_file());
    }

    #[test]
    fn invalid_writes_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        assert_eq!(
            manager.set_ui_state("Bad Key", Some(json!(1))),
            Err(UiStateError::Invalid(UiStateIssue::InvalidKey))
        );
        assert!(!dir.path().join(UI_STATE_FILE).exists());
        // A broken file reads as empty and is replaced by the next write.
        std::fs::write(dir.path().join(UI_STATE_FILE), "[1, 2").unwrap();
        assert!(manager.ui_state().is_empty());
        manager
            .set_ui_state("shell.view", Some(json!("mail")))
            .unwrap();
        assert_eq!(read_ui_state(dir.path())["shell.view"], json!("mail"));
        assert!(
            UiStateError::Io("x".into())
                .to_string()
                .contains(UI_STATE_FILE)
        );
    }
}
