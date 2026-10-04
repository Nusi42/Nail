//! Appearance: theme metadata and selection, density, the user's
//! font-size / radius / accent overrides, the OS light/dark state and the
//! token schema the themes share (M5a, ADR 0102).
//!
//! # Where things live
//!
//! * **Built-in themes** ship as `defaults/themes/<id>.kdl` ([`BUILTIN_THEMES`]:
//!   Frost and Sage (light), Graphite and Nocturne (dark), High contrast).
//!   Each is a normal theme file, `theme "<name>" extends="default"
//!   mode="light|dark|high-contrast" { … }`, on top of the shipped base
//!   `defaults/theme.kdl` ([`BASE_THEME_ID`], not listed as a theme).
//! * **User themes** are `<config-dir>/themes/<id>.kdl` ([`THEMES_DIR`]), the
//!   same format; they may extend `default`, a built-in or another user
//!   theme. The id is the file stem ([`is_theme_id`]); a user theme with a
//!   built-in's id is listed with a problem and never applied.
//! * **The selection and the overrides** are in the user's `theme.kdl`, so
//!   everything stays expressible in the file (plan section 10):
//!
//!   ```kdl
//!   theme "My settings" extends="frost" {
//!       appearance follow-system=#true light="frost" dark="graphite" density="compact"
//!       tokens {
//!           color "accent" "#3b5bdb"   // ThemeOverrides::accent
//!           font-size "body" 15        // ThemeOverrides::font_size
//!           radius "md" 8              // ThemeOverrides::radius
//!       }
//!       // …the user's own part rules, as before
//!   }
//!   ```
//!
//!   `extends` names the fixed theme ([`AppearanceSettings::theme`]); the
//!   `appearance` node holds Follow Windows, its light / dark pair and the
//!   density (every property optional, defaults in [`AppearanceSettings`]).
//!   Without a `theme.kdl`, or with `extends="default"` and no
//!   `appearance` node, Nail follows Windows with Frost / Graphite. The
//!   overrides are ordinary tokens of that file, so a hand-written
//!   `theme.kdl` and the Appearance settings edit the same thing; writes
//!   go through the format-preserving write-back (comments kept).
//!
//! # Which theme is active
//!
//! [`active_theme`] is the one rule, shared by the core and the UI's mock:
//! with Follow Windows on, the OS high-contrast mode picks
//! [`HIGH_CONTRAST_THEME`], else the OS colour scheme picks the light or
//! dark theme of the pair (light when the OS state is unknown); with it
//! off, the fixed theme. A theme that is missing or broken falls back to
//! the default of its kind. The resolved theme the UI renders
//! ([`crate::ResolvedTheme`]) is: base ← the active theme's `extends`
//! chain ← the user's `theme.kdl` (overrides and part rules); its
//! `density` comes from the `appearance` node.
//!
//! # Density
//!
//! Comfortable / Compact applies to every theme: density-dependent tokens
//! carry a `compact=` variant (`space "row" 12 compact=8`), resolved into
//! `ResolvedTheme::modes["compact"]`, which the UI applies under
//! `:root[data-nail-density="compact"]` after the colour modes. Part rules
//! that differ by density use the `app` part's condition:
//! `part "app[density=\"compact\"] message-list.row.tags" { display "none" }`.
//!
//! # Follow Windows
//!
//! The core learns the OS state from a [`SystemAppearanceSource`]
//! (`ServiceOptions::system_appearance`; on Windows the registry's
//! `AppsUseLightTheme` and the high-contrast flag, watched for changes;
//! tests and the demo use [`ManualSystemAppearance`]). A change re-selects
//! the theme and sends [`crate::Event::ThemeChanged`] (and
//! `ConfigChanged` with the new resolved theme when it switched).

use std::fmt;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::LocalizedMessage;

/// The shipped base every theme extends (`defaults/theme.kdl`); valid in
/// `extends`, not listed as a theme.
pub const BASE_THEME_ID: &str = "default";

