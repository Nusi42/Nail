//! `theme.kdl`: the file model, the KDL reader, merging onto the defaults and
//! resolution into [`ResolvedTheme`].

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use kdl::KdlNode;
use nail_api::{
    HtmlDarkMode, Icon, LocalizedMessage, MessageDisplay, PartRule, RawCss, ResolvedTheme,
    SourceLocation, ThemeMode, TokenSet,
};
use schemars::JsonSchema;

use crate::css::{self, TokenRef};
use crate::error::ConfigError;
use crate::kdl_util::{
    self as k, Errors, Located, Source, arg_str, args, check_max_args, check_no_children,
    check_props, children, is_dotted_kebab, is_kebab, prop_str, scalar_to_string, sort_errors,
};
use crate::l10n::msg;
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
    /// Reads a theme file without merging or resolving it. Returns the
    /// first error; [`Self::parse_all`] returns all of them.
    pub fn parse(text: &str, file: &str) -> Result<Self, ConfigError> {
        Self::parse_all(text, file).map_err(|mut errors| errors.swap_remove(0))
    }

    /// Reads a theme file without merging or resolving it, reporting every
    /// error of the file in file order: a bad item is recorded and skipped,
    /// and reading goes on with the next one.
    pub fn parse_all(text: &str, file: &str) -> Result<Self, Vec<ConfigError>> {
        let src = Source::new(file, text);
        let doc = src.parse_all()?;
        let root = single_root(&src, &doc, "theme").map_err(|e| vec![e])?;
        let errors = Errors::default();
        let theme = ThemeReader {
            src: &src,
            errors: &errors,
        }
        .theme(root);
        errors.finish(theme)
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
            msg("config-file-empty").arg("root", expected),
        ));
    };
    for node in nodes {
        if k::name(node) != expected {
            return Err(src.node_err(
                node,
                msg("config-top-level-unexpected")
                    .arg("node", k::name(node))
                    .arg("root", expected),
            ));
        }
    }
    if let Some(second) = nodes.get(1) {
        return Err(src.node_err(second, msg("config-root-duplicate").arg("root", expected)));
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
            msg("config-extends-unknown")
                .arg("base", base)
                .arg("default", DEFAULT_BASE),
        )),
    }
}

struct ThemeReader<'s, 'a> {
    src: &'s Source<'a>,
    errors: &'s Errors,
}

impl ThemeReader<'_, '_> {
    fn theme(&self, node: &KdlNode) -> ThemeFile {
        let src = self.src;
        let e = self.errors;
        e.ok(check_props(src, node, &["extends", "mode"]));
        e.ok(check_max_args(src, node, 1));
        let name = match e.ok(arg_str(src, node, 0)) {
            Some((name, entry)) => src.located(entry, name.to_owned()),
            None => Located::new(String::new(), src.node_loc(node)),
        };
        let mode = e
            .ok(prop_str(src, node, "mode"))
            .flatten()
            .and_then(|(m, entry)| {
                e.ok(ModeSetting::parse(m).ok_or_else(|| {
                    src.entry_err(entry, msg("config-mode-unknown").arg("found", m))
                }))
            });
        let mut file = ThemeFile {
            name,
            extends: e.ok(read_extends(src, node)).flatten(),
            mode,
            tokens: IndexMap::new(),
            parts: Vec::new(),
            icons: IndexMap::new(),
            message: MessageDef::default(),
            sounds: IndexMap::new(),
            raw_css: Vec::new(),
        };
        for child in children(node) {
            e.ok(self.section(&mut file, child));
        }
        file
    }

