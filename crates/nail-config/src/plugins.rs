//! Plug-in placements in `layout.kdl` (M2d, ADR 0050): the manifest's
//! suggested placements (`nail_api::plugins::PluginPlacement`) turned into
//! the user's layout file with the items added — `item
//! "plugin.<id>.<command>"` in a toolbar or menu (after / before the named
//! item, else at the end; a missing toolbar or menu is created; an item
//! already there is never added twice), `plugin-panel "<id>.<panel>"` after
//! / before the first widget of the named kind (else at the end of the
//! main window), `bind` lines for the suggested keys (never replacing a key
//! the user or the defaults bind). The core answers
//! `Request::PreviewPluginPlacements` with that text and the merge preview;
//! the user confirms in the merge dialog (`ApplyConfigMerge`).
//!
//! The file is edited in place ([`crate::kdl_edit`], like the write-back),
//! so everything else stays byte for byte. The reference algorithm of the
//! UI mock is `ui/src/features/plugins/placementText.ts`.
//!
//! [`InstalledPlugins`] tells the merge preview which `plugin.*` actions
//! and panels exist, so a plug-in's own placements are not flagged as
//! "not installed".

use std::collections::HashSet;

use indexmap::IndexMap;
use kdl::{KdlDocument, KdlNode, KdlValue};
use nail_api::plugins::{
    PLUGIN_PANEL_WIDGET, PluginManifest, PluginPlacement, plugin_command_id, plugin_panel_id,
};
use nail_api::{LayoutNode, MenuItem, PluginId, ResolvedLayout, ToolbarItem};

use crate::kdl_edit::{
    Style, append_rendered, ensure_multiline, parse_rendered, replace_rendered, set_prop, str_prop,
};
use crate::layout::normalize_key;
use crate::render;
use crate::write_back::{EditError, LayoutEditor, NEW_LAYOUT};
use crate::{LAYOUT_FILE, LayoutFile, parse_layout_all};

/// The window panels go to when their anchor is not found.
pub const MAIN_WINDOW: &str = "main";

/// The commands and panels of the installed plug-ins (for the merge
/// preview's "not installed" conflicts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledPlugins {
    /// `plugin.<id>.<command>` actions.
    pub actions: HashSet<String>,
    /// `<id>.<panel>` panels.
    pub panels: HashSet<String>,
}

impl InstalledPlugins {
    /// Adds a plug-in's contributions.
    pub fn add(&mut self, plugin: &PluginId, manifest: &PluginManifest) {
        for command in &manifest.contributes.commands {
            self.actions.insert(plugin_command_id(plugin, &command.id));
        }
        for panel in &manifest.contributes.panels {
            self.panels.insert(plugin_panel_id(plugin, &panel.id));
        }
    }
}

