//! The account manager: `accounts.kdl` loaded, edited with the
//! format-preserving writer ([`AccountsDocument`], ADR 0013), saved
//! atomically and watched for hand edits.
//!
//! An invalid file at start-up (or after a hand edit) keeps the last valid
//! configuration (empty at start-up) and is reported as
//! `Event::ConfigError`; edits through requests are refused while the file
//! on disk is invalid, so a user's half-written file is never overwritten.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nail_api::{AccountsConfig, ApiError, ErrorCode};
use nail_config::{AccountsDocument, ConfigError, ConfigErrorKind};

use crate::error::Result;

/// File name of the account configuration.
pub const ACCOUNTS_FILE: &str = "accounts.kdl";

#[derive(Debug)]
struct State {
    doc: AccountsDocument,
    /// The file on disk is invalid (the document is the last valid one).
    error: Option<ConfigError>,
}

/// What [`AccountManager::reload`] found.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reload {
    /// Same configuration as before.
    Unchanged,
    /// A new valid configuration.
    Changed,
    /// The file is invalid now; the last valid configuration stays.
    Invalid(ConfigError),
}

/// Owns `accounts.kdl`.
#[derive(Debug)]
pub(crate) struct AccountManager {
    dir: PathBuf,
    state: Mutex<State>,
}

impl AccountManager {
    /// Loads `<dir>/accounts.kdl` (missing = empty).
    pub(crate) fn load(dir: &Path) -> Self {
        let (doc, error) = match AccountsDocument::load(dir) {
            Ok(doc) => (doc, None),
            Err(err) => {
                tracing::warn!(%err, "invalid accounts.kdl; starting without accounts");
                (AccountsDocument::empty(ACCOUNTS_FILE), Some(err))
            }
        };
        Self {
            dir: dir.to_owned(),
            state: Mutex::new(State { doc, error }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The current (last valid) configuration.
    pub(crate) fn config(&self) -> AccountsConfig {
        self.state().doc.config().clone()
    }

    /// The error of the file on disk, if it is invalid.
    pub(crate) fn error(&self) -> Option<ConfigError> {
        self.state().error.clone()
    }

    /// Applies `edit` to a copy of the document, saves it and makes it
    /// current. Nothing changes when the edit or the save fails; an edit
    /// that makes the file invalid fails with `invalid_request`.
    pub(crate) fn edit<R>(
        &self,
        edit: impl FnOnce(&mut AccountsDocument) -> Result<R>,
    ) -> Result<(R, AccountsConfig)> {
        let mut state = self.state();
        if let Some(err) = &state.error {
            return Err(ApiError::new(
                ErrorCode::Config,
                format!("accounts.kdl is invalid; fix it first ({err})"),
            )
            .into());
        }
        let mut doc = state.doc.clone();
        let out = edit(&mut doc)?;
        doc.save(&self.dir)?;
        let config = doc.config().clone();
        state.doc = doc;
        Ok((out, config))
    }

    /// Reads the file again (after a change on disk).
    pub(crate) fn reload(&self) -> Reload {
        let loaded = AccountsDocument::load(&self.dir);
        let mut state = self.state();
        match loaded {
            Ok(doc) => {
                let changed = doc.config() != state.doc.config();
                state.doc = doc;
                let was_invalid = state.error.take().is_some();
                if changed || was_invalid {
                    Reload::Changed
                } else {
                    Reload::Unchanged
                }
            }
            Err(err) => {
                let same = state.error.as_ref() == Some(&err);
                state.error = Some(err.clone());
                if same {
                    Reload::Unchanged
                } else {
                    Reload::Invalid(err)
                }
            }
        }
    }
}

/// A validation error of an edited configuration as a request error.
pub(crate) fn edit_error(err: ConfigError) -> crate::CoreError {
    if err.kind == ConfigErrorKind::Io {
        err.into()
    } else {
        ApiError::invalid_request(err.message).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: &str = "// my accounts\naccount \"work\" email=\"ada@work.example\" {\n    imap host=\"imap.work.example\"\n}\n";

    #[test]
    fn loads_edits_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ACCOUNTS_FILE), WORK).unwrap();
        let manager = AccountManager::load(dir.path());
        assert_eq!(manager.config().accounts.len(), 1);
        assert!(manager.error().is_none());

        let (_, config) = manager
            .edit(|doc| {
                let mut account = doc.config().accounts[0].clone();
                account.name = "Work mail".into();
                doc.upsert_account(account).map_err(edit_error)
            })
            .unwrap();
        assert_eq!(config.accounts[0].name, "Work mail");
        let text = std::fs::read_to_string(dir.path().join(ACCOUNTS_FILE)).unwrap();
        assert!(text.starts_with("// my accounts"), "{text}");
        assert_eq!(manager.reload(), Reload::Unchanged);
    }

    #[test]
    fn invalid_files_block_edits_until_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ACCOUNTS_FILE);
        std::fs::write(&path, "account \"work\" {").unwrap();
        let manager = AccountManager::load(dir.path());
        assert!(manager.error().is_some());
        assert!(manager.config().accounts.is_empty());
        let err = manager.edit(|_| Ok(())).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Config);
        // The broken file is untouched.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "account \"work\" {"
        );

        std::fs::write(&path, WORK).unwrap();
        assert_eq!(manager.reload(), Reload::Changed);
        assert_eq!(manager.config().accounts.len(), 1);

        std::fs::write(&path, "account").unwrap();
        assert!(matches!(manager.reload(), Reload::Invalid(_)));
        assert_eq!(manager.reload(), Reload::Unchanged, "reported once");
        assert_eq!(manager.config().accounts.len(), 1, "last valid kept");
    }

    #[test]
    fn invalid_edits_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AccountManager::load(dir.path());
        let err = manager
            .edit(|doc| {
                let mut config = doc.config().clone();
                config.undo_send_secs = Some(100_000);
                doc.set_config(config).map_err(edit_error)
            })
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidRequest);
        assert!(!dir.path().join(ACCOUNTS_FILE).exists());
    }
}
