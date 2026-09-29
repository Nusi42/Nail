//! The resolved layout: the output of `nail-config` for `layout.kdl`, and the
//! input of the UI's layout engine.
//!
//! # Labels
//!
//! Every `label` in the layout is either literal text or a Fluent message
//! reference written `@<id>`, e.g. `"@mail.action.reply"`. The UI looks the id
//! up in the Fluent catalogs with dots replaced by hyphens
//! (`mail-action-reply`), because Fluent identifiers cannot contain dots.
//! See [`label_message_id`].
//!
//! # Actions
//!
//! Toolbars, menus and key bindings all refer to actions by id
//! (`mail.reply`, `view.toggle`, `plugin.<id>.<name>`), optionally with
//! arguments (`view.toggle "reader"` → `action: "view.toggle", args: ["reader"]`).
//! The layout's [`ResolvedLayout::actions`] table carries display metadata
//! (label, icon) for every action the layout uses, and may define aliases.
//!
//! # Breakpoints
//!
//! Each window has an ordered list of [`Breakpoint`]s. The renderer uses the
//! first breakpoint whose (inclusive) width range contains the current window
//! width; a breakpoint without bounds always matches.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::SourceLocation;

/// Returns the Fluent message id for a label written as `@id`, or `None` for
/// literal text. Dots become hyphens: `"@mail.action.reply"` →
/// `Some("mail-action-reply")`.
pub fn label_message_id(label: &str) -> Option<String> {
    label.strip_prefix('@').map(|id| id.replace('.', "-"))
}

/// Direction in which a [`LayoutNode::Split`] arranges its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    /// Children side by side, left to right (dividers are vertical lines).
    #[default]
    Horizontal,
    /// Children top to bottom (dividers are horizontal lines).
    Vertical,
}

/// A node of a window tree: a container or a widget.
///
/// Serialized with a `type` tag: `"split"`, `"stack"`, `"tabs"` or `"widget"`.
/// Further containers from the design (`grid`, `dock`, `scroll`, `overlay`)
/// will be added as new variants. Every node carries an optional `id` (stable
/// handle for write-back, `view.toggle` and the inspector) and an optional
/// `label` (tab title and accessible name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LayoutNode {
    /// Resizable panes in one direction.
    Split {
        /// Optional stable node id.
        #[serde(default)]
        id: Option<String>,
        /// Optional label (see the module docs for `@id` labels).
        #[serde(default)]
        label: Option<String>,
        /// Arrangement direction.
        #[serde(default)]
        orientation: Orientation,
        /// Initial size of each child as a CSS track size (`"240px"`, `"1fr"`,
        /// `"30%"`). Empty means equal sizes; otherwise one entry per child.
        #[serde(default)]
        sizes: Vec<String>,
        /// Minimum size per child (`null` = none). Empty or one entry per child.
        #[serde(default)]
        min_sizes: Vec<Option<String>>,
        /// Maximum size per child (`null` = none). Empty or one entry per child.
        #[serde(default)]
        max_sizes: Vec<Option<String>>,
        /// Whether each child can be collapsed. Empty or one entry per child.
        #[serde(default)]
        collapsible: Vec<bool>,
        /// Whether the user can drag the dividers.
        #[serde(default = "default_true")]
        resizable: bool,
        /// Child nodes, in order.
        children: Vec<LayoutNode>,
    },
    /// Children stacked top to bottom, without dividers.
    Stack {
        /// Optional stable node id.
        #[serde(default)]
        id: Option<String>,
        /// Optional label.
        #[serde(default)]
        label: Option<String>,
        /// Height of each child as a CSS track size (`"auto"` = natural
        /// height, `"1fr"` = share of the remaining space). Empty means the
        /// renderer's default per child: `"auto"` for bar-like widgets
        /// (`toolbar`, `status-bar`, `account-switcher`, `text`), `"1fr"` for
        /// everything else. Otherwise one entry per child.
        #[serde(default)]
        sizes: Vec<String>,
        /// Child nodes, top to bottom.
        children: Vec<LayoutNode>,
    },
    /// One child visible at a time, with a tab strip. Tab titles come from the
    /// children's `label`s (falling back to the widget kind's default label).
    Tabs {
        /// Optional stable node id.
        #[serde(default)]
        id: Option<String>,
        /// Optional label.
        #[serde(default)]
        label: Option<String>,
        /// Index of the initially active child.
        #[serde(default)]
        active: u32,
        /// Child nodes, one per tab.
        children: Vec<LayoutNode>,
    },
    /// A leaf widget from the widget registry.
    Widget {
        /// Optional stable node id.
        #[serde(default)]
        id: Option<String>,
        /// Optional label.
        #[serde(default)]
        label: Option<String>,
        /// Widget kind, e.g. `"folder-tree"`, `"message-list"`,
        /// `"message-view"`, `"account-switcher"`, `"toolbar"`, `"status-bar"`,
        /// `"text"`, `"spacer"`, or a plug-in widget `plugin.<id>.<name>`.
        kind: String,
        /// Typed options of the widget kind, as given in the file (e.g.
        /// `{"show-unread": true}` for `folder-tree`, `{"toolbar": "reader"}`
        /// for `toolbar`, `{"headers": "from to cc date"}` for
        /// `message-view`). Keys are kebab-case as in KDL. Validated by
        /// `nail-config` against the widget's option schema.
        #[serde(default)]
        options: serde_json::Map<String, Value>,
        /// Row templates (message-list rows, header lines), in the template
        /// language: `{from.name}`, `{date | relative}`, `{tags | chips}`, …
        #[serde(default)]
        rows: Vec<String>,
        /// Where the widget was placed, for the inspector.
        #[serde(default)]
        source: Option<SourceLocation>,
    },
}

