//! Context menus (right-click) resolved from `layout.kdl` (M2c, ADR 0043).
//!
//! The layout's `menu "context:<part>" { … }` nodes
//! ([`crate::ResolvedLayout::menus`]) define which actions a part's context
//! menu offers and in which order. [`resolve_menu`] turns such a menu into
//! what the UI renders: every entry with its label and icon (item override,
//! else the layout's action metadata, else `null` = the action's built-in
//! label) and the key binding shown as its shortcut; separators are
//! normalized (no leading, trailing or doubled ones) and empty submenus are
//! dropped. The UI implements the same function over the same types and is
//! tested against this one's behaviour.
//!
//! Which parts have a context menu, and which key context supplies their
//! shortcuts, is [`CONTEXT_MENU_PARTS`]. Actions receive the right-clicked
//! item through the UI's normal action context (the selection).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::config::layout::{MenuItem, ResolvedLayout};

/// Prefix of context-menu names in the layout.
pub const CONTEXT_MENU_PREFIX: &str = "context:";

/// The layout menu name of a part's context menu, e.g.
/// `"context:message-list.row"`.
pub fn context_menu_name(part: &str) -> String {
    format!("{CONTEXT_MENU_PREFIX}{part}")
}

/// A part that has a context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextMenuPart {
    /// The part name (`data-part`), e.g. `"message-list.row"`.
    pub part: &'static str,
    /// The key-binding context whose bindings are shown as shortcuts
    /// (`global` bindings are shown too).
    pub key_context: &'static str,
}

/// Parts with context menus in M2c.
pub const CONTEXT_MENU_PARTS: &[ContextMenuPart] = &[
    ContextMenuPart {
        part: "message-list.row",
        key_context: "list",
    },
    ContextMenuPart {
        part: "thread-view.message",
        key_context: "reader",
    },
    ContextMenuPart {
        part: "message-view",
        key_context: "reader",
    },
    ContextMenuPart {
        part: "message-view.attachment",
        key_context: "reader",
    },
    ContextMenuPart {
        part: "folder-tree.folder",
        key_context: "global",
    },
    ContextMenuPart {
        part: "folder-tree.account",
        key_context: "global",
    },
    ContextMenuPart {
        part: "composer",
        key_context: "composer",
    },
];

/// The key context of a part's context menu (`"global"` for parts not in
/// [`CONTEXT_MENU_PARTS`]).
pub fn key_context_of(part: &str) -> &'static str {
    CONTEXT_MENU_PARTS
        .iter()
        .find(|p| p.part == part)
        .map_or("global", |p| p.key_context)
}

/// One entry of a resolved menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ResolvedMenuEntry {
    /// An item invoking an action.
    Action {
        /// Action id.
        action: String,
        /// Action arguments.
        #[serde(default)]
        args: Vec<Value>,
        /// Label (literal or `@message.id`), `null` = the action's
        /// built-in label.
        label: Option<String>,
        /// Icon name, `null` = none.
        icon: Option<String>,
        /// The key sequence bound to the same action and arguments in the
        /// menu's key context (else `global`), `null` = none.
        shortcut: Option<String>,
    },
    /// A separator line.
    Separator,
    /// A nested menu (never empty).
    Submenu {
        /// Label (literal or `@message.id`).
        label: String,
        /// Entries.
        items: Vec<ResolvedMenuEntry>,
    },
}

/// A resolved menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ResolvedMenu {
    /// The layout name, e.g. `"context:message-list.row"`.
    pub name: String,
    /// Entries (never empty; a menu without actions resolves to `None`).
    pub items: Vec<ResolvedMenuEntry>,
}

/// Resolves the layout menu `name`; `key_context` selects the shortcuts
/// (context menus: [`key_context_of`] of the part). `None` when the layout
/// has no such menu or it has no action.
pub fn resolve_menu(
    layout: &ResolvedLayout,
    name: &str,
    key_context: &str,
) -> Option<ResolvedMenu> {
    let items = layout.menus.get(name)?;
    let items = resolve_items(layout, items, key_context);
    (!items.is_empty()).then(|| ResolvedMenu {
        name: name.to_owned(),
        items,
    })
}

fn resolve_items(
    layout: &ResolvedLayout,
    items: &[MenuItem],
    key_context: &str,
) -> Vec<ResolvedMenuEntry> {
    let mut out: Vec<ResolvedMenuEntry> = Vec::new();
    for item in items {
        match item {
            MenuItem::Action {
                action,
                args,
                label,
                icon,
            } => {
                let def = layout.actions.get(action);
                out.push(ResolvedMenuEntry::Action {
                    action: action.clone(),
                    args: args.clone(),
                    label: label.clone().or_else(|| def.and_then(|d| d.label.clone())),
                    icon: icon.clone().or_else(|| def.and_then(|d| d.icon.clone())),
                    shortcut: shortcut(layout, action, args, key_context),
                });
            }
            MenuItem::Separator => {
                if out
                    .last()
                    .is_some_and(|e| !matches!(e, ResolvedMenuEntry::Separator))
                {
                    out.push(ResolvedMenuEntry::Separator);
                }
            }
            MenuItem::Submenu { label, items } => {
                let items = resolve_items(layout, items, key_context);
                if !items.is_empty() {
                    out.push(ResolvedMenuEntry::Submenu {
                        label: label.clone(),
                        items,
                    });
                }
            }
        }
    }
    if matches!(out.last(), Some(ResolvedMenuEntry::Separator)) {
        out.pop();
    }
    out
}

