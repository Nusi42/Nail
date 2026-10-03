//! Themes and their selection (M5a, ADR 0102).
//!
//! * The **built-in themes** (`defaults/themes/*.kdl`) are embedded
//!   ([`BUILTIN_THEME_FILES`]); each is an ordinary theme file extending the
//!   shipped base (`defaults/theme.kdl`).
//! * **User themes** are `<config-dir>/themes/<id>.kdl`; [`ThemeCatalog`]
//!   lists both, follows `extends` chains (a built-in, a user theme or the
//!   base) and records a [`ThemeProblem`] plus the [`ConfigError`]s of every
//!   user theme that cannot be used (it does not parse, shadows a built-in,
//!   extends an unknown or broken theme, extends itself, or does not
//!   resolve). Unusable themes are listed anyway so the user sees them.
//! * The **selection** is read from the user's `theme.kdl`
//!   ([`appearance_settings`]): `extends` is the fixed theme, the
//!   `appearance` node holds Follow Windows, the pair and the density, and
//!   the override tokens are `font-size "body"`, `radius "md"` and
//!   `color "accent"`.
//! * [`resolve_selected`] renders what the UI shows: the base ← the active
//!   theme's chain ← the user's `theme.kdl`, with the density.

use std::collections::BTreeMap;
use std::path::Path;

use nail_api::appearance::{
    self, AppearanceSettings, SystemAppearance, ThemeInfo, ThemeKind, ThemeOverrides, ThemeProblem,
    ThemeSource,
};
use nail_api::{LocalizedMessage, ResolvedTheme, SourceLocation, ThemeMode};

use crate::error::ConfigError;
use crate::kdl_util::sort_errors;
use crate::l10n::msg;
use crate::theme::{DEFAULT_BASE, ThemeFile, TokenDef};

/// One embedded built-in theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTheme {
    /// The id (`nail_api::appearance::BUILTIN_THEMES`).
    pub id: &'static str,
    /// The file as named in errors and source locations.
    pub file: &'static str,
    /// The KDL text.
    pub kdl: &'static str,
}

/// The built-in themes, in gallery order (the order of
/// `nail_api::appearance::BUILTIN_THEMES`).
pub const BUILTIN_THEME_FILES: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: "frost",
        file: "defaults/themes/frost.kdl",
        kdl: include_str!("../../../defaults/themes/frost.kdl"),
    },
    BuiltinTheme {
        id: "sage",
        file: "defaults/themes/sage.kdl",
        kdl: include_str!("../../../defaults/themes/sage.kdl"),
    },
    BuiltinTheme {
        id: "graphite",
        file: "defaults/themes/graphite.kdl",
        kdl: include_str!("../../../defaults/themes/graphite.kdl"),
    },
    BuiltinTheme {
        id: "nocturne",
        file: "defaults/themes/nocturne.kdl",
        kdl: include_str!("../../../defaults/themes/nocturne.kdl"),
    },
    BuiltinTheme {
        id: "high-contrast",
        file: "defaults/themes/high-contrast.kdl",
        kdl: include_str!("../../../defaults/themes/high-contrast.kdl"),
    },
];

/// The built-in theme with this id.
pub fn builtin_theme(id: &str) -> Option<&'static BuiltinTheme> {
    BUILTIN_THEME_FILES.iter().find(|t| t.id == id)
}

/// Resolves a built-in theme onto the base (`None` for an unknown id).
pub fn resolve_builtin_theme(id: &str) -> Option<Result<ResolvedTheme, ConfigError>> {
    builtin_theme(id).map(|t| crate::parse_theme(t.kdl, t.file))
}

/// The built-in theme files parsed (not merged), once.
fn builtin_files() -> &'static [ThemeFile] {
    static FILES: std::sync::OnceLock<Vec<ThemeFile>> = std::sync::OnceLock::new();
    FILES.get_or_init(|| {
        BUILTIN_THEME_FILES
            .iter()
            .map(|t| {
                ThemeFile::parse(t.kdl, t.file)
                    .unwrap_or_else(|e| panic!("built-in theme {} is invalid: {e}", t.id))
            })
            .collect()
    })
}

/// A built-in theme's file merged onto its chain (`None` for an unknown
/// id). Used where no catalog is at hand (a theme file read on its own).
pub(crate) fn builtin_merged(id: &str) -> Option<ThemeFile> {
    builtin_merged_depth(id, 0)
}