fn default_true() -> bool {
    true
}

impl LayoutNode {
    /// The node's children (empty for widgets).
    pub fn children(&self) -> &[LayoutNode] {
        match self {
            Self::Split { children, .. }
            | Self::Stack { children, .. }
            | Self::Tabs { children, .. } => children,
            Self::Widget { .. } => &[],
        }
    }

    /// The node's optional id.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Split { id, .. }
            | Self::Stack { id, .. }
            | Self::Tabs { id, .. }
            | Self::Widget { id, .. } => id.as_deref(),
        }
    }

    /// Visits this node and all descendants depth-first, parents first.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a LayoutNode)) {
        f(self);
        for child in self.children() {
            child.walk(f);
        }
    }
}

/// One alternative window tree for a range of window widths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Breakpoint {
    /// Minimum window width in CSS pixels (inclusive), `null` = no minimum.
    #[serde(default)]
    pub min_width: Option<u32>,
    /// Maximum window width in CSS pixels (inclusive), `null` = no maximum.
    #[serde(default)]
    pub max_width: Option<u32>,
    /// The window tree for this range.
    pub root: LayoutNode,
}

impl Breakpoint {
    /// Whether the breakpoint applies to a window of the given width.
    pub fn matches(&self, width: u32) -> bool {
        self.min_width.is_none_or(|min| width >= min)
            && self.max_width.is_none_or(|max| width <= max)
    }
}

/// Display metadata for an action, and optionally an alias definition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ActionDef {
    /// Label (literal or `@message.id`), `null` = the built-in default label.
    pub label: Option<String>,
    /// Icon name from the theme's `icons`, `null` = no icon.
    pub icon: Option<String>,
    /// For user-defined aliases: the action this one invokes, e.g.
    /// `"nav.goto-folder"`. `null` for built-in and plug-in actions.
    pub alias_of: Option<String>,
    /// Arguments passed to [`Self::alias_of`].
    pub args: Vec<Value>,
}

/// A key binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct KeyBinding {
    /// Context in which the binding is active: `"global"`, `"list"`,
    /// `"reader"`, `"composer"`, `"calendar"`, …
    #[serde(default = "default_context")]
    pub context: String,
    /// Key sequence. Chords join keys with `+` (`"ctrl+shift+m"`); sequences
    /// separate steps with a space (`"g i"`). Key names follow
    /// `KeyboardEvent.key`, lowercased (`"j"`, `"enter"`, `"arrowdown"`).
    pub key: String,
    /// Action id.
    pub action: String,
    /// Action arguments.
    #[serde(default)]
    pub args: Vec<Value>,
}

fn default_context() -> String {
    "global".to_owned()
}