/// The user's layout text (`None` = no file yet) with `plugin`'s suggested
/// placements added, or `None` when nothing would change (everything is
/// already placed, keys are taken). Fails with [`EditError::Config`] when
/// the user's file is invalid.
pub fn placement_layout(
    current: Option<&str>,
    plugin: &PluginId,
    manifest: &PluginManifest,
) -> Result<Option<String>, EditError> {
    let text = current.unwrap_or(NEW_LAYOUT);
    let parsed = LayoutFile::parse_all(text, LAYOUT_FILE).map_err(EditError::Config)?;
    let resolved = parse_layout_all(text, LAYOUT_FILE).map_err(EditError::Config)?;
    let mut editor = LayoutEditor::new(text, parsed.extends.is_some())?;
    let contributes = &manifest.contributes;

    let mut toolbars: IndexMap<String, Vec<ToolbarItem>> = IndexMap::new();
    let mut menus: IndexMap<String, Vec<MenuItem>> = IndexMap::new();
    let mut panels: Vec<&PluginPlacement> = Vec::new();
    for placement in &contributes.placements {
        match (&placement.toolbar, &placement.menu, &placement.command) {
            (Some(toolbar), _, Some(command)) => {
                let items = toolbars
                    .entry(toolbar.clone())
                    .or_insert_with(|| resolved.toolbars.get(toolbar).cloned().unwrap_or_default());
                let action = plugin_command_id(plugin, command);
                insert_item(
                    items,
                    &action,
                    placement,
                    |item| match item {
                        ToolbarItem::Action { action, .. } => Some(action.as_str()),
                        _ => None,
                    },
                    |action| ToolbarItem::Action {
                        action,
                        args: Vec::new(),
                        label: None,
                        icon: None,
                    },
                );
            }
            (None, Some(menu), Some(command)) => {
                let items = menus
                    .entry(menu.clone())
                    .or_insert_with(|| resolved.menus.get(menu).cloned().unwrap_or_default());
                let action = plugin_command_id(plugin, command);
                insert_item(
                    items,
                    &action,
                    placement,
                    |item| match item {
                        MenuItem::Action { action, .. } => Some(action.as_str()),
                        _ => None,
                    },
                    |action| MenuItem::Action {
                        action,
                        args: Vec::new(),
                        label: None,
                        icon: None,
                    },
                );
            }
            _ if placement.panel.is_some() => panels.push(placement),
            _ => {}
        }
    }

    for (name, items) in &toolbars {
        if resolved.toolbars.get(name) != Some(items) {
            editor.set_toolbar(name, items)?;
        }
    }
    for (name, items) in &menus {
        if resolved.menus.get(name) != Some(items) {
            set_menu(&mut editor, name, items)?;
        }
    }

    let mut bound: Vec<(String, String)> = resolved
        .keys
        .iter()
        .map(|k| (k.context.clone(), k.key.clone()))
        .collect();
    for key in &contributes.keys {
        let Ok(normalized) = normalize_key(&key.key) else {
            continue;
        };
        let slot = (key.context.clone(), normalized);
        // A key the context or the global keys already bind stays the
        // user's (a context binding would hide the global one).
        let global = ("global".to_owned(), slot.1.clone());
        if bound.contains(&slot) || bound.contains(&global) {
            continue;
        }
        let action = plugin_command_id(plugin, &key.command);
        editor.set_binding(&slot.0, &slot.1, Some((&action, &Vec::new())))?;
        bound.push(slot);
    }

    let placed = placed_panels(&resolved);
    for placement in panels {
        let Some(panel) = &placement.panel else {
            continue;
        };
        let id = plugin_panel_id(plugin, panel);
        if !placed.contains(&id) {
            place_panel(&mut editor, &id, placement)?;
        }
    }

    let out = editor.finish();
    if out == text {
        return Ok(None);
    }
    if let Err(errors) = parse_layout_all(&out, LAYOUT_FILE) {
        return Err(EditError::Invalid(
            crate::l10n::msg("config-edit-makes-invalid")
                .arg("file", LAYOUT_FILE)
                .arg("error", errors[0].to_string()),
        ));
    }
    Ok(Some(out))
}

/// Inserts an action item after / before the anchor (else at the end),
/// unless it is already there.
fn insert_item<T>(
    items: &mut Vec<T>,
    action: &str,
    placement: &PluginPlacement,
    action_of: impl Fn(&T) -> Option<&str>,
    make: impl FnOnce(String) -> T,
) {
    if items.iter().any(|i| action_of(i) == Some(action)) {
        return;
    }
    let anchor = placement.after.as_deref().or(placement.before.as_deref());
    let at = anchor.and_then(|a| items.iter().position(|i| action_of(i) == Some(a)));
    let item = make(action.to_owned());
    match at {
        Some(at) if placement.after.is_some() => items.insert(at + 1, item),
        Some(at) => items.insert(at, item),
        None => items.push(item),
    }
}

/// Writes a whole menu (a new node, or the existing one replaced).
fn set_menu(editor: &mut LayoutEditor, name: &str, items: &[MenuItem]) -> Result<(), EditError> {
    let style = editor.style.clone();
    let plain = || EditError::Invalid(crate::l10n::msg("config-edit-plain-values"));
    let sites = LayoutEditor::named_sites(editor.body(), "menu", name);
    match sites.last() {
        None => {
            let text = render::menu(name, items, 1, &style).ok_or_else(plain)?;
            editor.append(&text);
        }
        Some(&(block, index)) => {
            let depth = if block.is_some() { 2 } else { 1 };
            let text = render::menu(name, items, depth, &style).ok_or_else(plain)?;
            let body = editor.body();
            let parent = match block {
                None => body,
                Some(top) => body.nodes_mut()[top].ensure_children(),
            };
            replace_rendered(parent, index, &text);
        }
    }
    Ok(())
}

