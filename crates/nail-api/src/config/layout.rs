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
//!
//! # Views, the rail and the settings page (M5b, ADR 0109)
//!
//! [`ResolvedLayout::views`] names the top-level views (Mail, Calendar,
//! Contacts, Tasks, Settings): each shows one window tree
//! ([`ViewDef::window`]) inside the `view-host` widget, and
//! [`ResolvedLayout::rail`] arranges them (plus action buttons such as the
//! confirmations) on the `nav-rail` widget. Without a `view-host` in the
//! layout, switching to a view opens its window instead.
//! [`ResolvedLayout::settings`] is the grouped settings page the
//! `settings-page` widget renders: groups → sections → a node tree each.
//!
//! # Toolbar overflow (M5b)
//!
//! Toolbar action items carry an optional `rank` and `display`
//! ([`ToolbarItem::Action`]); a toolbar may hold one [`ToolbarItem::More`]
//! menu, and [`ResolvedLayout::toolbar_settings`] says per toolbar whether
//! actions that cannot run are hidden and whether items that do not fit
//! move into "More" (in [`crate::views::overflow_order`]).

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
    /// Icon name: a Lucide name (`"reply-all"`) or one of the theme's
    /// `icons` (see [`crate::icons`]); `null` = no icon.
    pub icon: Option<String>,
    /// For user-defined aliases: the action this one invokes, e.g.
    /// `"nav.goto-folder"`. `null` for built-in and plug-in actions.
    pub alias_of: Option<String>,
    /// Arguments passed to [`Self::alias_of`].
    pub args: Vec<Value>,
}

/// A key binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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

/// How a toolbar button shows its action (M5b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ItemDisplay {
    /// Only the icon (the label is the tooltip and accessible name).
    Icon,
    /// Only the label.
    Label,
    /// Icon and label.
    IconLabel,
}

impl ItemDisplay {
    /// Every variant.
    pub const ALL: &'static [ItemDisplay] = &[Self::Icon, Self::Label, Self::IconLabel];

    /// The wire / KDL name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Icon => "icon",
            Self::Label => "label",
            Self::IconLabel => "icon-label",
        }
    }

    /// The variant with this name.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|d| d.as_str() == text)
    }
}

/// An entry of a toolbar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
        /// Icon override (a Lucide name or a theme icon, [`crate::icons`]),
        /// `null` = the action's icon.
        #[serde(default)]
        icon: Option<String>,
        /// Overflow rank (M5b): when the bar is too narrow, items move into
        /// "More" highest rank first (`rank=1` stays longest); unranked
        /// items go before every ranked one, from the end of the bar
        /// ([`crate::views::overflow_order`]). Absent = unranked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        rank: Option<u32>,
        /// Icon, label or both (M5b). Absent = the renderer's default (icon
        /// and label).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        display: Option<ItemDisplay>,
    },
    /// The "More" menu button (M5b): its own entries, followed by the
    /// items that do not fit the bar. A toolbar has at most one; an
    /// overflowing toolbar ([`ToolbarOverflow::More`]) without one gets one
    /// at the end.
    More {
        /// Label override, `null` = `@toolbar.more` ("More").
        #[serde(default)]
        label: Option<String>,
        /// Icon override, `null` = `ellipsis`.
        #[serde(default)]
        icon: Option<String>,
        /// The menu's own entries (headers, actions, separators, submenus).
        #[serde(default)]
        items: Vec<MenuItem>,
    },
    /// Flexible space pushing the following items to the end.
    Spacer,
    /// A visual separator.
    Separator,
}

impl ToolbarItem {
    /// A plain action item (no overrides, rank or display).
    pub fn action(action: impl Into<String>) -> Self {
        Self::Action {
            action: action.into(),
            args: Vec::new(),
            label: None,
            icon: None,
            rank: None,
            display: None,
        }
    }
}

/// What a toolbar does when its items do not fit (M5b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ToolbarOverflow {
    /// Items that do not fit move into the "More" menu, by rank.
    #[default]
    More,
    /// Items wrap onto further lines (the pre-M5b behaviour).
    Wrap,
}