    fn section(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        match k::name(node) {
            "tokens" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for token in children(node) {
                    self.errors.ok(self.token(file, token));
                }
            }
            "modes" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for mode in children(node) {
                    self.errors.ok(self.mode_block(file, mode));
                }
            }
            "part" => file.parts.extend(self.part(node)?),
            "parts" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for part in children(node) {
                    let part =
                        expect_name(src, part, "part", "parts").and_then(|()| self.part(part));
                    file.parts.extend(self.errors.ok(part).flatten());
                }
            }
            "icon" => self.icon(file, node)?,
            "icons" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                for icon in children(node) {
                    let icon = expect_name(src, icon, "icon", "icons")
                        .and_then(|()| self.icon(file, icon));
                    self.errors.ok(icon);
                }
            }
            "message" => self.message(&mut file.message, node)?,
            "sounds" => self.sounds(file, node)?,
            "raw-css" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 2)?;
                check_no_children(src, node)?;
                let (selector, sel_entry) = arg_str(src, node, 0)?;
                if !is_dotted_kebab(selector) {
                    return Err(src.entry_err(
                        sel_entry,
                        msg("config-part-name-invalid").arg("name", selector),
                    ));
                }
                let (css_text, css_entry) = arg_str(src, node, 1)?;
                file.raw_css.push(RawCssDef {
                    selector: selector.to_owned(),
                    css: src.located(css_entry, css_text.to_owned()),
                });
            }
            other => {
                return Err(
                    src.node_err(node, msg("config-theme-unknown-section").arg("node", other))
                );
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
        let (name, name_entry) = arg_str(src, node, 0)?;
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
                msg("config-token-needs-value-example")
                    .arg("category", category)
                    .arg("name", name),
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
                msg("config-mode-override-mode-unknown").arg("found", mode),
            ));
        }
        check_props(src, node, &[])?;
        check_max_args(src, node, 0)?;
        for token in children(node) {
            self.errors.ok(self.mode_token(file, mode, token));
        }
        Ok(())
    }

    /// `color "bg" "#000"` inside a mode block of `modes`.
    fn mode_token(
        &self,
        file: &mut ThemeFile,
        mode: &str,
        token: &KdlNode,
    ) -> Result<(), ConfigError> {
        let src = self.src;
        let category = token_category(src, token)?;
        check_props(src, token, &[])?;
        check_max_args(src, token, 2)?;
        check_no_children(src, token)?;
        let (name, name_entry) = arg_str(src, token, 0)?;
        check_token_name(src, name_entry, name)?;
        let entry = *args(token).get(1).ok_or_else(|| {
            src.node_err(
                token,
                msg("config-token-needs-value").arg("token", format!("{category}.{name}")),
            )
        })?;
        let value = self.token_value(category, entry)?;
        file.tokens
            .entry(category.to_owned())
            .or_default()
            .entry(name.to_owned())
            .or_default()
            .modes
            .insert(mode.to_owned(), value);
        Ok(())
    }

    fn token_value(
        &self,
        category: &str,
        entry: &kdl::KdlEntry,
    ) -> Result<Located<String>, ConfigError> {
        let src = self.src;
        let mut value = scalar_to_string(src, entry, &format!("`{category}`"))?;
        if !entry.value().is_string() && css::PX_TOKEN_CATEGORIES.contains(&category) {
            value = css::px(&value);
        }
        css::check_value_safety(&value).map_err(|msg| src.entry_err(entry, msg))?;
        Ok(src.located(entry, value))
    }

    /// A `part` rule. `Ok(None)` when a declaration was invalid: its error
    /// is recorded (every bad declaration is reported) and the rule skipped.
    fn part(&self, node: &KdlNode) -> Result<Option<PartDef>, ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 1)?;
        let (selector, sel_entry) = arg_str(src, node, 0)?;
        parse_selector(selector).map_err(|msg| src.entry_err(sel_entry, msg))?;
        let mut declarations = IndexMap::new();
        let mut failed = false;
        for decl in children(node) {
            match self.declaration(decl) {
                Ok((property, value)) => {
                    declarations.insert(property, value);
                }
                Err(err) => {
                    self.errors.push(err);
                    failed = true;
                }
            }
        }
        if failed {
            return Ok(None);
        }
        if declarations.is_empty() {
            return Err(src.node_err(node, msg("config-part-empty").arg("selector", selector)));
        }
        Ok(Some(PartDef {
            selector: src.located(sel_entry, selector.to_owned()),
            declarations,
        }))
    }

    /// One CSS declaration of a part rule.
    fn declaration(&self, decl: &KdlNode) -> Result<(String, Located<String>), ConfigError> {
        let src = self.src;
        let property = k::name(decl);
        if !css::PROPERTIES.contains(&property) {
            return Err(src.node_err(
                decl,
                msg("config-css-property-unsupported").arg("property", property),
            ));
        }
        check_props(src, decl, &[])?;
        check_no_children(src, decl)?;
        let values = args(decl);
        if values.is_empty() {
            return Err(src.node_err(decl, msg("config-needs-value").arg("node", property)));
        }
        let mut parts = Vec::with_capacity(values.len());
        for entry in &values {
            let mut v = scalar_to_string(src, entry, &format!("`{property}`"))?;
            if !entry.value().is_string() && css::is_length_property(property) {
                v = css::px(&v);
            }
            css::check_value_safety(&v).map_err(|msg| src.entry_err(entry, msg))?;
            parts.push(v);
        }
        Ok((property.to_owned(), src.located(values[0], parts.join(" "))))
    }

    fn icon(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["svg", "view-box"])?;
        check_max_args(src, node, 1)?;
        check_no_children(src, node)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        if !is_kebab(name) {
            return Err(src.entry_err(
                name_entry,
                msg("config-icon-name-invalid").arg("name", name),
            ));
        }
        let (svg, svg_entry) = prop_str(src, node, "svg")?
            .ok_or_else(|| src.node_err(node, msg("config-icon-needs-svg").arg("name", name)))?;
        if let Some(c) = svg.chars().find(|c| !is_path_char(*c)) {
            return Err(src.entry_err(
                svg_entry,
                msg("config-svg-character").arg("character", c.to_string()),
            ));
        }
        let view_box = match prop_str(src, node, "view-box")? {
            None => None,
            Some((vb, entry)) => {
                let numbers: Vec<_> = vb.split_whitespace().collect();
                if numbers.len() != 4 || numbers.iter().any(|n| n.parse::<f64>().is_err()) {
                    return Err(src.entry_err(entry, msg("config-view-box-invalid")));
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
            self.errors.ok(self.message_field(message, field));
        }
        Ok(())
    }

    /// One setting of the `message` section.
    fn message_field(&self, message: &mut MessageDef, field: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, field, &[])?;
        check_no_children(src, field)?;
        let key = k::name(field);
        let values = args(field);
        let Some(first) = values.first() else {
            return Err(src.node_err(field, msg("config-needs-value").arg("node", key)));
        };
        if key != "quote-colors" {
            check_max_args(src, field, 1)?;
        }
        let single = |what: &str| -> Result<Located<String>, ConfigError> {
            let v = scalar_to_string(src, first, &format!("`{what}`"))?;
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
                    let v = k::entry_str(src, entry, "`quote-colors`")?;
                    css::check_value_safety(v).map_err(|msg| src.entry_err(entry, msg))?;
                    colors.push(src.located(entry, v.to_owned()));
                }
                message.quote_colors = Some(colors);
            }
            "html-dark-mode" => {
                let v = k::entry_str(src, first, "`html-dark-mode`")?;
                message.html_dark_mode = Some(match v {
                    "off" => HtmlDarkModeSetting::Off,
                    "invert" => HtmlDarkModeSetting::Invert,
                    "smart" => HtmlDarkModeSetting::Smart,
                    _ => {
                        return Err(src.entry_err(
                            first,
                            msg("config-html-dark-mode-unknown").arg("found", v),
                        ));
                    }
                });
            }
            other => {
                return Err(src.node_err(
                    field,
                    msg("config-message-setting-unknown").arg("node", other),
                ));
            }
        }

        Ok(())
    }

    fn sounds(&self, file: &mut ThemeFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 0)?;
        for sound in children(node) {
            self.errors.ok(self.sound(file, sound));
        }
        Ok(())
    }

    /// One `<event> "<sound>"` entry of `sounds`.
    fn sound(&self, file: &mut ThemeFile, sound: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        let event = k::name(sound);
        if !is_dotted_kebab(event) {
            return Err(src.node_err(sound, msg("config-sound-event-invalid").arg("event", event)));
        }
        check_props(src, sound, &[])?;
        check_max_args(src, sound, 1)?;
        check_no_children(src, sound)?;
        let (value, entry) = arg_str(src, sound, 0)?;
        let bad = value.is_empty()
            || value.contains("..")
            || value.contains(':')
            || value.starts_with(['/', '\\']);
        if bad {
            return Err(src.entry_err(entry, msg("config-sound-invalid")));
        }
        file.sounds.insert(event.to_owned(), value.to_owned());

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
            msg("config-section-only")
                .arg("section", section)
                .arg("expected", expected)
                .arg("found", k::name(node)),
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
            msg("config-token-category-unknown-node")
                .arg("category", category)
                .arg("known", css::TOKEN_CATEGORIES.join(", ")),
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
        Err(src.entry_err(entry, msg("config-token-name-invalid").arg("name", name)))
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
    /// Resolves a complete (already merged) theme file. Returns the first
    /// error; [`Self::resolve_all`] returns all of them.
    pub fn resolve(&self) -> Result<ResolvedTheme, ConfigError> {
        self.resolve_unsorted()
            .map_err(|mut errors| errors.swap_remove(0))
    }

    /// Resolves a complete (already merged) theme file, reporting every
    /// error (unknown token references, cycles, missing message settings,
    /// invalid raw CSS) in file order.
    pub fn resolve_all(&self) -> Result<ResolvedTheme, Vec<ConfigError>> {
        self.resolve_unsorted().map_err(|mut errors| {
            sort_errors(&mut errors);
            errors
        })
    }

    /// All resolution errors in the order they are found (tokens, parts,
    /// message, raw CSS); the first one is what [`Self::resolve`] reports.
    fn resolve_unsorted(&self) -> Result<ResolvedTheme, Vec<ConfigError>> {
        let mut errors: Vec<ConfigError> = Vec::new();
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
                    errors.push(ConfigError::invalid(
                        &loc,
                        msg("config-mode-override-unknown")
                            .arg("token", format!("{category}.{name}")),
                    ));
                    continue;
                };
                let key = (category.clone(), name.clone());
                let (base_value, refs) = match resolve(base) {
                    Ok(resolved) => resolved,
                    Err(err) => {
                        errors.push(err);
                        continue;
                    }
                };
                let node_edges = edges.entry(key).or_default();
                node_edges.extend(refs.into_iter().map(|r| (r, base.loc.clone())));
                for (mode, value) in &def.modes {
                    let (mode_value, refs) = match resolve(value) {
                        Ok(resolved) => resolved,
                        Err(err) => {
                            errors.push(err);
                            continue;
                        }
                    };
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
        errors.extend(check_token_cycles(&self.tokens, &edges));

        // Parts; rules with an identical selector are merged.
        let mut parts: Vec<PartRule> = Vec::new();
        let mut index: HashMap<SelectorKey, usize> = HashMap::new();
        for def in &self.parts {
            let sel = match parse_selector(&def.selector.value) {
                Ok(sel) => sel,
                Err(msg) => {
                    errors.push(ConfigError::invalid(&def.selector.loc, msg));
                    continue;
                }
            };
            let mut declarations = IndexMap::new();
            for (property, value) in &def.declarations {
                match resolve_str(value) {
                    Ok(v) => {
                        declarations.insert(property.clone(), v);
                    }
                    Err(err) => errors.push(err),
                }
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
                msg("config-message-missing").arg("field", field),
            )
        };
        let mut required = |value: &Option<Located<String>>, field: &str| -> String {
            match value
                .as_ref()
                .ok_or_else(|| missing(field))
                .and_then(resolve_str)
            {
                Ok(v) => v,
                Err(err) => {
                    errors.push(err);
                    String::new()
                }
            }
        };
        let font_family = required(&m.font_family, "font-family");
        let text_font_family = required(&m.text_font_family, "text-font-family");
        let font_size = required(&m.font_size, "font-size");
        let line_height = required(&m.line_height, "line-height");
        let max_width = required(&m.max_width, "max-width");
        let mut quote_colors = Vec::new();
        match &m.quote_colors {
            None => errors.push(missing("quote-colors")),
            Some(colors) => {
                for color in colors {
                    match resolve_str(color) {
                        Ok(v) => quote_colors.push(v),
                        Err(err) => errors.push(err),
                    }
                }
            }
        }
        let html_dark_mode = m.html_dark_mode.map_or_else(
            || {
                errors.push(missing("html-dark-mode"));
                HtmlDarkMode::Off
            },
            Into::into,
        );
        let message = MessageDisplay {
            font_family,
            text_font_family,
            font_size,
            line_height,
            max_width,
            quote_colors,
            html_dark_mode,
        };

        let mut raw_css = Vec::new();
        for def in &self.raw_css {
            match resolve_raw_css(def, &resolve_str) {
                Ok(css) => raw_css.push(css),
                Err(err) => errors.push(err),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
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

/// Resolves and checks one `raw-css` block.
fn resolve_raw_css(
    def: &RawCssDef,
    resolve_str: &dyn Fn(&Located<String>) -> Result<String, ConfigError>,
) -> Result<RawCss, ConfigError> {
    let css_text = resolve_str(&def.css)?;
    let err = |msg: LocalizedMessage| ConfigError::invalid(&def.css.loc, msg);
    css::check_no_remote(&css_text).map_err(err)?;
    if let Some(c) = css_text
        .chars()
        .find(|c| matches!(c, '{' | '}' | '<' | '>' | '\\'))
    {
        return Err(err(
            msg("config-raw-css-character").arg("character", c.to_string())
        ));
    }
    Ok(RawCss {
        selector: def.selector.clone(),
        css: css_text,
    })
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

/// Finds token definitions that (indirectly) refer to themselves; one
/// error per cycle.
fn check_token_cycles(
    tokens: &IndexMap<String, IndexMap<String, TokenDef>>,
    edges: &HashMap<TokenKey, Vec<(TokenRef, SourceLocation)>>,
) -> Vec<ConfigError> {
    fn visit(
        key: &TokenKey,
        edges: &HashMap<TokenKey, Vec<(TokenRef, SourceLocation)>>,
        done: &mut HashSet<TokenKey>,
        path: &mut Vec<TokenKey>,
        errors: &mut Vec<ConfigError>,
    ) {
        if done.contains(key) {
            return;
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
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-token-cycle").arg("chain", chain.join(" → ")),
                ));
                continue;
            }
            visit(&target, edges, done, path, errors);
        }
        path.pop();
        done.insert(key.clone());
    }

    let mut errors = Vec::new();
    let mut done = HashSet::new();
    for (category, defs) in tokens {
        for name in defs.keys() {
            visit(
                &(category.clone(), name.clone()),
                edges,
                &mut done,
                &mut Vec::new(),
                &mut errors,
            );
        }
    }
    errors
}
