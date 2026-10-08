//! `layout.kdl`: the file model, the KDL reader, merging onto the defaults and
//! resolution into [`ResolvedLayout`].

use std::collections::HashSet;

use indexmap::IndexMap;
use kdl::{KdlEntry, KdlNode};
use nail_api::config::layout::used_actions;
use nail_api::views::{NAV_RAIL_WIDGET, SETTINGS_PAGE_WIDGET, VIEW_HOST_WIDGET};
use nail_api::windows::{WindowFrame, WindowInstances, WindowSize};
use nail_api::{
    ActionDef, ActionItem, BadgeSource, Breakpoint, ItemDisplay, KeyBinding, LayoutNode,
    LayoutPreset, LocalizedMessage, MenuItem, Orientation, PresetNode, RailItem, ResolvedLayout,
    SettingsGroup, SettingsSection, SourceLocation, ToolbarItem, ToolbarOverflow, ToolbarSettings,
    ViewDef,
};
use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::error::ConfigError;
use crate::kdl_util::{
    self as k, Errors, Located, Source, arg_str, args, check_max_args, check_no_children,
    check_props, children, describe, is_dotted_kebab, is_kebab, kebab_arg, kebab_prop, prop_str,
    prop_u32, sort_errors, to_json,
};
use crate::l10n::msg;
use crate::schema::{
    ActionSchema, BreakpointSchema, MenuItemSchema, PresetSchema, RailItemSchema,
    SettingsGroupSchema, ToolbarItemSchema, ToolbarSettingsSchema, ViewsSchema, WindowFrameSchema,
};
use crate::theme::{read_extends, read_root_file, root_name};
use crate::widgets::{self, OptionType};

/// The window-model properties a `window` node may carry (M5c, ADR 0116).
pub(crate) const WINDOW_FRAME_PROPS: &[&str] = &[
    "size",
    "min-size",
    "modal",
    "resizable",
    "pop-out",
    "instances",
];

/// Containers from the design that M0 does not support yet.
const FUTURE_CONTAINERS: &[&str] = &["grid", "dock", "scroll", "overlay"];

/// The `views` section (M5b): `views default="mail" { view "<id>" … }`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewsSection {
    /// `default=`: the view shown at start.
    pub default: Option<String>,
    /// The views by id, in file order.
    pub views: IndexMap<String, ViewDef>,
}

/// A `layout.kdl` file as written (possibly sparse).
///
/// KDL mapping: the file holds one node `layout "<name>" extends=… { … }`
/// whose children are `window`, `breakpoint`, `action(s)`, `keys`,
/// `toolbar(s)`, `menu(s)` and (M5b) `views`, `rail` and `settings-page`
/// nodes.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub struct LayoutFile {
    /// Layout name (`layout "<name>"`).
    pub name: Located<String>,
    /// Base layout to merge onto; only `"default"` exists. Without it the
    /// file must be complete.
    pub extends: Option<String>,
    /// Window trees: `window "<name>" { <node> }`, optionally inside
    /// `breakpoint min-width=… max-width=… { … }`. A window defined in a
    /// sparse file replaces all breakpoints of that window in the base.
    #[schemars(with = "IndexMap<String, Vec<BreakpointSchema>>")]
    #[serde(default)]
    pub windows: IndexMap<String, Vec<Breakpoint>>,
    /// `action "<id>" label=… icon=… alias-of=… <args>…`, merged field by
    /// field onto the base.
    #[schemars(with = "IndexMap<String, ActionSchema>")]
    #[serde(default)]
    pub actions: IndexMap<String, ActionDef>,
    /// `keys context="…" { bind "<key>" "<action> <args>…"; unbind "<key>" }`.
    #[serde(default)]
    pub keys: Vec<KeyDef>,
    /// `toolbar "<name>" { item "<action>"; spacer; separator }`; replaces a
    /// base toolbar of the same name.
    #[schemars(with = "IndexMap<String, Vec<ToolbarItemSchema>>")]
    #[serde(default)]
    pub toolbars: IndexMap<String, Vec<ToolbarItem>>,
    /// `menu "app" | "context:<part>" { item …; separator; submenu "<label>" { … } }`;
    /// replaces a base menu of the same name.
    #[schemars(with = "IndexMap<String, Vec<MenuItemSchema>>")]
    #[serde(default)]
    pub menus: IndexMap<String, Vec<MenuItem>>,
    /// The properties of each `toolbar` node (M5b: `hide-disabled=`,
    /// `overflow=`); replaced together with the toolbar.
    #[schemars(with = "IndexMap<String, ToolbarSettingsSchema>")]
    #[serde(default, rename = "toolbar-settings")]
    pub toolbar_settings: IndexMap<String, ToolbarSettings>,
    /// `views default="<id>" { view "<id>" label=… icon=… window=… badge=… }`
    /// (M5b); replaces the base's views as a whole.
    #[schemars(with = "Option<ViewsSchema>")]
    #[serde(default)]
    pub views: Option<ViewsSection>,
    /// `rail { view "<id>"; item "<action>" badge=…; spacer; separator }`
    /// (M5b); replaces the base's rail.
    #[schemars(with = "Option<Vec<RailItemSchema>>")]
    #[serde(default)]
    pub rail: Option<Vec<RailItem>>,
    /// `settings-page { group "<id>" label=… { section "<id>" label=… icon=…
    /// keywords=… { <nodes> } } }` (M5b); replaces the base's page.
    #[schemars(with = "Option<Vec<SettingsGroupSchema>>")]
    #[serde(default, rename = "settings-page")]
    pub settings_page: Option<Vec<SettingsGroup>>,
    /// The window-model properties of `window` nodes (M5c, ADR 0116:
    /// `size=`, `min-size=`, `modal=`, `resizable=`, `pop-out=`,
    /// `instances=`), merged over the window's nodes; a sparse file's
    /// properties override the base's field by field.
    #[schemars(with = "IndexMap<String, WindowFrameSchema>")]
    #[serde(default, rename = "window-frames")]
    pub window_frames: IndexMap<String, WindowFrame>,
    /// `presets { preset "<id>" window="<window>" label=… icon=… {
    /// <arrangement> } }` (M5d, ADR 0131): pane arrangements the layout
    /// editor offers; replaces the base's presets as a whole.
    #[schemars(with = "Option<Vec<PresetSchema>>")]
    #[serde(default)]
    pub presets: Option<Vec<LayoutPreset>>,
}

/// A `bind` or `unbind` entry.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub struct KeyDef {
    /// Context (`global` when the `keys` block has none).
    pub context: String,
    /// Key sequence, e.g. `"ctrl+shift+m"` or `"g i"`.
    pub key: String,
    /// Action id; `null` for `unbind`, which removes earlier bindings of the
    /// key in the context.
    pub action: Option<String>,
    /// Action arguments.
    #[serde(default)]
    pub args: Vec<Value>,
}

// ---------------------------------------------------------------------------
// Reading KDL

impl LayoutFile {
    /// Reads a layout file without merging or resolving it. Returns the
    /// first error; [`Self::parse_all`] returns all of them.
    pub fn parse(text: &str, file: &str) -> Result<Self, ConfigError> {
        Self::parse_all(text, file).map_err(|mut errors| errors.swap_remove(0))
    }

    /// Reads a layout file without merging or resolving it, reporting every
    /// error of the file in file order: a bad item is recorded and skipped,
    /// and reading goes on with the next one.
    pub fn parse_all(text: &str, file: &str) -> Result<Self, Vec<ConfigError>> {
        read_root_file(text, file, "layout", |src, errors, root| {
            LayoutReader { src, errors }.layout(root)
        })
    }