/// An entry of a toolbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolbarItem {
    /// A button invoking an action.
    Action {
        /// Action id.
        action: String,
        /// Action arguments.
        #[serde(default)]
        args: Vec<Value>,
        /// Label override, `null` = the action's label.
        #[serde(default)]
        label: Option<String>,
        /// Icon override, `null` = the action's icon.
        #[serde(default)]
        icon: Option<String>,
    },
    /// Flexible space pushing the following items to the end.
    Spacer,
    /// A visual separator.
    Separator,
}

/// An entry of a menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MenuItem {
    /// An item invoking an action.
    Action {
        /// Action id.
        action: String,
        /// Action arguments.
        #[serde(default)]
        args: Vec<Value>,
        /// Label override, `null` = the action's label.
        #[serde(default)]
        label: Option<String>,
        /// Icon override, `null` = the action's icon.
        #[serde(default)]
        icon: Option<String>,
    },
    /// A separator line.
    Separator,
    /// A nested menu.
    Submenu {
        /// Submenu label.
        label: String,
        /// Submenu entries.
        items: Vec<MenuItem>,
    },
}

/// A fully resolved layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ResolvedLayout {
    /// Layout name from `layout "<name>"`.
    pub name: String,
    /// Window trees by window type (`"main"`, `"composer"`, `"message"`,
    /// `"mobile"`, …): an ordered list of breakpoints, first match wins.
    pub windows: IndexMap<String, Vec<Breakpoint>>,
    /// Action metadata and aliases by action id.
    #[serde(default)]
    pub actions: IndexMap<String, ActionDef>,
    /// Key bindings in source order (later bindings win on conflicts).
    #[serde(default)]
    pub keys: Vec<KeyBinding>,
    /// Toolbars by name (referenced by `toolbar` widgets via their
    /// `toolbar` option), e.g. `"reader"`.
    #[serde(default)]
    pub toolbars: IndexMap<String, Vec<ToolbarItem>>,
    /// Menus by name: `"app"` for the application menu, `"context:<part>"`
    /// for the context menu of a part (e.g. `"context:message-list.row"`).
    #[serde(default)]
    pub menus: IndexMap<String, Vec<MenuItem>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn label_ids() {
        assert_eq!(
            label_message_id("@mail.action.reply").as_deref(),
            Some("mail-action-reply")
        );
        assert_eq!(label_message_id("Reply"), None);
    }

    #[test]
    fn breakpoint_ranges_are_inclusive() {
        let widget = LayoutNode::Widget {
            id: None,
            label: None,
            kind: "spacer".into(),
            options: Default::default(),
            rows: vec![],
            source: None,
        };
        let bp = Breakpoint {
            min_width: Some(900),
            max_width: None,
            root: widget.clone(),
        };
        assert!(bp.matches(900) && bp.matches(5000) && !bp.matches(899));
        let bp = Breakpoint {
            min_width: None,
            max_width: Some(899),
            root: widget,
        };
        assert!(bp.matches(0) && bp.matches(899) && !bp.matches(900));
    }

    #[test]
    fn layout_node_is_tagged_and_defaults_apply() {
        let node: LayoutNode = serde_json::from_value(json!({
            "type": "split",
            "children": [
                {"type": "widget", "kind": "folder-tree", "options": {"show-unread": true}},
                {"type": "tabs", "children": [{"type": "widget", "kind": "message-list"}]}
            ]
        }))
        .unwrap();
        let LayoutNode::Split {
            orientation,
            resizable,
            children,
            ..
        } = &node
        else {
            panic!("expected split");
        };
        assert_eq!(*orientation, Orientation::Horizontal);
        assert!(*resizable);
        assert_eq!(children.len(), 2);
        let mut kinds = Vec::new();
        node.walk(&mut |n| {
            if let LayoutNode::Widget { kind, .. } = n {
                kinds.push(kind.as_str());
            }
        });
        assert_eq!(kinds, ["folder-tree", "message-list"]);
    }

    #[test]
    fn key_binding_context_defaults_to_global() {
        let key: KeyBinding =
            serde_json::from_value(json!({"key": "j", "action": "mail.next"})).unwrap();
        assert_eq!(key.context, "global");
        assert!(key.args.is_empty());
    }

    #[test]
    fn toolbar_items_are_tagged() {
        let items: Vec<ToolbarItem> = serde_json::from_value(json!([
            {"type": "action", "action": "mail.reply"},
            {"type": "spacer"}
        ]))
        .unwrap();
        assert!(matches!(items[1], ToolbarItem::Spacer));
    }
}
