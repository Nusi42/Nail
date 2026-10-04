//! The declarative component set of plug-in UI (design.md, "Declarative
//! UI"; plan M2d WP60).
//!
//! Plug-ins describe dialogs and panels as trees of [`PluginUiNode`]s; the
//! React renderer draws them with themeable parts (`plugin.<id>.*`).
//! Nothing in a tree is code or markup: text is text, Markdown is rendered
//! without raw HTML, images are `data:` URIs of raster formats.
//!
//! JSON: every node is an object with a `type` (`"text"`, `"markdown"`,
//! `"input"`, `"textarea"`, `"select"`, `"checkbox"`, `"date"`,
//! `"contact-picker"`, `"folder-picker"`, `"message-list"`, `"table"`,
//! `"button"`, `"progress"`, `"image"`, `"stack"`) and camelCase fields,
//! e.g. `{"type": "date", "id": "due", "label": "Due", "default":
//! "tomorrow"}`. Optional fields may be left out.
//!
//! **Values.** Nodes with an `id` that take input report a value in a
//! dialog result or UI action ([`PluginUiNode::value_kind`]):
//!
//! | Node | Value |
//! | --- | --- |
//! | `input`, `textarea` | string |
//! | `select` | string (`multiple`: array of strings) |
//! | `checkbox` | boolean |
//! | `date` | `"YYYY-MM-DD"` (`time`: `"YYYY-MM-DDTHH:MM"`, local time), or `null` |
//! | `contact-picker` | array of `{name, email}` |
//! | `folder-picker` | folder id or `null` |
//! | `message-list` (`selectable`) | array of message ids |
//! | `table` (`selectable`) | array of row indexes |

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::runtime::PluginMessage;
use crate::domain::Address;
use crate::ids::{AccountId, FolderId, MessageId};