fn builtin_merged_depth(id: &str, depth: usize) -> Option<ThemeFile> {
    let index = BUILTIN_THEME_FILES.iter().position(|t| t.id == id)?;
    let file = builtin_files()[index].clone();
    let base = match file.extends.as_deref() {
        None => return Some(file),
        Some(DEFAULT_BASE) => crate::default_theme_file().clone(),
        // Built-ins only extend the base or another built-in (no cycles
        // ship; the depth guard keeps a mistake from recursing forever).
        Some(parent) if depth < BUILTIN_THEME_FILES.len() => {
            builtin_merged_depth(parent, depth + 1)?
        }
        Some(_) => return None,
    };
    Some(base.merge(file))
}

/// A theme's kind from its `mode` (`system` / none reads as light: the base
/// values are the light ones).
pub fn kind_of_mode(mode: ThemeMode) -> ThemeKind {
    match mode {
        ThemeMode::Dark => ThemeKind::Dark,
        ThemeMode::HighContrast => ThemeKind::HighContrast,
        ThemeMode::Light | ThemeMode::System => ThemeKind::Light,
    }
}

/// A resolved theme's preview colours ([`ThemeInfo::preview`]): the base
/// values of [`appearance::PREVIEW_TOKENS`] that are plain colours (a value
/// still naming another token is left out).
fn preview_colors(theme: &ResolvedTheme) -> indexmap::IndexMap<String, String> {
    appearance::PREVIEW_TOKENS
        .iter()
        .filter_map(|name| {
            let value = theme.tokens.get("color", name)?;
            (!value.contains("var(") && !value.starts_with('$'))
                .then(|| ((*name).to_owned(), value.to_owned()))
        })
        .collect()
}

/// The gallery entries of the built-in themes.
pub fn builtin_theme_infos() -> Vec<ThemeInfo> {
    ThemeCatalog::builtin().themes()
}

/// The file name of a user theme as shown in errors and [`ThemeInfo::file`]
/// (`themes/<id>.kdl`, relative to the config directory).
pub fn user_theme_file(id: &str) -> String {
    format!("{}/{id}.kdl", appearance::THEMES_DIR)
}

/// One theme of a [`ThemeCatalog`].
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    info: ThemeInfo,
    /// The file as read (not merged); `None` when it does not parse.
    file: Option<ThemeFile>,
    /// The file merged onto its chain (base ← … ← this file); `None` when
    /// the theme cannot be used.
    merged: Option<ThemeFile>,
    /// Every error of the file (user themes only), in file order.
    errors: Vec<ConfigError>,
}

/// The theme list: the built-ins and the user's `themes/*.kdl`, with their
/// `extends` chains resolved and every problem recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeCatalog {
    /// Built-ins first (gallery order), then user themes by id.
    entries: Vec<Entry>,
    /// Files in `themes/` that are not themes: a name that is not a theme
    /// id, or a file that cannot be read (file name → errors).
    ignored: BTreeMap<String, Vec<ConfigError>>,
}

/// A user theme file to add to a catalog ([`read_user_themes`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserThemeText {
    /// The file stem (the theme id when it is a valid one).
    pub stem: String,
    /// The text, or why it could not be read (English diagnostic).
    pub text: Result<String, String>,
}

/// Why a chain cannot be followed.
enum ChainError {
    Unknown(String),
    Broken(String),
    Cycle,
}

impl Default for ThemeCatalog {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ThemeCatalog {
    /// Only the built-in themes.
    pub fn builtin() -> Self {
        Self::with_user_themes(Vec::new())
    }

    /// Reads `<config_dir>/themes/*.kdl` (a missing directory = no user
    /// themes). Never fails: unreadable files are recorded as errors.
    pub fn load(config_dir: &Path) -> Self {
        Self::with_user_themes(read_user_themes(config_dir))
    }

