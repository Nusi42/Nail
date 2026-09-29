//! Loading a config directory, keeping the last valid configuration and
//! watching the files.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nail_api::{Event, ResolvedLayout, ResolvedTheme};
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::error::ConfigError;
use crate::{LAYOUT_FILE, THEME_FILE, default_layout, default_theme, parse_layout, parse_theme};

/// Default debounce interval for [`ConfigManager::watch`]: editors often
/// write a file in several steps.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(150);

/// Which configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKind {
    /// `theme.kdl`.
    Theme,
    /// `layout.kdl`.
    Layout,
}

impl ConfigKind {
    /// Both kinds.
    pub const ALL: [ConfigKind; 2] = [Self::Theme, Self::Layout];

    /// The file name in the config directory.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Theme => THEME_FILE,
            Self::Layout => LAYOUT_FILE,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Theme => 0,
            Self::Layout => 1,
        }
    }
}

/// A change reported by [`ConfigManager::reload`] and the watcher.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigUpdate {
    /// A file changed and was valid (or was removed, falling back to the
    /// default); this is the complete new configuration.
    Changed {
        /// Current theme.
        theme: Arc<ResolvedTheme>,
        /// Current layout.
        layout: Arc<ResolvedLayout>,
    },
    /// A file changed and is invalid; the last valid configuration stays.
    Error(ConfigError),
}

impl ConfigUpdate {
    /// The client event for this update ([`Event::ConfigChanged`] or
    /// [`Event::ConfigError`]).
    pub fn to_event(&self) -> Event {
        match self {
            Self::Changed { theme, layout } => Event::ConfigChanged {
                theme: Box::new((**theme).clone()),
                layout: Box::new((**layout).clone()),
            },
            Self::Error(err) => err.to_event(),
        }
    }
}

/// Owns the active theme and layout of a config directory.
///
/// Both files are optional: a missing file means the shipped default. An
/// invalid file never replaces a valid configuration; the error is recorded
/// (see [`Self::errors`]) and reported, and the last valid configuration (at
/// start-up: the default) stays active. Cloning is cheap and shares state.
#[derive(Debug, Clone)]
pub struct ConfigManager {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    dir: PathBuf,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    theme: Arc<ResolvedTheme>,
    layout: Arc<ResolvedLayout>,
    /// Last content seen per file (`None` = file absent), to ignore
    /// notifications that did not change anything.
    seen: [Option<Option<String>>; 2],
    errors: [Option<ConfigError>; 2],
}

enum Outcome {
    Unchanged,
    Applied,
    Failed(ConfigError),
}

impl ConfigManager {
    /// Loads `theme.kdl` and `layout.kdl` from `dir`. Never fails: missing
    /// files use the defaults, invalid files are recorded in
    /// [`Self::errors`] and replaced by the defaults.
    pub fn load(dir: impl Into<PathBuf>) -> Self {
        let manager = Self {
            inner: Arc::new(Inner {
                dir: dir.into(),
                state: Mutex::new(State {
                    theme: Arc::new(default_theme()),
                    layout: Arc::new(default_layout()),
                    seen: [None, None],
                    errors: [None, None],
                }),
            }),
        };
        for kind in ConfigKind::ALL {
            if let Outcome::Failed(err) = manager.reload_kind(kind) {
                tracing::warn!(%err, "invalid configuration file; using the default");
            }
        }
        manager
    }

    /// The config directory.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// The active theme.
    pub fn theme(&self) -> Arc<ResolvedTheme> {
        self.state().theme.clone()
    }

    /// The active layout.
    pub fn layout(&self) -> Arc<ResolvedLayout> {
        self.state().layout.clone()
    }

    /// Errors of the files as they are on disk now (empty when both are
    /// valid).
    pub fn errors(&self) -> Vec<ConfigError> {
        self.state().errors.iter().flatten().cloned().collect()
    }

    /// Re-reads one file. Returns `None` when its content did not change
    /// since the last read.
    pub fn reload(&self, kind: ConfigKind) -> Option<ConfigUpdate> {
        match self.reload_kind(kind) {
            Outcome::Unchanged => None,
            Outcome::Applied => Some(self.changed()),
            Outcome::Failed(err) => Some(ConfigUpdate::Error(err)),
        }
    }

