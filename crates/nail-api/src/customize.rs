//! Customization: configuration errors, write-back of UI edits into
//! `layout.kdl`, the merge dialog and backups (design.md, "UI
//! customization"; ADR 0022).
//!
//! * Errors: [`crate::Event::ConfigErrors`] reports **every** error of a
//!   file at once ([`ConfigIssue`]s); an empty list means the file is valid
//!   again.
//! * Write-back: [`crate::Request::UpdateLayout`] applies typed
//!   [`LayoutPatch`]es to `layout.kdl`, keeping comments and formatting; the
//!   new configuration arrives as [`crate::Event::ConfigChanged`].
//! * Merge dialog: [`crate::Request::PreviewConfigMerge`] compares an
//!   incoming theme or layout with the user's file section by section;
//!   [`crate::Request::ResolveConfigMerge`] resolves a set of choices for
//!   the live preview; [`crate::Request::ApplyConfigMerge`] writes the
//!   result after backing the old file up; [`crate::Request::RevertConfig`]
//!   restores a backup.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::config::layout::ToolbarItem;
use crate::ids::ConfigBackupId;

/// A configuration file in the config directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigFile {
    /// `theme.kdl`.
    Theme,
    /// `layout.kdl`.
    Layout,
    /// `accounts.kdl`.
    Accounts,
    /// `rules.kdl`.
    Rules,
}

impl ConfigFile {
    /// All files.
    pub const ALL: [ConfigFile; 4] = [Self::Theme, Self::Layout, Self::Accounts, Self::Rules];

    /// The file name in the config directory, e.g. `"theme.kdl"`.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Theme => "theme.kdl",
            Self::Layout => "layout.kdl",
            Self::Accounts => "accounts.kdl",
            Self::Rules => "rules.kdl",
        }
    }

    /// The file for a file name, e.g. `"layout.kdl"` → `Layout`.
    pub fn from_file_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.file_name() == name)
    }
}

/// One error in a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConfigIssue {
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// Diagnostic message (English, like a compiler message).
    pub message: String,
}

/// Addresses a node of a layout window tree: the window, the index of the
/// breakpoint in [`crate::ResolvedLayout::windows`], and the child indexes
/// from the breakpoint's root (empty = the root itself).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct LayoutNodeRef {
    /// Window name, e.g. `"main"`.
    pub window: String,
    /// Breakpoint index within the window.
    pub breakpoint: u32,
    /// Child indexes from the root.
    #[serde(default)]
    pub path: Vec<u32>,
}

/// An edit made in the UI, written back into `layout.kdl`. A node that
/// only exists in the defaults (sparse file) is copied into the file first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum LayoutPatch {
    /// New `sizes` of a split (dragging a divider).
    SplitSizes {
        /// The split.
        node: LayoutNodeRef,
        /// One CSS track size per child, e.g. `["240px", "1fr", "1.4fr"]`.
        sizes: Vec<String>,
    },
    /// New column order of a list widget (its `columns` option).
    ColumnOrder {
        /// The widget.
        node: LayoutNodeRef,
        /// Column ids in the new order.
        columns: Vec<String>,
    },
    /// New item order (or contents) of a toolbar.
    ToolbarOrder {
        /// Toolbar name, e.g. `"reader"`.
        toolbar: String,
        /// The items in the new order.
        items: Vec<ToolbarItem>,
    },
    /// Add a key binding (replacing one with the same context and key).
    AddKeyBinding {
        /// Context, e.g. `"list"`.
        context: String,
        /// Key sequence, e.g. `"ctrl+shift+m"`.
        key: String,
        /// Action id.
        action: String,
        /// Action arguments.
        #[serde(default)]
        args: Vec<Value>,
    },
    /// Remove a key binding (written as `unbind` when it comes from the
    /// defaults).
    RemoveKeyBinding {
        /// Context.
        context: String,
        /// Key sequence.
        key: String,
    },
}

/// A section of a theme or layout, as grouped by the merge dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MergeSection {
    /// Theme: file header (name, `extends`).
    Header,
    /// Theme: tokens.
    Tokens,
    /// Theme: light / dark / high-contrast modes.
    Modes,
    /// Theme: part styles.
    Parts,
    /// Theme: icons.
    Icons,
    /// Theme: received-message display.
    Message,
    /// Theme: raw CSS.
    RawCss,
    /// Layout: windows and breakpoints.
    Windows,
    /// Layout: action definitions and aliases.
    Actions,
    /// Layout: key bindings.
    Keys,
    /// Layout: toolbars.
    Toolbars,
    /// Layout: menus.
    Menus,
}