/// Every plug-in panel the layout places, in any window.
fn placed_panels(layout: &ResolvedLayout) -> HashSet<String> {
    fn walk(node: &LayoutNode, out: &mut HashSet<String>) {
        if let LayoutNode::Widget { kind, options, .. } = node
            && kind == PLUGIN_PANEL_WIDGET
            && let Some(panel) = options.get("panel").and_then(|v| v.as_str())
        {
            out.insert(panel.to_owned());
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    let mut out = HashSet::new();
    for breakpoints in layout.windows.values() {
        for breakpoint in breakpoints {
            walk(&breakpoint.root, &mut out);
        }
    }
    out
}

/// Containers of the layout tree (widgets' children are rows, not nodes).
const CONTAINERS: &[&str] = &["split", "stack", "tabs"];

/// Size lists of a container, with the value a new child gets.
fn list_default(container: &str, prop: &str) -> Option<&'static str> {
    match (container, prop) {
        ("split", "sizes") => Some("280px"),
        (_, "sizes") => Some("auto"),
        (_, "min-sizes" | "max-sizes") => Some("none"),
        (_, "collapsible") => Some("true"),
        _ => None,
    }
}

/// Places `plugin-panel "<panel>"` in every breakpoint of the main window:
/// after / before the first widget of the anchor kind, else as the last
/// child of the window's root container.
fn place_panel(
    editor: &mut LayoutEditor,
    panel: &str,
    placement: &PluginPlacement,
) -> Result<(), EditError> {
    if LayoutEditor::window_sites(editor.body(), MAIN_WINDOW).is_empty() {
        if editor_is_sparse(editor) {
            editor.copy_default_window(MAIN_WINDOW)?;
        } else {
            return Ok(());
        }
    }
    let style = editor.style.clone();
    let anchor = placement.after.as_deref().or(placement.before.as_deref());
    let after = placement.after.is_some();
    let sites = LayoutEditor::window_sites(editor.body(), MAIN_WINDOW);
    for site in sites {
        let body = editor.body();
        let window = match site.inner {
            None => &mut body.nodes_mut()[site.top],
            Some(inner) => &mut body.nodes_mut()[site.top].ensure_children().nodes_mut()[inner],
        };
        // Depth of the window node: 1 at the top, 2 inside a breakpoint.
        let window_depth = if site.inner.is_some() { 2 } else { 1 };
        let Some(root) = window
            .children_mut()
            .as_mut()
            .and_then(|c| c.nodes_mut().first_mut())
        else {
            continue;
        };
        let found = anchor.and_then(|kind| find_widget(root, kind, window_depth + 1));
        match found {
            Some((path, depth)) => {
                let (parent, index) = follow(root, &path);
                let at = if after { index + 1 } else { index };
                insert_child(parent, at, panel, depth, &style);
            }
            None => {
                if !CONTAINERS.contains(&root.name().value()) {
                    continue;
                }
                let count = root.children().map_or(0, |c| c.nodes().len());
                insert_child(root, count, panel, window_depth + 2, &style);
            }
        }
    }
    Ok(())
}

fn editor_is_sparse(editor: &mut LayoutEditor) -> bool {
    editor
        .root_node()
        .entries()
        .iter()
        .any(|e| e.name().is_some_and(|n| n.value() == "extends"))
}

/// The path (child indices below `root`) of the first widget of `kind` and
/// the depth of its line.
fn find_widget(root: &KdlNode, kind: &str, depth: usize) -> Option<(Vec<usize>, usize)> {
    if !CONTAINERS.contains(&root.name().value()) {
        return None;
    }
    for (i, child) in root
        .children()
        .map_or(&[][..], KdlDocument::nodes)
        .iter()
        .enumerate()
    {
        if child.name().value() == kind {
            return Some((vec![i], depth + 1));
        }
        if let Some((mut path, d)) = find_widget(child, kind, depth + 1) {
            path.insert(0, i);
            return Some((path, d));
        }
    }
    None
}

/// The parent of the node at `path` and its index there.
fn follow<'a>(root: &'a mut KdlNode, path: &[usize]) -> (&'a mut KdlNode, usize) {
    let (last, rest) = path.split_last().expect("a non-empty path");
    let mut node = root;
    for &i in rest {
        node = &mut node.ensure_children().nodes_mut()[i];
    }
    (node, *last)
}

/// Inserts `plugin-panel "<panel>"` as the `at`-th child of `parent` (its
/// line at `depth`) and extends the parent's size lists.
fn insert_child(parent: &mut KdlNode, at: usize, panel: &str, depth: usize, style: &Style) {
    let container = parent.name().value().to_owned();
    let count = parent.children().map_or(0, |c| c.nodes().len());
    for prop in ["sizes", "min-sizes", "max-sizes", "collapsible"] {
        let Some(value) = list_default(&container, prop) else {
            continue;
        };
        if let Some(list) = str_prop(parent, prop) {
            let mut parts: Vec<&str> = list.split_whitespace().collect();
            if parts.len() == count {
                parts.insert(at.min(parts.len()), value);
                let joined = parts.join(" ");
                set_prop(parent, prop, KdlValue::String(joined));
            }
        }
    }
    let line = format!(
        "{}{PLUGIN_PANEL_WIDGET} {}{}",
        style.pad(depth),
        quote(panel),
        style.nl
    );
    let block = parent.ensure_children();
    ensure_multiline(block, depth, style);
    let mut nodes = parse_rendered(&line);
    let node = nodes.remove(0);
    let nodes_len = block.nodes().len();
    if at >= nodes_len {
        append_rendered(block, depth, &line, style);
    } else {
        block.nodes_mut().insert(at, node);
    }
}