    /// Merges `overlay` (a sparse file extending this one) onto `self`.
    #[must_use]
    pub fn merge(mut self, overlay: LayoutFile) -> LayoutFile {
        self.name = overlay.name;
        self.extends = None;
        self.windows.extend(overlay.windows);
        // Window-model properties merge field by field, so a replaced window
        // without them (every pre-M5c file) keeps the base's sizes and
        // instances.
        for (name, frame) in overlay.window_frames {
            let base = self.window_frames.get(&name).copied().unwrap_or_default();
            self.window_frames.insert(name, base.merged(&frame));
        }
        for (id, def) in overlay.actions {
            let target = self.actions.entry(id).or_default();
            target.label = def.label.or(target.label.take());
            target.icon = def.icon.or(target.icon.take());
            target.alias_of = def.alias_of.or(target.alias_of.take());
            if !def.args.is_empty() {
                target.args = def.args;
            }
        }
        self.keys.extend(overlay.keys);
        for name in overlay.toolbars.keys() {
            // A replaced toolbar takes its settings from the overlay too.
            self.toolbar_settings.shift_remove(name);
        }
        self.toolbars.extend(overlay.toolbars);
        self.toolbar_settings.extend(overlay.toolbar_settings);
        self.menus.extend(overlay.menus);
        if overlay.views.is_some() {
            self.views = overlay.views;
        }
        if overlay.rail.is_some() {
            self.rail = overlay.rail;
        }
        if overlay.settings_page.is_some() {
            self.settings_page = overlay.settings_page;
        }
        if overlay.presets.is_some() {
            self.presets = overlay.presets;
        }
        self
    }
}

struct LayoutReader<'s, 'a> {
    src: &'s Source<'a>,
    errors: &'s Errors,
}

impl LayoutReader<'_, '_> {
    fn layout(&self, node: &KdlNode) -> LayoutFile {
        let src = self.src;
        let e = self.errors;
        let name = root_name(src, e, node, &["extends"]);
        let mut file = LayoutFile {
            name,
            extends: e.ok(read_extends(src, node)).flatten(),
            windows: IndexMap::new(),
            actions: IndexMap::new(),
            keys: Vec::new(),
            toolbars: IndexMap::new(),
            menus: IndexMap::new(),
            toolbar_settings: IndexMap::new(),
            views: None,
            rail: None,
            settings_page: None,
            window_frames: IndexMap::new(),
            presets: None,
        };
        for child in children(node) {
            e.ok(self.section(&mut file, child));
        }
        file
    }

    /// One top-level section; items inside a section are recovered one by
    /// one, so every bad item is reported.
    fn section(&self, file: &mut LayoutFile, child: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        let e = self.errors;
        match k::name(child) {
            "window" => self.window(file, child, None, None)?,
            "breakpoint" => self.breakpoint(file, child)?,
            "action" => self.action(file, child)?,
            "actions" => {
                self.block(child)?;
                for action in children(child) {
                    e.ok(self
                        .expect(action, "action", "actions")
                        .and_then(|()| self.action(file, action)));
                }
            }
            "keys" => self.keys(file, child)?,
            "toolbar" => self.toolbar(file, child)?,
            "toolbars" => {
                self.block(child)?;
                for toolbar in children(child) {
                    e.ok(self
                        .expect(toolbar, "toolbar", "toolbars")
                        .and_then(|()| self.toolbar(file, toolbar)));
                }
            }
            "menu" => self.menu(file, child)?,
            "menus" => {
                self.block(child)?;
                for menu in children(child) {
                    e.ok(self
                        .expect(menu, "menu", "menus")
                        .and_then(|()| self.menu(file, menu)));
                }
            }
            "views" => self.views(file, child)?,
            "rail" => self.rail(file, child)?,
            "settings-page" => self.settings_page(file, child)?,
            "presets" => self.presets(file, child)?,
            other => {
                return Err(src.node_err(
                    child,
                    msg("config-layout-unknown-section").arg("node", other),
                ));
            }
        }
        Ok(())
    }

    /// A plain section block: no arguments or properties.
    fn block(&self, node: &KdlNode) -> Result<(), ConfigError> {
        check_props(self.src, node, &[])?;
        check_max_args(self.src, node, 0)
    }

    fn expect(&self, node: &KdlNode, expected: &str, section: &str) -> Result<(), ConfigError> {
        if k::name(node) == expected {
            Ok(())
        } else {
            Err(self.src.node_err(
                node,
                msg("config-section-only")
                    .arg("section", section)
                    .arg("expected", expected)
                    .arg("found", k::name(node)),
            ))
        }
    }

