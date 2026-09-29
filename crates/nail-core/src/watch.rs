//! Watching `accounts.kdl` and `rules.kdl` for hand edits (ADR 0013,
//! ADR 0024). A valid change is applied (accounts: store rows and engines,
//! announced as `AccountsChanged`; rules: recompiled, announced as
//! `RulesChanged`); an invalid one is reported as `ConfigError` (the first
//! error, M1a) and `ConfigErrors` (all of them) and the last valid
//! configuration stays active. When a broken file becomes valid again an
//! empty `ConfigErrors` clears the UI's list. The core's own saves are seen
//! too but change nothing.

use nail_config::DEFAULT_DEBOUNCE;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::accounts::{ACCOUNTS_FILE, Reload};
use crate::error::Result;
use crate::service::Service;

/// Keeps `accounts.kdl` and `rules.kdl` watched; dropping it stops
/// watching.
pub(crate) struct AccountsWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl AccountsWatcher {
    /// Starts watching the service's config directory.
    pub(crate) fn start(service: &Service) -> Result<Self> {
        let dir = service.config().dir().to_owned();
        std::fs::create_dir_all(&dir)?;
        let weak = std::sync::Arc::downgrade(&service.inner);
        let runtime = tokio::runtime::Handle::try_current().ok();
        let handler = move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let touched = |name: &str| {
                events
                    .iter()
                    .any(|e| e.path.file_name().and_then(|n| n.to_str()) == Some(name))
            };
            let (accounts, rules) = (
                touched(ACCOUNTS_FILE),
                touched(nail_config::rules::RULES_FILE),
            );
            if !accounts && !rules {
                return;
            }
            let Some(inner) = weak.upgrade() else { return };
            let service = Service { inner };
            let _guard = runtime.as_ref().map(tokio::runtime::Handle::enter);
            if accounts {
                service.reload_accounts_file();
            }
            if rules {
                service.reload_rules_file();
            }
        };
        let mut debouncer = new_debouncer(DEFAULT_DEBOUNCE, handler)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

impl Service {
    /// Re-reads `accounts.kdl` after a change on disk and reports or
    /// applies it. Applying needs a tokio runtime (entered by the caller);
    /// without one only `AccountsChanged` is broadcast.
    pub(crate) fn reload_accounts_file(&self) {
        match self.inner.accounts.reload() {
            Reload::Unchanged => {}
            Reload::Invalid(err) => {
                tracing::warn!(%err, "invalid accounts.kdl; keeping the last valid accounts");
                self.broadcast(err.to_event());
                self.broadcast(nail_config::issues_event(
                    ACCOUNTS_FILE,
                    &self.inner.accounts.all_errors(),
                ));
            }
            Reload::Changed => {
                tracing::info!("accounts.kdl changed");
                // Clears a list the UI may still show for a broken file.
                self.broadcast(nail_config::issues_event(ACCOUNTS_FILE, &[]));
                match tokio::runtime::Handle::try_current() {
                    Ok(runtime) => {
                        let service = self.clone();
                        runtime.spawn(async move {
                            if let Err(err) = service.accounts_changed().await {
                                tracing::warn!(%err, "cannot apply the changed accounts");
                            }
                        });
                    }
                    Err(_) => self.broadcast(nail_api::Event::AccountsChanged),
                }
            }
        }
    }
}