/// The light theme of a fresh install (and of Follow Windows by default).
pub const DEFAULT_LIGHT_THEME: &str = "frost";

/// The dark theme of a fresh install (and of Follow Windows by default).
pub const DEFAULT_DARK_THEME: &str = "graphite";

/// The theme Follow Windows uses while Windows is in a high-contrast mode.
pub const HIGH_CONTRAST_THEME: &str = "high-contrast";

/// The built-in themes, in gallery order: `(id, kind)`. The files are
/// `defaults/themes/<id>.kdl`.
pub const BUILTIN_THEMES: &[(&str, ThemeKind)] = &[
    ("frost", ThemeKind::Light),
    ("sage", ThemeKind::Light),
    ("graphite", ThemeKind::Dark),
    ("nocturne", ThemeKind::Dark),
    ("high-contrast", ThemeKind::HighContrast),
];

/// Folder of user themes in the config directory (`themes/<id>.kdl`).
pub const THEMES_DIR: &str = "themes";

/// Name of the KDL node in `theme.kdl` holding the selection.
pub const APPEARANCE_NODE: &str = "appearance";

/// Longest theme id.
pub const MAX_THEME_ID_CHARS: usize = 64;

/// Smallest / largest body text size the overrides accept (px).
pub const FONT_SIZE_RANGE: (u32, u32) = (11, 20);

/// Smallest / largest corner radius (`radius.md`) the overrides accept (px).
pub const RADIUS_RANGE: (u32, u32) = (0, 20);

/// Whether `id` is a valid theme id: a kebab-case file stem (lowercase
/// letters, digits, single hyphens; starts with a letter), at most
/// [`MAX_THEME_ID_CHARS`].
pub fn is_theme_id(id: &str) -> bool {
    id.len() <= MAX_THEME_ID_CHARS
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `id` names a built-in theme.
pub fn is_builtin_theme(id: &str) -> bool {
    BUILTIN_THEMES.iter().any(|(b, _)| *b == id)
}

/// The Fluent id of a built-in theme's display name (`theme-name-<id>`),
/// `None` for other ids (user themes show their `theme "<name>"`).
pub fn builtin_theme_label(id: &str) -> Option<String> {
    is_builtin_theme(id).then(|| format!("theme-name-{id}"))
}

labeled_enum! {
    /// Whether a theme is light or dark (`theme-kind-*`), from its `mode`:
    /// `light` (and `system` / none: the base values are the light ones),
    /// `dark`, `high-contrast` (dark surfaces, never auto-picked for a pair).
    pub enum ThemeKind : "theme-kind" {
        /// A light theme.
        Light = "light",
        /// A dark theme.
        Dark = "dark",
        /// A high-contrast theme (WCAG AAA).
        HighContrast = "high-contrast",
    }
}

labeled_enum! {
    /// Where a theme comes from (`theme-source-*`).
    pub enum ThemeSource : "theme-source" {
        /// Shipped with Nail (`defaults/themes/`).
        Builtin = "builtin",
        /// The user's `<config-dir>/themes/`.
        User = "user",
    }
}

labeled_enum! {
    /// Row and control spacing (`density-*`); applies to every theme.
    #[derive(Default)]
    pub enum Density : "density" {
        /// Rows with avatar, subject, snippet and tag line (default).
        #[default]
        Comfortable = "comfortable",
        /// Tighter rows, no tag line.
        Compact = "compact",
    }
}

labeled_enum! {
    /// The OS colour scheme (`color-scheme-*`).
    pub enum ColorScheme : "color-scheme" {
        /// Apps use the light mode.
        Light = "light",
        /// Apps use the dark mode.
        Dark = "dark",
    }
}

labeled_enum! {
    /// Why theme overrides were refused (`theme-overrides-issue-*`).
    pub enum ThemeOverridesIssue : "theme-overrides-issue" {
        /// `font_size` outside [`FONT_SIZE_RANGE`].
        FontSizeRange = "font-size-range",
        /// `radius` outside [`RADIUS_RANGE`].
        RadiusRange = "radius-range",
        /// `accent` is not `#rgb` / `#rrggbb`.
        AccentInvalid = "accent-invalid",
    }
}

labeled_enum! {
    /// Why a theme cannot be used (`theme-problem-*`), on
    /// [`ThemeInfo::problem`]-style answers and errors.
    pub enum ThemeProblem : "theme-problem" {
        /// No theme with this id.
        Unknown = "unknown",
        /// The file does not parse or resolve (the detail names the error).
        Invalid = "invalid",
        /// A user theme uses a built-in theme's id.
        ShadowsBuiltin = "shadows-builtin",
        /// `extends` chains back to itself.
        ExtendsCycle = "extends-cycle",
    }
}

/// One selectable theme (the gallery entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThemeInfo {
    /// The id: file stem, used in `extends`, `light=`, `dark=`.
    pub id: String,
    /// `theme "<name>"` of the file (user data; may be any text).
    pub name: String,
    /// Fluent id of the display name for built-ins (`theme-name-<id>`); the
    /// UI shows it instead of `name` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    /// Light, dark or high contrast.
    pub kind: ThemeKind,
    /// Built-in or user.
    pub source: ThemeSource,
    /// The theme it extends (`default` for the shipped ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extends: Option<String>,
    /// The file, relative to the config directory for user themes
    /// (`themes/ocean.kdl`) or to the repository for built-ins
    /// (`defaults/themes/frost.kdl`).
    pub file: String,
    /// Why it cannot be used, when it cannot (it is listed anyway so the
    /// user sees it); `SetTheme` refuses it with `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub problem: Option<LocalizedMessage>,
    /// The theme's own resolved colours for the gallery preview (M5b): the
    /// base values of [`PREVIEW_TOKENS`] after its `extends` chain, without
    /// mode variants. `None` when the theme cannot be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preview: Option<indexmap::IndexMap<String, String>>,
}

