//! In-app editors (M5d, ADR 0131): the layout editor's and the toolbar
//! customizer's write-back patches, the theme editor's operations, the undo
//! history of both and how a change on disk is detected.
//!
//! * **Layout edits** are [`crate::LayoutPatch`]es sent with
//!   `UpdateLayout`, like the M1b divider / toolbar / key edits: move a pane
//!   (into a container, or docked beside another node — [`LayoutPlacement`]),
//!   swap two panes, add a widget ([`NewWidget`]) or remove a node, hide or
//!   show a node (`hidden=#true` on the node in `layout.kdl`), apply a preset
//!   (`presets` section of `layout.kdl`, [`crate::config::layout::LayoutPreset`]),
//!   reset a window to the default, and move / add / remove toolbar items
//!   between the bar and its "More" menu ([`ToolbarSlot`]). Nodes are
//!   addressed by [`crate::LayoutNodeRef`] (window, breakpoint, child path) as
//!   rendered from the current file.
//! * **Theme edits** are [`ThemePatch`]es sent with `EditTheme` against the
//!   user's `theme.kdl` or one of their own themes (`themes/<id>.kdl`,
//!   [`ThemeEditTarget`]): set or remove a token, set or remove one property
//!   of a part override, remove a part override. Built-in themes are never
//!   edited: `DuplicateTheme` copies one into `themes/` first. `RenameTheme`
//!   / `DeleteTheme` work on user themes only. `GetThemeDetails` lists a
//!   theme's tokens and part rules with where each comes from
//!   ([`ThemeDetails`]).
//! * **Undo history.** Every write of the layout or appearance files the
//!   core makes (the edits above, `SetTheme`, `SetThemePreferences`, merges,
//!   reverts) records an [`EditEntry`] in the history of its [`HistoryScope`]
//!   (kept in memory for the running core, at most [`MAX_EDIT_HISTORY`]
//!   entries per scope). `UndoEdit` / `RedoEdit` restore the files the entry
//!   changed; a new edit clears the redo list. `EditHistoryChanged` pushes
//!   the history after every change of it and after the scope's files
//!   changed on disk (its `revision` then changes).
//! * **Changes on disk.** A file's [`config_revision`] identifies its text.
//!   `UpdateLayout` and `EditTheme` may carry the revision the editor saw
//!   (`base_revision`); when the file changed since, the request answers
//!   `conflict` with [`EditIssue::Conflict`] and changes nothing. Undo / redo
//!   check that the files still hold what the entry wrote (or, for redo,
//!   what it replaced); otherwise `conflict` too, unless `force` (the current
//!   `theme.kdl` / `layout.kdl` is backed up first, ADR 0022 backups).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::appearance::ThemeInfo;
use crate::config::layout::{Orientation, ToolbarItem};
use crate::customize::LayoutNodeRef;
use crate::localized::LocalizedMessage;

/// Most entries kept per [`HistoryScope`] (undo and redo each); the oldest
/// undo entry is dropped beyond it.
pub const MAX_EDIT_HISTORY: usize = 50;

/// Longest theme display name (`theme "<name>"`).
pub const MAX_THEME_NAME_CHARS: usize = 64;

/// The [`config_revision`] of a file that does not exist.
pub const NO_REVISION: &str = "none";

/// The revision of a configuration file's text: 16 lowercase hex digits
/// (FNV-1a, 64 bit, over the UTF-8 bytes), or [`NO_REVISION`] for a missing
/// file. Only for noticing changes, not a security check.
pub fn config_revision(text: Option<&str>) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    match text {
        None => NO_REVISION.to_owned(),
        Some(text) => {
            let hash = text.bytes().fold(OFFSET, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(PRIME)
            });
            format!("{hash:016x}")
        }
    }
}

/// Whether `name` is a usable theme display name: not blank, at most
/// [`MAX_THEME_NAME_CHARS`] characters, no control characters.
pub fn is_theme_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= MAX_THEME_NAME_CHARS
        && !name.chars().any(char::is_control)
}

/// The side of a node a pane is docked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DockSide {
    /// Left of the target.
    Left,
    /// Right of the target.
    Right,
    /// Above the target.
    Top,
    /// Below the target.
    Bottom,
}