    /// Re-reads both files; returns the resulting updates (errors first,
    /// then at most one [`ConfigUpdate::Changed`]).
    pub fn reload_all(&self) -> Vec<ConfigUpdate> {
        let mut updates = Vec::new();
        let mut changed = false;
        for kind in ConfigKind::ALL {
            match self.reload_kind(kind) {
                Outcome::Unchanged => {}
                Outcome::Applied => changed = true,
                Outcome::Failed(err) => updates.push(ConfigUpdate::Error(err)),
            }
        }
        if changed {
            updates.push(self.changed());
        }
        updates
    }

    /// Watches the config directory (creating it if needed) and calls
    /// `on_update` from a background thread for every change, after
    /// `debounce` of quiet. Watching stops when the returned guard is
    /// dropped.
    pub fn watch<F>(&self, debounce: Duration, on_update: F) -> Result<ConfigWatcher, ConfigError>
    where
        F: Fn(ConfigUpdate) + Send + 'static,
    {
        let dir = self.inner.dir.clone();
        let dir_name = dir.display().to_string();
        std::fs::create_dir_all(&dir).map_err(|e| ConfigError::io(&dir_name, &e))?;

        let manager = self.clone();
        let handler = move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut kinds: Vec<ConfigKind> = Vec::new();
                for event in &events {
                    let name = event.path.file_name().and_then(|n| n.to_str());
                    if let Some(kind) = ConfigKind::ALL
                        .into_iter()
                        .find(|k| Some(k.file_name()) == name)
                        && !kinds.contains(&kind)
                    {
                        kinds.push(kind);
                    }
                }
                let mut changed = false;
                for kind in kinds {
                    match manager.reload_kind(kind) {
                        Outcome::Unchanged => {}
                        Outcome::Applied => changed = true,
                        Outcome::Failed(err) => {
                            tracing::warn!(%err, "invalid configuration file; keeping the last valid one");
                            on_update(ConfigUpdate::Error(err));
                        }
                    }
                }
                if changed {
                    on_update(manager.changed());
                }
            }
            Err(err) => tracing::warn!(%err, "config watcher error"),
        };

        let mut debouncer =
            new_debouncer(debounce, handler).map_err(|e| watch_error(&dir_name, &e))?;
        debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|e| watch_error(&dir_name, &e))?;
        Ok(ConfigWatcher {
            _debouncer: debouncer,
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock cannot leave the state half-updated
        // (every update is a single assignment), so poisoning is ignored.
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn changed(&self) -> ConfigUpdate {
        let state = self.state();
        ConfigUpdate::Changed {
            theme: state.theme.clone(),
            layout: state.layout.clone(),
        }
    }

    fn reload_kind(&self, kind: ConfigKind) -> Outcome {
        let path = self.inner.dir.join(kind.file_name());
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                let err = ConfigError::io(kind.file_name(), &e);
                let mut state = self.state();
                state.seen[kind.index()] = None;
                state.errors[kind.index()] = Some(err.clone());
                return Outcome::Failed(err);
            }
        };

        let mut state = self.state();
        if state.seen[kind.index()].as_ref() == Some(&text) {
            return Outcome::Unchanged;
        }
        let file = kind.file_name();
        let result = match (kind, &text) {
            (ConfigKind::Theme, None) => Ok(Resolved::Theme(default_theme())),
            (ConfigKind::Layout, None) => Ok(Resolved::Layout(default_layout())),
            (ConfigKind::Theme, Some(t)) => parse_theme(t, file).map(Resolved::Theme),
            (ConfigKind::Layout, Some(t)) => parse_layout(t, file).map(Resolved::Layout),
        };
        state.seen[kind.index()] = Some(text);
        match result {
            Ok(resolved) => {
                state.errors[kind.index()] = None;
                match resolved {
                    Resolved::Theme(theme) => state.theme = Arc::new(theme),
                    Resolved::Layout(layout) => state.layout = Arc::new(layout),
                }
                Outcome::Applied
            }
            Err(err) => {
                state.errors[kind.index()] = Some(err.clone());
                Outcome::Failed(err)
            }
        }
    }
}

enum Resolved {
    Theme(ResolvedTheme),
    Layout(ResolvedLayout),
}

fn watch_error(dir: &str, error: &notify_debouncer_mini::notify::Error) -> ConfigError {
    ConfigError::io(dir, &std::io::Error::other(error.to_string()))
}

/// Keeps a config directory watched; dropping it stops watching.
pub struct ConfigWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl std::fmt::Debug for ConfigWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigWatcher").finish_non_exhaustive()
    }
}
