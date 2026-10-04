//! The account manager: `accounts.kdl` loaded, edited with the
//! format-preserving writer ([`AccountsDocument`], ADR 0013), saved
//! atomically and watched for hand edits.
//!
//! An invalid file at start-up (or after a hand edit) keeps the last valid
//! configuration (empty at start-up) and is reported as
//! `Event::ConfigError`; edits through requests are refused while the file
//! on disk is invalid, so a user's half-written file is never overwritten.
//! An edit first compares the file on disk with the text it last loaded or
//! saved: a hand edit the watcher has not picked up yet (or ever, with
//! `--no-watch`) is loaded first and the edit applied on top of it, so it
//! is never silently lost.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nail_api::{AccountsConfig, ApiError, ErrorCode};
use nail_config::{AccountsDocument, ConfigError, ConfigErrorKind};

use crate::error::Result;

/// File name of the account configuration.
pub(crate) const ACCOUNTS_FILE: &str = "accounts.kdl";

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

    /// Every error of the file on disk (empty when it is valid), for the
    /// `ConfigErrors` event: the reader collects all of them, not only the
    /// first.
    pub(crate) fn all_errors(&self) -> Vec<ConfigError> {
        let Some(first) = self.error() else {
            return Vec::new();
        };
        match std::fs::read_to_string(self.dir.join(ACCOUNTS_FILE)) {
            Ok(text) => match nail_config::parse_accounts_all(&text, ACCOUNTS_FILE) {
                Err(errors) if !errors.is_empty() => errors,
                // Valid again since the last reload; the watcher reports it.
                _ => vec![first],
            },
            Err(_) => vec![first],
        }
    }

    /// Applies `edit` to a copy of the document, saves it and makes it
    /// current. Nothing changes when the edit or the save fails; an edit
    /// that makes the file invalid fails with `invalid_request`.
    ///
    /// When the file changed on disk since it was last loaded or saved (a
    /// hand edit not reloaded yet), the edit applies to the file as it is
    /// now; if that file is invalid, the edit is refused (`config`).
    pub(crate) fn edit<R>(
        &self,
        edit: impl FnOnce(&mut AccountsDocument) -> Result<R>,
    ) -> Result<(R, AccountsConfig)> {
        let mut state = self.state();
        if state.error.is_none() {
            self.catch_up(&mut state)?;
        }
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

    /// Loads the file when its text differs from the current document's
    /// (changed on disk behind our back). An invalid file refuses the edit.
    fn catch_up(&self, state: &mut State) -> Result<()> {
        let on_disk = match std::fs::read_to_string(self.dir.join(ACCOUNTS_FILE)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(ConfigError::io(ACCOUNTS_FILE, &e).into()),
        };
        if on_disk == state.doc.text() {
            return Ok(());
        }
        tracing::info!("accounts.kdl changed on disk; applying the edit to the new text");
        match AccountsDocument::parse(&on_disk, ACCOUNTS_FILE) {
            Ok(doc) => {
                state.doc = doc;
                Ok(())
            }
            // Left to the watcher's reload to record and report.
            Err(err) => Err(ApiError::new(
                ErrorCode::Config,
                format!("accounts.kdl is invalid; fix it first ({err})"),
            )
            .into()),
        }
    }

    /// Reads the file again (after a change on disk).
    ///
    /// The file is read under the lock that [`Self::edit`] saves under: a
    /// text read before an edit's save must not replace the newer document
    /// afterwards (the watcher's late reload of an earlier save would undo
    /// the edit until the next change on disk).
    pub(crate) fn reload(&self) -> Reload {
        let mut state = self.state();
        let loaded = AccountsDocument::load(&self.dir);
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
        assert!(manager.all_errors().is_empty());

        let ((), config) = manager
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
        assert!(!manager.all_errors().is_empty());
        assert_eq!(manager.reload(), Reload::Unchanged, "reported once");
        assert_eq!(manager.config().accounts.len(), 1, "last valid kept");
    }

    /// Review M1a: an app-side edit must not overwrite a hand edit that the
    /// watcher has not reloaded yet (or never will, with `--no-watch`).
    #[test]
    fn edits_keep_hand_edits_made_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ACCOUNTS_FILE);
        std::fs::write(&path, WORK).unwrap();
        let manager = AccountManager::load(dir.path());
        // The user adds an account by hand; nothing reloads the file.
        let hand = format!(
            "{WORK}// added by hand\naccount \"home\" email=\"ada@home.example\" {{\n    imap host=\"imap.home.example\"\n}}\n"
        );
        std::fs::write(&path, &hand).unwrap();
        let ((), config) = manager
            .edit(|doc| {
                let mut account = doc.config().account(&"work".into()).unwrap().clone();
                account.name = "Work mail".into();
                doc.upsert_account(account).map_err(edit_error)
            })
            .unwrap();
        assert_eq!(config.accounts.len(), 2, "the hand-added account is kept");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("// added by hand"), "{text}");
        assert!(text.contains("ada@home.example"), "{text}");
        assert!(text.contains("Work mail"), "{text}");
        assert_eq!(manager.reload(), Reload::Unchanged);

        // A broken hand edit refuses the app's edit and stays as it is.
        std::fs::write(&path, "account \"work\" {").unwrap();
        let err = manager.edit(|_| Ok(())).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Config);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "account \"work\" {"
        );
        assert_eq!(manager.config().accounts.len(), 2, "last valid kept");
        // The watcher still reports the broken file.
        assert!(matches!(manager.reload(), Reload::Invalid(_)));
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