/// The colour tokens a theme card previews ([`ThemeInfo::preview`]).
pub const PREVIEW_TOKENS: &[&str] = &["bg-subtle", "bg", "accent", "fg"];

impl ThemeInfo {
    /// Whether the theme can be applied.
    pub fn usable(&self) -> bool {
        self.problem.is_none()
    }
}

/// The user's font-size / corner-radius / accent overrides, written as
/// tokens of their `theme.kdl` (`font-size "body"`, `radius "md"`,
/// `color "accent"`). `None` = the theme's own value (the token line is
/// removed). The type scale and the other radii are defined relative to
/// these tokens in the base theme, so one value scales them all.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ThemeOverrides {
    /// Body text size in px ([`FONT_SIZE_RANGE`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub font_size: Option<u32>,
    /// Medium corner radius in px ([`RADIUS_RANGE`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub radius: Option<u32>,
    /// Accent colour, `#rgb` or `#rrggbb` (lowercase after normalizing).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub accent: Option<String>,
}

impl ThemeOverrides {
    /// Every problem, in field order (empty = valid).
    pub fn issues(&self) -> Vec<ThemeOverridesIssue> {
        let mut out = Vec::new();
        if self
            .font_size
            .is_some_and(|s| s < FONT_SIZE_RANGE.0 || s > FONT_SIZE_RANGE.1)
        {
            out.push(ThemeOverridesIssue::FontSizeRange);
        }
        if self
            .radius
            .is_some_and(|r| r < RADIUS_RANGE.0 || r > RADIUS_RANGE.1)
        {
            out.push(ThemeOverridesIssue::RadiusRange);
        }
        if self.accent.as_deref().is_some_and(|a| !is_hex_color(a)) {
            out.push(ThemeOverridesIssue::AccentInvalid);
        }
        out
    }

    /// Whether nothing is overridden.
    pub fn is_empty(&self) -> bool {
        self.font_size.is_none() && self.radius.is_none() && self.accent.is_none()
    }
}