/// The key bound to `action` + `args`: the last binding in `key_context`,
/// else the last `global` one (later bindings win, like the key handler).
pub fn shortcut(
    layout: &ResolvedLayout,
    action: &str,
    args: &[Value],
    key_context: &str,
) -> Option<String> {
    let find = |context: &str| {
        layout
            .keys
            .iter()
            .rev()
            .find(|k| k.context == context && k.action == action && k.args == args)
            .map(|k| k.key.clone())
    };
    find(key_context).or_else(|| find("global"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::layout::{ActionDef, KeyBinding};
    use indexmap::IndexMap;
    use serde_json::json;

    fn item(action: &str) -> MenuItem {
        MenuItem::Action {
            action: action.into(),
            args: vec![],
            label: None,
            icon: None,
        }
    }

    fn layout(menu: Vec<MenuItem>) -> ResolvedLayout {
        let mut actions = IndexMap::new();
        actions.insert(
            "mail.archive".to_owned(),
            ActionDef {
                label: Some("@mail.action.archive".into()),
                icon: Some("archive".into()),
                ..ActionDef::default()
            },
        );
        let mut menus = IndexMap::new();
        menus.insert(context_menu_name("message-list.row"), menu);
        ResolvedLayout {
            name: "t".into(),
            windows: IndexMap::new(),
            actions,
            keys: vec![
                KeyBinding {
                    context: "global".into(),
                    key: "ctrl+r".into(),
                    action: "mail.reply".into(),
                    args: vec![],
                },
                KeyBinding {
                    context: "list".into(),
                    key: "e".into(),
                    action: "mail.archive".into(),
                    args: vec![],
                },
                KeyBinding {
                    context: "list".into(),
                    key: "a".into(),
                    action: "mail.archive".into(),
                    args: vec![],
                },
                KeyBinding {
                    context: "reader".into(),
                    key: "r".into(),
                    action: "mail.reply".into(),
                    args: vec![],
                },
            ],
            toolbars: IndexMap::new(),
            menus,
        }
    }

    #[test]
    fn resolves_labels_icons_and_shortcuts() {
        let l = layout(vec![
            MenuItem::Separator,
            item("mail.reply"),
            MenuItem::Separator,
            MenuItem::Separator,
            item("mail.archive"),
            MenuItem::Action {
                action: "view.toggle".into(),
                args: vec![json!("reader")],
                label: Some("Reader".into()),
                icon: None,
            },
            MenuItem::Submenu {
                label: "@menu.empty".into(),
                items: vec![MenuItem::Separator],
            },
            MenuItem::Separator,
        ]);
        let name = context_menu_name("message-list.row");
        assert_eq!(name, "context:message-list.row");
        let menu = resolve_menu(&l, &name, key_context_of("message-list.row")).unwrap();
        assert_eq!(
            menu.items,
            vec![
                ResolvedMenuEntry::Action {
                    action: "mail.reply".into(),
                    args: vec![],
                    label: None,
                    icon: None,
                    // Not bound in `list`: the global binding.
                    shortcut: Some("ctrl+r".into()),
                },
                ResolvedMenuEntry::Separator,
                ResolvedMenuEntry::Action {
                    action: "mail.archive".into(),
                    args: vec![],
                    label: Some("@mail.action.archive".into()),
                    icon: Some("archive".into()),
                    // The later binding wins.
                    shortcut: Some("a".into()),
                },
                ResolvedMenuEntry::Action {
                    action: "view.toggle".into(),
                    args: vec![json!("reader")],
                    label: Some("Reader".into()),
                    icon: None,
                    shortcut: None,
                },
            ]
        );
        assert_eq!(
            serde_json::to_value(&menu.items[1]).unwrap(),
            json!({"type": "separator"})
        );
    }

    #[test]
    fn empty_and_missing_menus_resolve_to_none() {
        let l = layout(vec![MenuItem::Separator]);
        assert!(resolve_menu(&l, "context:message-list.row", "list").is_none());
        assert!(resolve_menu(&l, "context:nothing", "list").is_none());
        assert_eq!(key_context_of("composer"), "composer");
        assert_eq!(key_context_of("unknown.part"), "global");
        let parts: std::collections::HashSet<_> =
            CONTEXT_MENU_PARTS.iter().map(|p| p.part).collect();
        assert_eq!(parts.len(), CONTEXT_MENU_PARTS.len());
    }
}
