//! JSON Schema of the theme and layout files.
//!
//! The schema describes the structure of a file as a JSON object: each KDL
//! section becomes a property, each named KDL node an object key. It is
//! generated from the file models ([`ThemeFile`], [`LayoutFile`]); the types
//! below only describe the parts of the layout that are stored as
//! `nail_api` types.

#![expect(dead_code, reason = "schema-only mirror types are never constructed")]

use schemars::{JsonSchema, Schema, generate::SchemaSettings};
use serde_json::{Map, Value};

use crate::layout::LayoutFile;
use crate::theme::ThemeFile;

/// JSON Schema of `theme.kdl`.
pub fn theme_schema() -> Schema {
    generate::<ThemeFile>("Nail theme.kdl")
}

/// JSON Schema of `layout.kdl`.
pub fn layout_schema() -> Schema {
    generate::<LayoutFile>("Nail layout.kdl")
}

fn generate<T: JsonSchema>(title: &str) -> Schema {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>();
    schema.insert("title".to_owned(), Value::String(title.to_owned()));
    schema
}

/// One window tree for a range of window widths
/// (`breakpoint min-width=… max-width=… { window "<name>" { … } }`).
#[derive(JsonSchema)]
pub(crate) struct BreakpointSchema {
    /// Minimum window width in CSS pixels (inclusive).
    min_width: Option<u32>,
    /// Maximum window width in CSS pixels (inclusive).
    max_width: Option<u32>,
    /// The window's single root node.
    root: NodeSchema,
}

/// A container or widget node.
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum NodeSchema {
    /// `split "horizontal"|"vertical" sizes="240px 1fr" min-sizes="160px none"
    /// max-sizes="480px none" collapsible="true false" resizable=#true { … }`.
    Split {
        /// Stable node id.
        id: Option<String>,
        /// Label: literal text or `@message.id`.
        label: Option<String>,
        /// `horizontal` (side by side) or `vertical`.
        orientation: Option<OrientationSchema>,
        /// One CSS track size per child.
        sizes: Option<String>,
        /// One minimum size (or `none`) per child.
        min_sizes: Option<String>,
        /// One maximum size (or `none`) per child.
        max_sizes: Option<String>,
        /// One `true`/`false` per child.
        collapsible: Option<String>,
        /// Whether dividers can be dragged (default true).
        resizable: Option<bool>,
        /// Child nodes.
        children: Vec<NodeSchema>,
    },
    /// `stack sizes="auto 1fr" { … }`: children top to bottom.
    Stack {
        /// Stable node id.
        id: Option<String>,
        /// Label.
        label: Option<String>,
        /// One CSS track size per child.
        sizes: Option<String>,
        /// Child nodes.
        children: Vec<NodeSchema>,
    },
    /// `tabs active=0 { … }`: one child at a time.
    Tabs {
        /// Stable node id.
        id: Option<String>,
        /// Label.
        label: Option<String>,
        /// Index of the initially active tab.
        active: Option<u32>,
        /// Child nodes, one per tab.
        children: Vec<NodeSchema>,
    },
    /// A widget: `<kind> id=… label=… <option>=<value>… { row "<template>" }`.
    Widget {
        /// Widget kind: account-switcher, folder-tree, message-list,
        /// message-view, toolbar, status-bar, text, spacer (M0), or
        /// plugin.<id>.<name>.
        kind: String,
        /// Stable node id.
        id: Option<String>,
        /// Label.
        label: Option<String>,
        /// Widget options (all other properties of the node).
        #[serde(default)]
        options: Map<String, Value>,
        /// Row templates, e.g. `"{from.name}  {date | relative}"`.
        #[serde(default)]
        rows: Vec<String>,
    },
}

/// Split direction.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OrientationSchema {
    /// Side by side.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// `action "<id>" label=… icon=… alias-of=… <args>…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ActionSchema {
    /// Label: literal text or `@message.id`.
    label: Option<String>,
    /// Icon name from the theme.
    icon: Option<String>,
    /// For aliases: the action this one invokes.
    alias_of: Option<String>,
    /// Arguments for `alias-of`.
    #[serde(default)]
    args: Vec<Value>,
}

/// A toolbar entry.
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum ToolbarItemSchema {
    /// `item "<action> <args>…" label=… icon=… rank=… display=…`.
    Item {
        /// Action id and arguments.
        action: String,
        /// Label override.
        label: Option<String>,
        /// Icon override.
        icon: Option<String>,
        /// Overflow rank (M5b): 1 stays longest; unranked items move into
        /// "More" first.
        rank: Option<u32>,
        /// `icon`, `label` or `icon-label` (M5b).
        display: Option<ItemDisplaySchema>,
    },
    /// `more label=… icon=… { <menu items> }` (M5b): the "More" menu.
    More {
        /// Label override.
        label: Option<String>,
        /// Icon override.
        icon: Option<String>,
        /// The menu's own entries.
        items: Vec<MenuItemSchema>,
    },
    /// `spacer`: pushes the following items to the end.
    Spacer,
    /// `separator`.
    Separator,
}