    fn breakpoint(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["min-width", "max-width"])?;
        check_max_args(src, node, 0)?;
        let min = prop_u32(src, node, "min-width")?;
        let max = prop_u32(src, node, "max-width")?;
        if let (Some(lo), Some(hi)) = (min, max)
            && lo > hi
        {
            return Err(src.node_err(
                node,
                msg("config-breakpoint-range").arg("min", lo).arg("max", hi),
            ));
        }
        let windows = children(node);
        if windows.is_empty() {
            return Err(src.node_err(node, msg("config-breakpoint-empty")));
        }
        for window in windows {
            self.errors.ok(self
                .expect(window, "window", "breakpoint")
                .and_then(|()| self.window(file, window, min, max)));
        }
        Ok(())
    }

    fn window(
        &self,
        file: &mut LayoutFile,
        node: &KdlNode,
        min_width: Option<u32>,
        max_width: Option<u32>,
    ) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, WINDOW_FRAME_PROPS)?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        if !is_kebab(name) {
            return Err(src.entry_err(
                name_entry,
                msg("config-window-name-invalid").arg("name", name),
            ));
        }
        let frame = self.window_frame(node)?;
        let merged = file
            .window_frames
            .get(name)
            .copied()
            .unwrap_or_default()
            .merged(&frame);
        if let Some(issue) = merged.issue() {
            return Err(src.node_err(node, msg(&issue.message_id()).arg("name", name)));
        }
        if !merged.is_empty() {
            file.window_frames.insert(name.to_owned(), merged);
        }
        let [root] = children(node) else {
            return Err(src.node_err(node, msg("config-window-one-root").arg("name", name)));
        };
        let root_node = root;
        let root = self.node(root_node)?;
        self.check_unique_ids(root_node, &mut HashSet::new())?;
        file.windows
            .entry(name.to_owned())
            .or_default()
            .push(Breakpoint {
                min_width,
                max_width,
                root,
            });
        Ok(())
    }

    /// The window-model properties of a `window` node (M5c).
    fn window_frame(&self, node: &KdlNode) -> Result<WindowFrame, ConfigError> {
        let src = self.src;
        let size = |key: &str| -> Result<Option<WindowSize>, ConfigError> {
            match prop_str(src, node, key)? {
                None => Ok(None),
                Some((value, entry)) => WindowSize::parse(value).map(Some).ok_or_else(|| {
                    src.entry_err(
                        entry,
                        msg("config-window-size-invalid")
                            .arg("key", key)
                            .arg("found", describe(entry.value())),
                    )
                }),
            }
        };
        Ok(WindowFrame {
            modal: self.bool_prop(node, "modal")?,
            resizable: self.bool_prop(node, "resizable")?,
            min_size: size("min-size")?,
            size: size("size")?,
            pop_out: self.bool_prop(node, "pop-out")?,
            instances: match prop_str(src, node, "instances")? {
                None => None,
                Some((value, entry)) => Some(WindowInstances::parse(value).ok_or_else(|| {
                    src.entry_err(
                        entry,
                        msg("config-option-one-of")
                            .arg("key", "instances")
                            .arg("choices", "single, multiple")
                            .arg("found", describe(entry.value())),
                    )
                })?),
            },
        })
    }

    /// Checks that ids are unique within one window tree (the tree is
    /// already validated, so every `id` is a string).
    fn check_unique_ids(
        &self,
        node: &KdlNode,
        seen: &mut HashSet<String>,
    ) -> Result<(), ConfigError> {
        if let Some(entry) = node.entry("id")
            && let Some(id) = entry.value().as_string()
            && !seen.insert(id.to_owned())
        {
            return Err(self
                .src
                .entry_err(entry, msg("config-id-duplicate").arg("id", id)));
        }
        for child in children(node) {
            self.check_unique_ids(child, seen)?;
        }
        Ok(())
    }

    /// Common `id` / `label` properties.
    fn id_label(&self, node: &KdlNode) -> Result<(Option<String>, Option<String>), ConfigError> {
        let src = self.src;
        let id = kebab_prop(src, node, "id")?;
        let label = match prop_str(src, node, "label")? {
            None => None,
            Some((label, entry)) => Some(self.label(label, entry)?),
        };
        Ok((id, label))
    }

    /// `hidden=#true|#false` on any node (M5d, ADR 0131); absent = shown.
    fn hidden(&self, node: &KdlNode) -> Result<bool, ConfigError> {
        let src = self.src;
        match node.entry("hidden") {
            None => Ok(false),
            Some(entry) => entry.value().as_bool().ok_or_else(|| {
                src.entry_err(
                    entry,
                    msg("config-expected-bool")
                        .arg("what", "hidden")
                        .arg("found", describe(entry.value())),
                )
            }),
        }
    }

    fn label(&self, label: &str, entry: &KdlEntry) -> Result<String, ConfigError> {
        if let Some(id) = label.strip_prefix('@') {
            let valid = id.starts_with(|c: char| c.is_ascii_alphabetic())
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            if !valid {
                return Err(self
                    .src
                    .entry_err(entry, msg("config-label-invalid").arg("label", label)));
            }
        }
        Ok(label.to_owned())
    }

    fn node(&self, node: &KdlNode) -> Result<LayoutNode, ConfigError> {
        let src = self.src;
        match k::name(node) {
            "split" => {
                check_props(
                    src,
                    node,
                    &[
                        "id",
                        "label",
                        "orientation",
                        "sizes",
                        "min-sizes",
                        "max-sizes",
                        "collapsible",
                        "resizable",
                        "hidden",
                    ],
                )?;
                check_max_args(src, node, 1)?;
                let (id, label) = self.id_label(node)?;
                let orientation_entry = args(node)
                    .first()
                    .copied()
                    .or_else(|| node.entry("orientation"));
                let orientation = match orientation_entry {
                    None => Orientation::Horizontal,
                    Some(entry) => orientation(src, entry)?,
                };
                let children = self.children(node)?;
                let n = children.len();
                let sizes = self.list(node, "sizes", n)?.unwrap_or_default();
                let optional = |key: &str| -> Result<Vec<Option<String>>, ConfigError> {
                    Ok(self
                        .list(node, key, n)?
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| (s != "none").then_some(s))
                        .collect())
                };
                let min_sizes = optional("min-sizes")?;
                let max_sizes = optional("max-sizes")?;
                let collapsible = match self.list(node, "collapsible", n)? {
                    None => Vec::new(),
                    Some(flags) => flags
                        .iter()
                        .map(|f| match f.as_str() {
                            "true" => Ok(true),
                            "false" => Ok(false),
                            _ => Err(src.node_err(node, msg("config-collapsible-invalid"))),
                        })
                        .collect::<Result<_, _>>()?,
                };
                let resizable = match node.entry("resizable") {
                    None => true,
                    Some(entry) => entry.value().as_bool().ok_or_else(|| {
                        src.entry_err(
                            entry,
                            msg("config-expected-bool")
                                .arg("what", "resizable")
                                .arg("found", describe(entry.value())),
                        )
                    })?,
                };
                Ok(LayoutNode::Split {
                    id,
                    label,
                    orientation,
                    sizes,
                    min_sizes,
                    max_sizes,
                    collapsible,
                    resizable,
                    hidden: self.hidden(node)?,
                    children,
                })
            }
            "stack" => {
                check_props(src, node, &["id", "label", "sizes", "hidden"])?;
                check_max_args(src, node, 0)?;
                let (id, label) = self.id_label(node)?;
                let children = self.children(node)?;
                let sizes = self
                    .list(node, "sizes", children.len())?
                    .unwrap_or_default();
                Ok(LayoutNode::Stack {
                    id,
                    label,
                    sizes,
                    hidden: self.hidden(node)?,
                    children,
                })
            }
            "tabs" => {
                check_props(src, node, &["id", "label", "active", "hidden"])?;
                check_max_args(src, node, 0)?;
                let (id, label) = self.id_label(node)?;
                let children = self.children(node)?;
                let active = prop_u32(src, node, "active")?.unwrap_or(0);
                if active as usize >= children.len() {
                    return Err(src.node_err(
                        node,
                        msg("config-tabs-active-range")
                            .arg("active", active)
                            .arg("count", children.len()),
                    ));
                }
                Ok(LayoutNode::Tabs {
                    id,
                    label,
                    active,
                    hidden: self.hidden(node)?,
                    children,
                })
            }
            other if FUTURE_CONTAINERS.contains(&other) => {
                Err(src.node_err(node, msg("config-container-unsupported").arg("node", other)))
            }
            _ => self.widget(node),
        }
    }

    fn children(&self, node: &KdlNode) -> Result<Vec<LayoutNode>, ConfigError> {
        let nodes = children(node);
        if nodes.is_empty() {
            return Err(self
                .src
                .node_err(node, msg("config-needs-child").arg("node", k::name(node))));
        }
        // Every child is read, so that errors deeper in the tree are all
        // reported: the first error is returned, later ones are recorded.
        let mut result = Ok(Vec::with_capacity(nodes.len()));
        for n in nodes {
            match (self.node(n), &mut result) {
                (Ok(child), Ok(list)) => list.push(child),
                (Ok(_), Err(_)) => {}
                (Err(err), Ok(_)) => result = Err(err),
                (Err(err), Err(_)) => self.errors.push(err),
            }
        }
        result
    }

    /// A whitespace-separated list property with one entry per child.
    fn list(
        &self,
        node: &KdlNode,
        key: &str,
        count: usize,
    ) -> Result<Option<Vec<String>>, ConfigError> {
        let Some((value, entry)) = prop_str(self.src, node, key)? else {
            return Ok(None);
        };
        let items: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
        if items.len() != count {
            return Err(self.src.entry_err(
                entry,
                msg("config-list-length")
                    .arg("key", key)
                    .arg("count", items.len())
                    .arg("node", k::name(node))
                    .arg("children", count),
            ));
        }
        Ok(Some(items))
    }

    fn widget(&self, node: &KdlNode) -> Result<LayoutNode, ConfigError> {
        let src = self.src;
        let kind = k::name(node);
        let spec = widgets::find(kind);
        // The pre-M2d placeholder `plugin.<id>.<name>` is written
        // `plugin-panel "<id>.<name>"` now (ADR 0052).
        if let Some(rest) = kind.strip_prefix("plugin.")
            && rest.split('.').count() >= 2
            && is_dotted_kebab(rest)
        {
            return Err(src.node_err(
                node,
                msg("config-plugin-widget-renamed")
                    .arg("node", kind)
                    .arg("panel", rest),
            ));
        }
        if spec.is_none() {
            return Err(src.node_err(
                node,
                msg("config-widget-unknown")
                    .arg("node", kind)
                    .arg("widgets", widgets::kinds().collect::<Vec<_>>().join(", ")),
            ));
        }
        // `plugin-panel "<plugin>.<panel>"` takes its panel as an argument
        // (stored as the `panel` option; M2d, ADR 0050).
        let is_panel = kind == nail_api::plugins::PLUGIN_PANEL_WIDGET;
        check_max_args(src, node, usize::from(is_panel))?;
        let (id, label) = self.id_label(node)?;

        let mut options = Map::new();
        if is_panel && !args(node).is_empty() {
            let (panel, entry) = arg_str(src, node, 0)?;
            if nail_api::plugins::parse_plugin_panel_id(panel).is_none() {
                return Err(src.entry_err(
                    entry,
                    msg("config-plugin-panel-invalid").arg("panel", panel),
                ));
            }
            options.insert("panel".to_owned(), Value::from(panel));
        }
        for (key, entry) in k::props(node) {
            if key == "id" || key == "label" || key == "hidden" {
                continue;
            }
            if let Some(allowed) = spec.and_then(|s| s.options) {
                let Some((_, ty)) = allowed.iter().find(|(name, _)| *name == key) else {
                    let names: Vec<_> = allowed.iter().map(|(n, _)| *n).collect();
                    return Err(src.entry_err(
                        entry,
                        if names.is_empty() {
                            msg("config-option-unknown-none")
                                .arg("key", key)
                                .arg("node", kind)
                        } else {
                            msg("config-option-unknown")
                                .arg("key", key)
                                .arg("node", kind)
                                .arg("options", names.join(", "))
                        },
                    ));
                };
                check_option(src, entry, key, *ty)?;
            }
            if is_panel && key == "panel" {
                let panel = entry.value().as_string().unwrap_or_default();
                if options.contains_key("panel")
                    || nail_api::plugins::parse_plugin_panel_id(panel).is_none()
                {
                    return Err(src.entry_err(
                        entry,
                        msg("config-plugin-panel-invalid").arg("panel", panel),
                    ));
                }
            }
            options.insert(key.to_owned(), to_json(src, entry)?);
        }
        if let Some(spec) = spec {
            for required in spec.required {
                if !options.contains_key(*required) {
                    return Err(src.node_err(
                        node,
                        msg("config-option-required")
                            .arg("node", kind)
                            .arg("option", *required),
                    ));
                }
            }
        }

        let mut rows = Vec::new();
        for row in children(node) {
            if k::name(row) != "row" {
                return Err(src.node_err(
                    row,
                    msg("config-widget-row-only")
                        .arg("node", k::name(row))
                        .arg("widget", kind),
                ));
            }
            if spec.is_some_and(|s| !s.rows) {
                return Err(src.node_err(row, msg("config-widget-no-rows").arg("widget", kind)));
            }
            check_props(src, row, &[])?;
            check_max_args(src, row, 1)?;
            check_no_children(src, row)?;
            let (template, entry) = arg_str(src, row, 0)?;
            check_template(template).map_err(|msg| src.entry_err(entry, msg))?;
            rows.push(template.to_owned());
        }

        Ok(LayoutNode::Widget {
            id,
            label,
            kind: kind.to_owned(),
            options,
            rows,
            source: Some(src.node_loc(node)),
            hidden: self.hidden(node)?,
        })
    }

    fn action(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["label", "icon", "alias-of"])?;
        check_no_children(src, node)?;
        let (id, id_entry) = arg_str(src, node, 0)?;
        check_action_id(src, id_entry, id)?;
        let label = match prop_str(src, node, "label")? {
            None => None,
            Some((label, entry)) => Some(self.label(label, entry)?),
        };
        let icon = match prop_str(src, node, "icon")? {
            None => None,
            Some((icon, entry)) => {
                if !is_kebab(icon) {
                    return Err(
                        src.entry_err(entry, msg("config-icon-name-invalid").arg("name", icon))
                    );
                }
                Some(icon.to_owned())
            }
        };
        let alias_of = match prop_str(src, node, "alias-of")? {
            None => None,
            Some((target, entry)) => {
                check_action_id(src, entry, target)?;
                Some(target.to_owned())
            }
        };
        let extra = &args(node)[1..];
        if alias_of.is_none()
            && let Some(first) = extra.first()
        {
            return Err(src.entry_err(first, msg("config-action-args-need-alias")));
        }
        let args = extra
            .iter()
            .map(|e| to_json(src, e))
            .collect::<Result<_, _>>()?;
        file.actions.insert(
            id.to_owned(),
            ActionDef {
                label,
                icon,
                alias_of,
                args,
            },
        );
        Ok(())
    }

    fn keys(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["context"])?;
        check_max_args(src, node, 0)?;
        let context = match prop_str(src, node, "context")? {
            None => "global".to_owned(),
            Some((ctx, entry)) => {
                if !is_kebab(ctx) {
                    return Err(
                        src.entry_err(entry, msg("config-key-context-invalid").arg("context", ctx))
                    );
                }
                ctx.to_owned()
            }
        };
        for bind in children(node) {
            if let Some(def) = self.errors.ok(self.binding(&context, bind)) {
                file.keys.push(def);
            }
        }
        Ok(())
    }

    /// A `bind` or `unbind` node inside `keys`.
    fn binding(&self, context: &str, bind: &KdlNode) -> Result<KeyDef, ConfigError> {
        let src = self.src;
        check_props(src, bind, &[])?;
        check_no_children(src, bind)?;
        let (key, key_entry) = arg_str(src, bind, 0)?;
        let key = normalize_key(key).map_err(|msg| src.entry_err(key_entry, msg))?;
        match k::name(bind) {
            "bind" => {
                let (action, args) = self.action_ref(bind, 1)?;
                Ok(KeyDef {
                    context: context.to_owned(),
                    key,
                    action: Some(action),
                    args,
                })
            }
            "unbind" => {
                check_max_args(src, bind, 1)?;
                Ok(KeyDef {
                    context: context.to_owned(),
                    key,
                    action: None,
                    args: Vec::new(),
                })
            }
            other => Err(src.node_err(bind, msg("config-keys-unexpected").arg("node", other))),
        }
    }

    /// Reads `"<action> <args>…"` at argument `index`, plus any further
    /// arguments of the node.
    fn action_ref(
        &self,
        node: &KdlNode,
        index: usize,
    ) -> Result<(String, Vec<Value>), ConfigError> {
        let src = self.src;
        let (spec, entry) = arg_str(src, node, index)?;
        let (action, mut action_args) =
            parse_action_spec(spec).map_err(|msg| src.entry_err(entry, msg))?;
        check_action_id(src, entry, &action)?;
        for extra in &args(node)[index + 1..] {
            action_args.push(to_json(src, extra)?);
        }
        Ok((action, action_args))
    }

    fn toolbar(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &["hide-disabled", "overflow"])?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        if !is_kebab(name) {
            return Err(src.entry_err(
                name_entry,
                msg("config-toolbar-name-invalid").arg("name", name),
            ));
        }
        let settings = ToolbarSettings {
            hide_disabled: self.bool_prop(node, "hide-disabled")?.unwrap_or(false),
            overflow: match prop_str(src, node, "overflow")? {
                None => ToolbarOverflow::default(),
                Some((value, entry)) => ToolbarOverflow::parse(value).ok_or_else(|| {
                    src.entry_err(
                        entry,
                        msg("config-option-one-of")
                            .arg("key", "overflow")
                            .arg("choices", "more, wrap")
                            .arg("found", describe(entry.value())),
                    )
                })?,
            },
        };
        let mut items = Vec::new();
        let mut more_seen = false;
        for item in children(node) {
            if k::name(item) == "more" {
                if more_seen {
                    self.errors.push(
                        src.node_err(item, msg("config-toolbar-more-twice").arg("name", name)),
                    );
                    continue;
                }
                more_seen = true;
            }
            items.extend(self.errors.ok(self.toolbar_item(item)));
        }
        file.toolbars.insert(name.to_owned(), items);
        file.toolbar_settings.insert(name.to_owned(), settings);
        Ok(())
    }

    /// An optional boolean property.
    fn bool_prop(&self, node: &KdlNode, key: &str) -> Result<Option<bool>, ConfigError> {
        let Some(entry) = node.entry(key) else {
            return Ok(None);
        };
        entry.value().as_bool().map(Some).ok_or_else(|| {
            self.src.entry_err(
                entry,
                msg("config-expected-bool")
                    .arg("what", key)
                    .arg("found", describe(entry.value())),
            )
        })
    }

    /// An optional Lucide icon name property (kebab-case).
    fn icon_prop(&self, node: &KdlNode, key: &str) -> Result<Option<String>, ConfigError> {
        let src = self.src;
        match prop_str(src, node, key)? {
            None => Ok(None),
            Some((icon, entry)) => {
                if is_kebab(icon) {
                    Ok(Some(icon.to_owned()))
                } else {
                    Err(src.entry_err(entry, msg("config-icon-name-invalid").arg("name", icon)))
                }
            }
        }
    }

    /// An optional label property (`@message.id` checked).
    fn label_prop(&self, node: &KdlNode, key: &str) -> Result<Option<String>, ConfigError> {
        match prop_str(self.src, node, key)? {
            None => Ok(None),
            Some((label, entry)) => Ok(Some(self.label(label, entry)?)),
        }
    }

    /// An optional `badge=` property.
    fn badge_prop(&self, node: &KdlNode) -> Result<Option<BadgeSource>, ConfigError> {
        let src = self.src;
        match prop_str(src, node, "badge")? {
            None => Ok(None),
            Some((value, entry)) => BadgeSource::parse(value).map(Some).ok_or_else(|| {
                src.entry_err(
                    entry,
                    msg("config-option-one-of")
                        .arg("key", "badge")
                        .arg("choices", "unread, confirmations")
                        .arg("found", describe(entry.value())),
                )
            }),
        }
    }

    fn toolbar_item(&self, item: &KdlNode) -> Result<ToolbarItem, ConfigError> {
        let src = self.src;
        match k::name(item) {
            "item" => {
                check_props(src, item, &["label", "icon", "rank", "display"])?;
                check_no_children(src, item)?;
                let action = self.action_item(item)?;
                let rank = prop_u32(src, item, "rank")?;
                if rank == Some(0)
                    && let Some(entry) = item.entry("rank")
                {
                    return Err(src.entry_err(entry, msg("config-toolbar-rank-zero")));
                }
                let display = match prop_str(src, item, "display")? {
                    None => None,
                    Some((value, entry)) => Some(ItemDisplay::parse(value).ok_or_else(|| {
                        src.entry_err(
                            entry,
                            msg("config-option-one-of")
                                .arg("key", "display")
                                .arg("choices", "icon, label, icon-label")
                                .arg("found", describe(entry.value())),
                        )
                    })?),
                };
                Ok(ToolbarItem::Action {
                    item: action,
                    rank,
                    display,
                })
            }
            "more" => {
                check_props(src, item, &["label", "icon"])?;
                check_max_args(src, item, 0)?;
                Ok(ToolbarItem::More {
                    label: self.label_prop(item, "label")?,
                    icon: self.icon_prop(item, "icon")?,
                    items: self.menu_items(item)?,
                })
            }
            "spacer" | "separator" => {
                self.block(item)?;
                check_no_children(src, item)?;
                Ok(if k::name(item) == "spacer" {
                    ToolbarItem::Spacer
                } else {
                    ToolbarItem::Separator
                })
            }
            other => Err(src.node_err(item, msg("config-toolbar-unexpected").arg("node", other))),
        }
    }

    /// A toolbar or menu `item`: its action reference and overrides.
    fn action_item(&self, item: &KdlNode) -> Result<ActionItem, ConfigError> {
        let (action, args) = self.action_ref(item, 0)?;
        let (label, icon) = self.item_overrides(item)?;
        Ok(ActionItem {
            action,
            args,
            label,
            icon,
        })
    }

    fn item_overrides(
        &self,
        item: &KdlNode,
    ) -> Result<(Option<String>, Option<String>), ConfigError> {
        let src = self.src;
        let label = match prop_str(src, item, "label")? {
            None => None,
            Some((label, entry)) => Some(self.label(label, entry)?),
        };
        let icon = prop_str(src, item, "icon")?.map(|(icon, _)| icon.to_owned());
        Ok((label, icon))
    }

    /// Refuses a second `views` / `rail` / `settings-page` section.
    fn once(&self, node: &KdlNode, seen: bool) -> Result<(), ConfigError> {
        if seen {
            return Err(self.src.node_err(
                node,
                msg("config-layout-section-twice").arg("node", k::name(node)),
            ));
        }
        Ok(())
    }

    /// `presets { preset "<id>" window="<window>" label=… icon=… {
    /// <arrangement> } }` (M5d).
    fn presets(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        self.once(node, file.presets.is_some())?;
        self.block(node)?;
        let mut presets: Vec<LayoutPreset> = Vec::new();
        for item in children(node) {
            if let Some(preset) = self.errors.ok(self
                .expect(item, "preset", "presets")
                .and_then(|()| self.preset(item)))
            {
                if presets.iter().any(|p| p.id == preset.id) {
                    self.errors.push(src.node_err(
                        item,
                        msg("config-preset-duplicate").arg("preset", preset.id.as_str()),
                    ));
                    continue;
                }
                presets.push(preset);
            }
        }
        file.presets = Some(presets);
        Ok(())
    }

    fn preset(&self, node: &KdlNode) -> Result<LayoutPreset, ConfigError> {
        let src = self.src;
        check_props(src, node, &["window", "label", "icon"])?;
        check_max_args(src, node, 1)?;
        let id = kebab_arg(src, node, 0)?.0;
        let Some((window, _)) = prop_str(src, node, "window")? else {
            return Err(src.node_err(node, msg("config-preset-window-missing").arg("preset", id)));
        };
        let label = self.label_prop(node, "label")?;
        let icon = self.icon_prop(node, "icon")?;
        let nodes = children(node);
        let [root] = nodes else {
            return Err(src.node_err(node, msg("config-preset-one-root").arg("preset", id)));
        };
        let root = self.preset_node(root)?;
        let mut seen: Vec<&str> = Vec::new();
        for pane in root.pane_ids() {
            if seen.contains(&pane) {
                return Err(src.node_err(
                    node,
                    msg("config-preset-pane-twice")
                        .arg("preset", id)
                        .arg("pane", pane),
                ));
            }
            seen.push(pane);
        }
        Ok(LayoutPreset {
            id: id.to_owned(),
            window: window.to_owned(),
            label,
            icon,
            root,
        })
    }

    fn preset_node(&self, node: &KdlNode) -> Result<PresetNode, ConfigError> {
        let src = self.src;
        let kids = |node: &KdlNode| -> Result<Vec<PresetNode>, ConfigError> {
            let nodes = children(node);
            if nodes.is_empty() {
                return Err(
                    src.node_err(node, msg("config-needs-child").arg("node", k::name(node)))
                );
            }
            nodes.iter().map(|n| self.preset_node(n)).collect()
        };
        match k::name(node) {
            "split" => {
                check_props(src, node, &["sizes"])?;
                check_max_args(src, node, 1)?;
                let orientation = match args(node).first() {
                    None => Orientation::Horizontal,
                    Some(entry) => orientation(src, entry)?,
                };
                let children = kids(node)?;
                let sizes = self
                    .list(node, "sizes", children.len())?
                    .unwrap_or_default();
                Ok(PresetNode::Split {
                    orientation,
                    sizes,
                    children,
                })
            }
            "stack" => {
                check_props(src, node, &["sizes"])?;
                check_max_args(src, node, 0)?;
                let children = kids(node)?;
                let sizes = self
                    .list(node, "sizes", children.len())?
                    .unwrap_or_default();
                Ok(PresetNode::Stack { sizes, children })
            }
            "tabs" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 0)?;
                Ok(PresetNode::Tabs {
                    children: kids(node)?,
                })
            }
            "pane" => {
                check_props(src, node, &[])?;
                check_max_args(src, node, 1)?;
                check_no_children(src, node)?;
                let id = kebab_arg(src, node, 0)?.0;
                Ok(PresetNode::Pane { id: id.to_owned() })
            }
            other => Err(src.node_err(node, msg("config-preset-node-unknown").arg("node", other))),
        }
    }

    /// `views default="<id>" { view "<id>" label=… icon=… window=… badge=… }`.
    fn views(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        self.once(node, file.views.is_some())?;
        check_props(src, node, &["default"])?;
        check_max_args(src, node, 0)?;
        let default = kebab_prop(src, node, "default")?;
        let mut section = ViewsSection {
            default,
            views: IndexMap::new(),
        };
        for view in children(node) {
            if let Some((id, def)) = self.errors.ok(self
                .expect(view, "view", "views")
                .and_then(|()| self.view(view)))
            {
                if section.views.contains_key(&id) {
                    self.errors.push(
                        src.node_err(view, msg("config-view-duplicate").arg("view", id.as_str())),
                    );
                    continue;
                }
                section.views.insert(id, def);
            }
        }
        file.views = Some(section);
        Ok(())
    }

    fn view(&self, node: &KdlNode) -> Result<(String, ViewDef), ConfigError> {
        let src = self.src;
        check_props(src, node, &["label", "icon", "window", "badge"])?;
        check_max_args(src, node, 1)?;
        check_no_children(src, node)?;
        let id = kebab_arg(src, node, 0)?.0;
        let window = match prop_str(src, node, "window")? {
            None => {
                return Err(src.node_err(
                    node,
                    msg("config-option-required")
                        .arg("node", "view")
                        .arg("option", "window"),
                ));
            }
            Some((window, entry)) => {
                if !is_kebab(window) {
                    return Err(
                        src.entry_err(entry, msg("config-window-name-invalid").arg("name", window))
                    );
                }
                window.to_owned()
            }
        };
        Ok((
            id.to_owned(),
            ViewDef {
                label: self.label_prop(node, "label")?,
                icon: self.icon_prop(node, "icon")?,
                window,
                badge: self.badge_prop(node)?,
            },
        ))
    }

    /// `rail { view "<id>"; item "<action> <args>…" label=… icon=… badge=…;
    /// spacer; separator }`.
    fn rail(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        self.once(node, file.rail.is_some())?;
        self.block(node)?;
        let mut items = Vec::new();
        for item in children(node) {
            items.extend(self.errors.ok(self.rail_item(item)));
        }
        file.rail = Some(items);
        Ok(())
    }

    fn rail_item(&self, item: &KdlNode) -> Result<RailItem, ConfigError> {
        let src = self.src;
        match k::name(item) {
            "view" => {
                check_props(src, item, &[])?;
                check_max_args(src, item, 1)?;
                check_no_children(src, item)?;
                let view = kebab_arg(src, item, 0)?.0;
                Ok(RailItem::View {
                    view: view.to_owned(),
                })
            }
            "item" => {
                check_props(src, item, &["label", "icon", "badge"])?;
                check_no_children(src, item)?;
                let (action, args) = self.action_ref(item, 0)?;
                let (label, icon) = self.item_overrides(item)?;
                Ok(RailItem::Action {
                    action,
                    args,
                    label,
                    icon,
                    badge: self.badge_prop(item)?,
                })
            }
            "spacer" | "separator" => {
                self.block(item)?;
                check_no_children(src, item)?;
                Ok(if k::name(item) == "spacer" {
                    RailItem::Spacer
                } else {
                    RailItem::Separator
                })
            }
            other => Err(src.node_err(item, msg("config-rail-unexpected").arg("node", other))),
        }
    }

    /// `settings-page { group "<id>" label=… { section "<id>" label=… icon=…
    /// keywords=… { <nodes> } } }`. Group, section and node ids are unique
    /// on the whole page.
    fn settings_page(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        self.once(node, file.settings_page.is_some())?;
        self.block(node)?;
        let mut ids: HashSet<String> = HashSet::new();
        let mut node_ids: HashSet<String> = HashSet::new();
        let mut groups = Vec::new();
        for group in children(node) {
            let read = self
                .expect(group, "group", "settings-page")
                .and_then(|()| self.settings_group(group, &mut ids, &mut node_ids));
            groups.extend(self.errors.ok(read));
        }
        if children(node).is_empty() {
            return Err(src.node_err(node, msg("config-needs-child").arg("node", "settings-page")));
        }
        file.settings_page = Some(groups);
        Ok(())
    }

    fn page_id(&self, node: &KdlNode, ids: &mut HashSet<String>) -> Result<String, ConfigError> {
        let src = self.src;
        let (id, entry) = kebab_arg(src, node, 0)?;
        if !ids.insert(id.to_owned()) {
            return Err(src.entry_err(entry, msg("config-id-duplicate").arg("id", id)));
        }
        Ok(id.to_owned())
    }

    fn settings_group(
        &self,
        node: &KdlNode,
        ids: &mut HashSet<String>,
        node_ids: &mut HashSet<String>,
    ) -> Result<SettingsGroup, ConfigError> {
        let src = self.src;
        check_props(src, node, &["label"])?;
        check_max_args(src, node, 1)?;
        let id = self.page_id(node, ids)?;
        let label = self.label_prop(node, "label")?;
        let mut sections = Vec::new();
        for section in children(node) {
            let read = self
                .expect(section, "section", "group")
                .and_then(|()| self.settings_section(section, ids, node_ids));
            sections.extend(self.errors.ok(read));
        }
        if children(node).is_empty() {
            return Err(src.node_err(node, msg("config-needs-child").arg("node", "group")));
        }
        Ok(SettingsGroup {
            id,
            label,
            sections,
        })
    }

    fn settings_section(
        &self,
        node: &KdlNode,
        ids: &mut HashSet<String>,
        node_ids: &mut HashSet<String>,
    ) -> Result<SettingsSection, ConfigError> {
        let src = self.src;
        check_props(src, node, &["label", "icon", "keywords"])?;
        check_max_args(src, node, 1)?;
        let id = self.page_id(node, ids)?;
        let label = self.label_prop(node, "label")?;
        let icon = self.icon_prop(node, "icon")?;
        let keywords = self.label_prop(node, "keywords")?;
        let mut nodes = self.children(node)?;
        for child in children(node) {
            self.check_unique_ids(child, node_ids)?;
        }
        let root = if nodes.len() == 1 {
            nodes.remove(0)
        } else {
            LayoutNode::Stack {
                id: None,
                label: None,
                sizes: vec!["auto".to_owned(); nodes.len()],
                hidden: false,
                children: nodes,
            }
        };
        Ok(SettingsSection {
            id,
            label,
            icon,
            keywords,
            root,
        })
    }

    fn menu(&self, file: &mut LayoutFile, node: &KdlNode) -> Result<(), ConfigError> {
        let src = self.src;
        check_props(src, node, &[])?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        let valid = name == "app" || name.strip_prefix("context:").is_some_and(is_dotted_kebab);
        if !valid {
            return Err(src.entry_err(
                name_entry,
                msg("config-menu-name-invalid").arg("name", name),
            ));
        }
        let items = self.menu_items(node)?;
        file.menus.insert(name.to_owned(), items);
        Ok(())
    }

    fn menu_items(&self, node: &KdlNode) -> Result<Vec<MenuItem>, ConfigError> {
        let src = self.src;
        let mut items = Vec::new();
        for item in children(node) {
            items.push(match k::name(item) {
                "item" => {
                    check_props(src, item, &["label", "icon"])?;
                    check_no_children(src, item)?;
                    MenuItem::Action(self.action_item(item)?)
                }
                "separator" => {
                    self.block(item)?;
                    check_no_children(src, item)?;
                    MenuItem::Separator
                }
                "submenu" => {
                    check_props(src, item, &[])?;
                    check_max_args(src, item, 1)?;
                    let (label, entry) = arg_str(src, item, 0)?;
                    let label = self.label(label, entry)?;
                    MenuItem::Submenu {
                        label,
                        items: self.menu_items(item)?,
                    }
                }
                "header" => {
                    check_props(src, item, &[])?;
                    check_max_args(src, item, 1)?;
                    check_no_children(src, item)?;
                    let (label, entry) = arg_str(src, item, 0)?;
                    MenuItem::Header {
                        label: self.label(label, entry)?,
                    }
                }
                other => {
                    return Err(
                        src.node_err(item, msg("config-menu-unexpected").arg("node", other))
                    );
                }
            });
        }
        Ok(items)
    }
}

