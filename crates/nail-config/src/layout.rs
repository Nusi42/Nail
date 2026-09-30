//! `layout.kdl`: the file model, the KDL reader, merging onto the defaults and
//! resolution into [`ResolvedLayout`].

use std::collections::HashSet;

use indexmap::IndexMap;
use kdl::{KdlEntry, KdlNode};
use nail_api::{
    ActionDef, Breakpoint, KeyBinding, LayoutNode, LocalizedMessage, MenuItem, Orientation,
    ResolvedLayout, SourceLocation, ToolbarItem,
};
use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::error::ConfigError;
use crate::kdl_util::{
    self as k, Errors, Located, Source, arg_str, args, check_max_args, check_no_children,
    check_props, children, describe, is_dotted_kebab, is_kebab, prop_str, prop_u32, sort_errors,
    to_json,
};
use crate::l10n::msg;
use crate::schema::{ActionSchema, BreakpointSchema, MenuItemSchema, ToolbarItemSchema};
use crate::theme::{read_extends, single_root};
use crate::widgets::{self, OptionType};

/// Containers from the design that M0 does not support yet.
const FUTURE_CONTAINERS: &[&str] = &["grid", "dock", "scroll", "overlay"];

/// A `layout.kdl` file as written (possibly sparse).
///
/// KDL mapping: the file holds one node `layout "<name>" extends=… { … }`
/// whose children are `window`, `breakpoint`, `action(s)`, `keys`,
/// `toolbar(s)` and `menu(s)` nodes.
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
        let src = Source::new(file, text);
        let doc = src.parse_all()?;
        let root = single_root(&src, &doc, "layout").map_err(|e| vec![e])?;
        let errors = Errors::default();
        let layout = LayoutReader {
            src: &src,
            errors: &errors,
        }
        .layout(root);
        errors.finish(layout)
    }

    /// Merges `overlay` (a sparse file extending this one) onto `self`.
    pub fn merge(mut self, overlay: LayoutFile) -> LayoutFile {
        self.name = overlay.name;
        self.extends = None;
        self.windows.extend(overlay.windows);
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
        self.toolbars.extend(overlay.toolbars);
        self.menus.extend(overlay.menus);
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
        e.ok(check_props(src, node, &["extends"]));
        e.ok(check_max_args(src, node, 1));
        let name = match e.ok(arg_str(src, node, 0)) {
            Some((name, entry)) => src.located(entry, name.to_owned()),
            None => Located::new(String::new(), src.node_loc(node)),
        };
        let mut file = LayoutFile {
            name,
            extends: e.ok(read_extends(src, node)).flatten(),
            windows: IndexMap::new(),
            actions: IndexMap::new(),
            keys: Vec::new(),
            toolbars: IndexMap::new(),
            menus: IndexMap::new(),
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
        check_props(src, node, &[])?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        if !is_kebab(name) {
            return Err(src.entry_err(
                name_entry,
                msg("config-window-name-invalid").arg("name", name),
            ));
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
        let id = match prop_str(src, node, "id")? {
            None => None,
            Some((id, entry)) => {
                if !is_kebab(id) {
                    return Err(src.entry_err(entry, msg("config-id-invalid").arg("id", id)));
                }
                Some(id.to_owned())
            }
        };
        let label = match prop_str(src, node, "label")? {
            None => None,
            Some((label, entry)) => Some(self.label(label, entry)?),
        };
        Ok((id, label))
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
                    Some(entry) => match k::entry_str(src, entry, "`orientation`")? {
                        "horizontal" => Orientation::Horizontal,
                        "vertical" => Orientation::Vertical,
                        other => {
                            return Err(src.entry_err(
                                entry,
                                msg("config-orientation-unknown").arg("found", other),
                            ));
                        }
                    },
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
                    children,
                })
            }
            "stack" => {
                check_props(src, node, &["id", "label", "sizes"])?;
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
                    children,
                })
            }
            "tabs" => {
                check_props(src, node, &["id", "label", "active"])?;
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
        let is_plugin = kind
            .strip_prefix("plugin.")
            .is_some_and(|rest| rest.split('.').count() >= 2 && is_dotted_kebab(rest));
        if spec.is_none() && !is_plugin {
            return Err(src.node_err(
                node,
                msg("config-widget-unknown")
                    .arg("node", kind)
                    .arg("widgets", widgets::kinds().collect::<Vec<_>>().join(", ")),
            ));
        }
        check_max_args(src, node, 0)?;
        let (id, label) = self.id_label(node)?;

        let mut options = Map::new();
        for (key, entry) in k::props(node) {
            if key == "id" || key == "label" {
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
        check_props(src, node, &[])?;
        check_max_args(src, node, 1)?;
        let (name, name_entry) = arg_str(src, node, 0)?;
        if !is_kebab(name) {
            return Err(src.entry_err(
                name_entry,
                msg("config-toolbar-name-invalid").arg("name", name),
            ));
        }
        let mut items = Vec::new();
        for item in children(node) {
            items.extend(self.errors.ok(self.toolbar_item(item)));
        }
        file.toolbars.insert(name.to_owned(), items);
        Ok(())
    }

    fn toolbar_item(&self, item: &KdlNode) -> Result<ToolbarItem, ConfigError> {
        let src = self.src;
        match k::name(item) {
            "item" => {
                check_props(src, item, &["label", "icon"])?;
                check_no_children(src, item)?;
                let (action, args) = self.action_ref(item, 0)?;
                let (label, icon) = self.item_overrides(item)?;
                Ok(ToolbarItem::Action {
                    action,
                    args,
                    label,
                    icon,
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
                    let (action, args) = self.action_ref(item, 0)?;
                    let (label, icon) = self.item_overrides(item)?;
                    MenuItem::Action {
                        action,
                        args,
                        label,
                        icon,
                    }
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
        }
        .arg("key", key)
        .arg("found", describe(value)),
    ))
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
        if !errors.is_empty() {
            return Err(errors);
        }

        // Every used action gets an entry in `actions`.
        let mut actions = self.actions.clone();
        let mut used: Vec<&str> = keys.iter().map(|k| k.action.as_str()).collect();
        for items in self.toolbars.values() {
            used.extend(items.iter().filter_map(|i| match i {
                ToolbarItem::Action { action, .. } => Some(action.as_str()),
                _ => None,
            }));
        }
        fn menu_actions<'a>(items: &'a [MenuItem], out: &mut Vec<&'a str>) {
            for item in items {
                match item {
                    MenuItem::Action { action, .. } => out.push(action),
                    MenuItem::Submenu { items, .. } => menu_actions(items, out),
                    MenuItem::Separator => {}
                }
            }
        }
        for items in self.menus.values() {
            menu_actions(items, &mut used);
        }
        for action in used {
            if !actions.contains_key(action) {
                actions.insert(action.to_owned(), ActionDef::default());
            }
        }

        Ok(ResolvedLayout {
            name: self.name.value.clone(),
            windows: self.windows.clone(),
            actions,
            keys,
            toolbars: self.toolbars.clone(),
            menus: self.menus.clone(),
        })
    }
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
