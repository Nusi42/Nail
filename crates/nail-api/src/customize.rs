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
//! * M5d (ADR 0131): the layout editor's and the toolbar customizer's
//!   patches ([`LayoutPatch::MovePane`] … [`LayoutPatch::RemoveToolbarItem`];
//!   [`crate::editing`] has their parts, the theme edits and the undo
//!   history).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::config::layout::ToolbarItem;
#[cfg(doc)]
use crate::editing::EditIssue;
use crate::editing::{EditKind, LayoutPlacement, NewToolbarItem, NewWidget, ToolbarSlot};
use crate::ids::ConfigBackupId;
use crate::localized::LocalizedMessage;

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
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
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
    /// Restore a toolbar to the default layout's version (M2c): the file's
    /// definitions of it are removed from a sparse file (`extends`), or
    /// replaced by the default items in a full one. A toolbar the defaults
    /// do not have is removed.
    ResetToolbar {
        /// Toolbar name.
        toolbar: String,
    },
    /// Restore the default binding of a key (M2c): the file's `bind` /
    /// `unbind` lines for the key are removed; in a full file the default
    /// binding, if any, is written back.
    ResetKeyBinding {
        /// Context.
        context: String,
        /// Key sequence.
        key: String,
    },

    // --- M5d (ADR 0131): the layout editor and the toolbar customizer ---
    /// Move a node (a pane with everything in it) to another place: into a
    /// container, or docked beside a node ([`LayoutPlacement`]). Into a
    /// container of the kind it left (its own, a stack, a split of the same
    /// orientation) it keeps its size and limits. A container
    /// left with no children is removed; a split left with one child is
    /// replaced by that child. `invalid_request` with
    /// [`EditIssue::InvalidPlacement`] for a move into the node itself, its
    /// descendant or a widget, or of the window's root.
    MovePane {
        /// The node to move.
        node: LayoutNodeRef,
        /// Where it goes (references the tree before the move).
        to: LayoutPlacement,
    },
    /// Swap two nodes (with their subtrees); each takes the other's place
    /// and keeps its own split size and limits when both containers are of
    /// the same kind (stacks, or splits of one orientation), else takes
    /// the slot's. Neither may contain the other.
    SwapPanes {
        /// One node.
        a: LayoutNodeRef,
        /// The other node.
        b: LayoutNodeRef,
    },
    /// Add a widget from the palette. `invalid_request` with
    /// [`EditIssue::UnknownWidget`] for an unknown kind, the config's option
    /// errors for invalid options.
    AddWidget {
        /// The widget.
        widget: NewWidget,
        /// Where it goes.
        to: LayoutPlacement,
    },
    /// Remove a node with its subtree (not the window's root). Containers
    /// left empty or a split left with one child are tidied as in
    /// [`Self::MovePane`].
    RemoveNode {
        /// The node.
        node: LayoutNodeRef,
    },
    /// Hide or show a node: writes or removes `hidden=#true` on it
    /// ([`crate::config::layout::LayoutNode::is_hidden`]). Hidden nodes keep
    /// their place and options; the renderer skips them (the layout editor
    /// shows them dimmed with a "show" eye).
    SetHidden {
        /// The node.
        node: LayoutNodeRef,
        /// Hide (`true`) or show (`false`).
        hidden: bool,
    },
    /// Apply a preset of the layout's `presets` section to a window
    /// breakpoint: the panes it names (by node id) are arranged in the
    /// preset's containers with its sizes (each keeping its limits from a
    /// split of the same orientation); panes of the breakpoint the preset
    /// does not name stay, hidden, after the preset's tree in a stack at the
    /// root. `invalid_request` with [`EditIssue::UnknownPreset`] /
    /// [`EditIssue::PresetPaneMissing`].
    ApplyPreset {
        /// The preset id, e.g. `reader-below`.
        preset: String,
        /// The window breakpoint the preset is applied to (`path` ignored).
        target: LayoutNodeRef,
    },
    /// Restore one window (every breakpoint) to the default layout's
    /// version: removed from a sparse file, replaced by the default in a full
    /// one. A window the defaults do not have is removed.
    ResetWindow {
        /// The window name.
        window: String,
    },
    /// Move a toolbar item within the bar, within "More", or between them
    /// (an action becomes a menu item in "More" and a toolbar item on the
    /// bar; overrides such as `label` / `icon` move along, `rank` / `display`
    /// only exist on the bar). A toolbar only in the defaults is copied into
    /// the file first. `invalid_request` with [`EditIssue::ToolbarSlotInvalid`].
    MoveToolbarItem {
        /// The toolbar name.
        toolbar: String,
        /// Where the item is.
        from: ToolbarSlot,
        /// Where it goes (indexes after removing it from `from`).
        to: ToolbarSlot,
    },
    /// Put an item from the customizer's palette on a toolbar.
    AddToolbarItem {
        /// The toolbar name.
        toolbar: String,
        /// The item (an action, separator or spacer; no `more` item).
        item: NewToolbarItem,
        /// Where it goes.
        to: ToolbarSlot,
    },
    /// Take an item off a toolbar (the bar or "More").
    RemoveToolbarItem {
        /// The toolbar name.
        toolbar: String,
        /// Where the item is.
        from: ToolbarSlot,
    },
}