fn check_option(
    src: &Source<'_>,
    entry: &KdlEntry,
    key: &str,
    ty: OptionType,
) -> Result<(), ConfigError> {
    let value = entry.value();
    let ok = match ty {
        OptionType::Bool => value.is_bool(),
        OptionType::UInt => value.as_integer().is_some_and(|i| i >= 0),
        OptionType::Str => value.is_string(),
        OptionType::OneOf(choices) => value.as_string().is_some_and(|s| choices.contains(&s)),
        OptionType::Words(choices) => value.as_string().is_some_and(|s| {
            let words: Vec<&str> = s.split_whitespace().collect();
            words.iter().all(|w| choices.contains(w))
                && words
                    .iter()
                    .enumerate()
                    .all(|(i, w)| !words[..i].contains(w))
        }),
    };
    if ok {
        return Ok(());
    }
    Err(src.entry_err(
        entry,
        match ty {
            OptionType::Bool => msg("config-option-bool"),
            OptionType::UInt => msg("config-option-count"),
            OptionType::Str => msg("config-option-string"),
            OptionType::OneOf(choices) => {
                msg("config-option-one-of").arg("choices", choices.join(", "))
            }
            OptionType::Words(choices) => {
                msg("config-option-words").arg("choices", choices.join(", "))
            }
        }
        .arg("key", key)
        .arg("found", describe(value)),
    ))
}