/// Most nodes in one tree (a dialog body or a panel).
pub const MAX_UI_NODES: usize = 500;
/// Deepest nesting of `stack` nodes (a top-level node has depth 1).
pub const MAX_UI_DEPTH: usize = 8;
/// Longest text of a `text` or `markdown` node, label or option (bytes).
pub const MAX_UI_TEXT: usize = 64 * 1024;
/// Longest `data:` URI of an image (bytes; about 384 KiB of image data).
pub const MAX_UI_IMAGE: usize = 512 * 1024;
/// Most messages in a `message-list`, rows in a `table`, options in a
/// `select`.
pub const MAX_UI_ITEMS: usize = 500;
/// Image types an `image` node may carry (`data:<type>;base64,…`). No SVG:
/// it can carry scripts and remote references.
pub const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// A node of a plug-in UI tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum PluginUiNode {
    /// A paragraph of plain text.
    Text {
        /// The text.
        value: String,
        /// How it is shown.
        #[serde(default)]
        #[ts(optional, as = "Option<PluginTextStyle>")]
        style: PluginTextStyle,
    },
    /// CommonMark text (no raw HTML, no remote images; links open outside
    /// Nail after a confirmation).
    Markdown {
        /// The Markdown source.
        value: String,
    },
    /// A one-line text field.
    Input {
        /// Value id (unique in the tree).
        id: String,
        /// The label.
        label: String,
        /// Initial value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<String>,
        /// Placeholder text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        placeholder: Option<String>,
        /// Whether a value is needed to submit with a primary button.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
        /// The kind of text.
        #[serde(default)]
        #[ts(optional, as = "Option<PluginInputKind>")]
        kind: PluginInputKind,
        /// Longest value (characters).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        max_length: Option<u32>,
    },
    /// A multi-line text field.
    Textarea {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// Initial value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<String>,
        /// Placeholder text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        placeholder: Option<String>,
        /// Whether a value is needed.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
        /// Visible rows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        rows: Option<u32>,
        /// Longest value (characters).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        max_length: Option<u32>,
    },
    /// A choice from a list.
    Select {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// The choices.
        options: Vec<PluginSelectOption>,
        /// Initially chosen value(s) (each one of the options' values).
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        default: Vec<String>,
        /// Whether several can be chosen (the value is then an array).
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        multiple: bool,
        /// Whether a choice is needed.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
    },
    /// A check box.
    Checkbox {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// Initially checked.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        default: bool,
    },
    /// A date (or date and time) field.
    Date {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// Initial value: `YYYY-MM-DD` (`YYYY-MM-DDTHH:MM` with `time`),
        /// `today` or `tomorrow`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<String>,
        /// Earliest date (`YYYY-MM-DD`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        min: Option<String>,
        /// Latest date (`YYYY-MM-DD`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        max: Option<String>,
        /// Whether a time is picked too.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        time: bool,
        /// Whether a value is needed.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
    },
    /// Addresses picked from the collected addresses (contacts in M3).
    ContactPicker {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// Initial addresses.
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<Address>>")]
        default: Vec<Address>,
        /// Whether several can be picked.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        multiple: bool,
        /// Whether one is needed.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
    },
    /// A folder picked from the folder tree.
    FolderPicker {
        /// Value id.
        id: String,
        /// The label.
        label: String,
        /// Only this account's folders; `null` = every account.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        account_id: Option<AccountId>,
        /// Initial folder.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        default: Option<FolderId>,
        /// Whether one is needed.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        required: bool,
    },
    /// A list of messages (subject, sender, date). The plug-in names the
    /// ids; the host fills `messages` from what the plug-in may read
    /// (ids it may not read are dropped) before the tree reaches the UI.
    MessageList {
        /// Value id (needed when `selectable`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        id: Option<String>,
        /// The messages, in display order.
        message_ids: Vec<MessageId>,
        /// Filled by the host; ignored when a plug-in sends it.
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<PluginMessage>>")]
        messages: Vec<PluginMessage>,
        /// Whether rows can be selected (the value is the selected ids).
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        selectable: bool,
        /// Whether several rows can be selected.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        multiple: bool,
    },
    /// A table of text cells.
    Table {
        /// Value id (needed when `selectable`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        id: Option<String>,
        /// The columns.
        columns: Vec<PluginTableColumn>,
        /// The rows: one text per column.
        rows: Vec<Vec<String>>,
        /// Whether rows can be selected (the value is the row indexes).
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        selectable: bool,
        /// Whether several rows can be selected.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        multiple: bool,
    },
    /// A button inside a dialog body or panel: sends a UI action to the
    /// plug-in with the current values (dialog buttons that close the
    /// dialog are [`super::PluginDialogButton`]s instead).
    Button {
        /// Action id.
        id: String,
        /// The label.
        label: String,
        /// Its emphasis.
        #[serde(default)]
        #[ts(optional, as = "Option<PluginButtonVariant>")]
        variant: PluginButtonVariant,
        /// Icon name from the theme's icon set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        icon: Option<String>,
        /// Whether it is disabled.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        disabled: bool,
    },
    /// A progress bar.
    Progress {
        /// Percent done (0–100); `null` = unknown (indeterminate).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        value: Option<u8>,
        /// A label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        label: Option<String>,
    },
    /// An image.
    Image {
        /// `data:image/png;base64,…` (PNG, JPEG, GIF or WebP). A plug-in
        /// may name a file of its package (`assets/logo.png`); the host
        /// inlines it before the tree reaches the UI. Never a remote URL.
        src: String,
        /// The text alternative (empty = decorative).
        alt: String,
        /// Width in CSS pixels.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        width: Option<u32>,
        /// Height in CSS pixels.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        height: Option<u32>,
    },
    /// Arranges its children in a row or a column.
    Stack {
        /// The direction.
        #[serde(default)]
        #[ts(optional, as = "Option<PluginStackDirection>")]
        direction: PluginStackDirection,
        /// The children.
        children: Vec<PluginUiNode>,
    },
}

/// How a `text` node is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginTextStyle {
    /// Normal body text.
    #[default]
    Body,
    /// A heading.
    Heading,
    /// Secondary text.
    Muted,
    /// Emphasized text.
    Strong,
    /// An error message.
    Error,
}