/// Whether `value` is `#rgb` or `#rrggbb` (hex digits, any case).
pub fn is_hex_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// What the user chose (the `appearance` node and `extends` of their
/// `theme.kdl`, plus the override tokens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppearanceSettings {
    /// The fixed theme (`extends`), used while `follow_system` is off.
    /// `default` / none in the file reads as [`DEFAULT_LIGHT_THEME`].
    pub theme: String,
    /// Follow Windows: switch between `light` and `dark` with the OS.
    pub follow_system: bool,
    /// The light theme of the pair.
    pub light: String,
    /// The dark theme of the pair.
    pub dark: String,
    /// Comfortable or compact.
    pub density: Density,
    /// Font size / radius / accent.
    pub overrides: ThemeOverrides,
}

impl Default for AppearanceSettings {
    /// A fresh install: Follow Windows with Frost / Graphite, comfortable.
    fn default() -> Self {
        Self {
            theme: DEFAULT_LIGHT_THEME.to_owned(),
            follow_system: true,
            light: DEFAULT_LIGHT_THEME.to_owned(),
            dark: DEFAULT_DARK_THEME.to_owned(),
            density: Density::Comfortable,
            overrides: ThemeOverrides::default(),
        }
    }
}

/// Everything but the fixed theme ([`crate::Request::SetThemePreferences`];
/// the fixed theme is [`crate::Request::SetTheme`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThemePreferences {
    /// Follow Windows.
    pub follow_system: bool,
    /// The light theme of the pair (a usable light theme).
    pub light: String,
    /// The dark theme of the pair (a usable dark theme).
    pub dark: String,
    /// Comfortable or compact.
    pub density: Density,
    /// Font size / radius / accent (`None` fields remove the override).
    pub overrides: ThemeOverrides,
}

impl From<&AppearanceSettings> for ThemePreferences {
    fn from(s: &AppearanceSettings) -> Self {
        Self {
            follow_system: s.follow_system,
            light: s.light.clone(),
            dark: s.dark.clone(),
            density: s.density,
            overrides: s.overrides.clone(),
        }
    }
}

/// The OS appearance as the core sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SystemAppearance {
    /// Light or dark app mode.
    pub scheme: ColorScheme,
    /// A Windows contrast theme is on.
    pub high_contrast: bool,
}

impl SystemAppearance {
    /// Light, no high contrast.
    pub const LIGHT: Self = Self {
        scheme: ColorScheme::Light,
        high_contrast: false,
    };
    /// Dark, no high contrast.
    pub const DARK: Self = Self {
        scheme: ColorScheme::Dark,
        high_contrast: false,
    };
}

/// The answer to `ListThemes` / `SetTheme` / `SetThemePreferences` and the
/// payload of `ThemeChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Appearance {
    /// Every theme, built-ins first (gallery order), then user themes by id.
    pub themes: Vec<ThemeInfo>,
    /// What the user chose.
    pub settings: AppearanceSettings,
    /// The theme applied now ([`active_theme`]).
    pub active: String,
    /// The OS state, `None` when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub system: Option<SystemAppearance>,
}

/// The theme to apply (see the module docs). `themes` is the list of
/// [`Appearance::themes`]; unusable or missing choices fall back to the
/// default theme of their kind.
pub fn active_theme(
    settings: &AppearanceSettings,
    system: Option<SystemAppearance>,
    themes: &[ThemeInfo],
) -> String {
    let usable = |id: &str| themes.iter().any(|t| t.id == id && t.usable());
    let pick = |id: &str, fallback: &str| {
        if usable(id) {
            id.to_owned()
        } else {
            fallback.to_owned()
        }
    };
    if settings.follow_system {
        match system {
            Some(s) if s.high_contrast && usable(HIGH_CONTRAST_THEME) => {
                HIGH_CONTRAST_THEME.to_owned()
            }
            Some(SystemAppearance {
                scheme: ColorScheme::Dark,
                ..
            }) => pick(&settings.dark, DEFAULT_DARK_THEME),
            _ => pick(&settings.light, DEFAULT_LIGHT_THEME),
        }
    } else {
        let fallback = match themes.iter().find(|t| t.id == settings.theme) {
            Some(t) if t.kind == ThemeKind::Dark => DEFAULT_DARK_THEME,
            _ => DEFAULT_LIGHT_THEME,
        };
        pick(&settings.theme, fallback)
    }
}