    /// The built-ins plus these user theme files.
    pub fn with_user_themes(mut user: Vec<UserThemeText>) -> Self {
        let mut entries: Vec<Entry> = BUILTIN_THEME_FILES
            .iter()
            .zip(builtin_files())
            .map(|(t, file)| Entry {
                info: ThemeInfo {
                    id: t.id.to_owned(),
                    name: file.name.value.clone(),
                    label: appearance::builtin_theme_label(t.id),
                    kind: kind_of_mode(file.mode.map_or(ThemeMode::System, Into::into)),
                    source: ThemeSource::Builtin,
                    extends: file.extends.clone(),
                    file: t.file.to_owned(),
                    problem: None,
                    preview: None,
                },
                file: Some(file.clone()),
                merged: None,
                errors: Vec::new(),
            })
            .collect();
        let mut ignored = BTreeMap::new();
        user.sort_by(|a, b| a.stem.cmp(&b.stem));
        user.dedup_by(|a, b| a.stem == b.stem);
        for UserThemeText { stem, text } in user {
            let file_name = user_theme_file(&stem);
            if !appearance::is_theme_id(&stem) {
                let err = ConfigError::invalid(
                    &file_loc(&file_name),
                    msg("config-theme-file-name-invalid").arg("name", stem.as_str()),
                );
                ignored.insert(file_name, vec![err]);
                continue;
            }
            let text = match text {
                Ok(text) => text,
                Err(detail) => {
                    let err = ConfigError::io(file_name.clone(), &std::io::Error::other(detail));
                    ignored.insert(file_name, vec![err]);
                    continue;
                }
            };
            let (file, errors) = match ThemeFile::parse_all(&text, &file_name) {
                Ok(file) => (Some(file), Vec::new()),
                Err(errors) => (None, errors),
            };
            let name = file
                .as_ref()
                .map(|f| f.name.value.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| stem.clone());
            entries.push(Entry {
                info: ThemeInfo {
                    id: stem,
                    name,
                    label: None,
                    kind: file
                        .as_ref()
                        .and_then(|f| f.mode)
                        .map_or(ThemeKind::Light, |m| kind_of_mode(m.into())),
                    source: ThemeSource::User,
                    extends: file.as_ref().and_then(|f| f.extends.clone()),
                    file: file_name,
                    problem: None,
                    preview: None,
                },
                file,
                merged: None,
                errors,
            });
        }
        let mut catalog = Self { entries, ignored };
        catalog.check();
        catalog
    }

    /// Follows every chain, resolves every theme and records problems.
    fn check(&mut self) {
        for i in 0..self.entries.len() {
            let outcome = self.merged_chain(i, &mut Vec::new());
            let entry = &self.entries[i];
            let builtin = entry.info.source == ThemeSource::Builtin;
            let loc = entry
                .file
                .as_ref()
                .map_or_else(|| file_loc(&entry.info.file), |f| f.name.loc.clone());
            let mut problem: Option<(ThemeProblem, Option<String>)> = None;
            let mut errors = Vec::new();
            let mut merged = None;
            let mut preview = None;
            if entry.file.is_none() {
                let detail = entry.errors.first().map(ToString::to_string);
                problem = Some((ThemeProblem::Invalid, detail));
            } else if !builtin && appearance::is_builtin_theme(&entry.info.id) {
                errors.push(ConfigError::invalid(
                    &loc,
                    msg("config-theme-shadows-builtin").arg("id", entry.info.id.as_str()),
                ));
                problem = Some((ThemeProblem::ShadowsBuiltin, None));
            } else {
                match outcome {
                    Err(ChainError::Cycle) => {
                        errors.push(ConfigError::invalid(
                            &loc,
                            msg("config-theme-extends-cycle"),
                        ));
                        problem = Some((ThemeProblem::ExtendsCycle, None));
                    }
                    Err(ChainError::Unknown(base)) => {
                        let err = ConfigError::invalid(
                            &loc,
                            msg("config-theme-extends-unknown").arg("base", base),
                        );
                        problem = Some((ThemeProblem::Invalid, Some(err.to_string())));
                        errors.push(err);
                    }
                    Err(ChainError::Broken(base)) => {
                        let err = ConfigError::invalid(
                            &loc,
                            msg("config-theme-extends-broken").arg("base", base),
                        );
                        problem = Some((ThemeProblem::Invalid, Some(err.to_string())));
                        errors.push(err);
                    }
                    Ok(file) => match file.resolve_all() {
                        Ok(resolved) => {
                            preview = Some(preview_colors(&resolved));
                            merged = Some(file);
                        }
                        Err(resolve_errors) => {
                            let detail = resolve_errors.first().map(ToString::to_string);
                            problem = Some((ThemeProblem::Invalid, detail));
                            errors.extend(resolve_errors);
                        }
                    },
                }
            }
            assert!(
                !(builtin && problem.is_some()),
                "built-in theme {} does not resolve: {errors:?}",
                entry.info.id
            );
            let entry = &mut self.entries[i];
            if let Some(file) = &merged {
                entry.info.kind = kind_of_mode(file.mode.map_or(ThemeMode::System, Into::into));
            }
            entry.merged = merged;
            entry.info.preview = preview;
            entry.errors.extend(errors);
            sort_errors(&mut entry.errors);
            entry.info.problem = problem.map(|(p, detail)| {
                let message = LocalizedMessage::new(p.message_id());
                match detail {
                    Some(d) => message.arg("detail", d),
                    None => message,
                }
            });
        }
    }

