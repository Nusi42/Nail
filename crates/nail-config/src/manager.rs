//! Loading a config directory, keeping the last valid configuration and
//! watching the files.
//!
//! M5a (ADR 0102): the theme the manager serves is the *selected* theme —
//! the base ← the active theme's `extends` chain ← the user's `theme.kdl`,
//! with the density ([`crate::resolve_selected`]). The theme list
//! ([`ThemeCatalog`]: built-ins and `<config-dir>/themes/*.kdl`), the OS
//! light / dark state ([`ConfigManager::set_system_appearance`]) and the
//! selection in `theme.kdl` all feed it; any change re-resolves it and
//! reports [`ConfigUpdate::ThemeChanged`] (list, selection, active theme or
//! OS state changed) before [`ConfigUpdate::Changed`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nail_api::appearance::{Appearance, SystemAppearance, THEMES_DIR};
use nail_api::{ConfigFile, Event, ResolvedLayout, ResolvedTheme};
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::error::{ConfigError, issues_event};
use crate::theme::ThemeFile;
use crate::themes::{Selected, ThemeCatalog, UserThemeText, read_user_themes, resolve_selected};
use crate::{LAYOUT_FILE, THEME_FILE, default_layout, parse_layout_all};

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
    /// The theme list, the selection, the active theme or the OS light /
    /// dark state changed (M5a). Comes before the [`Self::Changed`] that
    /// carries the new resolved theme, when it changed.
    ThemeChanged {
        /// The new state.
        appearance: Box<Appearance>,
    },
    /// Every error of one user theme file (`themes/<id>.kdl`) after the
    /// themes directory was read (M5a). An empty list means the file became
    /// valid again (or was removed).
    ThemeFileErrors {
        /// The file as named in errors (`themes/<id>.kdl`).
        file: String,
        /// Its errors in file order.
        errors: Vec<ConfigError>,
    },
}

