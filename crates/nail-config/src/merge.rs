//! The merge dialog: applying someone else's theme or layout without
//! silently overwriting one's own (design.md, "Applying someone else's
//! theme or layout").
//!
//! Both files are compared as they take effect: a sparse file merged onto
//! the shipped defaults, a complete file as it is. The comparison is split
//! into items (a token, a part selector, an icon, a window, an action, a key
//! binding, a toolbar, …) grouped by section. For every item the user keeps
//! theirs ("mine") or takes the incoming version ("theirs"); items without a
//! choice use a default side.
//!
//! The result is written as one file: the user's file edited in place
//! ([`crate::kdl_edit`]), so everything kept stays byte for byte, and each
//! taken item is replaced by the incoming version in canonical form. Taking
//! every differing item gives the incoming text verbatim.
//!
//! Conflicts are flagged per item: two actions on one key (or a key that is
//! a prefix of another key sequence), actions and widgets that are not
//! installed (plug-ins), placements that would drop plug-in items the user
//! has, token references that only the incoming theme defines, and items a
//! sparse file cannot remove because the defaults define them.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use kdl::{KdlNode, KdlValue};
use nail_api::customize::{
    ConfigMergePreview, MergeChange, MergeChoice, MergeConflict, MergeConflictKind, MergeItem,
    MergeSection, MergeSectionPreview, MergeTake,
};
use nail_api::{
    ConfigFile, KeyBinding, LayoutNode, LocalizedMessage, MenuItem, ResolvedLayout, ResolvedTheme,
    ToolbarItem,
};

use crate::error::ConfigError;
use crate::kdl_edit::{
    Style, first_arg, remove_node, remove_prop, replace_rendered, set_first_arg, set_prop,
};
use crate::l10n::msg;
use crate::render;
use crate::theme::ThemeFile;
use crate::write_back::{EditError, LayoutEditor, NEW_LAYOUT};
use crate::{LAYOUT_FILE, LayoutFile, THEME_FILE, parse_layout_all, parse_theme_all};

/// The file written when there is no `theme.kdl` yet.
pub(crate) const NEW_THEME: &str = "// Changes made in Nail. See the theme reference in the user documentation.\ntheme \"custom\" extends=\"default\" {\n}\n";

/// The result of a merge: the text to write and what it resolves to.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeOutcome {
    /// The merged file text.
    pub text: String,
    /// The resolved theme (theme merges).
    pub theme: Option<ResolvedTheme>,
    /// The resolved layout (layout merges).
    pub layout: Option<ResolvedLayout>,
}

/// Compares an incoming theme or layout with the user's file (`None` = no
/// file, i.e. the defaults). Errors of the incoming file are reported in the
/// preview (without sections); an invalid file of the user's is an
/// [`EditError::Config`].
pub fn preview_merge(
    file: ConfigFile,
    mine: Option<&str>,
    incoming: &str,
) -> Result<ConfigMergePreview, EditError> {
    let plan = match Plan::build(file, mine, incoming)? {
        Ok(plan) => plan,
        Err(errors) => {
            return Ok(ConfigMergePreview {
                file,
                sections: Vec::new(),
                errors: crate::error::issues(file.file_name(), &errors),
            });
        }
    };
    let mut sections: Vec<MergeSectionPreview> = Vec::new();
    for section in plan.section_order() {
        let items: Vec<MergeItem> = plan
            .items
            .iter()
            .filter(|item| item.item.section() == *section)
            .map(|item| MergeItem {
                id: item.item.id(),
                key: item.item.label(),
                change: item.change(),
                mine: item.mine.clone(),
                theirs: item.theirs.clone(),
                conflicts: item.conflicts.clone(),
            })
            .collect();
        if !items.is_empty() {
            sections.push(MergeSectionPreview {
                section: *section,
                items,
            });
        }
    }
    Ok(ConfigMergePreview {
        file,
        sections,
        errors: Vec::new(),
    })
}

/// Merges with the given choices (items without one use `default`) and
/// resolves the result, without writing anything.
pub fn resolve_merge(
    file: ConfigFile,
    mine: Option<&str>,
    incoming: &str,
    choices: &[MergeChoice],
    default: MergeTake,
) -> Result<MergeOutcome, EditError> {
    let plan = Plan::build(file, mine, incoming)?.map_err(|errors| {
        EditError::Invalid(
            msg("config-merge-incoming-invalid")
                .arg("file", file.file_name())
                .arg("count", errors.len())
                .arg("error", errors[0].to_string()),
        )
    })?;
    let text = plan.merged_text(mine, incoming, choices, default)?;
    let name = file.file_name();
    let invalid = |errors: Vec<ConfigError>| EditError::Config(errors);
    Ok(match file {
        ConfigFile::Theme => MergeOutcome {
            theme: Some(parse_theme_all(&text, name).map_err(invalid)?),
            layout: None,
            text,
        },
        _ => MergeOutcome {
            layout: Some(parse_layout_all(&text, name).map_err(invalid)?),
            theme: None,
            text,
        },
    })
}

// ---------------------------------------------------------------------------
// Items

