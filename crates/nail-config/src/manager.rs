//! Loading a config directory, keeping the last valid configuration and
//! watching the files.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nail_api::{ConfigFile, Event, ResolvedLayout, ResolvedTheme};
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::error::{ConfigError, issues_event};
use crate::{
    LAYOUT_FILE, THEME_FILE, default_layout, default_theme, parse_layout_all, parse_theme_all,
};

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

    /// The protocol's name for the file.
    pub fn file(self) -> ConfigFile {
        match self {
            Self::Theme => ConfigFile::Theme,
            Self::Layout => ConfigFile::Layout,
        }
    }

    /// The kind of a protocol file; `None` for files the manager does not
    /// own (`accounts.kdl`, `rules.kdl`).
    pub fn from_file(file: ConfigFile) -> Option<Self> {
        match file {
            ConfigFile::Theme => Some(Self::Theme),
            ConfigFile::Layout => Some(Self::Layout),
            ConfigFile::Accounts | ConfigFile::Rules => None,
        }
    }

    pub(crate) fn index(self) -> usize {
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
    /// default); this is the complete new configuration. The watcher follows
    /// it with an `Error` for every file that is still invalid.
    Changed {
        /// Current theme.
        theme: Arc<ResolvedTheme>,
        /// Current layout.
        layout: Arc<ResolvedLayout>,
    },
    /// A file changed and is invalid; the last valid configuration stays.
    /// This is the file's first error (the M1a event); it is always followed
    /// by [`Self::Errors`] with all of them.
    Error(ConfigError),
    /// Every error of one file after it was read (M1b). An empty list means
    /// the file became valid again; it is only sent for a file that had
    /// errors before.
    Errors {
        /// The file.
        kind: ConfigKind,
        /// Its errors in file order.
        errors: Vec<ConfigError>,
    },
}