impl ConfigUpdate {
    /// The client event for this update ([`Event::ConfigChanged`],
    /// [`Event::ConfigError`], [`Event::ConfigErrors`] or
    /// [`Event::ThemeChanged`]).
    pub fn to_event(&self) -> Event {
        match self {
            Self::Changed { theme, layout } => Event::ConfigChanged {
                theme: Box::new((**theme).clone()),
                layout: Box::new((**layout).clone()),
            },
            Self::Error(err) => err.to_event(),
            Self::Errors { kind, errors } => issues_event(kind.file_name(), errors),
            Self::ThemeChanged { appearance } => Event::ThemeChanged {
                appearance: appearance.clone(),
            },
            Self::ThemeFileErrors { file, errors } => issues_event(file, errors),
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
    /// The editors' undo histories (M5d, ADR 0131).
    pub(crate) history: Mutex<crate::edit_history::Histories>,
}

/// What [`ConfigManager`] last read from a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    /// Not read yet, or the last read failed.
    Unknown,
    /// The file did not exist.
    Absent,
    /// The file's text.
    Text(String),
}

impl Seen {
    /// Whether `text` (`None` = file absent) is what was seen.
    fn is(&self, text: Option<&str>) -> bool {
        match (self, text) {
            (Self::Absent, None) => true,
            (Self::Text(seen), Some(text)) => seen == text,
            _ => false,
        }
    }
}

#[derive(Debug)]
struct State {
    theme: Arc<ResolvedTheme>,
    layout: Arc<ResolvedLayout>,
    /// Last content seen per file, to ignore notifications that did not
    /// change anything.
    seen: [Seen; 2],
    /// Every error per file (empty = valid).
    errors: [Vec<ConfigError>; 2],
    /// M5a: the built-in and user themes.
    catalog: Arc<ThemeCatalog>,
    /// The `themes/` files last read (to ignore unchanged notifications).
    themes_seen: Option<Vec<UserThemeText>>,
    /// The user's last valid `theme.kdl` as read (`None` = no file).
    user_theme: Option<ThemeFile>,
    /// The OS light / dark state (`None` = unknown).
    system: Option<SystemAppearance>,
    /// The current theme list, selection and active theme.
    appearance: Appearance,
}

enum Outcome {
    Unchanged,
    /// Valid; `was_invalid` when the file had errors before; `appearance`
    /// when the theme list / selection / active theme changed with it.
    Applied {
        was_invalid: bool,
        appearance: Option<Box<Appearance>>,
    },
    /// Invalid: every error, in file order (never empty).
    Failed(Vec<ConfigError>),
}

fn appearance_of(
    catalog: &ThemeCatalog,
    selected: &Selected,
    system: Option<SystemAppearance>,
) -> Appearance {
    Appearance {
        themes: catalog.themes(),
        settings: selected.settings.clone(),
        active: selected.active.clone(),
        system,
    }
}

impl ConfigManager {
    /// Loads `theme.kdl`, `layout.kdl` and the user themes in `themes/`
    /// from `dir`. Never fails: missing files use the defaults, invalid
    /// files are recorded in [`Self::errors`] /
    /// [`Self::theme_file_errors`] and replaced by the defaults.
    ///
    /// # Panics
    ///
    /// When the built-in themes do not resolve (a build defect the crate's
    /// tests catch).
    pub fn load(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let catalog = ThemeCatalog::builtin();
        let selected = resolve_selected(&catalog, None, None)
            .unwrap_or_else(|e| panic!("the default theme selection does not resolve: {e:?}"));
        let manager = Self {
            inner: Arc::new(Inner {
                dir,
                state: Mutex::new(State {
                    theme: Arc::new(selected.theme.clone()),
                    layout: Arc::new(default_layout()),
                    seen: [Seen::Unknown, Seen::Unknown],
                    errors: [Vec::new(), Vec::new()],
                    appearance: appearance_of(&catalog, &selected, None),
                    catalog: Arc::new(catalog),
                    themes_seen: None,
                    user_theme: None,
                    system: None,
                }),
                writing: Mutex::new(()),
                plugins: Mutex::new(crate::plugins::InstalledPlugins::default()),
                history: Mutex::new(crate::edit_history::Histories::default()),
            }),
        };
        for update in manager.reload_themes() {
            if let ConfigUpdate::ThemeFileErrors { errors, .. } = update {
                for err in errors {
                    tracing::warn!(%err, "invalid user theme; it cannot be used");
                }
            }
        }
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

    /// The active theme: the selected theme with the user's `theme.kdl`
    /// on top (M5a).
    pub fn theme(&self) -> Arc<ResolvedTheme> {
        self.state().theme.clone()
    }

    /// The active layout.
    pub fn layout(&self) -> Arc<ResolvedLayout> {
        self.state().layout.clone()
    }

    /// The theme list, the selection, the active theme and the OS state
    /// (M5a; the answer of `ListThemes`).
    pub fn appearance(&self) -> Appearance {
        self.state().appearance.clone()
    }

    /// The built-in and user themes as last read.
    pub fn theme_catalog(&self) -> Arc<ThemeCatalog> {
        self.state().catalog.clone()
    }

    /// The OS light / dark state the selection uses (`None` = unknown).
    pub fn system_appearance(&self) -> Option<SystemAppearance> {
        self.state().system
    }

    /// The appearance and the resolved theme a client would get if the OS
    /// state were `system` (M5b, ADR 0109: a remote browser session follows
    /// its own light / dark mode instead of the host's). Nothing changes
    /// for anyone else; `Appearance::system` is `system`. When the user's
    /// `theme.kdl` does not resolve on that theme, the current theme stays.
    pub fn appearance_for(
        &self,
        system: Option<SystemAppearance>,
    ) -> (Appearance, Arc<ResolvedTheme>) {
        let state = self.state();
        let active = nail_api::appearance::active_theme(
            &state.appearance.settings,
            system,
            &state.appearance.themes,
        );
        let current = || {
            let mut appearance = state.appearance.clone();
            appearance.system = system;
            (appearance, state.theme.clone())
        };
        if active == state.appearance.active {
            return current();
        }
        match resolve_selected(&state.catalog, state.user_theme.as_ref(), system) {
            Ok(selected) => (
                appearance_of(&state.catalog, &selected, system),
                Arc::new(selected.theme),
            ),
            Err(_) => current(),
        }
    }

    /// The first error of each file as it is on disk now (empty when both
    /// are valid). [`Self::file_errors`] has all errors of a file; user
    /// themes are in [`Self::theme_file_errors`].
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

    /// Every error of every user theme file (`themes/<id>.kdl` → errors in
    /// file order; valid files are left out).
    pub fn theme_file_errors(&self) -> BTreeMap<String, Vec<ConfigError>> {
        self.state().catalog.file_errors()
    }

    /// One [`Event::ConfigErrors`] per file (theme, layout), with every
    /// error of the file; valid files get an empty list; then one per user
    /// theme file that has errors (M5a). Transports send these to a client
    /// when it connects, next to the M1a [`Event::ConfigError`]s from
    /// [`Self::errors`].
    pub fn error_events(&self) -> Vec<Event> {
        let state = self.state();
        let mut events: Vec<Event> = ConfigKind::ALL
            .into_iter()
            .map(|kind| issues_event(kind.file_name(), &state.errors[kind.index()]))
            .collect();
        for (file, errors) in state.catalog.file_errors() {
            events.push(issues_event(&file, &errors));
        }
        events
    }

    /// Re-reads one file. Returns `None` when its content did not change
    /// since the last read. For an invalid file this is its first error;
    /// [`Self::reload_updates`] also returns all of them.
    pub fn reload(&self, kind: ConfigKind) -> Option<ConfigUpdate> {
        self.reload_updates(kind).into_iter().next()
    }

    /// Re-reads one file and returns every resulting update:
    /// nothing when it did not change; `Changed` (preceded by
    /// `ThemeChanged` when the selection changed, followed by an empty
    /// `Errors` when the file had errors before) when it is valid; `Error`
    /// (the first error) and `Errors` (all of them) when it is invalid.
    pub fn reload_updates(&self, kind: ConfigKind) -> Vec<ConfigUpdate> {
        match self.reload_kind(kind) {
            Outcome::Unchanged => Vec::new(),
            Outcome::Applied {
                was_invalid,
                appearance,
            } => {
                let mut updates = Vec::new();
                if let Some(appearance) = appearance {
                    updates.push(ConfigUpdate::ThemeChanged { appearance });
                }
                updates.push(self.changed());
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
    /// `ThemeChanged` and one [`ConfigUpdate::Changed`], followed by an
    /// empty `Errors` for each file that became valid).
    pub fn reload_all(&self) -> Vec<ConfigUpdate> {
        let mut updates = Vec::new();
        let mut cleared = Vec::new();
        let mut changed = false;
        let mut theme_changed = None;
        for kind in ConfigKind::ALL {
            match self.reload_kind(kind) {
                Outcome::Unchanged => {}
                Outcome::Applied {
                    was_invalid,
                    appearance,
                } => {
                    changed = true;
                    theme_changed = appearance.or(theme_changed);
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
            if let Some(appearance) = theme_changed {
                updates.push(ConfigUpdate::ThemeChanged { appearance });
            }
            updates.push(self.changed());
            updates.extend(cleared);
        }
        updates
    }

    /// Re-reads the user themes in `themes/` (M5a). Returns nothing when no
    /// file changed; else a `ThemeFileErrors` for every file whose errors
    /// changed (an empty list when it became valid or was removed), then
    /// `ThemeChanged` when the list or the active theme changed and
    /// `Changed` when the resolved theme changed.
    pub fn reload_themes(&self) -> Vec<ConfigUpdate> {
        let texts = read_user_themes(&self.inner.dir);
        let mut state = self.state();
        if state.themes_seen.as_ref() == Some(&texts) {
            return Vec::new();
        }
        let catalog = ThemeCatalog::with_user_themes(texts.clone());
        state.themes_seen = Some(texts);
        let old_errors = state.catalog.file_errors();
        let new_errors = catalog.file_errors();
        let mut updates = Vec::new();
        let files: std::collections::BTreeSet<&String> =
            old_errors.keys().chain(new_errors.keys()).collect();
        for file in files {
            let new = new_errors.get(file).cloned().unwrap_or_default();
            if old_errors.get(file) != Some(&new) {
                updates.push(ConfigUpdate::ThemeFileErrors {
                    file: file.clone(),
                    errors: new,
                });
            }
        }
        state.catalog = Arc::new(catalog);
        updates.extend(Self::reselect(&mut state));
        updates
    }

    /// Tells the manager the OS light / dark and high-contrast state
    /// (M5a, Follow Windows): from the platform watcher, or as reported by
    /// the UI (`prefers-color-scheme`). Returns `ThemeChanged` (and
    /// `Changed` when the active theme changed); nothing when the state is
    /// the same as before.
    pub fn set_system_appearance(&self, system: Option<SystemAppearance>) -> Vec<ConfigUpdate> {
        let mut state = self.state();
        if state.system == system {
            return Vec::new();
        }
        state.system = system;
        Self::reselect(&mut state)
    }

    /// Re-resolves the selection after the theme list or the OS state
    /// changed. The user's last valid `theme.kdl` is used; when it no
    /// longer resolves on the new active theme (a token reference only the
    /// old theme defined), the last valid theme stays and a warning is
    /// logged.
    fn reselect(state: &mut State) -> Vec<ConfigUpdate> {
        let selected = match resolve_selected(
            &state.catalog,
            state.user_theme.as_ref(),
            state.system,
        ) {
            Ok(selected) => selected,
            Err(errors) => {
                for err in errors {
                    tracing::warn!(%err, "theme.kdl does not resolve on the newly selected theme; keeping the last valid theme");
                }
                return Vec::new();
            }
        };
        let mut updates = Vec::new();
        let appearance = appearance_of(&state.catalog, &selected, state.system);
        if appearance != state.appearance {
            state.appearance = appearance.clone();
            updates.push(ConfigUpdate::ThemeChanged {
                appearance: Box::new(appearance),
            });
        }
        if *state.theme != selected.theme {
            state.theme = Arc::new(selected.theme);
            updates.push(ConfigUpdate::Changed {
                theme: state.theme.clone(),
                layout: state.layout.clone(),
            });
        }
        updates
    }

    /// Watches the config directory and its `themes/` folder (creating
    /// both if needed) and calls `on_update` from a background thread for
    /// every change, after `debounce` of quiet. Watching stops when the
    /// returned guard is dropped.
    pub fn watch<F>(&self, debounce: Duration, on_update: F) -> Result<ConfigWatcher, ConfigError>
    where
        F: Fn(ConfigUpdate) + Send + 'static,
    {
        let dir = self.inner.dir.clone();
        let dir_name = dir.display().to_string();
        std::fs::create_dir_all(&dir).map_err(|e| ConfigError::io(&dir_name, &e))?;
        let themes_dir = dir.join(THEMES_DIR);
        let themes_name = themes_dir.display().to_string();
        std::fs::create_dir_all(&themes_dir).map_err(|e| ConfigError::io(&themes_name, &e))?;

        let manager = self.clone();
        let handler = move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut kinds: Vec<ConfigKind> = Vec::new();
                let mut themes = false;
                for event in &events {
                    if is_theme_dir_event(&event.path) {
                        themes = true;
                        continue;
                    }
                    let name = event.path.file_name().and_then(|n| n.to_str());
                    if let Some(kind) = ConfigKind::ALL
                        .into_iter()
                        .find(|k| Some(k.file_name()) == name)
                        && !kinds.contains(&kind)
                    {
                        kinds.push(kind);
                    }
                }
                if themes {
                    for update in manager.reload_themes() {
                        if let ConfigUpdate::ThemeFileErrors { errors, .. } = &update {
                            for err in errors {
                                tracing::warn!(%err, "invalid user theme; it cannot be used");
                            }
                        }
                        on_update(update);
                    }
                }
                let mut changed = false;
                let mut theme_changed = None;
                let mut cleared = Vec::new();
                let mut reported = Vec::new();
                for kind in kinds {
                    match manager.reload_kind(kind) {
                        Outcome::Unchanged => {}
                        Outcome::Applied {
                            was_invalid,
                            appearance,
                        } => {
                            changed = true;
                            theme_changed = appearance.or(theme_changed);
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
                    if let Some(appearance) = theme_changed {
                        on_update(ConfigUpdate::ThemeChanged { appearance });
                    }
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
        debouncer
            .watcher()
            .watch(&themes_dir, RecursiveMode::NonRecursive)
            .map_err(|e| watch_error(&themes_name, &e))?;
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
                state.seen[kind.index()] = Seen::Unknown;
                state.errors[kind.index()].clone_from(&errors);
                return Outcome::Failed(errors);
            }
        };

        let mut state = self.state();
        if state.seen[kind.index()].is(text.as_deref()) {
            return Outcome::Unchanged;
        }
        let file = kind.file_name();
        let result = match (kind, &text) {
            (ConfigKind::Theme, text) => {
                let parsed = match text {
                    None => Ok(None),
                    Some(t) => ThemeFile::parse_all(t, file).map(Some),
                };
                parsed.and_then(|user| {
                    resolve_selected(&state.catalog, user.as_ref(), state.system)
                        .map(|selected| Resolved::Theme(Box::new((user, selected))))
                        .map_err(|mut errors| {
                            crate::kdl_util::sort_errors(&mut errors);
                            errors
                        })
                })
            }
            (ConfigKind::Layout, None) => Ok(Resolved::Layout(Box::new(default_layout()))),
            (ConfigKind::Layout, Some(t)) => {
                parse_layout_all(t, file).map(|l| Resolved::Layout(Box::new(l)))
            }
        };
        state.seen[kind.index()] = text.map_or(Seen::Absent, Seen::Text);
        match result {
            Ok(resolved) => {
                let was_invalid = !std::mem::take(&mut state.errors[kind.index()]).is_empty();
                let mut appearance = None;
                match resolved {
                    Resolved::Theme(boxed) => {
                        let (user, selected) = *boxed;
                        let new = appearance_of(&state.catalog, &selected, state.system);
                        if new != state.appearance {
                            state.appearance = new.clone();
                            appearance = Some(Box::new(new));
                        }
                        state.user_theme = user;
                        state.theme = Arc::new(selected.theme);
                    }
                    Resolved::Layout(layout) => state.layout = Arc::new(*layout),
                }
                Outcome::Applied {
                    was_invalid,
                    appearance,
                }
            }
            Err(errors) => {
                state.errors[kind.index()].clone_from(&errors);
                Outcome::Failed(errors)
            }
        }
    }
}

/// Whether a watcher event concerns the `themes/` folder: a file directly
/// in it (`<dir>/themes/<id>.kdl`) or the folder itself.
fn is_theme_dir_event(path: &Path) -> bool {
    let in_themes = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == THEMES_DIR);
    let kdl = path.extension().is_some_and(|e| e == "kdl");
    (in_themes && kdl) || path.file_name().is_some_and(|n| n == THEMES_DIR)
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
    Theme(Box<(Option<ThemeFile>, Selected)>),
    Layout(Box<ResolvedLayout>),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_dir_events() {
        let dir = Path::new("/cfg");
        assert!(is_theme_dir_event(&dir.join("themes").join("ocean.kdl")));
        assert!(is_theme_dir_event(&dir.join("themes")));
        assert!(!is_theme_dir_event(&dir.join("theme.kdl")));
        assert!(!is_theme_dir_event(
            &dir.join("themes").join(".ocean.kdl.tmp")
        ));
        assert!(!is_theme_dir_event(
            &dir.join("backups").join("theme-1.kdl")
        ));
    }
}