/// A split's orientation (`"horizontal"` / `"vertical"`) from `entry`.
fn orientation(src: &Source<'_>, entry: &KdlEntry) -> Result<Orientation, ConfigError> {
    let text = k::entry_str(src, entry, "`orientation`")?;
    Orientation::parse(text)
        .ok_or_else(|| src.entry_err(entry, msg("config-orientation-unknown").arg("found", text)))
}

fn check_action_id(src: &Source<'_>, entry: &KdlEntry, id: &str) -> Result<(), ConfigError> {
    if is_dotted_kebab(id) && id.contains('.') {
        Ok(())
    } else {
        Err(src.entry_err(entry, msg("config-action-id-invalid").arg("id", id)))
    }
}

/// Splits `nav.goto-folder "Inbox" 2` into the action id and its arguments.
/// Quoted words are strings; bare numbers are numbers; other bare words are
/// strings.
fn parse_action_spec(spec: &str) -> Result<(String, Vec<Value>), LocalizedMessage> {
    let mut words: Vec<Value> = Vec::new();
    let mut chars = spec.trim().chars().peekable();
    let mut action: Option<String> = None;
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let value = if c == '"' {
            chars.next();
            let mut word = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(ch) => word.push(ch),
                    None => return Err(msg("config-action-quote").arg("spec", spec)),
                }
            }
            Value::String(word)
        } else {
            let mut word = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
            if action.is_none() {
                action = Some(word);
                continue;
            }
            if let Ok(i) = word.parse::<i64>() {
                Value::from(i)
            } else if let Some(n) = word
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
            {
                Value::Number(n)
            } else {
                Value::String(word)
            }
        };
        if action.is_none() {
            return Err(msg("config-action-spec-start").arg("spec", spec));
        }
        words.push(value);
    }
    let action = action.ok_or_else(|| msg("config-action-empty"))?;
    Ok((action, words))
}