// ---------------------------------------------------------------------------
// Token schema

/// A token every theme can rely on: the base theme defines it, and themes
/// may override it. `px` is the base (comfortable) value in pixels where the
/// token is a length, for documentation and the defaults test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenSpec {
    /// Category (`space`, `font-size`, …).
    pub category: &'static str,
    /// Token name.
    pub name: &'static str,
    /// The intended base value (px, or a unitless factor / weight).
    pub value: f32,
    /// Whether the base theme must give it a `compact=` variant.
    pub density: bool,
}

const fn spec(category: &'static str, name: &'static str, value: f32, density: bool) -> TokenSpec {
    TokenSpec {
        category,
        name,
        value,
        density,
    }
}

/// The spacing scale on a 4 px grid (`space.*`; `xxs` is the half step for
/// hairline gaps) plus the semantic spacings the components use.
pub const SPACE_TOKENS: &[TokenSpec] = &[
    spec("space", "xxs", 2.0, false),
    spec("space", "xs", 4.0, false),
    spec("space", "sm", 8.0, false),
    spec("space", "md", 12.0, false),
    spec("space", "lg", 16.0, false),
    spec("space", "xl", 24.0, false),
    spec("space", "xxl", 32.0, false),
    // Inner padding of every pane, so edges line up across the window.
    spec("space", "pane", 16.0, true),
    // Gap between controls in a bar or a form row.
    spec("space", "gap", 8.0, false),
    // Gap between sections of a page or dialog.
    spec("space", "section", 24.0, false),
    // Vertical padding of list rows (Compact: 8).
    spec("space", "row", 12.0, true),
];

/// The type scale (Segoe UI Variable; `font-size.*` in px, unitless in KDL).
/// The base defines the other steps relative to `body`, so the font-size
/// override scales the whole scale. The pre-M5 names `xs`…`xl` stay as
/// aliases for user themes.
pub const TYPE_TOKENS: &[TokenSpec] = &[
    // 11.5 / 600, uppercase section labels.
    spec("font-size", "label", 11.5, false),
    // 12.5: secondary text, dates, sizes.
    spec("font-size", "small", 12.5, false),
    // 14 (Compact: 13): body and controls.
    spec("font-size", "body", 14.0, true),
    // 16 / 600: pane and section headings.
    spec("font-size", "heading", 16.0, false),
    // 20 / 600: the reader subject, page titles.
    spec("font-size", "title", 20.0, true),
    spec("font-weight", "regular", 400.0, false),
    spec("font-weight", "medium", 500.0, false),
    spec("font-weight", "semibold", 600.0, false),
    spec("line-height", "tight", 1.25, false),
    spec("line-height", "normal", 1.45, false),
    // The reader body.
    spec("line-height", "relaxed", 1.65, false),
];

/// Corner radii (`radius.*`): small for inputs and menu items, medium for
/// buttons and cards (the radius override), large for windows, pill for
/// chips and badges.
pub const RADIUS_TOKENS: &[TokenSpec] = &[
    spec("radius", "sm", 6.0, false),
    spec("radius", "md", 10.0, false),
    spec("radius", "lg", 14.0, false),
    spec("radius", "pill", 999.0, false),
];

/// Elevation (`shadow.*`): only floating things get a shadow. `raised` for
/// popovers and cards that float, `overlay` for menus, `window` for
/// dialogs and tool windows. Values are CSS shadows (no px value; 0 here).
pub const ELEVATION_TOKENS: &[TokenSpec] = &[
    spec("shadow", "raised", 0.0, false),
    spec("shadow", "overlay", 0.0, false),
    spec("shadow", "window", 0.0, false),
];