    /// The entry a chain uses for `id`: the built-in when one has this id
    /// (a user theme shadowing it is never applied), else the user theme.
    fn lookup(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.info.id == id && e.info.source == ThemeSource::Builtin)
            .or_else(|| self.entries.iter().position(|e| e.info.id == id))
    }

    /// The file of entry `i` merged onto its chain.
    fn merged_chain(&self, i: usize, stack: &mut Vec<usize>) -> Result<ThemeFile, ChainError> {
        if stack.contains(&i) {
            return Err(ChainError::Cycle);
        }
        let entry = &self.entries[i];
        let Some(file) = &entry.file else {
            return Err(ChainError::Broken(entry.info.id.clone()));
        };
        let base = match file.extends.as_deref() {
            None => return Ok(file.clone()),
            Some(DEFAULT_BASE) => crate::default_theme_file().clone(),
            Some(id) => {
                let Some(parent) = self.lookup(id) else {
                    return Err(ChainError::Unknown(id.to_owned()));
                };
                stack.push(i);
                let result = self.merged_chain(parent, stack);
                stack.pop();
                match result {
                    Ok(base) => base,
                    Err(ChainError::Cycle) => return Err(ChainError::Cycle),
                    Err(_) => return Err(ChainError::Broken(id.to_owned())),
                }
            }
        };
        Ok(base.merge(file.clone()))
    }

    /// Every theme (built-ins first, then user themes by id).
    pub fn themes(&self) -> Vec<ThemeInfo> {
        self.entries.iter().map(|e| e.info.clone()).collect()
    }

    /// The theme a chain or the selection uses for `id` (`None` = unknown):
    /// the built-in when a user theme has the same id.
    pub fn get(&self, id: &str) -> Option<&ThemeInfo> {
        self.lookup(id).map(|i| &self.entries[i].info)
    }

    /// The theme's file merged onto its chain (base ← … ← the theme);
    /// `"default"` is the base itself. `None` for an unknown or unusable
    /// theme.
    pub fn merged(&self, id: &str) -> Option<ThemeFile> {
        if id == DEFAULT_BASE {
            return Some(crate::default_theme_file().clone());
        }
        self.lookup(id).and_then(|i| self.entries[i].merged.clone())
    }

    /// Resolves a usable theme on its own (`None` for an unknown or
    /// unusable one).
    pub fn resolve(&self, id: &str) -> Option<ResolvedTheme> {
        self.merged(id).and_then(|f| f.resolve().ok())
    }

    /// Every error of every file in `themes/` (file name → errors in file
    /// order; valid files are left out).
    pub fn file_errors(&self) -> BTreeMap<String, Vec<ConfigError>> {
        let mut out = self.ignored.clone();
        for e in &self.entries {
            if e.info.source == ThemeSource::User && !e.errors.is_empty() {
                out.insert(e.info.file.clone(), e.errors.clone());
            }
        }
        out
    }
}

fn file_loc(file: &str) -> SourceLocation {
    SourceLocation {
        file: file.to_owned(),
        line: 1,
        column: 1,
    }
}

/// Reads the `*.kdl` files of `<config_dir>/themes/` (none when the
/// directory does not exist).
pub fn read_user_themes(config_dir: &Path) -> Vec<UserThemeText> {
    let dir = config_dir.join(appearance::THEMES_DIR);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "kdl") || !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push(UserThemeText {
            stem: stem.to_owned(),
            text: std::fs::read_to_string(&path).map_err(|e| e.to_string()),
        });
    }
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}

