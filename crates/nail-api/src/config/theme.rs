//! The resolved theme: the output of `nail-config` for `theme.kdl`, and the
//! input of the UI's theme compiler.
//!
//! "Resolved" means: `extends` has been merged onto the shipped defaults,
//! every value has been validated, and every token reference has been
//! rewritten to a CSS custom property. The renderer does no further
//! resolution beyond emitting CSS.
//!
//! # Token references
//!
//! In `theme.kdl` a value may reference a token as `$<category>.<name>`
//! (e.g. `"1px solid $color.border"`). In the resolved model every such
//! reference is rewritten to `var(--nail-<category>-<name>)` (see
//! [`css_var_name`]), both inside part declarations and inside other token
//! values. Mode switching therefore only swaps custom-property values; the
//! part stylesheet never changes with the mode.
//!
//! # Parts, states and conditions
//!
//! Every rendered element carries `data-part="<part name>"`. A [`PartRule`]
//! targets one part name and may additionally require:
//!
//! * **states** – interaction states map to CSS pseudo-classes
//!   ([`PSEUDO_CLASS_STATES`]: `hover`, `active`, `focus`, `focus-visible`,
//!   `focus-within`). Every other state (`selected`, `unread`, `flagged`,
//!   `dragover`, `disabled`, `expanded`, …) is an application state, which
//!   components expose as a whitespace-separated token in `data-state`
//!   (selector: `[data-state~="selected"]`).
//! * **conditions** – data conditions such as `[tag="work"]` or
//!   `[encrypted]`, which components expose as `data-<key>` attributes whose
//!   value is a whitespace-separated list (selector: `[data-tag~="work"]`, or
//!   `[data-encrypted]` when no value is given).
//! * **ancestors** – selectors that must match ancestor elements, outermost
//!   first (the KDL form `"message-list.row:unread .subject"` becomes the part
//!   `message-list.row.subject` with the ancestor `message-list.row:unread`).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::SourceLocation;

/// Prefix of every CSS custom property generated from theme tokens.
pub const CSS_VAR_PREFIX: &str = "--nail-";

/// States that map to CSS pseudo-classes instead of `data-state` tokens.
pub const PSEUDO_CLASS_STATES: &[&str] =
    &["hover", "active", "focus", "focus-visible", "focus-within"];

/// Returns the CSS custom property name for a token, e.g.
/// `css_var_name("color", "accent")` → `"--nail-color-accent"`.
pub fn css_var_name(category: &str, name: &str) -> String {
    format!("{CSS_VAR_PREFIX}{category}-{name}")
}

/// Which colour mode the theme wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    /// Follow the OS / browser (`prefers-color-scheme`, `prefers-contrast`).
    #[default]
    System,
    /// Always use the `light` mode overrides.
    Light,
    /// Always use the `dark` mode overrides.
    Dark,
    /// Always use the `high-contrast` mode overrides.
    HighContrast,
}

/// Design tokens grouped by category: `category → name → CSS value`
/// (serialized as a plain nested JSON object).
///
/// Known categories (the schema in `nail-config` is authoritative):
/// `color`, `font` (families), `font-size`, `font-weight`, `line-height`,
/// `space`, `size` (e.g. `touch-target`), `radius`, `border`, `shadow`,
/// `motion` (durations and easings), `opacity`, `z-index`.
///
/// Values are final CSS values: unitless numbers from KDL have already been
/// given their unit (px for `space`, `size`, `radius`), and token references
/// have been rewritten to `var(--nail-…)`. Category and token names are
/// lowercase kebab-case; order follows the source file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct TokenSet(pub IndexMap<String, IndexMap<String, String>>);

impl TokenSet {
    /// Looks up a token value.
    pub fn get(&self, category: &str, name: &str) -> Option<&str> {
        self.0.get(category)?.get(name).map(String::as_str)
    }

    /// Inserts or replaces a token value.
    pub fn insert(
        &mut self,
        category: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.0
            .entry(category.into())
            .or_default()
            .insert(name.into(), value.into());
    }

    /// Iterates over `(category, name, value)` triples in source order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.0.iter().flat_map(|(category, tokens)| {
            tokens
                .iter()
                .map(move |(name, value)| (category.as_str(), name.as_str(), value.as_str()))
        })
    }
}

/// A data condition on a part, e.g. `[tag="work"]` or `[encrypted]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct PartCondition {
    /// Condition key without the `data-` prefix, e.g. `"tag"`, `"account"`,
    /// `"priority"`, `"encrypted"`.
    pub key: String,
    /// Required value. `null` means "attribute present" (e.g. `[encrypted]`).
    /// A value matches when it is one of the whitespace-separated values of
    /// the element's `data-<key>` attribute.
    #[serde(default)]
    pub value: Option<String>,
}

/// A selector for an ancestor element of a [`PartRule`] target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AncestorSelector {
    /// Full part name of the ancestor, e.g. `"message-list.row"`.
    pub selector: String,
    /// Required states of the ancestor (see the module docs).
    #[serde(default)]
    pub states: Vec<String>,
    /// Required data conditions of the ancestor.
    #[serde(default)]
    pub conditions: Vec<PartCondition>,
}