impl DockSide {
    /// Every side.
    pub const ALL: [DockSide; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// The split orientation the docked pane and its target share.
    pub fn orientation(self) -> Orientation {
        match self {
            Self::Left | Self::Right => Orientation::Horizontal,
            Self::Top | Self::Bottom => Orientation::Vertical,
        }
    }

    /// Whether the docked pane comes before the target.
    pub fn before(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// Where a moved or added node goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LayoutPlacement {
    /// Inside a container (`split`, `stack`, `tabs`) at a child index
    /// (clamped to the end). A split's per-child lists (`sizes`, …) get an
    /// entry for the new child (`1fr`, no minimum / maximum, not
    /// collapsible); a stack's `sizes` the renderer's default.
    Into {
        /// The container.
        parent: LayoutNodeRef,
        /// The child index the node gets.
        index: u32,
    },
    /// Docked beside a node: into the target's parent next to it when the
    /// parent is a split of the side's orientation, otherwise the target is
    /// wrapped in a new split of that orientation (sizes `1fr 1fr`) holding
    /// both.
    Beside {
        /// The node to dock to.
        target: LayoutNodeRef,
        /// Which side of it.
        side: DockSide,
    },
}

/// A widget added from the layout editor's palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NewWidget {
    /// The widget kind, a registered kind such as `mini-month`. A plug-in
    /// panel is kind `plugin-panel` with the option `panel` =
    /// `"<plugin>.<panel>"` (written as the node's argument,
    /// `plugin-panel "<plugin>.<panel>"`).
    pub kind: String,
    /// The node id, `null` = one derived from the kind that is unique in
    /// the window.
    #[serde(default)]
    pub id: Option<String>,
    /// The node label, `null` = none.
    #[serde(default)]
    pub label: Option<String>,
    /// Widget options as in KDL (kebab-case keys), checked against the
    /// kind's option schema.
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
}

/// The two places of a toolbar's items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ToolbarArea {
    /// The bar itself (the toolbar's items, without the `more {}` item).
    Bar,
    /// The toolbar's "More" menu (the `more {}` item's own entries; one is
    /// added at the end of the bar when the toolbar has none).
    More,
}

/// A position among a toolbar's items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ToolbarSlot {
    /// The bar or the "More" menu.
    pub area: ToolbarArea,
    /// The index in that area: in [`ToolbarArea::Bar`] among the bar's items
    /// not counting the `more {}` item, in [`ToolbarArea::More`] among the
    /// menu's entries. As a destination it is clamped to the end.
    pub index: u32,
}

/// An item put on a toolbar from the customizer's palette: an action
/// (plug-in actions included), a separator or a spacer. In the "More" menu
/// an action becomes a menu item and a spacer is refused.
pub type NewToolbarItem = ToolbarItem;

/// Which file a theme edit writes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ThemeEditTarget {
    /// The user's `theme.kdl` (its tokens and parts override the active
    /// theme, like the M5a accent / radius / font-size overrides).
    User,
    /// A user theme, `themes/<theme_id>.kdl` (built-in themes are refused
    /// with [`EditIssue::NotEditable`]).
    Theme {
        /// The theme id.
        theme_id: String,
    },
}

/// One theme edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum ThemePatch {
    /// Set a token in the file's `tokens` block, or remove it (`value`
    /// `null`: the extended theme's value applies again).
    SetToken {
        /// Token category as in KDL, e.g. `color`, `radius`, `font-size`,
        /// `space`, `motion`.
        category: String,
        /// Token name, e.g. `accent`, `md`.
        name: String,
        /// The value as written in KDL (`"#3b5bdb"`, `"10"`, `"$color.fg"`),
        /// `null` = remove.
        value: Option<String>,
        /// A mode / variant property instead of the base value: `dark`,
        /// `high-contrast`, `print`, `compact`; `null` = the base value.
        #[serde(default)]
        mode: Option<String>,
    },
    /// Set one property of a part rule (`part "<selector>" { <property>
    /// "<value>" }`), creating the rule when needed, or remove the property
    /// (`value` `null`; an empty rule is removed).
    SetPartProperty {
        /// The part selector, e.g. `message-list.row[state~="unread"]`.
        selector: String,
        /// The property, e.g. `background-color`.
        property: String,
        /// The value, `null` = remove.
        value: Option<String>,
    },
    /// Remove the file's whole rule for a selector.
    ResetPart {
        /// The part selector.
        selector: String,
    },
}

/// Where a token's or part rule's effective value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ValueOrigin {
    /// The base theme (`defaults/theme.kdl`).
    Base,
    /// A theme this one extends (other than the base).
    Extended,
    /// This theme's own file.
    Own,
}