impl ToolbarOverflow {
    /// Every variant.
    pub const ALL: &'static [ToolbarOverflow] = &[Self::More, Self::Wrap];

    /// The wire / KDL name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::More => "more",
            Self::Wrap => "wrap",
        }
    }

    /// The variant with this name.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|o| o.as_str() == text)
    }
}

/// Per-toolbar settings (M5b): `toolbar "<name>" hide-disabled=#true
/// overflow="more"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ToolbarSettings {
    /// Actions that cannot run now are hidden (not shown disabled), in the
    /// "More" menu too.
    pub hide_disabled: bool,
    /// Overflow behaviour.
    pub overflow: ToolbarOverflow,
}

/// An entry of a menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
        /// Icon override (a Lucide name or a theme icon, [`crate::icons`]),
        /// `null` = the action's icon.
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
    /// A group heading (M5b; not selectable), e.g. "Message" or "Turn into"
    /// in the reader's "More" menu.
    Header {
        /// Heading label (literal or `@message.id`).
        label: String,
    },
}

/// Where a badge on the rail takes its number from (M5b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum BadgeSource {
    /// Unread messages in the inboxes of every account (the unified inbox).
    Unread,
    /// Changes by agents / plug-ins waiting for a confirmation.
    Confirmations,
}

impl BadgeSource {
    /// Every variant.
    pub const ALL: &'static [BadgeSource] = &[Self::Unread, Self::Confirmations];

    /// The wire / KDL name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Confirmations => "confirmations",
        }
    }

    /// The variant with this name.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|b| b.as_str() == text)
    }
}

/// A top-level view (M5b): `views { view "calendar" label=… icon=…
/// window="calendar" badge=… }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ViewDef {
    /// Label (literal or `@message.id`), `null` = `@view.<id>`.
    #[serde(default)]
    pub label: Option<String>,
    /// Lucide icon name, `null` = none.
    #[serde(default)]
    pub icon: Option<String>,
    /// The window (`window "<name>"`) whose tree the view shows.
    pub window: String,
    /// Badge on the view's rail button, `null` = none.
    #[serde(default)]
    pub badge: Option<BadgeSource>,
}

/// An entry of the navigation rail (M5b): `rail { view "mail"; spacer;
/// item "agents.open-center" badge="confirmations"; view "settings" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RailItem {
    /// A button switching to a view ([`ResolvedLayout::views`]).
    View {
        /// The view id.
        view: String,
    },
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
        /// Badge, `null` = none.
        #[serde(default)]
        badge: Option<BadgeSource>,
    },
    /// Flexible space pushing the following items to the bottom.
    Spacer,
    /// A visual separator.
    Separator,
}

/// One section of the settings page (M5b): a navigation entry and the node
/// tree shown when it is selected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SettingsSection {
    /// Stable id (kebab-case, unique on the page), e.g. `"accounts"`.
    pub id: String,
    /// Label (literal or `@message.id`), `null` = `@settings.section.<id>`.
    #[serde(default)]
    pub label: Option<String>,
    /// Lucide icon name, `null` = none.
    #[serde(default)]
    pub icon: Option<String>,
    /// Extra search words (literal or `@message.id`), `null` = none.
    #[serde(default)]
    pub keywords: Option<String>,
    /// The content: the section's node, or a `stack` (every child `auto`)
    /// of its nodes when it has several.
    pub root: LayoutNode,
}

/// A group of the settings page (M5b): General, Look & feel, Security,
/// Extensions in the default layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SettingsGroup {
    /// Stable id (kebab-case, unique on the page), e.g. `"look-and-feel"`.
    pub id: String,
    /// Label (literal or `@message.id`), `null` = `@settings.group.<id>`.
    #[serde(default)]
    pub label: Option<String>,
    /// The group's sections, in navigation order.
    pub sections: Vec<SettingsSection>,
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
    /// Settings of every toolbar in [`Self::toolbars`] (M5b; defaults for
    /// toolbars that set nothing).
    #[serde(default)]
    pub toolbar_settings: IndexMap<String, ToolbarSettings>,
    /// The top-level views by id, in definition order (M5b).
    #[serde(default)]
    pub views: IndexMap<String, ViewDef>,
    /// The view shown at start (`views default="mail"`), `null` = the first
    /// view.
    #[serde(default)]
    pub default_view: Option<String>,
    /// The navigation rail's entries (M5b), top to bottom.
    #[serde(default)]
    pub rail: Vec<RailItem>,
    /// The settings page (M5b): groups of sections.
    #[serde(default)]
    pub settings: Vec<SettingsGroup>,
}

