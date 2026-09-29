//! `theme.kdl`: the file model, the KDL reader, merging onto the defaults and
//! resolution into [`ResolvedTheme`].

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use kdl::KdlNode;
use nail_api::{
    HtmlDarkMode, Icon, MessageDisplay, PartRule, RawCss, ResolvedTheme, SourceLocation, ThemeMode,
    TokenSet,
};
use schemars::JsonSchema;

use crate::css::{self, TokenRef};
use crate::error::ConfigError;
use crate::kdl_util::{
    self as k, Located, Source, arg_str, args, check_max_args, check_no_children, check_props,
    children, is_dotted_kebab, is_kebab, prop_str, scalar_to_string,
};
use crate::selector::{Selector, parse_selector};

/// The only base a file can extend in M0.
pub const DEFAULT_BASE: &str = "default";

/// Colour mode setting of a theme (`theme "x" mode="dark"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ModeSetting {
    /// Follow the operating system.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// Always high contrast.
    HighContrast,
}

impl ModeSetting {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "system" => Self::System,
            "light" => Self::Light,
            "dark" => Self::Dark,
            "high-contrast" => Self::HighContrast,
            _ => return None,
        })
    }
}

impl From<ModeSetting> for ThemeMode {
    fn from(mode: ModeSetting) -> Self {
        match mode {
            ModeSetting::System => Self::System,
            ModeSetting::Light => Self::Light,
            ModeSetting::Dark => Self::Dark,
            ModeSetting::HighContrast => Self::HighContrast,
        }
    }
}

/// How HTML mail is adapted in dark mode (`html-dark-mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HtmlDarkModeSetting {
    /// Show HTML mail as sent.
    Off,
    /// Invert colours.
    Invert,
    /// Recolour while keeping hues.
    Smart,
}

impl From<HtmlDarkModeSetting> for HtmlDarkMode {
    fn from(mode: HtmlDarkModeSetting) -> Self {
        match mode {
            HtmlDarkModeSetting::Off => Self::Off,
            HtmlDarkModeSetting::Invert => Self::Invert,
            HtmlDarkModeSetting::Smart => Self::Smart,
        }
    }
}

/// A `theme.kdl` file as written (possibly sparse).
///
/// KDL mapping: the file holds one node `theme "<name>" extends=… mode=… { … }`;
/// each property below is a child section of that node.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct ThemeFile {
    /// Theme name (`theme "<name>"`).
    pub name: Located<String>,
    /// Base theme to merge onto; only `"default"` exists. Without it the file
    /// must be complete.
    pub extends: Option<String>,
    /// Which colour mode to use (default: follow the OS).
    pub mode: Option<ModeSetting>,
    /// `tokens { <category> "<name>" <value> dark=… high-contrast=… }`:
    /// category → name → definition. Categories: color, font, font-size,
    /// font-weight, line-height, space, size, radius, border, shadow, motion,
    /// opacity, z-index. Unitless numbers get `px` for font-size, space,
    /// size and radius.
    #[serde(default)]
    pub tokens: IndexMap<String, IndexMap<String, TokenDef>>,
    /// `part "<selector>" { <css-property> <value>… }` rules, in order.
    #[serde(default)]
    pub parts: Vec<PartDef>,
    /// `icon "<name>" svg="<path data>" view-box="0 0 24 24"`.
    #[serde(default)]
    pub icons: IndexMap<String, IconDef>,
    /// `message { … }`: how received mail is displayed.
    #[serde(default)]
    pub message: MessageDef,
    /// `sounds { <event> "<sound>" }`, e.g. `mail.new "chime"`.
    #[serde(default)]
    pub sounds: IndexMap<String, String>,
    /// `raw-css "<part>" "<declarations>"` (unstable escape hatch).
    #[serde(default)]
    pub raw_css: Vec<RawCssDef>,
}

/// One token definition.
#[derive(Debug, Clone, PartialEq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct TokenDef {
    /// Base (light) value; absent when the file only overrides modes of a
    /// token defined elsewhere (`modes { dark { color "bg" "#000" } }`).
    pub value: Option<Located<String>>,
    /// Per-mode values: `light`, `dark`, `high-contrast`.
    #[serde(default)]
    pub modes: IndexMap<String, Located<String>>,
}