/// A token of a theme as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThemeTokenValue {
    /// Category, e.g. `color`.
    pub category: String,
    /// Name, e.g. `accent`.
    pub name: String,
    /// The effective base value.
    pub value: String,
    /// Mode / variant values (`dark`, `high-contrast`, `print`, `compact`).
    #[serde(default)]
    pub modes: IndexMap<String, String>,
    /// Where it is defined.
    pub origin: ValueOrigin,
}

/// A part rule of a theme as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThemePartRule {
    /// The selector.
    pub selector: String,
    /// Its properties in file order.
    pub properties: IndexMap<String, String>,
    /// Where the rule is defined (a theme's own rule replaces nothing: rules
    /// of the base and of extended themes apply too, in that order).
    pub origin: ValueOrigin,
}

/// Everything the theme editor needs about one theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ThemeDetails {
    /// The gallery entry.
    pub info: ThemeInfo,
    /// Whether `EditTheme` accepts it (user themes; built-ins are copied
    /// with `DuplicateTheme` first).
    pub editable: bool,
    /// The [`config_revision`] of the theme's file (for `base_revision`).
    pub revision: String,
    /// Every token, base tokens included, in schema order.
    pub tokens: Vec<ThemeTokenValue>,
    /// Every part rule that applies, base rules first.
    pub parts: Vec<ThemePartRule>,
}

labeled_enum! {
    /// The two undo histories (`history-scope-*`).
    pub enum HistoryScope : "history-scope" {
        /// `layout.kdl`.
        Layout = "layout",
        /// `theme.kdl` and the user themes in `themes/`.
        Appearance = "appearance",
    }
}

labeled_enum! {
    /// What an undo entry did (`edit-kind-*`, the label of Undo / Redo and
    /// of the history list).
    pub enum EditKind : "edit-kind" {
        /// Divider positions.
        SplitSizes = "split-sizes",
        /// A list's column order.
        ColumnOrder = "column-order",
        /// A toolbar's items.
        ToolbarOrder = "toolbar-order",
        /// A key binding added.
        AddKeyBinding = "add-key-binding",
        /// A key binding removed.
        RemoveKeyBinding = "remove-key-binding",
        /// A toolbar reset to the default.
        ResetToolbar = "reset-toolbar",
        /// A key binding reset to the default.
        ResetKeyBinding = "reset-key-binding",
        /// A pane moved or docked.
        MovePane = "move-pane",
        /// Two panes swapped.
        SwapPanes = "swap-panes",
        /// A widget added.
        AddWidget = "add-widget",
        /// A node removed.
        RemoveNode = "remove-node",
        /// A node hidden or shown.
        SetHidden = "set-hidden",
        /// A preset applied.
        ApplyPreset = "apply-preset",
        /// A window reset to the default.
        ResetWindow = "reset-window",
        /// A toolbar item moved (bar ↔ More).
        MoveToolbarItem = "move-toolbar-item",
        /// A toolbar item added.
        AddToolbarItem = "add-toolbar-item",
        /// A toolbar item removed.
        RemoveToolbarItem = "remove-toolbar-item",
        /// Several layout edits at once.
        LayoutEdits = "layout-edits",
        /// A fixed theme chosen.
        SetTheme = "set-theme",
        /// Follow Windows, density or the overrides changed.
        SetThemePreferences = "set-theme-preferences",
        /// Tokens or part overrides edited.
        EditTheme = "edit-theme",
        /// A theme duplicated.
        DuplicateTheme = "duplicate-theme",
        /// A theme renamed.
        RenameTheme = "rename-theme",
        /// A theme deleted.
        DeleteTheme = "delete-theme",
        /// A merge applied.
        Merge = "merge",
        /// A backup restored.
        Revert = "revert",
    }
}

labeled_enum! {
    /// Why an editor request was refused (`edit-issue-*`).
    pub enum EditIssue : "edit-issue" {
        /// The file changed on disk since the editor read it, or no longer
        /// holds what the undo entry expects (`conflict`).
        Conflict = "conflict",
        /// Nothing to undo.
        NothingToUndo = "nothing-to-undo",
        /// Nothing to redo.
        NothingToRedo = "nothing-to-redo",
        /// A node reference names no node of the file.
        NodeNotFound = "node-not-found",
        /// The placement is impossible (into a widget, into the node itself
        /// or its descendant, the window root removed, …).
        InvalidPlacement = "invalid-placement",
        /// The widget kind is not registered.
        UnknownWidget = "unknown-widget",
        /// No such preset (or it is for another window).
        UnknownPreset = "unknown-preset",
        /// The preset names a pane id the window does not have.
        PresetPaneMissing = "preset-pane-missing",
        /// A toolbar slot is out of range, or a spacer was put into "More".
        ToolbarSlotInvalid = "toolbar-slot-invalid",
        /// The theme cannot be edited (built-in); duplicate it first.
        NotEditable = "not-editable",
        /// A theme with that id exists already.
        ThemeExists = "theme-exists",
        /// No such theme.
        ThemeNotFound = "theme-not-found",
        /// The theme name is blank, too long or has control characters.
        InvalidThemeName = "invalid-theme-name",
        /// The token category, name, mode or value is invalid.
        InvalidToken = "invalid-token",
        /// The part selector, property or value is invalid.
        InvalidPart = "invalid-part",
    }
}