// ---------------------------------------------------------------------------
// The selection in the user's theme.kdl

/// The token that holds the text-size override (`font-size "body"`).
pub const FONT_SIZE_TOKEN: (&str, &str) = ("font-size", "body");
/// The token that holds the corner-radius override (`radius "md"`).
pub const RADIUS_TOKEN: (&str, &str) = ("radius", "md");
/// The token that holds the accent override (`color "accent"`).
pub const ACCENT_TOKEN: (&str, &str) = ("color", "accent");

/// The selection and overrides of the user's `theme.kdl` (`None` = no
/// file). See ADR 0102: `extends` is the fixed theme (`default` / none:
/// Frost); Follow Windows defaults to on only for `default` / none; the
/// `appearance` node overrides each value.
pub fn appearance_settings(file: Option<&ThemeFile>) -> AppearanceSettings {
    let mut settings = AppearanceSettings::default();
    let Some(file) = file else {
        return settings;
    };
    match file.extends.as_deref() {
        None | Some(DEFAULT_BASE) => {}
        Some(id) => {
            settings.theme = id.to_owned();
            settings.follow_system = false;
        }
    }
    if let Some(a) = &file.appearance {
        if let Some(follow) = a.follow_system {
            settings.follow_system = follow;
        }
        if let Some(light) = &a.light {
            settings.light.clone_from(light);
        }
        if let Some(dark) = &a.dark {
            settings.dark.clone_from(dark);
        }
        if let Some(density) = a.density {
            settings.density = density.into();
        }
    }
    settings.overrides = theme_overrides(file);
    settings
}

/// The font-size / radius / accent overrides: the base values of those
/// tokens in the file itself (not merged). A value that is not a whole
/// number of pixels (`"0.9rem"`, a token reference) is not an override the
/// settings can show and reads as `None`.
pub fn theme_overrides(file: &ThemeFile) -> ThemeOverrides {
    let token = |(category, name): (&str, &str)| -> Option<&TokenDef> {
        file.tokens.get(category).and_then(|t| t.get(name))
    };
    let pixels = |key| {
        token(key)
            .and_then(|d| d.value.as_ref())
            .and_then(|v| whole_pixels(&v.value))
    };
    ThemeOverrides {
        font_size: pixels(FONT_SIZE_TOKEN),
        radius: pixels(RADIUS_TOKEN),
        accent: token(ACCENT_TOKEN)
            .and_then(|d| d.value.as_ref())
            .map(|v| v.value.clone()),
    }
}

/// `"15px"` / `"15"` → 15; anything else (fractions, other units,
/// references, negative numbers) → `None`.
pub(crate) fn whole_pixels(value: &str) -> Option<u32> {
    let number = value.strip_suffix("px").unwrap_or(value).trim();
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// What the UI renders ([`resolve_selected`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    /// The settings read from the user's `theme.kdl`.
    pub settings: AppearanceSettings,
    /// The active theme id ([`appearance::active_theme`]).
    pub active: String,
    /// The base ← the active theme's chain ← the user's `theme.kdl`, with
    /// the density.
    pub theme: ResolvedTheme,
}