impl LayoutPatch {
    /// The history kind of this patch (the label of its undo entry).
    pub fn kind(&self) -> EditKind {
        match self {
            Self::SplitSizes { .. } => EditKind::SplitSizes,
            Self::ColumnOrder { .. } => EditKind::ColumnOrder,
            Self::ToolbarOrder { .. } => EditKind::ToolbarOrder,
            Self::AddKeyBinding { .. } => EditKind::AddKeyBinding,
            Self::RemoveKeyBinding { .. } => EditKind::RemoveKeyBinding,
            Self::ResetToolbar { .. } => EditKind::ResetToolbar,
            Self::ResetKeyBinding { .. } => EditKind::ResetKeyBinding,
            Self::MovePane { .. } => EditKind::MovePane,
            Self::SwapPanes { .. } => EditKind::SwapPanes,
            Self::AddWidget { .. } => EditKind::AddWidget,
            Self::RemoveNode { .. } => EditKind::RemoveNode,
            Self::SetHidden { .. } => EditKind::SetHidden,
            Self::ApplyPreset { .. } => EditKind::ApplyPreset,
            Self::ResetWindow { .. } => EditKind::ResetWindow,
            Self::MoveToolbarItem { .. } => EditKind::MoveToolbarItem,
            Self::AddToolbarItem { .. } => EditKind::AddToolbarItem,
            Self::RemoveToolbarItem { .. } => EditKind::RemoveToolbarItem,
        }
    }

    /// The history kind of an `UpdateLayout` with these patches: the one
    /// patch's kind, the common kind of several patches of one kind (e.g.
    /// several divider moves), else [`EditKind::LayoutEdits`].
    pub fn kind_of(patches: &[LayoutPatch]) -> EditKind {
        match patches.split_first() {
            Some((first, rest)) if rest.iter().all(|p| p.kind() == first.kind()) => first.kind(),
            _ => EditKind::LayoutEdits,
        }
    }
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
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
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
        let reset: LayoutPatch =
            serde_json::from_value(json!({"op": "reset-toolbar", "toolbar": "reader"})).unwrap();
        assert!(matches!(reset, LayoutPatch::ResetToolbar { .. }));
        let reset: LayoutPatch = serde_json::from_value(
            json!({"op": "reset-key-binding", "context": "list", "key": "e"}),
        )
        .unwrap();
        assert!(matches!(reset, LayoutPatch::ResetKeyBinding { .. }));
        assert_eq!(MergeTake::default(), MergeTake::Mine);
    }

    #[test]
    fn m5d_patches() {
        let hide: LayoutPatch = serde_json::from_value(json!({
            "op": "set-hidden",
            "node": {"window": "mail", "breakpoint": 0, "path": [0]},
            "hidden": true
        }))
        .unwrap();
        assert_eq!(hide.kind(), EditKind::SetHidden);
        let moved: LayoutPatch = serde_json::from_value(json!({
            "op": "move-toolbar-item",
            "toolbar": "reader",
            "from": {"area": "bar", "index": 3},
            "to": {"area": "more", "index": 0}
        }))
        .unwrap();
        assert_eq!(moved.kind(), EditKind::MoveToolbarItem);
        let preset = LayoutPatch::ApplyPreset {
            preset: "focus".into(),
            target: LayoutNodeRef {
                window: "mail".into(),
                breakpoint: 0,
                path: vec![],
            },
        };
        assert_eq!(
            serde_json::to_value(&preset).unwrap()["op"],
            json!("apply-preset")
        );
        assert_eq!(LayoutPatch::kind_of(&[]), EditKind::LayoutEdits);
        assert_eq!(
            LayoutPatch::kind_of(&[preset.clone(), preset.clone()]),
            EditKind::ApplyPreset
        );
        assert_eq!(LayoutPatch::kind_of(&[preset, hide]), EditKind::LayoutEdits);
    }
}