impl ResolvedLayout {
    /// The view shown at start: [`Self::default_view`], else the first
    /// view; `None` without views.
    pub fn start_view(&self) -> Option<&str> {
        self.default_view
            .as_deref()
            .filter(|id| self.views.contains_key(*id))
            .or_else(|| self.views.keys().next().map(String::as_str))
    }

    /// The settings section with this id.
    pub fn settings_section(&self, id: &str) -> Option<&SettingsSection> {
        self.settings
            .iter()
            .flat_map(|g| g.sections.iter())
            .find(|s| s.id == id)
    }
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
    fn m5b_items_serialize_compactly_and_parse_with_defaults() {
        // rank / display are left out when absent, so pre-M5b JSON and the
        // UI's hand-written items stay valid.
        assert_eq!(
            serde_json::to_value(ToolbarItem::action("mail.reply")).unwrap(),
            json!({"type": "action", "action": "mail.reply", "args": [], "label": null, "icon": null})
        );
        let items: Vec<ToolbarItem> = serde_json::from_value(json!([
            {"type": "action", "action": "mail.archive", "rank": 4, "display": "icon"},
            {"type": "more", "items": [
                {"type": "header", "label": "@toolbar.group.message"},
                {"type": "action", "action": "mail.toggle-read"}
            ]}
        ]))
        .unwrap();
        assert!(matches!(
            &items[0],
            ToolbarItem::Action {
                rank: Some(4),
                display: Some(ItemDisplay::Icon),
                ..
            }
        ));
        let ToolbarItem::More { label, icon, items } = &items[1] else {
            panic!("expected more");
        };
        assert!(label.is_none() && icon.is_none());
        assert!(
            matches!(&items[0], MenuItem::Header { label } if label == "@toolbar.group.message")
        );
        let settings: ToolbarSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings, ToolbarSettings::default());
        assert_eq!(settings.overflow, ToolbarOverflow::More);
        for d in ItemDisplay::ALL {
            assert_eq!(ItemDisplay::parse(d.as_str()), Some(*d));
            assert_eq!(serde_json::to_value(d).unwrap(), json!(d.as_str()));
        }
        for b in BadgeSource::ALL {
            assert_eq!(BadgeSource::parse(b.as_str()), Some(*b));
            assert_eq!(serde_json::to_value(b).unwrap(), json!(b.as_str()));
        }
        for o in ToolbarOverflow::ALL {
            assert_eq!(ToolbarOverflow::parse(o.as_str()), Some(*o));
            assert_eq!(serde_json::to_value(o).unwrap(), json!(o.as_str()));
        }
    }

    #[test]
    fn start_view_and_settings_sections() {
        let mut layout: ResolvedLayout = serde_json::from_value(json!({
            "name": "x",
            "windows": {},
            "views": {
                "mail": {"window": "mail", "badge": "unread"},
                "tasks": {"window": "tasks"}
            },
            "rail": [{"type": "view", "view": "mail"}, {"type": "spacer"}],
            "settings": [{"id": "general", "sections": [
                {"id": "accounts", "root": {"type": "widget", "kind": "account-settings"}}
            ]}]
        }))
        .unwrap();
        assert_eq!(layout.start_view(), Some("mail"));
        layout.default_view = Some("tasks".into());
        assert_eq!(layout.start_view(), Some("tasks"));
        layout.default_view = Some("nope".into());
        assert_eq!(layout.start_view(), Some("mail"));
        assert_eq!(layout.views["mail"].badge, Some(BadgeSource::Unread));
        assert!(layout.settings_section("accounts").is_some());
        assert!(layout.settings_section("nope").is_none());
        assert!(layout.toolbar_settings.is_empty());
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