/// One mergeable item.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Item {
    Name,
    Mode,
    Token(String, String),
    Part(String),
    Icon(String),
    Setting(String),
    Sound(String),
    RawCss(String),
    Window(String),
    Action(String),
    Key(String, String),
    Toolbar(String),
    Menu(String),
}

impl Item {
    fn section(&self) -> MergeSection {
        match self {
            Self::Name => MergeSection::Header,
            Self::Mode => MergeSection::Modes,
            Self::Token(..) => MergeSection::Tokens,
            Self::Part(_) => MergeSection::Parts,
            Self::Icon(_) => MergeSection::Icons,
            Self::Setting(_) | Self::Sound(_) => MergeSection::Message,
            Self::RawCss(_) => MergeSection::RawCss,
            Self::Window(_) => MergeSection::Windows,
            Self::Action(_) => MergeSection::Actions,
            Self::Key(..) => MergeSection::Keys,
            Self::Toolbar(_) => MergeSection::Toolbars,
            Self::Menu(_) => MergeSection::Menus,
        }
    }

    /// Display key: token name, selector, window name, `context key`, …
    fn label(&self) -> String {
        match self {
            Self::Name => "name".to_owned(),
            Self::Mode => "mode".to_owned(),
            Self::Token(category, name) => format!("{category}.{name}"),
            Self::Sound(event) => format!("sounds.{event}"),
            Self::Key(context, key) => format!("{context}: {key}"),
            Self::Part(s)
            | Self::Icon(s)
            | Self::Setting(s)
            | Self::RawCss(s)
            | Self::Window(s)
            | Self::Action(s)
            | Self::Toolbar(s)
            | Self::Menu(s) => s.clone(),
        }
    }

    /// Stable id within a preview: `<section>:<key>`.
    fn id(&self) -> String {
        let section = serde_json::to_value(self.section())
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let kind = match self {
            Self::Sound(_) => "sound:",
            Self::Setting(_) => "setting:",
            _ => "",
        };
        format!("{section}:{kind}{}", self.label())
    }
}

/// Canonical text (depth 0) of every item of one side, in file order.
type Items = IndexMap<Item, String>;

fn theme_items(file: &ThemeFile) -> Items {
    let style = Style::default();
    let mut items = Items::new();
    items.insert(Item::Name, format!("theme {}", quote(&file.name.value)));
    if let Some(mode) = file.mode {
        items.insert(Item::Mode, format!("mode={}", quote(mode_name(mode))));
    }
    for (category, defs) in &file.tokens {
        for (name, def) in defs {
            items.insert(
                Item::Token(category.clone(), name.clone()),
                render::token(category, name, def, 0, &style),
            );
        }
    }
    for (selector, declarations) in theme_parts(file) {
        let text = render::part(&selector, &declarations, 0, &style);
        items.insert(Item::Part(selector), text);
    }
    for (name, def) in &file.icons {
        items.insert(Item::Icon(name.clone()), render::icon(name, def, 0, &style));
    }
    for (key, values) in theme_settings(file) {
        let text = render::setting(key, &values, 0, &style);
        items.insert(Item::Setting(key.to_owned()), text);
    }
    for (event, sound) in &file.sounds {
        items.insert(
            Item::Sound(event.clone()),
            render::setting(event, std::slice::from_ref(sound), 0, &style),
        );
    }
    for (selector, css) in theme_raw_css(file) {
        let text: String = css
            .iter()
            .map(|c| render::raw_css(&selector, c, 0, &style))
            .collect();
        items.insert(Item::RawCss(selector), text);
    }
    items
}

fn mode_name(mode: crate::theme::ModeSetting) -> &'static str {
    use crate::theme::ModeSetting;
    match mode {
        ModeSetting::System => "system",
        ModeSetting::Light => "light",
        ModeSetting::Dark => "dark",
        ModeSetting::HighContrast => "high-contrast",
    }
}

fn quote(s: &str) -> String {
    crate::accounts::write::quote(s)
}

/// Part rules grouped by selector (later declarations win).
fn theme_parts(file: &ThemeFile) -> IndexMap<String, IndexMap<String, String>> {
    let mut parts: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    for part in &file.parts {
        let entry = parts
            .entry(part.selector.value.trim().to_owned())
            .or_default();
        for (property, value) in &part.declarations {
            entry.insert(property.clone(), value.value.clone());
        }
    }
    parts
}

/// `raw-css` blocks grouped by part.
fn theme_raw_css(file: &ThemeFile) -> IndexMap<String, Vec<String>> {
    let mut raw: IndexMap<String, Vec<String>> = IndexMap::new();
    for def in &file.raw_css {
        raw.entry(def.selector.clone())
            .or_default()
            .push(def.css.value.clone());
    }
    raw
}