/// Component sizes (`size.*`).
pub const SIZE_TOKENS: &[TokenSpec] = &[
    // Line icons (Lucide) everywhere.
    spec("size", "icon", 16.0, false),
    // Height of buttons, inputs and selects.
    spec("size", "control", 32.0, true),
    // The list-row avatar (Compact: 28).
    spec("size", "avatar", 34.0, true),
];

/// Every token of the schema.
pub fn token_schema() -> impl Iterator<Item = &'static TokenSpec> {
    SPACE_TOKENS
        .iter()
        .chain(TYPE_TOKENS)
        .chain(RADIUS_TOKENS)
        .chain(ELEVATION_TOKENS)
        .chain(SIZE_TOKENS)
}

/// The token variant key of Compact density in `TokenDef` modes and
/// `ResolvedTheme::modes`.
pub const COMPACT_VARIANT: &str = "compact";

// ---------------------------------------------------------------------------
// The OS appearance (Rust only)

/// Called with the new OS appearance on every change.
pub type SystemAppearanceListener = Arc<dyn Fn(SystemAppearance) + Send + Sync>;

/// Where the core learns the OS light/dark and high-contrast state
/// (`ServiceOptions::system_appearance`). Implementations: the Windows
/// registry watcher in `nail-core` (WP152), [`ManualSystemAppearance`] for
/// tests and seeded runs.
pub trait SystemAppearanceSource: Send + Sync + fmt::Debug {
    /// The state now, `None` when unknown.
    fn current(&self) -> Option<SystemAppearance>;
    /// Registers `listener`, called after every change (from any thread)
    /// until [`Self::unwatch`]; returns its handle.
    fn watch(&self, listener: SystemAppearanceListener) -> u64;
    /// Removes a listener.
    fn unwatch(&self, handle: u64);
}

/// A [`SystemAppearanceSource`] set by hand (tests, the demo's test control).
#[derive(Default)]
pub struct ManualSystemAppearance {
    state: Mutex<ManualState>,
}

#[derive(Default)]
struct ManualState {
    current: Option<SystemAppearance>,
    next: u64,
    listeners: Vec<(u64, SystemAppearanceListener)>,
}

impl fmt::Debug for ManualSystemAppearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f.debug_struct("ManualSystemAppearance")
            .field("current", &state.current)
            .field("listeners", &state.listeners.len())
            .finish()
    }
}

impl ManualSystemAppearance {
    /// Starts with `initial`.
    pub fn new(initial: Option<SystemAppearance>) -> Self {
        let source = Self::default();
        source
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current = initial;
        source
    }

    /// Sets the state and tells the listeners when it changed.
    pub fn set(&self, appearance: SystemAppearance) {
        let listeners: Vec<SystemAppearanceListener> = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.current == Some(appearance) {
                return;
            }
            state.current = Some(appearance);
            state.listeners.iter().map(|(_, l)| l.clone()).collect()
        };
        for listener in listeners {
            listener(appearance);
        }
    }
}