/// One undoable change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditEntry {
    /// Stable within the running core.
    pub id: String,
    /// What it did.
    pub kind: EditKind,
    /// The user-facing label (`edit-kind-<kind>` with arguments such as
    /// `preset`, `theme`, `toolbar`).
    pub label: LocalizedMessage,
    /// When (RFC 3339).
    pub at: String,
    /// The files it changed, relative to the config directory
    /// (`layout.kdl`, `theme.kdl`, `themes/<id>.kdl`).
    pub files: Vec<String>,
}

/// The undo history of one scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditHistory {
    /// The scope.
    pub scope: HistoryScope,
    /// The [`config_revision`] of the scope's main file (`layout.kdl` /
    /// `theme.kdl`) now.
    pub revision: String,
    /// Undoable entries, oldest first (the last one is undone next).
    pub undo: Vec<EditEntry>,
    /// Redoable entries, the last one is redone next.
    pub redo: Vec<EditEntry>,
}

impl EditHistory {
    /// An empty history of `scope` at `revision`.
    pub fn empty(scope: HistoryScope, revision: impl Into<String>) -> Self {
        Self {
            scope,
            revision: revision.into(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use serde_json::json;

    #[test]
    fn labels() {
        assert_labels(
            HistoryScope::ALL,
            HistoryScope::as_str,
            HistoryScope::message_id,
        );
        assert_labels(EditKind::ALL, EditKind::as_str, EditKind::message_id);
        assert_labels(EditIssue::ALL, EditIssue::as_str, EditIssue::message_id);
        assert_eq!(EditIssue::Conflict.message_id(), "edit-issue-conflict");
    }

    #[test]
    fn revisions() {
        assert_eq!(config_revision(None), NO_REVISION);
        // FNV-1a 64 test vectors.
        assert_eq!(config_revision(Some("")), "cbf29ce484222325");
        assert_eq!(config_revision(Some("a")), "af63dc4c8601ec8c");
        assert_ne!(
            config_revision(Some("layout \"a\" {}\n")),
            config_revision(Some("layout \"b\" {}\n"))
        );
    }

    #[test]
    fn theme_names() {
        assert!(is_theme_name("My theme"));
        assert!(!is_theme_name("  "));
        assert!(!is_theme_name("a\nb"));
        assert!(!is_theme_name(&"x".repeat(MAX_THEME_NAME_CHARS + 1)));
    }

    #[test]
    fn dock_sides() {
        assert_eq!(DockSide::Left.orientation(), Orientation::Horizontal);
        assert_eq!(DockSide::Bottom.orientation(), Orientation::Vertical);
        assert!(DockSide::Top.before());
        assert!(!DockSide::Right.before());
    }

    #[test]
    fn wire_shapes() {
        let placement = LayoutPlacement::Beside {
            target: LayoutNodeRef {
                window: "mail".into(),
                breakpoint: 0,
                path: vec![2],
            },
            side: DockSide::Bottom,
        };
        assert_eq!(
            serde_json::to_value(&placement).unwrap(),
            json!({"kind": "beside", "target": {"window": "mail", "breakpoint": 0, "path": [2]},
                   "side": "bottom"})
        );
        let patch: ThemePatch = serde_json::from_value(
            json!({"op": "set-token", "category": "color", "name": "accent", "value": "#123456"}),
        )
        .unwrap();
        assert!(matches!(patch, ThemePatch::SetToken { mode: None, .. }));
        let target: ThemeEditTarget =
            serde_json::from_value(json!({"kind": "theme", "theme_id": "paper"})).unwrap();
        assert_eq!(
            target,
            ThemeEditTarget::Theme {
                theme_id: "paper".into()
            }
        );
        let slot: ToolbarSlot =
            serde_json::from_value(json!({"area": "more", "index": 0})).unwrap();
        assert_eq!(slot.area, ToolbarArea::More);
    }
}