/// The kind of text of an `input` node (keyboard and validation hints).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginInputKind {
    /// Any text.
    #[default]
    Text,
    /// An email address.
    Email,
    /// A URL.
    Url,
    /// A number (the value is still a string).
    Number,
}

/// The emphasis of a button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginButtonVariant {
    /// A normal button.
    #[default]
    Default,
    /// The main action.
    Primary,
    /// A destructive action.
    Danger,
}

/// The direction of a `stack` node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginStackDirection {
    /// Top to bottom.
    #[default]
    Vertical,
    /// Start to end.
    Horizontal,
}

/// A choice of a `select` node (also used by `select` settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginSelectOption {
    /// The value reported when chosen.
    pub value: String,
    /// The text shown.
    pub label: String,
}

/// A column of a `table` node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginTableColumn {
    /// The header text.
    pub label: String,
}

/// What value a node reports (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginValueKind {
    /// A string.
    Text,
    /// An array of strings.
    TextList,
    /// A boolean.
    Bool,
    /// A string or `null` (dates, folders).
    OptionalText,
    /// An array of addresses.
    Addresses,
    /// An array of row indexes.
    Indexes,
}

impl PluginUiNode {
    /// The node's value id, if it reports a value or sends actions.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Input { id, .. }
            | Self::Textarea { id, .. }
            | Self::Select { id, .. }
            | Self::Checkbox { id, .. }
            | Self::Date { id, .. }
            | Self::ContactPicker { id, .. }
            | Self::FolderPicker { id, .. }
            | Self::Button { id, .. } => Some(id),
            Self::MessageList { id, .. } | Self::Table { id, .. } => id.as_deref(),
            Self::Text { .. }
            | Self::Markdown { .. }
            | Self::Progress { .. }
            | Self::Image { .. }
            | Self::Stack { .. } => None,
        }
    }

    /// The value this node reports, `None` for nodes without a value.
    pub fn value_kind(&self) -> Option<PluginValueKind> {
        Some(match self {
            Self::Input { .. } | Self::Textarea { .. } => PluginValueKind::Text,
            Self::Select { multiple, .. } => {
                if *multiple {
                    PluginValueKind::TextList
                } else {
                    PluginValueKind::Text
                }
            }
            Self::Checkbox { .. } => PluginValueKind::Bool,
            Self::Date { .. } | Self::FolderPicker { .. } => PluginValueKind::OptionalText,
            Self::ContactPicker { .. } => PluginValueKind::Addresses,
            Self::MessageList {
                selectable: true, ..
            } => PluginValueKind::TextList,
            Self::Table {
                selectable: true, ..
            } => PluginValueKind::Indexes,
            _ => return None,
        })
    }

    /// The node's children (only `stack` has any).
    pub fn children(&self) -> &[PluginUiNode] {
        match self {
            Self::Stack { children, .. } => children,
            _ => &[],
        }
    }
}

/// What is wrong with a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginUiIssueKind {
    /// More than [`MAX_UI_NODES`] nodes.
    TooManyNodes,
    /// `stack`s nested deeper than [`MAX_UI_DEPTH`].
    TooDeep,
    /// A text longer than [`MAX_UI_TEXT`].
    TextTooLong,
    /// A value id that is empty, longer than 64 characters or not made of
    /// ASCII letters, digits, `-` and `_`.
    InvalidId,
    /// Two nodes with the same id.
    DuplicateId,
    /// A `selectable` list or table without an id.
    MissingId,
    /// More than [`MAX_UI_ITEMS`] options, messages or rows.
    TooManyItems,
    /// A default value of a `select` that is not an option, or several
    /// defaults without `multiple`.
    InvalidDefault,
    /// A `date` value that is not `YYYY-MM-DD` (`THH:MM`), `today` or
    /// `tomorrow`.
    InvalidDate,
    /// A table row whose length differs from the columns.
    RowLength,
    /// A progress value above 100.
    InvalidProgress,
    /// An image that is not a `data:` URI of an allowed type, or too big.
    InvalidImage,
}

