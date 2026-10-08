//! UI state the app remembers across restarts (M5b, ADR 0109): small JSON
//! values by key — which folder-pane sections are collapsed, the last view,
//! the last settings section; M5c adds window geometry under `window.*`.
//!
//! This is not configuration: it lives in `<config-dir>/ui-state.json`
//! (`nail-config`), next to but separate from `layout.kdl`, which keeps
//! describing structure. It is read with [`crate::Request::GetUiState`] and
//! written with [`crate::Request::SetUiState`] (the file is rewritten
//! atomically; every change is pushed as [`crate::Event::UiStateChanged`]).
//! Keys are dotted kebab-case names (`folder-pane.collapsed`), values any
//! JSON up to [`MAX_UI_STATE_VALUE_BYTES`].

use std::collections::BTreeMap;

use serde_json::Value;

/// The file in the config directory.
pub const UI_STATE_FILE: &str = "ui-state.json";
/// Longest key.
pub const MAX_UI_STATE_KEY_CHARS: usize = 64;
/// Largest value (serialized JSON).
pub const MAX_UI_STATE_VALUE_BYTES: usize = 16 * 1024;
/// Most keys kept; setting a new key beyond it is refused.
pub const MAX_UI_STATE_KEYS: usize = 512;

/// The whole UI state.
pub type UiState = BTreeMap<String, Value>;

/// Whether `key` is a valid UI-state key: dotted kebab-case segments
/// (`[a-z0-9-]`, each starting with a letter), at most
/// [`MAX_UI_STATE_KEY_CHARS`].
pub fn is_ui_state_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_UI_STATE_KEY_CHARS
        && key.split('.').all(|seg| {
            seg.starts_with(|c: char| c.is_ascii_lowercase())
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !seg.ends_with('-')
        })
}

labeled_enum! {
    /// Why a [`crate::Request::SetUiState`] was refused (`invalid_request`).
    pub enum UiStateIssue : "ui-state-issue" {
        /// The key is not a dotted kebab-case name.
        InvalidKey = "invalid-key",
        /// The value is larger than [`MAX_UI_STATE_VALUE_BYTES`].
        TooLarge = "too-large",
        /// There are already [`MAX_UI_STATE_KEYS`] keys.
        TooManyKeys = "too-many-keys",
    }
}

/// Checks a write: `None` when `key` / `value` may be stored next to
/// `current` (`value` `None` = remove the key, always allowed for a valid
/// key).
pub fn check_ui_state_write(
    current: &UiState,
    key: &str,
    value: Option<&Value>,
) -> Option<UiStateIssue> {
    if !is_ui_state_key(key) {
        return Some(UiStateIssue::InvalidKey);
    }
    let value = value?;
    if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_UI_STATE_VALUE_BYTES) {
        return Some(UiStateIssue::TooLarge);
    }
    if !current.contains_key(key) && current.len() >= MAX_UI_STATE_KEYS {
        return Some(UiStateIssue::TooManyKeys);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys() {
        for good in [
            "folder-pane.collapsed",
            "shell.view",
            "window.composer-1",
            "a",
        ] {
            assert!(is_ui_state_key(good), "{good}");
        }
        for bad in [
            "",
            "Shell.view",
            "shell..view",
            ".x",
            "x.",
            "1x",
            "a-",
            "a b",
            &"x".repeat(65),
        ] {
            assert!(!is_ui_state_key(bad), "{bad}");
        }
    }

    #[test]
    fn writes_are_checked() {
        let mut state = UiState::new();
        assert_eq!(
            check_ui_state_write(&state, "shell.view", Some(&json!("mail"))),
            None
        );
        assert_eq!(
            check_ui_state_write(&state, "Bad", Some(&json!(1))),
            Some(UiStateIssue::InvalidKey)
        );
        assert_eq!(
            check_ui_state_write(&state, "Bad", None),
            Some(UiStateIssue::InvalidKey)
        );
        let big = json!("x".repeat(MAX_UI_STATE_VALUE_BYTES));
        assert_eq!(
            check_ui_state_write(&state, "a.b", Some(&big)),
            Some(UiStateIssue::TooLarge)
        );
        for i in 0..MAX_UI_STATE_KEYS {
            state.insert(format!("k.k{i}"), json!(i));
        }
        assert_eq!(
            check_ui_state_write(&state, "new.key", Some(&json!(1))),
            Some(UiStateIssue::TooManyKeys)
        );
        // Replacing or removing an existing key is fine at the limit.
        assert_eq!(check_ui_state_write(&state, "k.k0", Some(&json!(2))), None);
        assert_eq!(check_ui_state_write(&state, "new.key", None), None);
        assert_wire_labels!(UiStateIssue);
    }
}