/// How a toolbar button shows its action (M5b).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ItemDisplaySchema {
    /// Only the icon.
    Icon,
    /// Only the label.
    Label,
    /// Icon and label.
    IconLabel,
}

/// The properties of a `toolbar "<name>"` node (M5b).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ToolbarSettingsSchema {
    /// Hide actions that cannot run now instead of showing them disabled.
    hide_disabled: Option<bool>,
    /// `more` (default: items that do not fit move into "More") or `wrap`.
    overflow: Option<ToolbarOverflowSchema>,
}

/// The window-model properties of a `window "<name>"` node (M5c).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct WindowFrameSchema {
    /// Default size, `"<width>x<height>"` in CSS pixels (e.g. `"760x600"`).
    size: Option<String>,
    /// Smallest size, `"<width>x<height>"`.
    min_size: Option<String>,
    /// Blocks the rest of the window while open (only confirmations).
    modal: Option<bool>,
    /// The user can resize it.
    resizable: Option<bool>,
    /// May open as its own OS window in the desktop app.
    pop_out: Option<bool>,
    /// `single` (default) or `multiple` (e.g. one composer per draft).
    instances: Option<WindowInstancesSchema>,
}

/// How many windows of a kind may be open (M5c).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowInstancesSchema {
    /// One at a time.
    Single,
    /// Several.
    Multiple,
}

/// Toolbar overflow (M5b).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ToolbarOverflowSchema {
    /// Into the "More" menu, by rank.
    More,
    /// Onto further lines.
    Wrap,
}

/// Where a rail badge counts from (M5b).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BadgeSchema {
    /// Unread mail in the inboxes.
    Unread,
    /// Confirmations waiting.
    Confirmations,
}

/// `views default="<id>" { view "<id>" … }` (M5b).
#[derive(JsonSchema)]
pub(crate) struct ViewsSchema {
    /// The view shown at start.
    default: Option<String>,
    /// The views by id.
    views: std::collections::BTreeMap<String, ViewSchema>,
}

/// `view "<id>" label=… icon=… window=… badge=…`.
#[derive(JsonSchema)]
pub(crate) struct ViewSchema {
    /// Label: literal text or `@message.id`.
    label: Option<String>,
    /// Lucide icon name.
    icon: Option<String>,
    /// The window the view shows.
    window: String,
    /// Badge on the rail button.
    badge: Option<BadgeSchema>,
}

/// A rail entry (M5b).
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum RailItemSchema {
    /// `view "<id>"`.
    View {
        /// The view.
        view: String,
    },
    /// `item "<action> <args>…" label=… icon=… badge=…`.
    Item {
        /// Action id and arguments.
        action: String,
        /// Label override.
        label: Option<String>,
        /// Icon override.
        icon: Option<String>,
        /// Badge.
        badge: Option<BadgeSchema>,
    },
    /// `spacer`.
    Spacer,
    /// `separator`.
    Separator,
}

/// `group "<id>" label=… { section … }` of the settings page (M5b).
#[derive(JsonSchema)]
pub(crate) struct SettingsGroupSchema {
    /// Group id.
    id: String,
    /// Label: literal text or `@message.id`.
    label: Option<String>,
    /// The group's sections.
    sections: Vec<SettingsSectionSchema>,
}

/// `section "<id>" label=… icon=… keywords=… { <nodes> }` (M5b).
#[derive(JsonSchema)]
pub(crate) struct SettingsSectionSchema {
    /// Section id.
    id: String,
    /// Label: literal text or `@message.id`.
    label: Option<String>,
    /// Lucide icon name.
    icon: Option<String>,
    /// Extra search words: literal text or `@message.id`.
    keywords: Option<String>,
    /// The section's nodes.
    children: Vec<NodeSchema>,
}

/// A menu entry.
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum MenuItemSchema {
    /// `item "<action> <args>…" label=… icon=…`.
    Item {
        /// Action id and arguments.
        action: String,
        /// Label override.
        label: Option<String>,
        /// Icon override.
        icon: Option<String>,
    },
    /// `separator`.
    Separator,
    /// `submenu "<label>" { … }`.
    Submenu {
        /// Label.
        label: String,
        /// Entries.
        items: Vec<MenuItemSchema>,
    },
    /// `header "<label>"` (M5b): a group heading.
    Header {
        /// Label.
        label: String,
    },
}