impl SystemAppearanceSource for ManualSystemAppearance {
    fn current(&self) -> Option<SystemAppearance> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
    }

    fn watch(&self, listener: SystemAppearanceListener) -> u64 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.next += 1;
        let handle = state.next;
        state.listeners.push((handle, listener));
        handle
    }

    fn unwatch(&self, handle: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.listeners.retain(|(h, _)| *h != handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn info(id: &str, kind: ThemeKind, usable: bool) -> ThemeInfo {
        ThemeInfo {
            id: id.into(),
            name: id.into(),
            label: builtin_theme_label(id),
            kind,
            source: if is_builtin_theme(id) {
                ThemeSource::Builtin
            } else {
                ThemeSource::User
            },
            extends: Some(BASE_THEME_ID.into()),
            file: format!("themes/{id}.kdl"),
            problem: (!usable).then(|| LocalizedMessage::new(ThemeProblem::Invalid.message_id())),
            preview: None,
        }
    }

    fn builtins() -> Vec<ThemeInfo> {
        BUILTIN_THEMES
            .iter()
            .map(|(id, kind)| info(id, *kind, true))
            .collect()
    }

    #[test]
    fn labels() {
        assert_labels(ThemeKind::ALL, ThemeKind::as_str, ThemeKind::message_id);
        assert_labels(
            ThemeSource::ALL,
            ThemeSource::as_str,
            ThemeSource::message_id,
        );
        assert_labels(Density::ALL, Density::as_str, Density::message_id);
        assert_labels(
            ColorScheme::ALL,
            ColorScheme::as_str,
            ColorScheme::message_id,
        );
        assert_labels(
            ThemeOverridesIssue::ALL,
            ThemeOverridesIssue::as_str,
            ThemeOverridesIssue::message_id,
        );
        assert_labels(
            ThemeProblem::ALL,
            ThemeProblem::as_str,
            ThemeProblem::message_id,
        );
        assert_eq!(Density::default(), Density::Comfortable);
    }

    #[test]
    fn theme_ids() {
        for id in ["frost", "high-contrast", "ocean-2", "a"] {
            assert!(is_theme_id(id), "{id}");
        }
        for id in [
            "",
            "Frost",
            "2tone",
            "-a",
            "a-",
            "a--b",
            "a_b",
            "a.b",
            &"a".repeat(65),
        ] {
            assert!(!is_theme_id(id), "{id}");
        }
        for (id, _) in BUILTIN_THEMES {
            assert!(is_theme_id(id) && is_builtin_theme(id));
            assert_eq!(builtin_theme_label(id), Some(format!("theme-name-{id}")));
        }
        assert!(!is_builtin_theme(BASE_THEME_ID));
        assert_eq!(builtin_theme_label("ocean"), None);
        assert!(is_builtin_theme(DEFAULT_LIGHT_THEME));
        assert!(is_builtin_theme(DEFAULT_DARK_THEME));
        assert!(is_builtin_theme(HIGH_CONTRAST_THEME));
    }

    #[test]
    fn defaults_follow_windows_with_frost_and_graphite() {
        let s = AppearanceSettings::default();
        assert!(s.follow_system);
        assert_eq!((s.light.as_str(), s.dark.as_str()), ("frost", "graphite"));
        assert_eq!(s.density, Density::Comfortable);
        assert!(s.overrides.is_empty());
        let p = ThemePreferences::from(&s);
        assert_eq!(p.light, "frost");
        assert!(p.follow_system);
    }

    #[test]
    fn active_theme_follows_the_os() {
        let themes = builtins();
        let mut s = AppearanceSettings {
            light: "sage".into(),
            dark: "nocturne".into(),
            ..AppearanceSettings::default()
        };
        assert_eq!(
            active_theme(&s, Some(SystemAppearance::LIGHT), &themes),
            "sage"
        );
        assert_eq!(
            active_theme(&s, Some(SystemAppearance::DARK), &themes),
            "nocturne"
        );
        // Unknown OS state: the light theme.
        assert_eq!(active_theme(&s, None, &themes), "sage");
        // A Windows contrast theme wins.
        let hc = SystemAppearance {
            scheme: ColorScheme::Dark,
            high_contrast: true,
        };
        assert_eq!(active_theme(&s, Some(hc), &themes), HIGH_CONTRAST_THEME);
        // Off: the fixed theme, whatever the OS says.
        s.follow_system = false;
        s.theme = "graphite".into();
        assert_eq!(
            active_theme(&s, Some(SystemAppearance::LIGHT), &themes),
            "graphite"
        );
        assert_eq!(active_theme(&s, Some(hc), &themes), "graphite");
    }

    #[test]
    fn broken_or_missing_choices_fall_back_by_kind() {
        let mut themes = builtins();
        themes.push(info("ocean", ThemeKind::Dark, false));
        themes.push(info("paper", ThemeKind::Light, true));
        let mut s = AppearanceSettings {
            follow_system: false,
            theme: "ocean".into(),
            ..AppearanceSettings::default()
        };
        assert_eq!(active_theme(&s, None, &themes), DEFAULT_DARK_THEME);
        s.theme = "gone".into();
        assert_eq!(active_theme(&s, None, &themes), DEFAULT_LIGHT_THEME);
        s.theme = "paper".into();
        assert_eq!(active_theme(&s, None, &themes), "paper");
        s.follow_system = true;
        s.dark = "ocean".into();
        assert_eq!(
            active_theme(&s, Some(SystemAppearance::DARK), &themes),
            DEFAULT_DARK_THEME
        );
        // No usable high-contrast theme: the scheme decides.
        themes.retain(|t| t.id != HIGH_CONTRAST_THEME);
        let hc = SystemAppearance {
            scheme: ColorScheme::Light,
            high_contrast: true,
        };
        assert_eq!(active_theme(&s, Some(hc), &themes), "frost");
    }

    #[test]
    fn override_issues() {
        assert!(ThemeOverrides::default().issues().is_empty());
        let ok = ThemeOverrides {
            font_size: Some(15),
            radius: Some(0),
            accent: Some("#3B5bdb".into()),
        };
        assert!(ok.issues().is_empty() && !ok.is_empty());
        let bad = ThemeOverrides {
            font_size: Some(30),
            radius: Some(21),
            accent: Some("blue".into()),
        };
        assert_eq!(
            bad.issues(),
            vec![
                ThemeOverridesIssue::FontSizeRange,
                ThemeOverridesIssue::RadiusRange,
                ThemeOverridesIssue::AccentInvalid
            ]
        );
        for c in ["#abc", "#A1B2C3"] {
            assert!(is_hex_color(c), "{c}");
        }
        for c in ["abc", "#abcd", "#ggg", "#", "rgb(0 0 0)"] {
            assert!(!is_hex_color(c), "{c}");
        }
    }

    #[test]
    fn wire_shapes() {
        let s = AppearanceSettings::default();
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "theme": "frost", "follow_system": true, "light": "frost",
                "dark": "graphite", "density": "comfortable", "overrides": {}
            })
        );
        let o: ThemeOverrides = serde_json::from_value(serde_json::json!({"radius": 8})).unwrap();
        assert_eq!(o.radius, Some(8));
        let sys = serde_json::to_value(SystemAppearance::DARK).unwrap();
        assert_eq!(
            sys,
            serde_json::json!({"scheme": "dark", "high_contrast": false})
        );
    }

    #[test]
    fn token_schema_is_on_the_grid_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for t in token_schema() {
            assert!(seen.insert((t.category, t.name)), "{t:?}");
        }
        for t in SPACE_TOKENS {
            if t.name != "xxs" {
                assert!(
                    (t.value % 4.0).abs() < f32::EPSILON,
                    "{t:?} is off the 4 px grid"
                );
            }
        }
        assert!(token_schema().any(|t| t.category == "font-size" && t.name == "body" && t.density));
        assert!(token_schema().any(|t| t.category == "space" && t.name == "row" && t.density));
    }

    #[test]
    fn manual_source_notifies_on_change_only() {
        let source = ManualSystemAppearance::new(Some(SystemAppearance::LIGHT));
        assert_eq!(source.current(), Some(SystemAppearance::LIGHT));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let handle = source.watch(Arc::new(move |a| {
            assert_eq!(a, SystemAppearance::DARK);
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        source.set(SystemAppearance::DARK);
        source.set(SystemAppearance::DARK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        source.unwatch(handle);
        source.set(SystemAppearance::LIGHT);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(format!("{source:?}").contains("listeners: 0"));
    }
}