impl PluginUiIssueKind {
    /// All issues.
    pub const ALL: [PluginUiIssueKind; 12] = [
        Self::TooManyNodes,
        Self::TooDeep,
        Self::TextTooLong,
        Self::InvalidId,
        Self::DuplicateId,
        Self::MissingId,
        Self::TooManyItems,
        Self::InvalidDefault,
        Self::InvalidDate,
        Self::RowLength,
        Self::InvalidProgress,
        Self::InvalidImage,
    ];

    /// Fluent message id (`plugin-ui-issue-<kind>`), for the plug-in log.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::TooManyNodes => "plugin-ui-issue-too-many-nodes",
            Self::TooDeep => "plugin-ui-issue-too-deep",
            Self::TextTooLong => "plugin-ui-issue-text-too-long",
            Self::InvalidId => "plugin-ui-issue-invalid-id",
            Self::DuplicateId => "plugin-ui-issue-duplicate-id",
            Self::MissingId => "plugin-ui-issue-missing-id",
            Self::TooManyItems => "plugin-ui-issue-too-many-items",
            Self::InvalidDefault => "plugin-ui-issue-invalid-default",
            Self::InvalidDate => "plugin-ui-issue-invalid-date",
            Self::RowLength => "plugin-ui-issue-row-length",
            Self::InvalidProgress => "plugin-ui-issue-invalid-progress",
            Self::InvalidImage => "plugin-ui-issue-invalid-image",
        }
    }
}

/// A problem at one node of a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginUiIssue {
    /// Where: `[2].children[0]` (indexes into the top-level list and
    /// `children`); empty for the whole tree.
    pub path: String,
    /// What.
    pub kind: PluginUiIssueKind,
}