/// The `message` settings that are set, as `(key, values)`.
fn theme_settings(file: &ThemeFile) -> Vec<(&'static str, Vec<String>)> {
    use crate::theme::HtmlDarkModeSetting;
    let m = &file.message;
    let one = |v: &Option<crate::Located<String>>| v.as_ref().map(|l| vec![l.value.clone()]);
    let mut out = Vec::new();
    for (key, values) in [
        ("font-family", one(&m.font_family)),
        ("text-font-family", one(&m.text_font_family)),
        ("font-size", one(&m.font_size)),
        ("line-height", one(&m.line_height)),
        ("max-width", one(&m.max_width)),
        (
            "quote-colors",
            m.quote_colors
                .as_ref()
                .map(|c| c.iter().map(|l| l.value.clone()).collect()),
        ),
        (
            "html-dark-mode",
            m.html_dark_mode.map(|d| {
                vec![
                    match d {
                        HtmlDarkModeSetting::Off => "off",
                        HtmlDarkModeSetting::Invert => "invert",
                        HtmlDarkModeSetting::Smart => "smart",
                    }
                    .to_owned(),
                ]
            }),
        ),
    ] {
        if let Some(values) = values {
            out.push((key, values));
        }
    }
    out
}

/// Items of a layout; `keys` are its effective bindings.
fn layout_items(file: &LayoutFile, keys: &[KeyBinding]) -> Items {
    let style = Style::default();
    let mut items = Items::new();
    items.insert(Item::Name, format!("layout {}", quote(&file.name.value)));
    for (name, breakpoints) in &file.windows {
        if let Some(text) = render::window(name, breakpoints, 0, &style) {
            items.insert(Item::Window(name.clone()), text);
        }
    }
    for (id, def) in &file.actions {
        if let Some(text) = render::action(id, def, 0, &style) {
            items.insert(Item::Action(id.clone()), text);
        }
    }
    for binding in keys {
        if let Some(text) = render::bind(&binding.key, &binding.action, &binding.args, 0, &style) {
            items.insert(
                Item::Key(binding.context.clone(), binding.key.clone()),
                text,
            );
        }
    }
    for (name, toolbar) in &file.toolbars {
        if let Some(text) = render::toolbar(name, toolbar, 0, &style) {
            items.insert(Item::Toolbar(name.clone()), text);
        }
    }
    for (name, menu) in &file.menus {
        if let Some(text) = render::menu(name, menu, 0, &style) {
            items.insert(Item::Menu(name.clone()), text);
        }
    }
    items
}

/// The items a layout file names explicitly (bindings and `unbind`s
/// included), without the defaults.
fn layout_explicit(file: &LayoutFile) -> HashSet<Item> {
    let mut out = HashSet::new();
    out.insert(Item::Name);
    out.extend(file.windows.keys().cloned().map(Item::Window));
    out.extend(file.actions.keys().cloned().map(Item::Action));
    out.extend(
        file.keys
            .iter()
            .map(|k| Item::Key(k.context.clone(), k.key.clone())),
    );
    out.extend(file.toolbars.keys().cloned().map(Item::Toolbar));
    out.extend(file.menus.keys().cloned().map(Item::Menu));
    out
}

// ---------------------------------------------------------------------------
// The plan: both sides compared

struct PlannedItem {
    item: Item,
    mine: Option<String>,
    theirs: Option<String>,
    conflicts: Vec<MergeConflict>,
}

impl PlannedItem {
    fn change(&self) -> MergeChange {
        match (&self.mine, &self.theirs) {
            (None, Some(_)) => MergeChange::Added,
            (Some(_), None) => MergeChange::Removed,
            (Some(a), Some(b)) if a != b => MergeChange::Changed,
            _ => MergeChange::Unchanged,
        }
    }
}

enum Models {
    Theme {
        theirs: Box<ThemeFile>,
    },
    Layout {
        theirs: Box<LayoutFile>,
        theirs_keys: Vec<KeyBinding>,
    },
}

struct Plan {
    file: ConfigFile,
    /// Whether the user's file (or the file started for them) extends the
    /// defaults.
    sparse: bool,
    items: Vec<PlannedItem>,
    models: Models,
}

fn conflict(kind: MergeConflictKind, localized: LocalizedMessage) -> MergeConflict {
    MergeConflict {
        kind,
        message: crate::l10n::english(&localized),
        localized: Some(localized),
    }
}

impl Plan {
    /// `Ok(Err(errors))` when the incoming file is invalid.
    fn build(
        file: ConfigFile,
        mine: Option<&str>,
        incoming: &str,
    ) -> Result<Result<Self, Vec<ConfigError>>, EditError> {
        match file {
            ConfigFile::Theme => Self::build_theme(mine, incoming),
            ConfigFile::Layout => Self::build_layout(mine, incoming),
            other => Err(EditError::Invalid(
                msg("config-merge-unsupported").arg("file", other.file_name()),
            )),
        }
    }