/// Lowercases a key sequence and checks its shape (`ctrl+shift+m`, `g i`).
pub(crate) fn normalize_key(key: &str) -> Result<String, LocalizedMessage> {
    let steps: Vec<String> = key.split_whitespace().map(str::to_lowercase).collect();
    if steps.is_empty() {
        return Err(msg("config-key-empty"));
    }
    for step in &steps {
        // `+` alone (or as the last key of a chord, `ctrl++`) is the plus key.
        let keys = step.strip_suffix("++").map_or_else(
            || step.split('+').collect::<Vec<_>>(),
            |head| head.split('+').chain(std::iter::once("+")).collect(),
        );
        if step != "+" && keys.iter().any(|k| k.is_empty()) {
            return Err(msg("config-key-invalid").arg("key", key));
        }
    }
    Ok(steps.join(" "))
}

/// Checks that `{…}` placeholders in a row template are balanced.
fn check_template(template: &str) -> Result<(), LocalizedMessage> {
    let mut open = false;
    for c in template.chars() {
        match (c, open) {
            ('{', false) => open = true,
            ('}', true) => open = false,
            ('{', true) => return Err(msg("config-template-nested").arg("template", template)),
            ('}', false) => return Err(msg("config-template-unmatched").arg("template", template)),
            _ => {}
        }
    }
    if open {
        return Err(msg("config-template-unclosed").arg("template", template));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolution

impl LayoutFile {
    /// Resolves a complete (already merged) layout file. Returns the first
    /// error; [`Self::resolve_all`] returns all of them.
    pub fn resolve(&self) -> Result<ResolvedLayout, ConfigError> {
        self.resolve_unsorted()
            .map_err(|mut errors| errors.swap_remove(0))
    }

    /// Resolves a complete (already merged) layout file, reporting every
    /// error (missing `main` window, unknown toolbars) in file order.
    pub fn resolve_all(&self) -> Result<ResolvedLayout, Vec<ConfigError>> {
        self.resolve_unsorted().map_err(|mut errors| {
            sort_errors(&mut errors);
            errors
        })
    }

    /// All resolution errors in the order they are found; the first one is
    /// what [`Self::resolve`] reports.
    fn resolve_unsorted(&self) -> Result<ResolvedLayout, Vec<ConfigError>> {
        let mut errors = Vec::new();
        if !self.windows.contains_key("main") {
            errors.push(ConfigError::invalid(
                &self.name.loc,
                msg("config-layout-no-main"),
            ));
        }

        // Keys: a later binding of the same key in the same context replaces
        // the earlier one in place; `unbind` removes it.
        let mut keys: Vec<KeyBinding> = Vec::new();
        for def in &self.keys {
            let existing = keys
                .iter()
                .position(|k| k.context == def.context && k.key == def.key);
            match (&def.action, existing) {
                (None, Some(i)) => {
                    keys.remove(i);
                }
                (None, None) => {}
                (Some(action), existing) => {
                    let binding = KeyBinding {
                        context: def.context.clone(),
                        key: def.key.clone(),
                        action: action.clone(),
                        args: def.args.clone(),
                    };
                    match existing {
                        Some(i) => keys[i] = binding,
                        None => keys.push(binding),
                    }
                }
            }
        }

        // Every toolbar widget must name an existing toolbar.
        for breakpoints in self.windows.values() {
            for bp in breakpoints {
                check_toolbar_refs(&bp.root, &self.toolbars, &mut errors);
            }
        }
        let settings_page = self.settings_page.clone().unwrap_or_default();
        for section in settings_page.iter().flat_map(|g| g.sections.iter()) {
            check_toolbar_refs(&section.root, &self.toolbars, &mut errors);
        }
        let views = self.views.clone().unwrap_or_default();
        let rail = self.rail.clone().unwrap_or_default();
        self.check_shell(&views, &rail, &settings_page, &mut errors);
        let presets = self.presets.clone().unwrap_or_default();
        self.check_presets(&presets, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }

        // Every used action gets an entry in `actions`.
        let mut actions = self.actions.clone();
        let used = used_actions(&keys, self.toolbars.values(), self.menus.values(), &rail);
        for action in used {
            if !actions.contains_key(action) {
                actions.insert(action.to_owned(), ActionDef::default());
            }
        }

        let toolbar_settings = self
            .toolbars
            .keys()
            .map(|name| {
                let settings = self.toolbar_settings.get(name).copied().unwrap_or_default();
                (name.clone(), settings)
            })
            .collect();

        Ok(ResolvedLayout {
            name: self.name.value.clone(),
            windows: self.windows.clone(),
            actions,
            keys,
            toolbars: self.toolbars.clone(),
            menus: self.menus.clone(),
            toolbar_settings,
            default_view: views.default.clone(),
            views: views.views,
            rail,
            settings: settings_page,
            window_frames: self
                .window_frames
                .iter()
                .filter(|(name, frame)| self.windows.contains_key(*name) && !frame.is_empty())
                .map(|(name, frame)| (name.clone(), *frame))
                .collect(),
            presets,
        })
    }

    /// M5d check: a preset arranges an existing window. Panes it names that
    /// a breakpoint lacks are not an error (a user file may replace the
    /// window the default presets were written for): `ApplyPreset` refuses
    /// such a breakpoint with `edit-issue-preset-pane-missing`, and the
    /// layout editor offers only presets whose panes the breakpoint has.
    fn check_presets(&self, presets: &[LayoutPreset], errors: &mut Vec<ConfigError>) {
        let loc = &self.name.loc;
        for preset in presets {
            if !self.windows.contains_key(&preset.window) {
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-preset-window-unknown")
                        .arg("preset", preset.id.as_str())
                        .arg("window", preset.window.as_str()),
                ));
            }
        }
    }

    /// M5b checks: a view shows an existing window that does not itself
    /// host views; the default view and the rail's views exist; a
    /// `view-host` needs views, a `nav-rail` rail entries, a
    /// `settings-page` widget the page's sections.
    fn check_shell(
        &self,
        views: &ViewsSection,
        rail: &[RailItem],
        settings_page: &[SettingsGroup],
        errors: &mut Vec<ConfigError>,
    ) {
        let loc = &self.name.loc;
        for (id, view) in &views.views {
            let Some(breakpoints) = self.windows.get(&view.window) else {
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-view-window-unknown")
                        .arg("view", id.as_str())
                        .arg("window", view.window.as_str()),
                ));
                continue;
            };
            if breakpoints
                .iter()
                .any(|bp| uses_widget(&bp.root, VIEW_HOST_WIDGET))
            {
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-view-hosts-views")
                        .arg("view", id.as_str())
                        .arg("window", view.window.as_str()),
                ));
            }
        }
        if let Some(default) = &views.default
            && !views.views.contains_key(default)
        {
            errors.push(ConfigError::invalid(
                loc,
                msg("config-view-default-unknown").arg("view", default.as_str()),
            ));
        }
        for item in rail {
            if let RailItem::View { view } = item
                && !views.views.contains_key(view)
            {
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-rail-view-unknown").arg("view", view.as_str()),
                ));
            }
        }
        let needs = [
            (VIEW_HOST_WIDGET, views.views.is_empty(), "views"),
            (NAV_RAIL_WIDGET, rail.is_empty(), "rail"),
            (
                SETTINGS_PAGE_WIDGET,
                settings_page.is_empty(),
                "settings-page",
            ),
        ];
        for (widget, missing, section) in needs {
            if !missing {
                continue;
            }
            let used = self
                .windows
                .values()
                .flatten()
                .any(|bp| uses_widget(&bp.root, widget));
            if used {
                errors.push(ConfigError::invalid(
                    loc,
                    msg("config-widget-needs-section")
                        .arg("widget", widget)
                        .arg("section", section),
                ));
            }
        }
    }
}