/// Resolves the theme the UI renders.
///
/// `user` is the user's `theme.kdl` as read (`None` = no file). A sparse
/// file (with `extends`) is merged onto the active theme's chain; its
/// `extends` and `appearance` only select. A complete file (no `extends`)
/// is used as it is, as before M5 (only its density setting applies).
/// Errors are those of the user's file merged onto the active theme (e.g.
/// a reference to a token no theme in the chain defines).
pub fn resolve_selected(
    catalog: &ThemeCatalog,
    user: Option<&ThemeFile>,
    system: Option<SystemAppearance>,
) -> Result<Selected, Vec<ConfigError>> {
    let settings = appearance_settings(user);
    let active = appearance::active_theme(&settings, system, &catalog.themes());
    let base = catalog
        .merged(&active)
        .or_else(|| catalog.merged(appearance::DEFAULT_LIGHT_THEME))
        .unwrap_or_else(|| crate::default_theme_file().clone());
    let file = match user {
        None => base,
        Some(user) if user.extends.is_none() => user.clone(),
        Some(user) => base.merge(user.clone()),
    };
    let mut theme = file.resolve_all()?;
    theme.density = settings.density;
    Ok(Selected {
        settings,
        active,
        theme,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(stem: &str, text: &str) -> UserThemeText {
        UserThemeText {
            stem: stem.to_owned(),
            text: Ok(text.to_owned()),
        }
    }

    #[test]
    fn the_files_match_the_contract_list() {
        let ids: Vec<_> = BUILTIN_THEME_FILES.iter().map(|t| t.id).collect();
        let contract: Vec<_> = appearance::BUILTIN_THEMES
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, contract);
        for (info, (id, kind)) in builtin_theme_infos().iter().zip(appearance::BUILTIN_THEMES) {
            assert_eq!(info.id, *id);
            assert_eq!(
                info.kind, *kind,
                "{id}: mode does not match the contract kind"
            );
            assert_eq!(info.extends.as_deref(), Some(appearance::BASE_THEME_ID));
            assert_eq!(
                info.label.as_deref(),
                Some(format!("theme-name-{id}").as_str())
            );
            assert!(info.usable());
        }
    }

    #[test]
    fn every_builtin_resolves() {
        for t in BUILTIN_THEME_FILES {
            let theme = resolve_builtin_theme(t.id).unwrap().unwrap();
            assert!(theme.tokens.get("color", "accent").is_some(), "{}", t.id);
            let merged = builtin_merged(t.id).unwrap().resolve().unwrap();
            assert_eq!(merged, theme, "{}", t.id);
        }
        assert!(resolve_builtin_theme("nope").is_none());
        assert!(builtin_merged("nope").is_none());
    }

    #[test]
    fn previews_carry_each_themes_own_colours() {
        let catalog = ThemeCatalog::with_user_themes(vec![
            user(
                "ocean",
                "theme \"Ocean\" extends=\"sage\" {
    tokens {
        color \"accent\" \"#0077aa\"
    }
}
",
            ),
            user("broken", "theme \"Broken\" extends=\"nope\""),
        ]);
        let sage = catalog.get("sage").unwrap().preview.clone().unwrap();
        assert_eq!(
            sage.keys().map(String::as_str).collect::<Vec<_>>(),
            appearance::PREVIEW_TOKENS
        );
        assert_eq!(sage["bg"], "#fbfcfa");
        let ocean = catalog.get("ocean").unwrap().preview.clone().unwrap();
        // Its own accent, the rest from Sage.
        assert_eq!(ocean["accent"], "#0077aa");
        assert_eq!(ocean["bg"], sage["bg"]);
        assert!(catalog.get("broken").unwrap().preview.is_none());
        for info in builtin_theme_infos() {
            assert_eq!(
                info.preview.unwrap().len(),
                appearance::PREVIEW_TOKENS.len()
            );
        }
    }

    #[test]
    fn kinds_from_modes() {
        assert_eq!(kind_of_mode(ThemeMode::System), ThemeKind::Light);
        assert_eq!(kind_of_mode(ThemeMode::Dark), ThemeKind::Dark);
        assert_eq!(
            kind_of_mode(ThemeMode::HighContrast),
            ThemeKind::HighContrast
        );
    }

    #[test]
    fn whole_pixels_only() {
        assert_eq!(whole_pixels("15px"), Some(15));
        assert_eq!(whole_pixels("0"), Some(0));
        for v in ["14.5px", "1rem", "$font-size.md", "-2px", "px", ""] {
            assert_eq!(whole_pixels(v), None, "{v}");
        }
    }

    #[test]
    fn user_files_with_bad_names_or_unreadable_are_reported_not_listed() {
        let catalog = ThemeCatalog::with_user_themes(vec![
            user("Bad_Name", "theme \"x\" extends=\"default\""),
            UserThemeText {
                stem: "locked".into(),
                text: Err("access denied".into()),
            },
        ]);
        assert_eq!(catalog.themes().len(), BUILTIN_THEME_FILES.len());
        let errors = catalog.file_errors();
        assert_eq!(
            errors.keys().collect::<Vec<_>>(),
            ["themes/Bad_Name.kdl", "themes/locked.kdl"]
        );
        assert_eq!(
            errors["themes/Bad_Name.kdl"][0].localized.id,
            "config-theme-file-name-invalid"
        );
        assert_eq!(
            errors["themes/locked.kdl"][0].kind,
            crate::ConfigErrorKind::Io
        );
    }
}