/// One style rule of the theme's `parts` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PartRule {
    /// Full part name of the target element, e.g. `"message-list.row.subject"`.
    /// Part names are dot-separated, lowercase kebab-case segments.
    pub selector: String,
    /// Required states of the target, all of which must hold, e.g.
    /// `["selected", "hover"]`.
    #[serde(default)]
    pub states: Vec<String>,
    /// Required data conditions of the target, all of which must hold.
    #[serde(default)]
    pub conditions: Vec<PartCondition>,
    /// Ancestor constraints, outermost first. Empty for plain rules.
    #[serde(default)]
    pub ancestors: Vec<AncestorSelector>,
    /// CSS declarations `property → value`, from the validated property
    /// subset, in source order. Values contain no token syntax any more.
    pub declarations: IndexMap<String, String>,
    /// Where the rule was defined, for the inspector and error messages.
    #[serde(default)]
    pub source: Option<SourceLocation>,
}

/// An icon: SVG path data rendered inside an `<svg>` with the given viewBox.
/// Icons from bundled sets are resolved to path data by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Icon {
    /// SVG path data (`d` attribute), e.g. `"M3 3h18v4H3z"`.
    pub path: String,
    /// SVG viewBox, `"0 0 24 24"` unless the icon says otherwise.
    #[serde(default = "default_view_box")]
    pub view_box: String,
}

fn default_view_box() -> String {
    "0 0 24 24".to_owned()
}

/// How HTML mail is adapted when the UI is in a dark mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum HtmlDarkMode {
    /// Render the mail as sent (usually light).
    Off,
    /// Invert colours with a CSS filter (images are re-inverted).
    Invert,
    /// Recolour backgrounds and text while keeping hues.
    #[default]
    Smart,
}

/// How received mail is displayed in the reader (the theme's `message`
/// section). All values are CSS values; token references are already
/// rewritten to `var(--nail-…)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageDisplay {
    /// Font family for plain-text mail and the reader chrome.
    pub font_family: String,
    /// Font family for plain-text bodies (often monospace).
    pub text_font_family: String,
    /// Base font size, e.g. `"15px"`.
    pub font_size: String,
    /// Line height, e.g. `"1.5"`.
    pub line_height: String,
    /// Maximum width of the reading column, e.g. `"72ch"` or `"none"`.
    pub max_width: String,
    /// Colours for quote levels 1, 2, 3, … (cycled when deeper).
    pub quote_colors: Vec<String>,
    /// Dark-mode handling of HTML mail.
    pub html_dark_mode: HtmlDarkMode,
}

/// An escape-hatch CSS block scoped to one part (`raw-css`). Unstable across
/// versions. Validation in `nail-config` rejects `url()`, `@import` and other
/// constructs that could load remote content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RawCss {
    /// Full part name the CSS is scoped to.
    pub selector: String,
    /// Declarations block contents (without braces).
    pub css: String,
}

/// A fully resolved theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResolvedTheme {
    /// Theme name from `theme "<name>"`.
    pub name: String,
    /// Which mode to apply.
    #[serde(default)]
    pub mode: ThemeMode,
    /// Base token values, applied in every mode.
    pub tokens: TokenSet,
    /// Per-mode token overrides keyed by mode name (`"light"`, `"dark"`,
    /// `"high-contrast"`). Only tokens that differ from [`Self::tokens`] appear.
    #[serde(default)]
    pub modes: IndexMap<String, TokenSet>,
    /// Part style rules in source order (later rules win on equal specificity).
    #[serde(default)]
    pub parts: Vec<PartRule>,
    /// Icons by name, e.g. `"archive"`.
    #[serde(default)]
    pub icons: IndexMap<String, Icon>,
    /// Reader display settings.
    pub message: MessageDisplay,
    /// Notification sounds: event name (e.g. `"mail.new"`) → bundled sound
    /// name or path relative to the config directory. Empty = silent.
    #[serde(default)]
    pub sounds: IndexMap<String, String>,
    /// Escape-hatch CSS blocks, applied after all part rules.
    #[serde(default)]
    pub raw_css: Vec<RawCss>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn css_var_names() {
        assert_eq!(css_var_name("color", "accent"), "--nail-color-accent");
        assert_eq!(css_var_name("font-size", "sm"), "--nail-font-size-sm");
    }

    #[test]
    fn token_set_access_and_order() {
        let mut tokens = TokenSet::default();
        tokens.insert("color", "bg", "#fff");
        tokens.insert("space", "row", "4px");
        tokens.insert("color", "fg", "#000");
        assert_eq!(tokens.get("color", "fg"), Some("#000"));
        assert_eq!(tokens.get("color", "nope"), None);
        let all: Vec<_> = tokens.iter().collect();
        assert_eq!(
            all,
            vec![
                ("color", "bg", "#fff"),
                ("color", "fg", "#000"),
                ("space", "row", "4px")
            ]
        );
        assert_eq!(
            serde_json::to_value(&tokens).unwrap(),
            json!({"color": {"bg": "#fff", "fg": "#000"}, "space": {"row": "4px"}})
        );
    }

    #[test]
    fn part_rule_defaults() {
        let rule: PartRule = serde_json::from_value(json!({
            "selector": "message-list.row",
            "declarations": {"padding": "4px 8px"}
        }))
        .unwrap();
        assert!(rule.states.is_empty() && rule.conditions.is_empty() && rule.ancestors.is_empty());
        assert_eq!(rule.source, None);
    }

    #[test]
    fn icon_view_box_defaults() {
        let icon: Icon = serde_json::from_value(json!({"path": "M0 0h1v1z"})).unwrap();
        assert_eq!(icon.view_box, "0 0 24 24");
    }
}