/// Whether the tree uses a widget of `kind`.
fn uses_widget(node: &LayoutNode, kind: &str) -> bool {
    let mut found = false;
    node.walk(&mut |n| {
        if let LayoutNode::Widget { kind: k, .. } = n
            && k == kind
        {
            found = true;
        }
    });
    found
}

fn check_toolbar_refs(
    node: &LayoutNode,
    toolbars: &IndexMap<String, Vec<ToolbarItem>>,
    errors: &mut Vec<ConfigError>,
) {
    if let LayoutNode::Widget {
        kind,
        options,
        source,
        ..
    } = node
        && kind == "toolbar"
        && let Some(name) = options.get("toolbar").and_then(Value::as_str)
        && !toolbars.contains_key(name)
    {
        let loc = source.clone().unwrap_or_else(|| SourceLocation {
            file: String::new(),
            line: 1,
            column: 1,
        });
        errors.push(ConfigError::invalid(
            &loc,
            msg("config-toolbar-unknown").arg("name", name),
        ));
    }
    for child in node.children() {
        check_toolbar_refs(child, toolbars, errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn action_specs() {
        assert_eq!(
            parse_action_spec("mail.next").unwrap(),
            ("mail.next".into(), vec![])
        );
        assert_eq!(
            parse_action_spec("nav.goto-folder \"Inbox\"").unwrap(),
            ("nav.goto-folder".into(), vec![json!("Inbox")])
        );
        assert_eq!(
            parse_action_spec("view.toggle \"mini month\" 2 x")
                .unwrap()
                .1,
            vec![json!("mini month"), json!(2), json!("x")]
        );
        assert!(parse_action_spec("").is_err());
        assert!(parse_action_spec("\"x\" y").is_err());
        assert!(parse_action_spec("a.b \"open").is_err());
    }

    #[test]
    fn keys_are_normalized() {
        assert_eq!(normalize_key("Ctrl+Shift+M").unwrap(), "ctrl+shift+m");
        assert_eq!(normalize_key("g  i").unwrap(), "g i");
        assert_eq!(normalize_key("ctrl++").unwrap(), "ctrl++");
        assert_eq!(normalize_key("+").unwrap(), "+");
        assert!(normalize_key("ctrl+").is_err());
        assert!(normalize_key("  ").is_err());
    }

    #[test]
    fn templates() {
        assert!(check_template("{from.name | bold-if-unread}  {date | relative}").is_ok());
        assert!(check_template("{subject").is_err());
        assert!(check_template("subject}").is_err());
        assert!(check_template("{a {b}}").is_err());
    }
}