/// How an item differs between the user's file and the incoming one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MergeChange {
    /// Only in the incoming file.
    Added,
    /// Only in the user's file.
    Removed,
    /// In both, different.
    Changed,
    /// In both, equal (listed for completeness, nothing to choose).
    Unchanged,
}

/// Kinds of merge conflicts the dialog flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MergeConflictKind {
    /// Two different actions on the same key.
    DuplicateKey,
    /// An action that does not exist (e.g. from a plug-in not installed).
    UnknownAction,
    /// A widget that does not exist (e.g. from a plug-in not installed).
    UnknownWidget,
    /// Taking this item drops plug-in items the user has placed.
    DropsPluginItems,
    /// The item is invalid on its own (e.g. a token reference that no
    /// longer resolves).
    Invalid,
}

/// A conflict attached to a merge item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeConflict {
    /// Kind (the UI shows `merge-conflict-<kind>`).
    pub kind: MergeConflictKind,
    /// Diagnostic detail (English).
    pub message: String,
}

/// One item the user chooses for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeItem {
    /// Stable id within this preview (for [`MergeChoice`]).
    pub id: String,
    /// What the item is, for display: a token name, part selector, window
    /// name, key, toolbar name, …
    pub key: String,
    /// How it differs.
    pub change: MergeChange,
    /// The user's version as KDL text, `null` when absent.
    pub mine: Option<String>,
    /// The incoming version as KDL text, `null` when absent.
    pub theirs: Option<String>,
    /// Conflicts when taking the incoming version.
    pub conflicts: Vec<MergeConflict>,
}

/// The items of one section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeSectionPreview {
    /// The section.
    pub section: MergeSection,
    /// Its items, in file order.
    pub items: Vec<MergeItem>,
}

/// The comparison shown by the merge dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConfigMergePreview {
    /// Theme or layout.
    pub file: ConfigFile,
    /// Sections with at least one item.
    pub sections: Vec<MergeSectionPreview>,
    /// Errors of the incoming file; when non-empty there are no sections
    /// and nothing can be applied.
    pub errors: Vec<ConfigIssue>,
}

/// Which side to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MergeTake {
    /// Keep the user's version.
    #[default]
    Mine,
    /// Take the incoming version.
    Theirs,
}

/// The user's choice for one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeChoice {
    /// [`MergeItem::id`].
    pub item_id: String,
    /// The side to keep.
    pub take: MergeTake,
}

/// A backup of a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConfigBackup {
    /// The backup.
    pub id: ConfigBackupId,
    /// The file it belongs to.
    pub file: ConfigFile,
    /// When it was taken (RFC 3339).
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_names_round_trip() {
        for file in ConfigFile::ALL {
            assert_eq!(ConfigFile::from_file_name(file.file_name()), Some(file));
        }
        assert_eq!(ConfigFile::from_file_name("x.kdl"), None);
        assert_eq!(
            serde_json::to_value(ConfigFile::Layout).unwrap(),
            json!("layout")
        );
    }

    #[test]
    fn patches_are_op_tagged() {
        let patch = LayoutPatch::SplitSizes {
            node: LayoutNodeRef {
                window: "main".into(),
                breakpoint: 0,
                path: vec![],
            },
            sizes: vec!["240px".into(), "1fr".into()],
        };
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            json!({"op": "split-sizes", "node": {"window": "main", "breakpoint": 0, "path": []},
                   "sizes": ["240px", "1fr"]})
        );
        let remove: LayoutPatch = serde_json::from_value(
            json!({"op": "remove-key-binding", "context": "list", "key": "e"}),
        )
        .unwrap();
        assert!(matches!(remove, LayoutPatch::RemoveKeyBinding { .. }));
        let add: LayoutPatch = serde_json::from_value(
            json!({"op": "add-key-binding", "context": "list", "key": "a", "action": "mail.archive"}),
        )
        .unwrap();
        assert!(matches!(add, LayoutPatch::AddKeyBinding { args, .. } if args.is_empty()));
        assert_eq!(MergeTake::default(), MergeTake::Mine);
    }
}
