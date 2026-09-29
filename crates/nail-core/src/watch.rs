//! Watching `accounts.kdl` for hand edits (ADR 0013): a valid change is
//! applied (store rows, engines) and announced as `AccountsChanged`; an
//! invalid one is reported as `ConfigError` and the last valid accounts
//! stay active. The core's own saves are seen too but change nothing.

use nail_config::DEFAULT_DEBOUNCE;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::accounts::{ACCOUNTS_FILE, Reload};
use crate::error::Result;
use crate::service::Service;

/// Keeps `accounts.kdl` watched; dropping it stops watching.
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
            if !events
                .iter()
                .any(|e| e.path.file_name().and_then(|n| n.to_str()) == Some(ACCOUNTS_FILE))
            {
                return;
            }
            let Some(inner) = weak.upgrade() else { return };
            let service = Service { inner };
            match service.inner.accounts.reload() {
                Reload::Unchanged => {}
                Reload::Invalid(err) => {
                    tracing::warn!(%err, "invalid accounts.kdl; keeping the last valid accounts");
                    service.broadcast(err.to_event());
                }
                Reload::Changed => {
                    tracing::info!("accounts.kdl changed");
                    match &runtime {
                        Some(runtime) => {
                            runtime.spawn(async move {
                                if let Err(err) = service.accounts_changed().await {
                                    tracing::warn!(%err, "cannot apply the changed accounts");
                                }
                            });
                        }
                        None => service.broadcast(nail_api::Event::AccountsChanged),
                    }
                }
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