/// Whether `id` is a valid value id: 1–64 ASCII letters, digits, `-`, `_`.
pub fn is_value_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether `value` is a date value of a `date` node (`YYYY-MM-DD`, with
/// `time` also `YYYY-MM-DDTHH:MM`).
pub fn is_date_value(value: &str, time: bool) -> bool {
    let date_ok = |d: &str| {
        let parts: Vec<&str> = d.split('-').collect();
        parts.len() == 3
            && parts[0].len() == 4
            && parts[1].len() == 2
            && parts[2].len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
            && matches!(parts[1].parse::<u8>(), Ok(1..=12))
            && matches!(parts[2].parse::<u8>(), Ok(1..=31))
    };
    let time_ok = |t: &str| {
        t.len() == 5
            && t.as_bytes()[2] == b':'
            && matches!(t[..2].parse::<u8>(), Ok(0..=23))
            && matches!(t[3..].parse::<u8>(), Ok(0..=59))
            && t[..2]
                .bytes()
                .chain(t[3..].bytes())
                .all(|b| b.is_ascii_digit())
    };
    match value.split_once('T') {
        Some((d, t)) => time && date_ok(d) && time_ok(t),
        None => date_ok(value),
    }
}

/// Whether `src` is an allowed image source on the wire (a `data:` URI of
/// [`IMAGE_TYPES`], base64, at most [`MAX_UI_IMAGE`] bytes).
pub fn is_image_data_uri(src: &str) -> bool {
    if src.len() > MAX_UI_IMAGE {
        return false;
    }
    let Some(rest) = src.strip_prefix("data:") else {
        return false;
    };
    let Some((media, data)) = rest.split_once(";base64,") else {
        return false;
    };
    IMAGE_TYPES.contains(&media)
        && !data.is_empty()
        && data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// Checks a tree (a dialog body or a panel) as it goes to the UI: limits,
/// ids, defaults, dates, images. The host rejects a tree with issues and
/// logs them to the plug-in's log. An empty result means it is fine.
pub fn validate_tree(nodes: &[PluginUiNode]) -> Vec<PluginUiIssue> {
    let mut v = Validator::default();
    for (i, node) in nodes.iter().enumerate() {
        v.node(node, &format!("[{i}]"), 1);
    }
    if v.count > MAX_UI_NODES {
        v.issues.insert(
            0,
            PluginUiIssue {
                path: String::new(),
                kind: PluginUiIssueKind::TooManyNodes,
            },
        );
    }
    v.issues
}

#[derive(Default)]
struct Validator {
    count: usize,
    ids: HashSet<String>,
    issues: Vec<PluginUiIssue>,
}

impl Validator {
    fn issue(&mut self, path: &str, kind: PluginUiIssueKind) {
        self.issues.push(PluginUiIssue {
            path: path.to_owned(),
            kind,
        });
    }

    fn text(&mut self, path: &str, texts: &[&str]) {
        if texts.iter().any(|t| t.len() > MAX_UI_TEXT) {
            self.issue(path, PluginUiIssueKind::TextTooLong);
        }
    }

    fn node(&mut self, node: &PluginUiNode, path: &str, depth: usize) {
        use PluginUiIssueKind as K;
        self.count += 1;
        if let Some(id) = node.id() {
            if !is_value_id(id) {
                self.issue(path, K::InvalidId);
            } else if !self.ids.insert(id.to_owned()) {
                self.issue(path, K::DuplicateId);
            }
        }
        match node {
            PluginUiNode::Text { value, .. } | PluginUiNode::Markdown { value } => {
                self.text(path, &[value]);
            }
            PluginUiNode::Input {
                label,
                default,
                placeholder,
                ..
            }
            | PluginUiNode::Textarea {
                label,
                default,
                placeholder,
                ..
            } => {
                self.text(
                    path,
                    &[
                        label,
                        default.as_deref().unwrap_or_default(),
                        placeholder.as_deref().unwrap_or_default(),
                    ],
                );
            }
            PluginUiNode::Select {
                label,
                options,
                default,
                multiple,
                ..
            } => {
                self.text(path, &[label]);
                if options
                    .iter()
                    .any(|o| o.label.len() > MAX_UI_TEXT || o.value.len() > MAX_UI_TEXT)
                {
                    self.issue(path, K::TextTooLong);
                }
                if options.len() > MAX_UI_ITEMS {
                    self.issue(path, K::TooManyItems);
                }
                let unknown = default
                    .iter()
                    .any(|d| !options.iter().any(|o| &o.value == d));
                if unknown || (!multiple && default.len() > 1) {
                    self.issue(path, K::InvalidDefault);
                }
            }
            PluginUiNode::Checkbox { label, .. } | PluginUiNode::FolderPicker { label, .. } => {
                self.text(path, &[label]);
            }
            PluginUiNode::ContactPicker {
                label,
                default,
                multiple,
                ..
            } => {
                self.text(path, &[label]);
                if !multiple && default.len() > 1 {
                    self.issue(path, K::InvalidDefault);
                }
            }
            PluginUiNode::Date {
                label,
                default,
                min,
                max,
                time,
                ..
            } => {
                self.text(path, &[label]);
                let default_ok = default
                    .as_deref()
                    .is_none_or(|d| d == "today" || d == "tomorrow" || is_date_value(d, *time));
                let bounds_ok = [min, max]
                    .iter()
                    .all(|b| b.as_deref().is_none_or(|b| is_date_value(b, false)));
                if !default_ok || !bounds_ok {
                    self.issue(path, K::InvalidDate);
                }
            }
            PluginUiNode::MessageList {
                id,
                message_ids,
                selectable,
                ..
            } => {
                if message_ids.len() > MAX_UI_ITEMS {
                    self.issue(path, K::TooManyItems);
                }
                if *selectable && id.is_none() {
                    self.issue(path, K::MissingId);
                }
            }
            PluginUiNode::Table {
                id,
                columns,
                rows,
                selectable,
                ..
            } => {
                if rows.len() > MAX_UI_ITEMS || columns.len() > 32 {
                    self.issue(path, K::TooManyItems);
                }
                if rows.iter().any(|r| r.len() != columns.len()) {
                    self.issue(path, K::RowLength);
                }
                let long_cell = rows.iter().flatten().any(|c| c.len() > MAX_UI_TEXT)
                    || columns.iter().any(|c| c.label.len() > MAX_UI_TEXT);
                if long_cell {
                    self.issue(path, K::TextTooLong);
                }
                if *selectable && id.is_none() {
                    self.issue(path, K::MissingId);
                }
            }
            PluginUiNode::Button { label, .. } => self.text(path, &[label]),
            PluginUiNode::Progress { value, label } => {
                if value.is_some_and(|v| v > 100) {
                    self.issue(path, K::InvalidProgress);
                }
                self.text(path, &[label.as_deref().unwrap_or_default()]);
            }
            PluginUiNode::Image { src, alt, .. } => {
                if !is_image_data_uri(src) {
                    self.issue(path, K::InvalidImage);
                }
                self.text(path, &[alt]);
            }
            PluginUiNode::Stack { children, .. } => {
                if depth >= MAX_UI_DEPTH && !children.is_empty() {
                    self.issue(path, K::TooDeep);
                    return;
                }
                for (i, child) in children.iter().enumerate() {
                    if self.count > MAX_UI_NODES {
                        return;
                    }
                    self.node(child, &format!("{path}.children[{i}]"), depth + 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> PluginUiNode {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn design_example_parses() {
        let body: Vec<PluginUiNode> = serde_json::from_value(json!([
            { "type": "text", "value": "Remind me about \"Hi\"" },
            { "type": "date", "id": "due", "label": "Due", "default": "tomorrow" },
            { "type": "select", "id": "list", "label": "Task list",
              "options": [{ "value": "inbox", "label": "Inbox" }] },
        ]))
        .unwrap();
        assert_eq!(
            body[0],
            PluginUiNode::Text {
                value: "Remind me about \"Hi\"".into(),
                style: PluginTextStyle::Body
            }
        );
        assert!(
            validate_tree(&body).is_empty(),
            "{:?}",
            validate_tree(&body)
        );
        assert_eq!(body[1].value_kind(), Some(PluginValueKind::OptionalText));
        assert_eq!(body[2].value_kind(), Some(PluginValueKind::Text));
    }

    #[test]
    fn json_shape_is_stable() {
        let node = PluginUiNode::Input {
            id: "title".into(),
            label: "Title".into(),
            default: None,
            placeholder: Some("…".into()),
            required: true,
            kind: PluginInputKind::Email,
            max_length: Some(80),
        };
        assert_eq!(
            serde_json::to_value(&node).unwrap(),
            json!({
                "type": "input", "id": "title", "label": "Title",
                "placeholder": "…", "required": true, "kind": "email", "maxLength": 80
            })
        );
        let list = parse(json!({
            "type": "message-list", "id": "hits", "messageIds": ["m1"], "selectable": true
        }));
        assert_eq!(list.value_kind(), Some(PluginValueKind::TextList));
        let value = serde_json::to_value(&list).unwrap();
        assert_eq!(value["type"], "message-list");
        assert_eq!(value["messageIds"], json!(["m1"]));
        assert_eq!(serde_json::from_value::<PluginUiNode>(value).unwrap(), list);

        for ty in [
            "contact-picker",
            "folder-picker",
            "checkbox",
            "textarea",
            "select",
            "date",
        ] {
            let mut v = json!({"type": ty, "id": "x", "label": "X"});
            if ty == "select" {
                v["options"] = json!([]);
            }
            let node = parse(v);
            assert_eq!(serde_json::to_value(&node).unwrap()["type"], ty);
        }
        let stack = parse(json!({"type": "stack", "children": [
            {"type": "button", "id": "go", "label": "Go", "variant": "primary"},
            {"type": "progress"},
            {"type": "table", "columns": [{"label": "A"}], "rows": [["1"]]},
            {"type": "markdown", "value": "*hi*"},
        ]}));
        assert_eq!(stack.children().len(), 4);
        assert!(validate_tree(std::slice::from_ref(&stack)).is_empty());
        assert!(serde_json::from_value::<PluginUiNode>(json!({"type": "webview"})).is_err());
    }

    #[test]
    fn ids_must_be_unique_and_well_formed() {
        let body = vec![
            parse(json!({"type": "input", "id": "a", "label": "A"})),
            parse(json!({"type": "stack", "children": [
                {"type": "checkbox", "id": "a", "label": "B"},
                {"type": "checkbox", "id": "b c", "label": "C"},
            ]})),
            parse(json!({"type": "table", "columns": [], "rows": [], "selectable": true})),
        ];
        let kinds: Vec<_> = validate_tree(&body)
            .into_iter()
            .map(|i| (i.path, i.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("[1].children[0]".to_owned(), PluginUiIssueKind::DuplicateId),
                ("[1].children[1]".to_owned(), PluginUiIssueKind::InvalidId),
                ("[2]".to_owned(), PluginUiIssueKind::MissingId),
            ]
        );
    }

    #[test]
    fn values_and_limits_are_checked() {
        use PluginUiIssueKind as K;
        let check = |v: serde_json::Value| -> Vec<PluginUiIssueKind> {
            validate_tree(&[parse(v)])
                .into_iter()
                .map(|i| i.kind)
                .collect()
        };
        assert_eq!(
            check(json!({"type": "select", "id": "s", "label": "S",
                "options": [{"value": "a", "label": "A"}], "default": ["b"]})),
            [K::InvalidDefault]
        );
        assert_eq!(
            check(json!({"type": "select", "id": "s", "label": "S",
                "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}],
                "default": ["a", "b"]})),
            [K::InvalidDefault]
        );
        assert_eq!(
            check(json!({"type": "date", "id": "d", "label": "D", "default": "2026-13-01"})),
            [K::InvalidDate]
        );
        assert!(
            check(
                json!({"type": "date", "id": "d", "label": "D", "time": true,
            "default": "2026-10-01T09:30", "min": "2026-01-01"})
            )
            .is_empty()
        );
        assert_eq!(
            check(json!({"type": "date", "id": "d", "label": "D", "default": "2026-10-01T09:30"})),
            [K::InvalidDate]
        );
        assert_eq!(
            check(json!({"type": "table", "columns": [{"label": "A"}], "rows": [["1", "2"]]})),
            [K::RowLength]
        );
        assert_eq!(
            check(json!({"type": "progress", "value": 101})),
            [K::InvalidProgress]
        );
        assert_eq!(
            check(json!({"type": "image", "src": "https://example.com/x.png", "alt": ""})),
            [K::InvalidImage]
        );
        assert_eq!(
            check(json!({"type": "image", "src": "data:image/svg+xml;base64,PHN2Zz4=", "alt": ""})),
            [K::InvalidImage]
        );
        assert!(
            check(
                json!({"type": "image", "src": "data:image/png;base64,iVBORw0KGgo=", "alt": "Logo"})
            )
            .is_empty()
        );
        assert_eq!(
            check(json!({"type": "text", "value": "x".repeat(MAX_UI_TEXT + 1)})),
            [K::TextTooLong]
        );
    }

    #[test]
    fn size_and_depth_are_bounded() {
        let mut node = parse(json!({"type": "text", "value": "leaf"}));
        for _ in 0..MAX_UI_DEPTH {
            node = PluginUiNode::Stack {
                direction: PluginStackDirection::Vertical,
                children: vec![node],
            };
        }
        let issues = validate_tree(std::slice::from_ref(&node));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, PluginUiIssueKind::TooDeep);

        let many: Vec<PluginUiNode> = (0..=MAX_UI_NODES)
            .map(|_| parse(json!({"type": "text", "value": "x"})))
            .collect();
        assert_eq!(
            validate_tree(&many)[0].kind,
            PluginUiIssueKind::TooManyNodes
        );
    }

    #[test]
    fn issue_ids_are_fluent_ids() {
        for kind in PluginUiIssueKind::ALL {
            assert!(crate::localized::is_message_id(kind.message_id()));
        }
    }
}