    fn section_order(&self) -> &'static [MergeSection] {
        match self.file {
            ConfigFile::Theme => &[
                MergeSection::Header,
                MergeSection::Modes,
                MergeSection::Tokens,
                MergeSection::Parts,
                MergeSection::Icons,
                MergeSection::Message,
                MergeSection::RawCss,
            ],
            _ => &[
                MergeSection::Header,
                MergeSection::Windows,
                MergeSection::Actions,
                MergeSection::Keys,
                MergeSection::Toolbars,
                MergeSection::Menus,
            ],
        }
    }

    fn build_theme(
        mine: Option<&str>,
        incoming: &str,
    ) -> Result<Result<Self, Vec<ConfigError>>, EditError> {
        let mine_text = mine.unwrap_or(NEW_THEME);
        parse_theme_all(mine_text, THEME_FILE).map_err(EditError::Config)?;
        let mine_raw = ThemeFile::parse_all(mine_text, THEME_FILE).map_err(EditError::Config)?;
        let mine_model =
            crate::theme_file_merged(mine_text, THEME_FILE).map_err(EditError::Config)?;
        if let Err(errors) = parse_theme_all(incoming, THEME_FILE) {
            return Ok(Err(errors));
        }
        let theirs_raw = match ThemeFile::parse_all(incoming, THEME_FILE) {
            Ok(raw) => raw,
            Err(errors) => return Ok(Err(errors)),
        };
        let theirs_model = match crate::theme_file_merged(incoming, THEME_FILE) {
            Ok(model) => model,
            Err(errors) => return Ok(Err(errors)),
        };
        let sparse = mine_raw.extends.is_some();
        let mine_items = theme_items(&mine_model);
        let theirs_items = theme_items(&theirs_model);
        let explicit: HashSet<Item> = theme_items(&theirs_raw).into_keys().collect();
        let defaults: HashSet<Item> = theme_items(crate::default_theme_file())
            .into_keys()
            .collect();
        let mine_tokens: HashSet<(String, String)> = mine_items
            .keys()
            .filter_map(|i| match i {
                Item::Token(c, n) => Some((c.clone(), n.clone())),
                _ => None,
            })
            .collect();

        let mut items = plan_items(&mine_items, &theirs_items, &explicit);
        for item in &mut items {
            if item.theirs.is_none() && sparse && defaults.contains(&item.item) {
                item.conflicts.push(conflict(
                    MergeConflictKind::Invalid,
                    msg("config-merge-default-theme-item").arg("item", item.item.label()),
                ));
            }
            if let Some(text) = &item.theirs {
                for (category, name) in token_refs(text) {
                    let own = item.item == Item::Token(category.clone(), name.clone());
                    if !own && !mine_tokens.contains(&(category.clone(), name.clone())) {
                        item.conflicts.push(conflict(
                            MergeConflictKind::Invalid,
                            msg("config-merge-needs-token")
                                .arg("token", format!("${category}.{name}")),
                        ));
                    }
                }
            }
        }
        Ok(Ok(Self {
            file: ConfigFile::Theme,
            sparse,
            items,
            models: Models::Theme {
                theirs: Box::new(theirs_model),
            },
        }))
    }

    fn build_layout(
        mine: Option<&str>,
        incoming: &str,
    ) -> Result<Result<Self, Vec<ConfigError>>, EditError> {
        let mine_text = mine.unwrap_or(NEW_LAYOUT);
        let mine_resolved = parse_layout_all(mine_text, LAYOUT_FILE).map_err(EditError::Config)?;
        let mine_raw = LayoutFile::parse_all(mine_text, LAYOUT_FILE).map_err(EditError::Config)?;
        let mine_model =
            crate::layout_file_merged(mine_text, LAYOUT_FILE).map_err(EditError::Config)?;
        let theirs_resolved = match parse_layout_all(incoming, LAYOUT_FILE) {
            Ok(resolved) => resolved,
            Err(errors) => return Ok(Err(errors)),
        };
        let theirs_raw = match LayoutFile::parse_all(incoming, LAYOUT_FILE) {
            Ok(raw) => raw,
            Err(errors) => return Ok(Err(errors)),
        };
        let theirs_model = match crate::layout_file_merged(incoming, LAYOUT_FILE) {
            Ok(model) => model,
            Err(errors) => return Ok(Err(errors)),
        };
        let sparse = mine_raw.extends.is_some();
        let mine_items = layout_items(&mine_model, &mine_resolved.keys);
        let theirs_items = layout_items(&theirs_model, &theirs_resolved.keys);
        let explicit = layout_explicit(&theirs_raw);
        let defaults_file = crate::default_layout_file();
        let default_layout = crate::default_layout();
        let defaults: HashSet<Item> = layout_items(defaults_file, &default_layout.keys)
            .into_keys()
            .collect();

        // Actions the app knows: the defaults', the user's, and aliases the
        // incoming file defines. Plug-in actions are never installed (M1).
        let mut known: HashSet<String> = default_layout.actions.keys().cloned().collect();
        known.extend(mine_resolved.actions.keys().cloned());
        known.extend(
            theirs_model
                .actions
                .iter()
                .filter(|(_, def)| def.alias_of.is_some())
                .map(|(id, _)| id.clone()),
        );
        let is_known = |action: &str| !action.starts_with("plugin.") && known.contains(action);

        let mut items = plan_items(&mine_items, &theirs_items, &explicit);
        for item in &mut items {
            let mut conflicts = Vec::new();
            if item.theirs.is_none()
                && sparse
                && defaults.contains(&item.item)
                && !matches!(item.item, Item::Key(..))
            {
                conflicts.push(conflict(
                    MergeConflictKind::Invalid,
                    msg("config-merge-default-layout-item").arg("item", item.item.label()),
                ));
            }
            match &item.item {
                Item::Window(name) => {
                    let theirs_kinds = theirs_model
                        .windows
                        .get(name)
                        .map(|bps| widget_kinds(bps.iter().map(|b| &b.root)))
                        .unwrap_or_default();
                    for kind in theirs_kinds.iter().filter(|k| k.starts_with("plugin.")) {
                        conflicts.push(conflict(
                            MergeConflictKind::UnknownWidget,
                            msg("config-merge-widget-missing").arg("widget", kind.as_str()),
                        ));
                    }
                    let mine_kinds = mine_model
                        .windows
                        .get(name)
                        .map(|bps| widget_kinds(bps.iter().map(|b| &b.root)))
                        .unwrap_or_default();
                    let dropped: Vec<&String> = mine_kinds
                        .iter()
                        .filter(|k| k.starts_with("plugin.") && !theirs_kinds.contains(k))
                        .collect();
                    if item.change() != MergeChange::Unchanged && !dropped.is_empty() {
                        conflicts.push(dropped_conflict(&dropped));
                    }
                }
                Item::Toolbar(name) | Item::Menu(name) => {
                    let (theirs_actions, mine_actions) = if matches!(item.item, Item::Toolbar(_)) {
                        (
                            theirs_model.toolbars.get(name).map(|t| toolbar_actions(t)),
                            mine_model.toolbars.get(name).map(|t| toolbar_actions(t)),
                        )
                    } else {
                        (
                            theirs_model.menus.get(name).map(|m| menu_actions(m)),
                            mine_model.menus.get(name).map(|m| menu_actions(m)),
                        )
                    };
                    let theirs_actions = theirs_actions.unwrap_or_default();
                    for action in theirs_actions.iter().filter(|a| !is_known(a)) {
                        conflicts.push(unknown_action(action));
                    }
                    let mine_actions = mine_actions.unwrap_or_default();
                    let dropped: Vec<&String> = mine_actions
                        .iter()
                        .filter(|a| a.starts_with("plugin.") && !theirs_actions.contains(a))
                        .collect();
                    if item.change() != MergeChange::Unchanged && !dropped.is_empty() {
                        conflicts.push(dropped_conflict(&dropped));
                    }
                }
                Item::Action(id) => {
                    if let Some(def) = theirs_model.actions.get(id) {
                        if id.starts_with("plugin.") {
                            conflicts.push(unknown_action(id));
                        }
                        if let Some(target) = &def.alias_of
                            && !is_known(target)
                        {
                            conflicts.push(unknown_action(target));
                        }
                    }
                }
                Item::Key(context, key) => {
                    if let Some(binding) = theirs_resolved
                        .keys
                        .iter()
                        .find(|k| &k.context == context && &k.key == key)
                    {
                        if !is_known(&binding.action) {
                            conflicts.push(unknown_action(&binding.action));
                        }
                        conflicts.extend(key_conflicts(binding, &mine_resolved.keys));
                    }
                }
                _ => {}
            }
            item.conflicts = conflicts;
        }
        Ok(Ok(Self {
            file: ConfigFile::Layout,
            sparse,
            items,
            models: Models::Layout {
                theirs: Box::new(theirs_model),
                theirs_keys: theirs_resolved.keys,
            },
        }))
    }

    /// The merged file text for a set of choices.
    fn merged_text(
        &self,
        mine: Option<&str>,
        incoming: &str,
        choices: &[MergeChoice],
        default: MergeTake,
    ) -> Result<String, EditError> {
        let mut chosen: HashMap<&str, MergeTake> = HashMap::new();
        let ids: HashSet<String> = self.items.iter().map(|i| i.item.id()).collect();
        for choice in choices {
            if !ids.contains(&choice.item_id) {
                return Err(EditError::Invalid(
                    msg("config-merge-item-unknown").arg("item", choice.item_id.to_string()),
                ));
            }
            chosen.insert(&choice.item_id, choice.take);
        }
        let differing: Vec<&PlannedItem> = self
            .items
            .iter()
            .filter(|i| i.change() != MergeChange::Unchanged)
            .collect();
        let take = |item: &PlannedItem| {
            chosen
                .get(item.item.id().as_str())
                .copied()
                .unwrap_or(default)
        };
        let taken: Vec<&PlannedItem> = differing
            .iter()
            .copied()
            .filter(|i| take(i) == MergeTake::Theirs)
            .collect();
        if !differing.is_empty() && taken.len() == differing.len() {
            let mut text = incoming.to_owned();
            if !text.ends_with('\n') {
                text.push('\n');
            }
            return Ok(text);
        }
        let (skeleton, root) = match self.file {
            ConfigFile::Theme => (NEW_THEME, "theme"),
            _ => (NEW_LAYOUT, "layout"),
        };
        let mine_text = mine.unwrap_or(skeleton);
        if taken.is_empty() {
            return Ok(mine_text.to_owned());
        }
        let mut editor = LayoutEditor::open(mine_text, root, self.sparse)?;
        for item in taken {
            match &self.models {
                Models::Theme { theirs } => apply_theme_item(&mut editor, &item.item, theirs),
                Models::Layout {
                    theirs,
                    theirs_keys,
                } => apply_layout_item(&mut editor, &item.item, theirs, theirs_keys, self.sparse)?,
            }
        }
        Ok(editor.finish())
    }
}