impl ConfigUpdate {
    /// The client event for this update ([`Event::ConfigChanged`],
    /// [`Event::ConfigError`] or [`Event::ConfigErrors`]).
    pub fn to_event(&self) -> Event {
        match self {
            Self::Changed { theme, layout } => Event::ConfigChanged {
                theme: Box::new((**theme).clone()),
                layout: Box::new((**layout).clone()),
            },
            Self::Error(err) => err.to_event(),
            Self::Errors { kind, errors } => issues_event(kind.file_name(), errors),
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
    pub(crate) inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    dir: PathBuf,
    state: Mutex<State>,
    /// Serializes writes to the config files (write-back, merge, revert).
    pub(crate) writing: Mutex<()>,
    /// The installed plug-ins' commands and panels (merge conflicts).
    pub(crate) plugins: Mutex<crate::plugins::InstalledPlugins>,
}

#[derive(Debug)]
struct State {
    theme: Arc<ResolvedTheme>,
    layout: Arc<ResolvedLayout>,
    /// Last content seen per file (`None` = file absent), to ignore
    /// notifications that did not change anything.
    seen: [Option<Option<String>>; 2],
    /// Every error per file (empty = valid).
    errors: [Vec<ConfigError>; 2],
}

enum Outcome {
    Unchanged,
    /// Valid; `was_invalid` when the file had errors before.
    Applied {
        was_invalid: bool,
    },
    /// Invalid: every error, in file order (never empty).
    Failed(Vec<ConfigError>),
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
                    errors: [Vec::new(), Vec::new()],
                }),
                writing: Mutex::new(()),
                plugins: Mutex::new(crate::plugins::InstalledPlugins::default()),
            }),
        };
        for kind in ConfigKind::ALL {
            if let Outcome::Failed(errors) = manager.reload_kind(kind) {
                for err in errors {
                    tracing::warn!(%err, "invalid configuration file; using the default");
                }
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

    /// The first error of each file as it is on disk now (empty when both
    /// are valid). [`Self::file_errors`] has all errors of a file.
    pub fn errors(&self) -> Vec<ConfigError> {
        self.state()
            .errors
            .iter()
            .filter_map(|errors| errors.first().cloned())
            .collect()
    }

    /// Every error of one file as it is on disk now, in file order (empty
    /// when it is valid or absent).
    pub fn file_errors(&self, kind: ConfigKind) -> Vec<ConfigError> {
        self.state().errors[kind.index()].clone()
    }

    /// One [`Event::ConfigErrors`] per file (theme, layout), with every
    /// error of the file; valid files get an empty list. Transports send
    /// these to a client when it connects, next to the M1a
    /// [`Event::ConfigError`]s from [`Self::errors`].
    pub fn error_events(&self) -> Vec<Event> {
        let state = self.state();
        ConfigKind::ALL
            .into_iter()
            .map(|kind| issues_event(kind.file_name(), &state.errors[kind.index()]))
            .collect()
    }

    /// Re-reads one file. Returns `None` when its content did not change
    /// since the last read. For an invalid file this is its first error;
    /// [`Self::reload_updates`] also returns all of them.
    pub fn reload(&self, kind: ConfigKind) -> Option<ConfigUpdate> {
        self.reload_updates(kind).into_iter().next()
    }

    /// Re-reads one file and returns every resulting update:
    /// nothing when it did not change; `Changed` (plus an empty `Errors`
    /// when the file had errors before) when it is valid; `Error` (the first
    /// error) and `Errors` (all of them) when it is invalid.
    pub fn reload_updates(&self, kind: ConfigKind) -> Vec<ConfigUpdate> {
        match self.reload_kind(kind) {
            Outcome::Unchanged => Vec::new(),
            Outcome::Applied { was_invalid } => {
                let mut updates = vec![self.changed()];
                if was_invalid {
                    updates.push(ConfigUpdate::Errors {
                        kind,
                        errors: Vec::new(),
                    });
                }
                updates
            }
            Outcome::Failed(errors) => failed_updates(kind, errors),
        }
    }

    /// Re-reads both files; returns the resulting updates (errors first,
    /// each file's `Error` followed by its `Errors`; then at most one
    /// [`ConfigUpdate::Changed`], followed by an empty `Errors` for each
    /// file that became valid).
    pub fn reload_all(&self) -> Vec<ConfigUpdate> {
        let mut updates = Vec::new();
        let mut cleared = Vec::new();
        let mut changed = false;
        for kind in ConfigKind::ALL {
            match self.reload_kind(kind) {
                Outcome::Unchanged => {}
                Outcome::Applied { was_invalid } => {
                    changed = true;
                    if was_invalid {
                        cleared.push(ConfigUpdate::Errors {
                            kind,
                            errors: Vec::new(),
                        });
                    }
                }
                Outcome::Failed(errors) => updates.extend(failed_updates(kind, errors)),
            }
        }
        if changed {
            updates.push(self.changed());
            updates.extend(cleared);
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
                let mut cleared = Vec::new();
                let mut reported = Vec::new();
                for kind in kinds {
                    match manager.reload_kind(kind) {
                        Outcome::Unchanged => {}
                        Outcome::Applied { was_invalid } => {
                            changed = true;
                            if was_invalid {
                                cleared.push(kind);
                            }
                        }
                        Outcome::Failed(errors) => {
                            for err in &errors {
                                tracing::warn!(%err, "invalid configuration file; keeping the last valid one");
                            }
                            reported.push(kind);
                            for update in failed_updates(kind, errors) {
                                on_update(update);
                            }
                        }
                    }
                }
                if changed {
                    // `Changed` clears the (M1a) error display; files that
                    // became valid get an empty error list, files that are
                    // still invalid (and were not just reported) say so again.
                    on_update(manager.changed());
                    for kind in cleared {
                        on_update(ConfigUpdate::Errors {
                            kind,
                            errors: Vec::new(),
                        });
                    }
                    for kind in ConfigKind::ALL {
                        let errors = manager.file_errors(kind);
                        if !errors.is_empty() && !reported.contains(&kind) {
                            for update in failed_updates(kind, errors) {
                                on_update(update);
                            }
                        }
                    }
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

    pub(crate) fn changed(&self) -> ConfigUpdate {
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
                let errors = vec![ConfigError::io(kind.file_name(), &e)];
                let mut state = self.state();
                state.seen[kind.index()] = None;
                state.errors[kind.index()].clone_from(&errors);
                return Outcome::Failed(errors);
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
            (ConfigKind::Theme, Some(t)) => parse_theme_all(t, file).map(Resolved::Theme),
            (ConfigKind::Layout, Some(t)) => parse_layout_all(t, file).map(Resolved::Layout),
        };
        state.seen[kind.index()] = Some(text);
        match result {
            Ok(resolved) => {
                let was_invalid = !std::mem::take(&mut state.errors[kind.index()]).is_empty();
                match resolved {
                    Resolved::Theme(theme) => state.theme = Arc::new(theme),
                    Resolved::Layout(layout) => state.layout = Arc::new(layout),
                }
                Outcome::Applied { was_invalid }
            }
            Err(errors) => {
                state.errors[kind.index()].clone_from(&errors);
                Outcome::Failed(errors)
            }
        }
    }
}

/// `Error` (the first error, M1a) followed by `Errors` (all of them).
fn failed_updates(kind: ConfigKind, errors: Vec<ConfigError>) -> Vec<ConfigUpdate> {
    let mut updates = Vec::with_capacity(2);
    if let Some(first) = errors.first() {
        updates.push(ConfigUpdate::Error(first.clone()));
    }
    updates.push(ConfigUpdate::Errors { kind, errors });
    updates
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