/// One part rule.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub struct PartDef {
    /// Selector, e.g. `"message-list.row:unread .subject"` or
    /// `"folder-tree.item[role=\"inbox\"]"`.
    pub selector: Located<String>,
    /// CSS declarations from the supported subset; values may reference
    /// tokens as `$category.name`.
    pub declarations: IndexMap<String, Located<String>>,
}

/// One icon.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct IconDef {
    /// SVG path data.
    pub svg: String,
    /// SVG viewBox (default `"0 0 24 24"`).
    pub view_box: Option<String>,
}

/// The `message` section; every field is optional in a sparse file.
#[derive(Debug, Clone, PartialEq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct MessageDef {
    /// Reader font family.
    pub font_family: Option<Located<String>>,
    /// Font family for plain-text bodies.
    pub text_font_family: Option<Located<String>>,
    /// Base font size.
    pub font_size: Option<Located<String>>,
    /// Line height.
    pub line_height: Option<Located<String>>,
    /// Maximum reading width, e.g. `"80ch"`.
    pub max_width: Option<Located<String>>,
    /// Quote colours per level (`quote-colors "$color.quote-1" …`).
    pub quote_colors: Option<Vec<Located<String>>>,
    /// Dark-mode handling of HTML mail.
    pub html_dark_mode: Option<HtmlDarkModeSetting>,
}

/// A `raw-css` block.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub struct RawCssDef {
    /// Part name the CSS is scoped to.
    pub selector: String,
    /// Declarations (no braces, no `url()`, no `@import`).
    pub css: Located<String>,
}

// ---------------------------------------------------------------------------
// Reading KDL

impl ThemeFile {
    /// Reads a theme file without merging or resolving it.
    pub fn parse(text: &str, file: &str) -> Result<Self, ConfigError> {
        let src = Source::new(file, text);
        let doc = src.parse()?;
        let root = single_root(&src, &doc, "theme")?;
        let reader = ThemeReader { src: &src };
        reader.theme(root)
    }

    /// Merges `overlay` (a sparse file extending this one) onto `self`.
    pub fn merge(mut self, overlay: ThemeFile) -> ThemeFile {
        self.name = overlay.name;
        self.extends = None;
        self.mode = overlay.mode.or(self.mode);
        for (category, tokens) in overlay.tokens {
            let target = self.tokens.entry(category).or_default();
            for (name, def) in tokens {
                match (def.value.is_some(), target.get_mut(&name)) {
                    (false, Some(existing)) => existing.modes.extend(def.modes),
                    _ => {
                        target.insert(name, def);
                    }
                }
            }
        }
        self.parts.extend(overlay.parts);
        self.icons.extend(overlay.icons);
        let (m, o) = (&mut self.message, overlay.message);
        m.font_family = o.font_family.or(m.font_family.take());
        m.text_font_family = o.text_font_family.or(m.text_font_family.take());
        m.font_size = o.font_size.or(m.font_size.take());
        m.line_height = o.line_height.or(m.line_height.take());
        m.max_width = o.max_width.or(m.max_width.take());
        m.quote_colors = o.quote_colors.or(m.quote_colors.take());
        m.html_dark_mode = o.html_dark_mode.or(m.html_dark_mode);
        self.sounds.extend(overlay.sounds);
        self.raw_css.extend(overlay.raw_css);
        self
    }
}

/// Returns the single top-level node, which must be called `expected`.
pub(crate) fn single_root<'d>(
    src: &Source<'_>,
    doc: &'d kdl::KdlDocument,
    expected: &str,
) -> Result<&'d KdlNode, ConfigError> {
    let nodes = doc.nodes();
    let Some(first) = nodes.first() else {
        return Err(ConfigError::invalid(
            &src.loc_at(0),
            format!("the file is empty; expected `{expected} \"<name>\" {{ … }}`"),
        ));
    };
    for node in nodes {
        if k::name(node) != expected {
            return Err(src.node_err(
                node,
                format!(
                    "unexpected `{}` at the top level; the file contains a single `{expected} \"<name>\" {{ … }}` node",
                    k::name(node)
                ),
            ));
        }
    }
    if let Some(second) = nodes.get(1) {
        return Err(src.node_err(second, format!("only one `{expected}` node is allowed")));
    }
    Ok(first)
}