/// Pairs both sides' items: the incoming file's order first, then items
/// only the user has. Unchanged items are listed only when the incoming
/// file names them explicitly.
fn plan_items(mine: &Items, theirs: &Items, explicit: &HashSet<Item>) -> Vec<PlannedItem> {
    let mut out = Vec::new();
    for (item, text) in theirs {
        let mine_text = mine.get(item).cloned();
        if mine_text.as_deref() == Some(text.as_str()) && !explicit.contains(item) {
            continue;
        }
        out.push(PlannedItem {
            item: item.clone(),
            mine: mine_text,
            theirs: Some(text.clone()),
            conflicts: Vec::new(),
        });
    }
    for (item, text) in mine {
        if !theirs.contains_key(item) {
            out.push(PlannedItem {
                item: item.clone(),
                mine: Some(text.clone()),
                theirs: None,
                conflicts: Vec::new(),
            });
        }
    }
    out
}

/// `$category.name` references in rendered text.
fn token_refs(text: &str) -> Vec<(String, String)> {
    let mut refs = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let rest = &text[i + 1..];
            let category: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            let after = &rest[category.len()..];
            if !category.is_empty()
                && let Some(after) = after.strip_prefix('.')
            {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                    .collect();
                if !name.is_empty() && !refs.contains(&(category.clone(), name.clone())) {
                    refs.push((category, name));
                }
            }
        }
        i += 1;
    }
    refs
}