/// A quoted KDL string (panel ids are kebab-case with one dot).
fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(contributes: &str) -> PluginManifest {
        let text = format!(
            r#"{{"id":"follow-up","name":"Follow up","version":"1.0.0","apiVersion":"1","main":"index.js","contributes":{contributes}}}"#
        );
        PluginManifest::parse(text.as_bytes()).unwrap_or_else(|issues| panic!("{issues:?}"))
    }

    fn follow_up() -> PluginManifest {
        manifest(
            r#"{
                "commands":[{"id":"follow-up","title":"Follow up"}],
                "panels":[{"id":"due","title":"Due"}],
                "placements":[
                    {"toolbar":"reader","after":"mail.forward","command":"follow-up"},
                    {"menu":"context:message-list.row","before":"mail.delete","command":"follow-up"},
                    {"panel":"due","after":"message-list"}
                ],
                "keys":[{"key":"shift+f","command":"follow-up","context":"list"}]
            }"#,
        )
    }

    fn id() -> PluginId {
        PluginId::new("follow-up")
    }

    fn place(current: Option<&str>, manifest: &PluginManifest) -> Option<String> {
        placement_layout(current, &id(), manifest).unwrap()
    }

    fn toolbar_actions(layout: &ResolvedLayout, name: &str) -> Vec<String> {
        layout.toolbars[name]
            .iter()
            .filter_map(|i| match i {
                ToolbarItem::Action { action, .. } => Some(action.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_new_file_gets_every_placement() {
        let text = place(None, &follow_up()).expect("changes");
        assert!(text.starts_with("// Changes made in Nail."), "{text}");
        let layout = parse_layout_all(&text, LAYOUT_FILE).unwrap();
        let reader = toolbar_actions(&layout, "reader");
        let at = reader.iter().position(|a| a == "mail.forward").unwrap();
        assert_eq!(reader[at + 1], "plugin.follow-up.follow-up");
        let menu: Vec<&str> = layout.menus["context:message-list.row"]
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { action, .. } => Some(action.as_str()),
                _ => None,
            })
            .collect();
        let delete = menu.iter().position(|a| *a == "mail.delete").unwrap();
        assert_eq!(menu[delete - 1], "plugin.follow-up.follow-up");
        assert!(layout.keys.iter().any(|k| k.context == "list"
            && k.key == "shift+f"
            && k.action == "plugin.follow-up.follow-up"));
        // The panel sits after the message list in both breakpoints (inside
        // the list pane's stack, under the automatic-replies banner, the Gmail category / Focused tabs and the
        // list); the stack's size list grew with it.
        assert_eq!(
            placed_panels(&layout),
            HashSet::from(["follow-up.due".to_owned()])
        );
        assert_eq!(text.matches("plugin-panel \"follow-up.due\"").count(), 2);
        assert_eq!(
            text.matches(
                "stack id=\"list-pane\" label=\"@layout.pane.messages\" sizes=\"auto auto auto 1fr auto\""
            )
            .count(),
            2,
            "{text}"
        );
    }

    #[test]
    fn placing_again_changes_nothing() {
        let once = place(None, &follow_up()).unwrap();
        assert_eq!(place(Some(&once), &follow_up()), None);
    }

    #[test]
    fn the_file_is_edited_in_place() {
        let mine = "// My layout\nlayout \"mine\" extends=\"default\" {\n    // my keys\n    keys context=\"list\" {\n        bind \"e\" \"mail.archive\" // muscle memory\n    }\n    toolbar \"reader\" {\n        item \"mail.reply\"\n        // forwarding\n        item \"mail.forward\"\n        item \"mail.delete\"\n    }\n}\n";
        let text = place(
            Some(mine),
            &manifest(
                r#"{"commands":[{"id":"follow-up","title":"F"}],
                    "placements":[{"toolbar":"reader","after":"mail.forward","command":"follow-up"}],
                    "keys":[{"key":"shift+f","command":"follow-up","context":"list"}]}"#,
            ),
        )
        .unwrap();
        assert_eq!(
            text,
            "// My layout\nlayout \"mine\" extends=\"default\" {\n    // my keys\n    keys context=\"list\" {\n        bind \"e\" \"mail.archive\" // muscle memory\n        bind \"shift+f\" \"plugin.follow-up.follow-up\"\n    }\n    toolbar \"reader\" {\n        item \"mail.reply\"\n        // forwarding\n        item \"mail.forward\"\n        item \"plugin.follow-up.follow-up\"\n        item \"mail.delete\"\n    }\n}\n"
        );
    }

    #[test]
    fn taken_keys_and_existing_items_are_left_alone() {
        // `f` is the default forward key in the list; the toolbar item
        // exists already.
        let mine = "layout \"mine\" extends=\"default\" {\n    toolbar \"reader\" {\n        item \"plugin.follow-up.follow-up\"\n    }\n}\n";
        let m = manifest(
            r#"{"commands":[{"id":"follow-up","title":"F"}],
                "placements":[{"toolbar":"reader","command":"follow-up"}],
                "keys":[{"key":"f","command":"follow-up","context":"list"}]}"#,
        );
        assert_eq!(place(Some(mine), &m), None);
    }

    #[test]
    fn missing_toolbars_and_menus_are_created_and_anchors_fall_back_to_the_end() {
        let m = manifest(
            r#"{"commands":[{"id":"follow-up","title":"F"}],
                "placements":[
                    {"toolbar":"follow-up-bar","command":"follow-up"},
                    {"toolbar":"reader","after":"no.such-action","command":"follow-up"},
                    {"menu":"app","command":"follow-up"}
                ]}"#,
        );
        let text = place(None, &m).unwrap();
        let layout = parse_layout_all(&text, LAYOUT_FILE).unwrap();
        assert_eq!(
            toolbar_actions(&layout, "follow-up-bar"),
            ["plugin.follow-up.follow-up"]
        );
        assert_eq!(
            toolbar_actions(&layout, "reader")
                .last()
                .map(String::as_str),
            Some("plugin.follow-up.follow-up")
        );
        assert!(matches!(
            layout.menus["app"].last(),
            Some(MenuItem::Action { action, .. }) if action == "plugin.follow-up.follow-up"
        ));
    }

    #[test]
    fn panels_without_an_anchor_go_to_the_end_of_the_main_window() {
        let m = manifest(
            r#"{"panels":[{"id":"due","title":"Due"}],
                "placements":[{"panel":"due","before":"no-such-widget"}]}"#,
        );
        let mine = "layout \"mine\" {\n    window \"main\" {\n        stack sizes=\"1fr\" {\n            message-list\n        }\n    }\n}\n";
        let text = place(Some(mine), &m).unwrap();
        assert_eq!(
            text,
            "layout \"mine\" {\n    window \"main\" {\n        stack sizes=\"1fr auto\" {\n            message-list\n            plugin-panel \"follow-up.due\"\n        }\n    }\n}\n"
        );
        // A main window that is a single widget has no place for it.
        let single = "layout \"mine\" {
    window \"main\" {
        message-list
    }
}
";
        assert_eq!(place(Some(single), &m), None);
    }

    #[test]
    fn panels_before_an_anchor() {
        let m = manifest(
            r#"{"panels":[{"id":"due","title":"Due"}],
                "placements":[{"panel":"due","before":"message-list"}]}"#,
        );
        let mine = "layout \"mine\" {\n    window \"main\" {\n        split \"horizontal\" sizes=\"1fr 2fr\" {\n            folder-tree\n            message-list\n        }\n    }\n}\n";
        let text = place(Some(mine), &m).unwrap();
        assert_eq!(
            text,
            "layout \"mine\" {\n    window \"main\" {\n        split \"horizontal\" sizes=\"1fr 280px 2fr\" {\n            folder-tree\n            plugin-panel \"follow-up.due\"\n            message-list\n        }\n    }\n}\n"
        );
    }

    #[test]
    fn an_invalid_file_is_an_error() {
        let err = placement_layout(Some("layout \"x\" {"), &id(), &follow_up()).unwrap_err();
        assert!(matches!(err, EditError::Config(_)));
    }

    #[test]
    fn installed_plugins_list_commands_and_panels() {
        let mut installed = InstalledPlugins::default();
        installed.add(&id(), &follow_up());
        assert!(installed.actions.contains("plugin.follow-up.follow-up"));
        assert!(installed.panels.contains("follow-up.due"));
    }
}