/// Reads the `extends` property (only `"default"` is known).
pub(crate) fn read_extends(
    src: &Source<'_>,
    node: &KdlNode,
) -> Result<Option<String>, ConfigError> {
    match prop_str(src, node, "extends")? {
        None => Ok(None),
        Some((base, _)) if base == DEFAULT_BASE => Ok(Some(base.to_owned())),
        Some((base, entry)) => Err(src.entry_err(
            entry,
            format!("unknown base `{base}`: only extends=\"{DEFAULT_BASE}\" is supported"),
        )),
    }
}

struct ThemeReader<'s, 'a> {
    src: &'s Source<'a>,
}

impl ThemeReader<'_, '_> {
    fn theme(&self, node: &KdlNode) -> Result<ThemeFile, ConfigError> {
        let src = self.src;
        check_props(src, node, &["extends", "mode"])?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0, "the theme name")?;
        let mode = match prop_str(src, node, "mode")? {
            None => None,
            Some((m, entry)) => Some(ModeSetting::parse(m).ok_or_else(|| {
                src.entry_err(
                    entry,
                    format!("unknown mode `{m}` (use system, light, dark or high-contrast)"),
                )
            })?),
        };
        let mut file = ThemeFile {
            name: src.located(name_entry, name.to_owned()),
            extends: read_extends(src, node)?,
            mode,
            tokens: IndexMap::new(),
            parts: Vec::new(),
            icons: IndexMap::new(),
            message: MessageDef::default(),
            sounds: IndexMap::new(),
            raw_css: Vec::new(),
        };
        for child in children(node) {
            self.section(&mut file, child)?;
        }
        Ok(file)
    }

    fn section(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        match k::name(node) {
            "tokens" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for token in children(node) {
                    self.token(file, token)?;
                }
            }
            "modes" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for mode in children(node) {
                    self.mode_block(file, mode)?;
                }
            }
            "part" => file.parts.push(self.part(node)?),
            "parts" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for part in children(node) {
                    expect_name(src, part, "part", "parts")?;
                    file.parts.push(self.part(part)?);
                }
            }
            "icon" => self.icon(file, node)?,
            "icons" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for icon in children(node) {
                    expect_name(src, icon, "icon", "icons")?;
                    self.icon(file, icon)?;
                }
            }
            "message" => self.message(&mut file.message, node)?,
            "sounds" => self.sounds(file, node)?,
            "raw-css" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 2)?;
                check_no_children(src, node)?;
                let (selector, sel_entry) = arg_str(src, node, 0, "the part name")?;
                if !is_dotted_kebab(selector) {
                    return Err(src.entry_err(sel_entry, format!("invalid part name `{selector}`")));
                }
                let (css_text, css_entry) = arg_str(src, node, 1, "the CSS declarations")?;
                file.raw_css.push(RawCssDef {
                    selector: selector.to_owned(),
                    css: src.located(css_entry, css_text.to_owned()),
                });
            }
            other => {
                return Err(src.node_err(
                    node,
                    format!(
                        "unknown theme section `{other}` (expected tokens, modes, part, parts, icon, icons, message, sounds or raw-css)"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// `color "bg" "#fff" dark="#000"` inside `tokens`.
    fn token(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        let category = token_category(src, node)?;
        check_props(src, node, css::MODES)?;
        check_max_args(src, node, 2)?;
        check_no_children(src, node)?;
        let (name, name_entry) = arg_str(src, node, 0, "the token name")?;
        check_token_name(src, name_entry, name)?;

        let mut def = TokenDef::default();
        if let Some(entry) = args(node).get(1) {
            def.value = Some(self.token_value(category, entry)?);
        }
        for (mode, entry) in k::props(node) {
            let value = self.token_value(category, entry)?;
            if mode == "light" && def.value.is_none() {
                def.value = Some(value);
            } else {
                def.modes.insert(mode.to_owned(), value);
            }
        }
        if def.value.is_none() {
            return Err(src.node_err(
                node,
                format!(
                    "token `{category}.{name}` needs a value, e.g. `{category} \"{name}\" <value>`"
                ),
            ));
        }
        let tokens = file.tokens.entry(category.to_owned()).or_default();
        match tokens.get_mut(name) {
            // A `modes` block earlier in this file already named the token.
            Some(existing) if existing.value.is_none() => {
                existing.value = def.value;
                existing.modes.extend(def.modes);
            }
            _ => {
                tokens.insert(name.to_owned(), def);
            }
        }
        Ok(())
    }

    /// `dark { color "bg" "#000" }` inside `modes`.
    fn mode_block(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        let mode = k::name(node);
        if !css::MODES.contains(&mode) {
            return Err(src.node_err(
                node,
                format!("unknown mode `{mode}` (expected light, dark or high-contrast)"),
            ));
        }
        check_props(src, node, &[])?;
        check_max_args(src, node, 0)?;
        for token in children(node) {
            let category = token_category(src, token)?;
            check_props(src, token, &[])?;
            check_max_args(src, token, 2)?;
            check_no_children(src, token)?;
            let (name, name_entry) = arg_str(src, token, 0, "the token name")?;
            check_token_name(src, name_entry, name)?;
            let entry = *args(token).get(1).ok_or_else(|| {
                src.node_err(token, format!("token `{category}.{name}` needs a value"))
            })?;
            let value = self.token_value(category, entry)?;
            file.tokens
                .entry(category.to_owned())
                .or_default()
                .entry(name.to_owned())
                .or_default()
                .modes
                .insert(mode.to_owned(), value);
        }
        Ok(())
    }

    fn token_value(
        &self,
        category: &str,
        entry: &kdl::KdlEntry,
    ) -> Result<Located<String>, ConfigError> {
        let src = self.src;
        let mut value = scalar_to_string(src, entry, "a token value")?;
        if !entry.value().is_string() && css::PX_TOKEN_CATEGORIES.contains(&category) {
            value = css::px(&value);
        }
        css::check_value_safety(&value).map_err(|msg| src.entry_err(entry, msg))?;
        Ok(src.located(entry, value))
    }

    fn part(&self, node: &KdlNode) -> Result<PartDef, ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 1)?;
        let (selector, sel_entry) = arg_str(src, node, 0, "the part selector")?;
        parse_selector(selector).map_err(|msg| src.entry_err(sel_entry, msg))?;
        let mut declarations = IndexMap::new();
        for decl in children(node) {
            let property = k::name(decl);
            if !css::PROPERTIES.contains(&property) {
                return Err(src.node_err(
                    decl,
                    format!(
                        "unsupported CSS property `{property}` (see the theme reference for the supported subset; `raw-css` covers the rest)"
                    ),
                ));
            }
            check_props(src, decl, &[])?;
            check_no_children(src, decl)?;
            let values = args(decl);
            if values.is_empty() {
                return Err(src.node_err(decl, format!("`{property}` needs a value")));
            }
            let mut parts = Vec::with_capacity(values.len());
            for entry in &values {
                let mut v = scalar_to_string(src, entry, "a CSS value")?;
                if !entry.value().is_string() && css::is_length_property(property) {
                    v = css::px(&v);
                }
                css::check_value_safety(&v).map_err(|msg| src.entry_err(entry, msg))?;
                parts.push(v);
            }
            declarations.insert(property.to_owned(), src.located(values[0], parts.join(" ")));
        }
        if declarations.is_empty() {
            return Err(src.node_err(node, format!("part `{selector}` has no declarations")));
        }
        Ok(PartDef {
            selector: src.located(sel_entry, selector.to_owned()),
            declarations,
        })
    }

    fn icon(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["svg", "view-box"])?;
        check_max_args(src, node, 1)?;
        check_no_children(src, node)?;
        let (name, name_entry) = arg_str(src, node, 0, "the icon name")?;
        if !is_kebab(name) {
            return Err(src.entry_err(name_entry, format!("invalid icon name `{name}`")));
        }
        let (svg, svg_entry) = prop_str(src, node, "svg")?.ok_or_else(|| {
            src.node_err(node, format!("icon `{name}` needs svg=\"<path data>\""))
        })?;
        if let Some(c) = svg.chars().find(|c| !is_path_char(*c)) {
            return Err(src.entry_err(svg_entry, format!("`{c}` is not allowed in SVG path data")));
        }
        let view_box = match prop_str(src, node, "view-box")? {
            None => None,
            Some((vb, entry)) => {
                let numbers: Vec<_> = vb.split_whitespace().collect();
                if numbers.len() != 4 || numbers.iter().any(|n| n.parse::<f64>().is_err()) {
                    return Err(
                        src.entry_err(entry, "view-box must be four numbers, e.g. \"0 0 24 24\"")
                    );
                }
                Some(vb.to_owned())
            }
        };
        file.icons.insert(
            name.to_owned(),
            IconDef {
                svg: svg.to_owned(),
                view_box,
            },
        );
        Ok(())
    }

    fn message(&self, message: &mut MessageDef, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 0)?;
        for field in children(node) {
            check_props(src, field, &[])?;
            check_no_children(src, field)?;
            let key = k::name(field);
            let values = args(field);
            let Some(first) = values.first() else {
                return Err(src.node_err(field, format!("`{key}` needs a value")));
            };
            if key != "quote-colors" {
                check_max_args(src, field, 1)?;
            }
            let single = |what: &str| -> Result<Located<String>, ConfigError> {
                let v = scalar_to_string(src, first, what)?;
                css::check_value_safety(&v).map_err(|msg| src.entry_err(first, msg))?;
                Ok(src.located(first, v))
            };
            match key {
                "font-family" => message.font_family = Some(single("font-family")?),
                "text-font-family" => message.text_font_family = Some(single("text-font-family")?),
                "font-size" => {
                    let mut v = single("font-size")?;
                    if !first.value().is_string() {
                        v.value = css::px(&v.value);
                    }
                    message.font_size = Some(v);
                }
                "line-height" => message.line_height = Some(single("line-height")?),
                "max-width" => message.max_width = Some(single("max-width")?),
                "quote-colors" => {
                    let mut colors = Vec::new();
                    for entry in &values {
                        let v = k::entry_str(src, entry, "a quote colour")?;
                        css::check_value_safety(v).map_err(|msg| src.entry_err(entry, msg))?;
                        colors.push(src.located(entry, v.to_owned()));
                    }
                    message.quote_colors = Some(colors);
                }
                "html-dark-mode" => {
                    let v = k::entry_str(src, first, "html-dark-mode")?;
                    message.html_dark_mode = Some(match v {
                        "off" => HtmlDarkModeSetting::Off,
                        "invert" => HtmlDarkModeSetting::Invert,
                        "smart" => HtmlDarkModeSetting::Smart,
                        _ => {
                            return Err(src.entry_err(
                                first,
                                format!("unknown html-dark-mode `{v}` (use off, invert or smart)"),
                            ));
                        }
                    });
                }
                other => {
                    return Err(src.node_err(
                        field,
                        format!(
                            "unknown message setting `{other}` (expected font-family, text-font-family, font-size, line-height, max-width, quote-colors or html-dark-mode)"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn sounds(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 0)?;
        for sound in children(node) {
            let event = k::name(sound);
            if !is_dotted_kebab(event) {
                return Err(src.node_err(
                    sound,
                    format!("invalid event name `{event}`, e.g. mail.new"),
                ));
            }
            check_props(src, sound, &[])?;
            check_max_args(src, sound, 1)?;
            check_no_children(src, sound)?;
            let (value, entry) = arg_str(src, sound, 0, "the sound name or file")?;
            let bad = value.is_empty()
                || value.contains("..")
                || value.contains(':')
                || value.starts_with(['/', '\\']);
            if bad {
                return Err(src.entry_err(
                    entry,
                    "sounds must be a bundled sound name or a path inside the config directory",
                ));
            }
            file.sounds.insert(event.to_owned(), value.to_owned());
        }
        Ok(())
    }
}

fn expect_name(
    src: &Source<'_>,
    node: &KdlNode,
    expected: &str,
    section: &str,
) -> Result<(), ConfigError> {
    if k::name(node) == expected {
        Ok(())
    } else {
        Err(src.node_err(
            node,
            format!(
                "`{section}` may only contain `{expected}` nodes, found `{}`",
                k::name(node)
            ),
        ))
    }
}

fn token_category<'n>(src: &Source<'_>, node: &'n KdlNode) -> Result<&'n str, ConfigError> {
    let category = k::name(node);
    if css::TOKEN_CATEGORIES.contains(&category) {
        Ok(category)
    } else {
        Err(src.node_err(
            node,
            format!(
                "unknown token category `{category}` (expected one of {})",
                css::TOKEN_CATEGORIES.join(", ")
            ),
        ))
    }
}

fn check_token_name(
    src: &Source<'_>,
    entry: &kdl::KdlEntry,
    name: &str,
) -> Result<(), ConfigError> {
    let valid = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(src.entry_err(
            entry,
            format!("invalid token name `{name}`: use lowercase letters, digits and hyphens"),
        ))
    }
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            'M' | 'm'
                | 'Z'
                | 'z'
                | 'L'
                | 'l'
                | 'H'
                | 'h'
                | 'V'
                | 'v'
                | 'C'
                | 'c'
                | 'S'
                | 's'
                | 'Q'
                | 'q'
                | 'T'
                | 't'
                | 'A'
                | 'a'
                | 'e'
                | 'E'
                | ' '
                | ','
                | '.'
                | '-'
                | '+'
                | '\n'
                | '\t'
        )
}

// ---------------------------------------------------------------------------
// Resolution

impl ThemeFile {
    /// Resolves a complete (already merged) theme file.
    pub fn resolve(&self) -> Result<ResolvedTheme, ConfigError> {
        let defined = |cat: &str, name: &str| {
            self.tokens
                .get(cat)
                .and_then(|t| t.get(name))
                .is_some_and(|d| d.value.is_some())
        };
        let resolve = |value: &Located<String>| -> Result<(String, Vec<TokenRef>), ConfigError> {
            css::resolve_token_refs(&value.value, defined)
                .map_err(|msg| ConfigError::invalid(&value.loc, msg))
        };
        let resolve_str = |value: &Located<String>| resolve(value).map(|(s, _)| s);

        // Tokens and modes.
        let mut tokens = TokenSet::default();
        let mut modes: IndexMap<String, TokenSet> = IndexMap::new();
        let mut edges: HashMap<(String, String), Vec<(TokenRef, SourceLocation)>> = HashMap::new();
        for (category, defs) in &self.tokens {
            for (name, def) in defs {
                let Some(base) = &def.value else {
                    let loc = def.modes.values().next().map(|v| v.loc.clone());
                    let loc = loc.unwrap_or_else(|| self.name.loc.clone());
                    return Err(ConfigError::invalid(
                        &loc,
                        format!("mode override for unknown token `{category}.{name}`"),
                    ));
                };
                let key = (category.clone(), name.clone());
                let (base_value, refs) = resolve(base)?;
                let node_edges = edges.entry(key).or_default();
                node_edges.extend(refs.into_iter().map(|r| (r, base.loc.clone())));
                for (mode, value) in &def.modes {
                    let (mode_value, refs) = resolve(value)?;
                    node_edges.extend(refs.into_iter().map(|r| (r, value.loc.clone())));
                    if mode_value != base_value {
                        modes.entry(mode.clone()).or_default().insert(
                            category.as_str(),
                            name.as_str(),
                            mode_value,
                        );
                    }
                }
                tokens.insert(category.as_str(), name.as_str(), base_value);
            }
        }
        check_token_cycles(&self.tokens, &edges)?;

        // Parts; rules with an identical selector are merged.
        let mut parts: Vec<PartRule> = Vec::new();
        let mut index: HashMap<SelectorKey, usize> = HashMap::new();
        for def in &self.parts {
            let sel = parse_selector(&def.selector.value)
                .map_err(|msg| ConfigError::invalid(&def.selector.loc, msg))?;
            let mut declarations = IndexMap::new();
            for (property, value) in &def.declarations {
                declarations.insert(property.clone(), resolve_str(value)?);
            }
            let key = SelectorKey::from(&sel);
            if let Some(&i) = index.get(&key) {
                let rule = &mut parts[i];
                rule.declarations.extend(declarations);
                rule.source = Some(def.selector.loc.clone());
                continue;
            }
            index.insert(key, parts.len());
            parts.push(PartRule {
                selector: sel.selector,
                states: sel.states,
                conditions: sel.conditions,
                ancestors: sel.ancestors,
                declarations,
                source: Some(def.selector.loc.clone()),
            });
        }

        let icons = self
            .icons
            .iter()
            .map(|(name, def)| {
                (
                    name.clone(),
                    Icon {
                        path: def.svg.clone(),
                        view_box: def
                            .view_box
                            .clone()
                            .unwrap_or_else(|| "0 0 24 24".to_owned()),
                    },
                )
            })
            .collect();

        let m = &self.message;
        let missing = |field: &str| {
            ConfigError::invalid(
                &self.name.loc,
                format!("the theme's `message` section is missing `{field}`"),
            )
        };
        let required = |value: &Option<Located<String>>, field: &str| {
            value
                .as_ref()
                .ok_or_else(|| missing(field))
                .and_then(resolve_str)
        };
        let message = MessageDisplay {
            font_family: required(&m.font_family, "font-family")?,
            text_font_family: required(&m.text_font_family, "text-font-family")?,
            font_size: required(&m.font_size, "font-size")?,
            line_height: required(&m.line_height, "line-height")?,
            max_width: required(&m.max_width, "max-width")?,
            quote_colors: m
                .quote_colors
                .as_ref()
                .ok_or_else(|| missing("quote-colors"))?
                .iter()
                .map(resolve_str)
                .collect::<Result<_, _>>()?,
            html_dark_mode: m
                .html_dark_mode
                .ok_or_else(|| missing("html-dark-mode"))?
                .into(),
        };

        let mut raw_css = Vec::new();
        for def in &self.raw_css {
            let css_text = resolve_str(&def.css)?;
            let err = |msg: String| ConfigError::invalid(&def.css.loc, msg);
            css::check_no_remote(&css_text).map_err(err)?;
            if let Some(c) = css_text
                .chars()
                .find(|c| matches!(c, '{' | '}' | '<' | '>' | '\\'))
            {
                return Err(err(format!(
                    "the character `{c}` is not allowed in raw-css"
                )));
            }
            raw_css.push(RawCss {
                selector: def.selector.clone(),
                css: css_text,
            });
        }

        Ok(ResolvedTheme {
            name: self.name.value.clone(),
            mode: self.mode.map_or(ThemeMode::System, Into::into),
            tokens,
            modes,
            parts,
            icons,
            message,
            sounds: self.sounds.clone(),
            raw_css,
        })
    }
}

/// Identity of a part rule for merging: everything but the declarations.
#[derive(PartialEq, Eq, Hash)]
struct SelectorKey(String);

impl From<&Selector> for SelectorKey {
    fn from(sel: &Selector) -> Self {
        // Debug output is a faithful, deterministic encoding of the selector.
        Self(format!(
            "{:?}|{:?}|{:?}|{:?}",
            sel.selector, sel.states, sel.conditions, sel.ancestors
        ))
    }
}

type TokenKey = (String, String);

/// Rejects token definitions that (indirectly) refer to themselves.
fn check_token_cycles(
    tokens: &IndexMap<String, IndexMap<String, TokenDef>>,
    edges: &HashMap<TokenKey, Vec<(TokenRef, SourceLocation)>>,
) -> Result<(), ConfigError> {
    fn visit(
        key: &TokenKey,
        edges: &HashMap<TokenKey, Vec<(TokenRef, SourceLocation)>>,
        done: &mut HashSet<TokenKey>,
        path: &mut Vec<TokenKey>,
    ) -> Result<(), ConfigError> {
        if done.contains(key) {
            return Ok(());
        }
        path.push(key.clone());
        for (r, loc) in edges.get(key).into_iter().flatten() {
            let target = (r.category.clone(), r.name.clone());
            if let Some(start) = path.iter().position(|k| *k == target) {
                let chain: Vec<String> = path[start..]
                    .iter()
                    .chain(std::iter::once(&target))
                    .map(|(c, n)| format!("${c}.{n}"))
                    .collect();
                return Err(ConfigError::invalid(
                    loc,
                    format!("token reference cycle: {}", chain.join(" → ")),
                ));
            }
            visit(&target, edges, done, path)?;
        }
        path.pop();
        done.insert(key.clone());
        Ok(())
    }

    let mut done = HashSet::new();
    for (category, defs) in tokens {
        for name in defs.keys() {
            visit(
                &(category.clone(), name.clone()),
                edges,
                &mut done,
                &mut Vec::new(),
            )?;
        }
    }
    Ok(())
}