fn widget_kinds<'a>(roots: impl Iterator<Item = &'a LayoutNode>) -> Vec<String> {
    fn walk(node: &LayoutNode, out: &mut Vec<String>) {
        if let LayoutNode::Widget { kind, .. } = node
            && !out.contains(kind)
        {
            out.push(kind.clone());
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, &mut out);
    }
    out
}

fn toolbar_actions(items: &[ToolbarItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            ToolbarItem::Action { action, .. } => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn menu_actions(items: &[MenuItem]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        match item {
            MenuItem::Action { action, .. } => out.push(action.clone()),
            MenuItem::Submenu { items, .. } => out.extend(menu_actions(items)),
            MenuItem::Separator => {}
        }
    }
    out
}

fn unknown_action(action: &str) -> MergeConflict {
    conflict(
        MergeConflictKind::UnknownAction,
        msg("config-merge-action-missing").arg("action", action),
    )
}

fn dropped_conflict(dropped: &[&String]) -> MergeConflict {
    let names: Vec<&str> = dropped.iter().map(|s| s.as_str()).collect();
    conflict(
        MergeConflictKind::DropsPluginItems,
        msg("config-merge-drops-plugin-items").arg("items", names.join(", ")),
    )
}

/// Two actions on one key: the user binds the same key in the same context
/// to something else, binds a sequence it starts (or a prefix of it) in a
/// related context, or hides an incoming global binding with a context one.
fn key_conflicts(theirs: &KeyBinding, mine: &[KeyBinding]) -> Vec<MergeConflict> {
    let mut out = Vec::new();
    for other in mine {
        let related_context = other.context == theirs.context
            || other.context == "global"
            || theirs.context == "global";
        if !related_context || other.action == theirs.action && other.args == theirs.args {
            continue;
        }
        let same = other.key == theirs.key && other.context == theirs.context;
        let prefix = other.key.starts_with(&format!("{} ", theirs.key))
            || theirs.key.starts_with(&format!("{} ", other.key));
        // A global binding of theirs that my context binding would hide.
        let shadow =
            other.key == theirs.key && theirs.context == "global" && other.context != "global";
        if same || prefix || shadow {
            out.push(conflict(
                MergeConflictKind::DuplicateKey,
                msg("config-merge-key-taken")
                    .arg("key", other.key.as_str())
                    .arg("action", other.action.as_str())
                    .arg("context", other.context.as_str()),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Writing taken items into the user's file

/// Where a node is in the user's file: `(None, i)` at the top level (depth
/// 1), `(Some(b), i)` the `i`-th child of the section block `b` (depth 2).
type Site = (Option<usize>, usize);

/// Top-level nodes and children of `<section>` blocks that match.
fn sites(
    editor: &mut LayoutEditor,
    section: Option<&str>,
    top: bool,
    matches: &dyn Fn(&KdlNode) -> bool,
) -> Vec<Site> {
    let body = editor.body();
    let mut out = Vec::new();
    for (b, node) in body.nodes().iter().enumerate() {
        if top && matches(node) {
            out.push((None, b));
        }
        if section.is_some_and(|s| node.name().value() == s && node.entries().is_empty()) {
            for (i, child) in node
                .children()
                .map_or(&[][..], kdl::KdlDocument::nodes)
                .iter()
                .enumerate()
            {
                if matches(child) {
                    out.push((Some(b), i));
                }
            }
        }
    }
    out
}

/// Removes nodes (block children first, then top-level nodes, each in
/// descending order so indexes stay valid).
fn remove_sites(editor: &mut LayoutEditor, mut sites: Vec<Site>) {
    let style = editor.style.clone();
    sites.sort_by(|a, b| {
        (
            a.0.is_none(),
            std::cmp::Reverse(a.0),
            std::cmp::Reverse(a.1),
        )
            .cmp(&(
                b.0.is_none(),
                std::cmp::Reverse(b.0),
                std::cmp::Reverse(b.1),
            ))
    });
    let body = editor.body();
    for (block, index) in sites {
        match block {
            Some(b) => remove_node(body.nodes_mut()[b].ensure_children(), index, &style),
            None => remove_node(body, index, &style),
        }
    }
}

/// Writes a taken item: when the user's file defines it exactly once, the
/// node is replaced in place (keeping its comments and position);
/// otherwise every definition is removed and `append` adds the new one.
/// `render` gets the depth of the node it replaces.
fn put(
    editor: &mut LayoutEditor,
    sites: Vec<Site>,
    render: &dyn Fn(usize) -> Option<String>,
    append: &dyn Fn(&mut LayoutEditor),
) {
    if let [(block, index)] = sites[..]
        && let Some(text) = render(if block.is_some() { 2 } else { 1 })
    {
        let body = editor.body();
        let parent = match block {
            Some(b) => body.nodes_mut()[b].ensure_children(),
            None => body,
        };
        replace_rendered(parent, index, &text);
        return;
    }
    remove_sites(editor, sites);
    append(editor);
}

fn named<'a>(kind: &'a str, name: &'a str) -> impl Fn(&KdlNode) -> bool + 'a {
    move |n: &KdlNode| n.name().value() == kind && first_arg(n) == Some(name)
}

fn apply_theme_item(editor: &mut LayoutEditor, item: &Item, theirs: &ThemeFile) {
    let style = editor.style.clone();
    match item {
        Item::Name => set_first_arg(
            editor.root_node(),
            KdlValue::String(theirs.name.value.clone()),
        ),
        Item::Mode => match theirs.mode {
            Some(mode) => set_prop(
                editor.root_node(),
                "mode",
                KdlValue::String(mode_name(mode).to_owned()),
            ),
            None => remove_prop(editor.root_node(), "mode"),
        },
        Item::Token(category, name) => {
            let is_token = named(category, name);
            // Mode overrides (`modes { dark { color "bg" … } }`) go: the
            // taken token carries its own mode values.
            let body = editor.body();
            for b in 0..body.nodes().len() {
                if body.nodes()[b].name().value() != "modes" {
                    continue;
                }
                let modes = body.nodes_mut()[b].ensure_children();
                for m in 0..modes.nodes().len() {
                    let block = modes.nodes_mut()[m].ensure_children();
                    for i in (0..block.nodes().len()).rev() {
                        if is_token(&block.nodes()[i]) {
                            remove_node(block, i, &style);
                        }
                    }
                }
            }
            let found = sites(editor, Some("tokens"), false, &is_token);
            let def = theirs.tokens.get(category).and_then(|t| t.get(name));
            match def {
                Some(def) => put(
                    editor,
                    found,
                    &|depth| Some(render::token(category, name, def, depth, &style)),
                    &|e| {
                        e.append_in_section(
                            "tokens",
                            &render::token(category, name, def, 2, &style),
                        )
                    },
                ),
                None => remove_sites(editor, found),
            }
        }
        Item::Part(selector) => {
            let is_part = |n: &KdlNode| {
                n.name().value() == "part" && first_arg(n).map(str::trim) == Some(selector.as_str())
            };
            let found = sites(editor, Some("parts"), true, &is_part);
            match theme_parts(theirs).get(selector) {
                Some(declarations) => put(
                    editor,
                    found,
                    &|depth| Some(render::part(selector, declarations, depth, &style)),
                    &|e| e.append(&render::part(selector, declarations, 1, &style)),
                ),
                None => remove_sites(editor, found),
            }
        }
        Item::Icon(name) => {
            let found = sites(editor, Some("icons"), true, &named("icon", name));
            match theirs.icons.get(name) {
                Some(def) => put(
                    editor,
                    found,
                    &|depth| Some(render::icon(name, def, depth, &style)),
                    &|e| e.append(&render::icon(name, def, 1, &style)),
                ),
                None => remove_sites(editor, found),
            }
        }
        Item::Setting(key) => {
            let found = sites(editor, Some("message"), false, &|n: &KdlNode| {
                n.name().value() == key
            });
            match theme_settings(theirs).into_iter().find(|(k, _)| k == key) {
                Some((_, values)) => put(
                    editor,
                    found,
                    &|depth| Some(render::setting(key, &values, depth, &style)),
                    &|e| e.append_in_section("message", &render::setting(key, &values, 2, &style)),
                ),
                None => remove_sites(editor, found),
            }
        }
        Item::Sound(event) => {
            let found = sites(editor, Some("sounds"), false, &|n: &KdlNode| {
                n.name().value() == event
            });
            match theirs.sounds.get(event) {
                Some(sound) => {
                    let line =
                        |depth| render::setting(event, std::slice::from_ref(sound), depth, &style);
                    put(editor, found, &|depth| Some(line(depth)), &|e| {
                        e.append_in_section("sounds", &line(2))
                    });
                }
                None => remove_sites(editor, found),
            }
        }
        Item::RawCss(selector) => {
            let found = sites(editor, None, true, &named("raw-css", selector));
            remove_sites(editor, found);
            if let Some(css) = theme_raw_css(theirs).get(selector) {
                let text: String = css
                    .iter()
                    .map(|c| render::raw_css(selector, c, 1, &style))
                    .collect();
                editor.append(&text);
            }
        }
        _ => {}
    }
}

fn apply_layout_item(
    editor: &mut LayoutEditor,
    item: &Item,
    theirs: &LayoutFile,
    theirs_keys: &[KeyBinding],
    sparse: bool,
) -> Result<(), EditError> {
    let style = editor.style.clone();
    let plain = || EditError::Invalid(msg("config-merge-not-kdl"));
    match item {
        Item::Name => set_first_arg(
            editor.root_node(),
            KdlValue::String(theirs.name.value.clone()),
        ),
        Item::Window(name) => {
            let taken = theirs.windows.get(name);
            if taken.is_none() && sparse {
                return Ok(()); // the defaults keep it (flagged in the preview)
            }
            editor.remove_window(name);
            if let Some(breakpoints) = taken {
                editor.append(&render::window(name, breakpoints, 1, &style).ok_or_else(plain)?);
            }
        }
        Item::Action(id) => {
            let taken = theirs.actions.get(id);
            if taken.is_none() && sparse {
                return Ok(());
            }
            let found = sites(editor, Some("actions"), true, &named("action", id));
            match taken {
                Some(def) => {
                    let line = |depth| render::action(id, def, depth, &style);
                    if line(1).is_none() {
                        return Err(plain());
                    }
                    put(editor, found, &line, &|e| {
                        e.append_in_section("actions", &line(2).unwrap_or_default())
                    });
                }
                None => remove_sites(editor, found),
            }
        }
        Item::Key(context, key) => {
            let binding = theirs_keys
                .iter()
                .find(|k| &k.context == context && &k.key == key);
            editor.set_binding(context, key, binding.map(|b| (&b.action, &b.args)))?;
        }
        Item::Toolbar(name) | Item::Menu(name) => {
            let is_toolbar = matches!(item, Item::Toolbar(_));
            let (kind, section) = if is_toolbar {
                ("toolbar", "toolbars")
            } else {
                ("menu", "menus")
            };
            let render_at = |depth: usize| {
                if is_toolbar {
                    theirs
                        .toolbars
                        .get(name)
                        .and_then(|items| render::toolbar(name, items, depth, &style))
                } else {
                    theirs
                        .menus
                        .get(name)
                        .and_then(|items| render::menu(name, items, depth, &style))
                }
            };
            let exists = if is_toolbar {
                theirs.toolbars.contains_key(name)
            } else {
                theirs.menus.contains_key(name)
            };
            if !exists && sparse {
                return Ok(());
            }
            let found = sites(editor, Some(section), true, &named(kind, name));
            if exists {
                let text = render_at(1).ok_or_else(plain)?;
                put(editor, found, &render_at, &|e| e.append(&text));
            } else {
                remove_sites(editor, found);
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_references_are_found() {
        assert_eq!(
            token_refs(
                "part \"x\" {\n    border \"1px solid $color.border-2\"\n    color \"$color.fg\"\n}"
            ),
            vec![
                ("color".to_owned(), "border-2".to_owned()),
                ("color".to_owned(), "fg".to_owned())
            ]
        );
        assert!(token_refs("price $5").is_empty());
    }

    #[test]
    fn ids_are_section_prefixed() {
        assert_eq!(
            Item::Token("color".into(), "bg".into()).id(),
            "tokens:color.bg"
        );
        assert_eq!(Item::Key("list".into(), "e".into()).id(), "keys:list: e");
        assert_eq!(
            Item::Sound("mail.new".into()).id(),
            "message:sound:sounds.mail.new"
        );
        assert_eq!(Item::RawCss("row".into()).id(), "raw-css:row");
    }

    #[test]
    fn key_conflicts_cover_same_key_prefix_and_global() {
        let bind = |context: &str, key: &str, action: &str| KeyBinding {
            context: context.into(),
            key: key.into(),
            action: action.into(),
            args: vec![],
        };
        let mine = vec![
            bind("list", "e", "mail.archive"),
            bind("global", "g i", "nav.inbox"),
            bind("reader", "x", "mail.delete"),
        ];
        assert_eq!(
            key_conflicts(&bind("list", "e", "mail.delete"), &mine).len(),
            1
        );
        assert_eq!(
            key_conflicts(&bind("list", "e", "mail.archive"), &mine).len(),
            0
        );
        assert_eq!(
            key_conflicts(&bind("list", "g", "mail.next"), &mine).len(),
            1
        );
        assert_eq!(
            key_conflicts(&bind("list", "x", "mail.next"), &mine).len(),
            0
        );
        assert_eq!(
            key_conflicts(&bind("global", "x", "mail.next"), &mine).len(),
            1
        );
    }
}
