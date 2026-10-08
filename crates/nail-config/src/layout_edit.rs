//! The layout editor's and toolbar customizer's write-back patches (M5d,
//! ADR 0131): move / swap / dock panes, add / remove widgets, hide / show,
//! presets, window reset, toolbar items between the bar and "More".
//!
//! Every patch edits the user's `layout.kdl` in place ([`crate::kdl_edit`]):
//! a node that moves takes its subtree and the comment block directly above
//! it along and is re-indented for its new depth; comments separated from
//! it by a blank line stay where they were; everything the patch does not
//! touch stays byte for byte. A window or toolbar that only the defaults
//! define is copied into a sparse file first (as the M1b patches do).
//!
//! Tree rules (the semantics documented on `nail_api::LayoutPatch`):
//!
//! * A split's per-child lists (`sizes`, `min-sizes`, `max-sizes`,
//!   `collapsible`) and a stack's `sizes` lose the entry of a child that
//!   leaves and get one for a child that arrives (`1fr`, `none`, `none`,
//!   `false`; in a stack `auto` for bar-like widgets, else `1fr`). A node
//!   that moves (or swaps) between containers of the same kind — within its
//!   own container, stack to stack, split to split of the same orientation —
//!   carries its own entries along (ADR 0138: a swapped folder pane keeps
//!   its width and limits). Lists that are not written stay unwritten. A
//!   `tabs` node's `active` follows its tab.
//! * A preset writes its own `sizes`; a pane it places in a split keeps its
//!   `min-sizes` / `max-sizes` / `collapsible` from a split of the same
//!   orientation (its current one, else the default layout's).
//! * A container left without children is removed; a split left with one
//!   child is replaced by that child (the split's comments stay above it).
//!   The window's root is never removed.
//! * Docking beside a node goes into its parent split when that split has
//!   the side's orientation, else the node is wrapped in a new split
//!   (`sizes="1fr 1fr"`).
//!
//! Node references ([`LayoutNodeRef`]) address the tree before the patch;
//! while a patch rearranges the tree it keeps track of the nodes it still
//! needs with temporary marker properties that never reach the file.

use std::collections::HashMap;

use kdl::{KdlNode, KdlValue};
use nail_api::editing::{EditIssue, LayoutPlacement, NewWidget, ToolbarArea, ToolbarSlot};
use nail_api::plugins::PLUGIN_PANEL_WIDGET;
use nail_api::{LayoutNodeRef, LocalizedMessage, Orientation, PresetNode, ToolbarItem};

use crate::kdl_edit::{
    Style, block_of, detach, ensure_multiline, extracted, first_arg, json_text, parse_one,
    parse_rendered, put, reindent, remove_prop, replace_with, set_prop, str_prop,
};
use crate::kdl_util::{children, is_kebab};
use crate::kdl_write::quote;
use crate::l10n::msg;
use crate::write_back::{EditError, LayoutEditor, Site};
use crate::{default_layout, render, widgets};

/// Marks the node a placement targets while the tree changes.
const MARK_TARGET: &str = "__nail_edit_target";
/// Marks the container a node left while the tree changes.
const MARK_SOURCE: &str = "__nail_edit_source";

/// Widgets a stack gives `auto` height by default (the renderer's rule,
/// `nail_api::LayoutNode::Stack::sizes`).
const BAR_WIDGETS: &[&str] = &["toolbar", "status-bar", "account-switcher", "text"];

/// A split's per-child lists with the entry a new child gets (a stack has
/// only `sizes`).
const CHILD_LISTS: &[(&str, &str)] = &[
    ("sizes", "1fr"),
    ("min-sizes", "none"),
    ("max-sizes", "none"),
    ("collapsible", "false"),
];

fn issue(issue: EditIssue) -> LocalizedMessage {
    msg(&issue.message_id())
}

fn invalid(issue_id: EditIssue) -> EditError {
    EditError::Invalid(issue(issue_id))
}

fn not_found(node: &LayoutNodeRef) -> EditError {
    EditError::Invalid(
        issue(EditIssue::NodeNotFound)
            .arg("window", node.window.as_str())
            .arg("breakpoint", node.breakpoint)
            .arg("path", format!("{:?}", node.path)),
    )
}

fn bad_placement() -> EditError {
    invalid(EditIssue::InvalidPlacement)
}

fn bad_slot(toolbar: &str) -> EditError {
    EditError::Invalid(issue(EditIssue::ToolbarSlotInvalid).arg("toolbar", toolbar))
}

/// A value KDL cannot hold, or a rearranged node that no longer parses
/// (only a multi-line string whose indentation changed can do that).
fn not_plain() -> EditError {
    EditError::Invalid(msg("config-edit-plain-values"))
}

fn is_container(node: &KdlNode) -> bool {
    matches!(node.name().value(), "split" | "stack" | "tabs")
}

fn child_count(node: &KdlNode) -> usize {
    node.children().map_or(0, |c| c.nodes().len())
}

/// A split's orientation (argument or `orientation=`; horizontal by default).
fn orientation_of(node: &KdlNode) -> Orientation {
    match first_arg(node).or_else(|| str_prop(node, "orientation")) {
        Some(text) => Orientation::parse(text).unwrap_or_default(),
        None => Orientation::Horizontal,
    }
}

fn to_index(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// A whitespace-separated list property, split (`None` when absent).
fn list_prop(node: &KdlNode, key: &str) -> Option<Vec<String>> {
    str_prop(node, key).map(|v| v.split_whitespace().map(str::to_owned).collect())
}

fn set_list(node: &mut KdlNode, key: &str, items: &[String]) {
    set_prop(node, key, KdlValue::String(items.join(" ")));
}

fn active_of(node: &KdlNode) -> u32 {
    node.get("active")
        .and_then(KdlValue::as_integer)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

fn set_active(node: &mut KdlNode, active: u32) {
    if active == 0 {
        remove_prop(node, "active");
    } else {
        set_prop(node, "active", KdlValue::Integer(i128::from(active)));
    }
}

/// `header id="<id>"` when there is an id.
fn with_id(mut header: String, id: Option<&str>) -> String {
    if let Some(id) = id {
        header.push_str(" id=");
        header.push_str(&quote(id));
    }
    header
}

/// `header sizes="…"` when there are sizes.
fn with_sizes<S: AsRef<str>>(mut header: String, sizes: &[S]) -> String {
    if !sizes.is_empty() {
        let joined: Vec<&str> = sizes.iter().map(AsRef::as_ref).collect();
        header.push_str(" sizes=");
        header.push_str(&quote(&joined.join(" ")));
    }
    header
}

/// The per-child list entries a node took out of its container.
type Entries = Vec<(String, String)>;

/// The kind of container whose per-child list entries a node carries when
/// it moves: entries only travel between containers of the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Stack,
    Split(Orientation),
}

fn list_kind(node: &KdlNode) -> Option<ListKind> {
    match node.name().value() {
        "stack" => Some(ListKind::Stack),
        "split" => Some(ListKind::Split(orientation_of(node))),
        _ => None,
    }
}

/// A list property with one entry per child (`None` when absent or out of
/// step with the children).
fn full_list(node: &KdlNode, key: &str) -> Option<Vec<String>> {
    list_prop(node, key).filter(|items| items.len() == child_count(node))
}

/// The entries a node takes along, and the kind of container it left.
#[derive(Debug, Clone, Copy)]
struct Carried<'a> {
    entries: &'a Entries,
    from: Option<ListKind>,
}

/// `header key="…"` with the values.
fn with_list(mut header: String, key: &str, values: &[String]) -> String {
    header.push(' ');
    header.push_str(key);
    header.push('=');
    header.push_str(&quote(&values.join(" ")));
    header
}

// ---------------------------------------------------------------------------
// One window breakpoint

/// The `window` node of one breakpoint of the file.
#[derive(Debug, Clone, Copy)]
struct Win {
    site: Site,
    /// The nesting depth of the `window` node (1, or 2 inside a breakpoint).
    depth: usize,
}

impl Win {
    /// The window breakpoint a reference names, copying the window from the
    /// defaults into a sparse file first.
    fn open(editor: &mut LayoutEditor, node: &LayoutNodeRef) -> Result<Self, EditError> {
        if LayoutEditor::window_sites(editor.body(), &node.window).is_empty() {
            editor
                .copy_default_window(&node.window)
                .map_err(|_| not_found(node))?;
        }
        let sites = LayoutEditor::window_sites(editor.body(), &node.window);
        let site = sites
            .get(to_index(node.breakpoint))
            .copied()
            .ok_or_else(|| not_found(node))?;
        Ok(Self {
            site,
            depth: if site.inner.is_some() { 2 } else { 1 },
        })
    }

    fn tree<'e>(&self, editor: &'e mut LayoutEditor) -> Tree<'e> {
        let style = editor.style.clone();
        let body = editor.body();
        let window = match self.site.inner {
            None => &mut body.nodes_mut()[self.site.top],
            Some(inner) => &mut body.nodes_mut()[self.site.top]
                .ensure_children()
                .nodes_mut()[inner],
        };
        Tree {
            window,
            depth: self.depth,
            style,
        }
    }
}

/// A window tree being edited. Nodes are addressed by KDL paths from the
/// `window` node: `[0]` is the layout root, `[0, i, …]` follows a
/// [`LayoutNodeRef::path`]; `[]` is the window node itself.
struct Tree<'e> {
    window: &'e mut KdlNode,
    depth: usize,
    style: Style,
}

impl Tree<'_> {
    fn get(&self, path: &[usize]) -> Option<&KdlNode> {
        let mut current: &KdlNode = self.window;
        for &i in path {
            current = current.children()?.nodes().get(i)?;
        }
        Some(current)
    }

    fn get_mut(&mut self, path: &[usize]) -> Option<&mut KdlNode> {
        let mut current: &mut KdlNode = self.window;
        for &i in path {
            current = current.children_mut().as_mut()?.nodes_mut().get_mut(i)?;
        }
        Some(current)
    }

    /// `get_mut` for a path that must exist.
    fn at(&mut self, path: &[usize]) -> Result<&mut KdlNode, EditError> {
        self.get_mut(path).ok_or_else(bad_placement)
    }

    /// The nesting depth of the node at `path`.
    fn depth_of(&self, path: &[usize]) -> usize {
        self.depth + path.len()
    }

    /// The KDL path of a layout node reference (every step but the last
    /// through a container, so widget `row`s are never addressed).
    fn resolve(&self, node: &LayoutNodeRef) -> Result<Vec<usize>, EditError> {
        let mut path = vec![0];
        let mut current = self.get(&path).ok_or_else(|| not_found(node))?;
        for &step in &node.path {
            if !is_container(current) {
                return Err(not_found(node));
            }
            let index = to_index(step);
            current = children(current)
                .get(index)
                .ok_or_else(|| not_found(node))?;
            path.push(index);
        }
        Ok(path)
    }

    fn mark(&mut self, path: &[usize], mark: &str) -> Result<(), EditError> {
        set_prop(self.at(path)?, mark, KdlValue::Bool(true));
        Ok(())
    }

    /// Finds a marked node and removes the mark.
    fn unmark(&mut self, mark: &str) -> Result<Vec<usize>, EditError> {
        let mut path = vec![0];
        let found = self
            .get(&path)
            .is_some_and(|root| find_node(root, &mut path, &|n| n.entry(mark).is_some()));
        if !found {
            return Err(bad_placement());
        }
        remove_prop(self.at(&path)?, mark);
        Ok(path)
    }

    /// The path of the layout node with this id.
    fn find_id(&self, id: &str) -> Option<Vec<usize>> {
        let mut path = vec![0];
        let root = self.get(&path)?;
        find_node(root, &mut path, &|n| str_prop(n, "id") == Some(id)).then_some(path)
    }

    /// Every node id of the tree.
    fn ids(&self) -> Vec<String> {
        fn collect(node: &KdlNode, out: &mut Vec<String>) {
            if let Some(id) = str_prop(node, "id") {
                out.push(id.to_owned());
            }
            if is_container(node) {
                for child in children(node) {
                    collect(child, out);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(root) = self.get(&[0]) {
            collect(root, &mut out);
        }
        out
    }

    /// Takes child `index` out of the container at `parent` (its list
    /// entries go too). Returns the detached node (indented for its old
    /// depth) and the entries it had.
    fn take(&mut self, parent: &[usize], index: usize) -> Result<(KdlNode, Entries), EditError> {
        let depth = self.depth_of(parent) + 1;
        let style = self.style.clone();
        let container = self.at(parent)?;
        let count = child_count(container);
        if index >= count {
            return Err(bad_placement());
        }
        let mut entries = Vec::new();
        match container.name().value() {
            "split" | "stack" => {
                for &(key, _) in CHILD_LISTS {
                    if let Some(mut items) = list_prop(container, key)
                        && index < items.len()
                    {
                        entries.push((key.to_owned(), items.remove(index)));
                        set_list(container, key, &items);
                    }
                }
            }
            "tabs" => {
                let active = to_index(active_of(container));
                let next = if active > index {
                    active - 1
                } else {
                    active.min(count.saturating_sub(2))
                };
                set_active(container, u32::try_from(next).unwrap_or(0));
            }
            _ => {}
        }
        let block = block_of(container);
        Ok((detach(block, index, depth, &style), entries))
    }

    /// Puts a detached node (indented for `from`) into the container at
    /// `parent` at `index` (clamped). `carried` = the entries it had in its
    /// old container, used when that was of the same [`ListKind`]; else new
    /// entries are written.
    fn insert(
        &mut self,
        parent: &[usize],
        index: usize,
        node: &KdlNode,
        from: usize,
        carried: Option<Carried<'_>>,
    ) -> Result<(), EditError> {
        let depth = self.depth_of(parent) + 1;
        let style = self.style.clone();
        let node = reindent(node, from, depth, &style).ok_or_else(not_plain)?;
        let kind = node.name().value().to_owned();
        let container = self.at(parent)?;
        let count = child_count(container);
        let index = index.min(count);
        let name = container.name().value().to_owned();
        let kind_here = list_kind(container);
        let carried = carried
            .filter(|c| c.from.is_some() && c.from == kind_here)
            .map(|c| c.entries);
        match name.as_str() {
            "split" | "stack" => {
                for &(key, default) in CHILD_LISTS {
                    let Some(mut items) = list_prop(container, key) else {
                        continue;
                    };
                    if items.len() != count {
                        continue;
                    }
                    let carried = carried
                        .and_then(|c| c.iter().find(|(k, _)| k == key))
                        .map(|(_, v)| v.clone());
                    let value = carried.unwrap_or_else(|| {
                        if name == "stack" {
                            stack_size(&kind).to_owned()
                        } else {
                            default.to_owned()
                        }
                    });
                    items.insert(index, value);
                    set_list(container, key, &items);
                }
            }
            "tabs" => {
                let active = active_of(container);
                if count > 0 && to_index(active) >= index {
                    set_active(container, active.saturating_add(1));
                }
            }
            _ => return Err(bad_placement()),
        }
        put(block_of(container), index, node, depth, &style);
        Ok(())
    }

    /// Swaps the per-child list entries of two nodes whose containers are
    /// of the same [`ListKind`] (nothing otherwise).
    fn swap_entries(&mut self, a: &[usize], b: &[usize]) -> Result<(), EditError> {
        let (&ia, pa) = a.split_last().ok_or_else(bad_placement)?;
        let (&ib, pb) = b.split_last().ok_or_else(bad_placement)?;
        let ka = self.get(pa).and_then(list_kind);
        if ka.is_none() || ka != self.get(pb).and_then(list_kind) {
            return Ok(());
        }
        for &(key, _) in CHILD_LISTS {
            let la = self.get(pa).and_then(|n| full_list(n, key));
            if pa == pb {
                if let Some(mut items) = la {
                    items.swap(ia, ib);
                    set_list(self.at(pa)?, key, &items);
                }
                continue;
            }
            let lb = self.get(pb).and_then(|n| full_list(n, key));
            if let (Some(mut la), Some(mut lb)) = (la, lb) {
                std::mem::swap(&mut la[ia], &mut lb[ib]);
                set_list(self.at(pa)?, key, &la);
                set_list(self.at(pb)?, key, &lb);
            }
        }
        Ok(())
    }

    /// The `min-sizes` / `max-sizes` / `collapsible` entries of the node at
    /// `path` in its container, with that container's kind.
    fn limits_of(&self, path: &[usize]) -> Option<(ListKind, Entries)> {
        let (&index, parent) = path.split_last()?;
        limits_in(self.get(parent)?, index)
    }

    /// Removes containers a change left empty and replaces a split left
    /// with one child by that child, from `path` upwards.
    fn tidy(&mut self, path: &[usize]) -> Result<(), EditError> {
        let node = self.get(path).ok_or_else(bad_placement)?;
        let count = child_count(node);
        let is_split = node.name().value() == "split";
        let Some((&index, parent)) = path.split_last() else {
            return Ok(());
        };
        if count == 0 {
            if parent.is_empty() {
                // The window's root would go.
                return Err(bad_placement());
            }
            self.take(parent, index)?;
            return self.tidy(parent);
        }
        if count == 1 && is_split {
            let depth = self.depth_of(path);
            let style = self.style.clone();
            let split = self.at(path)?;
            let child = detach(block_of(split), 0, depth + 1, &style);
            let child = reindent(&child, depth + 1, depth, &style).ok_or_else(not_plain)?;
            let parent_block = block_of(self.at(parent)?);
            ensure_multiline(parent_block, depth, &style);
            replace_with(parent_block, index, child, true, &style);
        }
        Ok(())
    }

    /// Puts a node (detached, indented for `from`) where `placement` says;
    /// `target` is the placement's node as a KDL path.
    fn place(
        &mut self,
        node: &KdlNode,
        from: usize,
        target: &[usize],
        placement: &LayoutPlacement,
        carried: Option<Carried<'_>>,
    ) -> Result<(), EditError> {
        match placement {
            LayoutPlacement::Into { index, .. } => {
                if !self.get(target).is_some_and(is_container) {
                    return Err(bad_placement());
                }
                self.insert(target, to_index(*index), node, from, carried)
            }
            LayoutPlacement::Beside { side, .. } => {
                let (&t_index, parent) = target.split_last().ok_or_else(bad_placement)?;
                let parent_node = self.get(parent).ok_or_else(bad_placement)?;
                if !parent.is_empty()
                    && parent_node.name().value() == "split"
                    && orientation_of(parent_node) == side.orientation()
                {
                    let index = if side.before() { t_index } else { t_index + 1 };
                    return self.insert(parent, index, node, from, carried);
                }
                // Wrap the target in a new split holding both.
                let depth = self.depth_of(target);
                let style = self.style.clone();
                let header = format!(
                    "split {} sizes=\"1fr 1fr\"",
                    quote(Orientation::as_str(side.orientation()))
                );
                let parent_block = block_of(self.at(parent)?);
                let wrapped = detach(parent_block, t_index, depth, &style);
                let wrapped = reindent(&wrapped, depth, depth + 1, &style).ok_or_else(not_plain)?;
                let docked = reindent(node, from, depth + 1, &style).ok_or_else(not_plain)?;
                let pair = if side.before() {
                    [docked, wrapped]
                } else {
                    [wrapped, docked]
                };
                let split = container(&header, depth, pair, &style)?;
                put(parent_block, t_index, split, depth, &style);
                Ok(())
            }
        }
    }
}

/// Depth-first search below `node` (whose path is `path`) through
/// containers; on success `path` is the found node's path.
fn find_node(node: &KdlNode, path: &mut Vec<usize>, hit: &dyn Fn(&KdlNode) -> bool) -> bool {
    if hit(node) {
        return true;
    }
    if !is_container(node) {
        return false;
    }
    for (i, child) in children(node).iter().enumerate() {
        path.push(i);
        if find_node(child, path, hit) {
            return true;
        }
        path.pop();
    }
    false
}

/// The default `sizes` entry of a stack child of this kind.
fn stack_size(kind: &str) -> &'static str {
    if BAR_WIDGETS.contains(&kind) {
        "auto"
    } else {
        "1fr"
    }
}

/// A new container node (`header` = its line without the block) at `depth`
/// holding `children` (detached, indented for `depth + 1`).
fn container(
    header: &str,
    depth: usize,
    children: impl IntoIterator<Item = KdlNode>,
    style: &Style,
) -> Result<KdlNode, EditError> {
    let pad = style.pad(depth);
    let mut node = parse_one(&format!("{pad}{header} {{{nl}{pad}}}{nl}", nl = style.nl))
        .ok_or_else(not_plain)?;
    let block = node.ensure_children();
    for child in children {
        let end = block.nodes().len();
        put(block, end, child, depth + 1, style);
    }
    Ok(node)
}

fn placement_target(placement: &LayoutPlacement) -> &LayoutNodeRef {
    match placement {
        LayoutPlacement::Into { parent, .. } => parent,
        LayoutPlacement::Beside { target, .. } => target,
    }
}

/// Both references must name the same window breakpoint.
fn same_breakpoint(a: &LayoutNodeRef, b: &LayoutNodeRef) -> Result<(), EditError> {
    if a.window == b.window && a.breakpoint == b.breakpoint {
        Ok(())
    } else {
        Err(bad_placement())
    }
}

// ---------------------------------------------------------------------------
// Pane patches

/// `move-pane`: moves a node with its subtree into a container or docks it
/// beside a node (references name the tree before the move).
pub(crate) fn move_pane(
    editor: &mut LayoutEditor,
    node: &LayoutNodeRef,
    to: &LayoutPlacement,
) -> Result<(), EditError> {
    let target_ref = placement_target(to);
    same_breakpoint(node, target_ref)?;
    let win = Win::open(editor, node)?;
    let mut tree = win.tree(editor);
    let source = tree.resolve(node)?;
    let target = tree.resolve(target_ref)?;
    let into = matches!(to, LayoutPlacement::Into { .. });
    let Some((&index, parent)) = source.split_last() else {
        return Err(bad_placement());
    };
    if parent.is_empty()
        || target.starts_with(&source)
        || (into && !tree.get(&target).is_some_and(is_container))
    {
        // The window's root; into / beside the node itself or its subtree;
        // into a widget.
        return Err(bad_placement());
    }
    let parent = parent.to_vec();
    let source_kind = tree.get(&parent).and_then(list_kind);
    tree.mark(&target, MARK_TARGET)?;
    tree.mark(&parent, MARK_SOURCE)?;
    let from = tree.depth_of(&source);
    let (moved, entries) = tree.take(&parent, index)?;
    let target = tree.unmark(MARK_TARGET)?;
    let carried = Carried {
        entries: &entries,
        from: source_kind,
    };
    tree.place(&moved, from, &target, to, Some(carried))?;
    let old_parent = tree.unmark(MARK_SOURCE)?;
    tree.tidy(&old_parent)
}

/// `swap-panes`: the two nodes trade places; each takes its own split size
/// and limits along when both containers are of the same [`ListKind`]
/// (ADR 0138), else the slots keep theirs.
pub(crate) fn swap_panes(
    editor: &mut LayoutEditor,
    a: &LayoutNodeRef,
    b: &LayoutNodeRef,
) -> Result<(), EditError> {
    same_breakpoint(a, b)?;
    let win = Win::open(editor, a)?;
    let mut tree = win.tree(editor);
    let pa = tree.resolve(a)?;
    let pb = tree.resolve(b)?;
    if pa == pb {
        return Ok(());
    }
    if pa.starts_with(&pb) || pb.starts_with(&pa) {
        return Err(bad_placement());
    }
    tree.swap_entries(&pa, &pb)?;
    let style = tree.style.clone();
    let (da, db) = (tree.depth_of(&pa), tree.depth_of(&pb));
    let na = extracted(tree.get(&pa).ok_or_else(bad_placement)?, &style);
    let nb = extracted(tree.get(&pb).ok_or_else(bad_placement)?, &style);
    let into_a = reindent(&nb, db, da, &style).ok_or_else(not_plain)?;
    let into_b = reindent(&na, da, db, &style).ok_or_else(not_plain)?;
    for (path, depth, node) in [(pa, da, into_a), (pb, db, into_b)] {
        let (&index, parent) = path.split_last().ok_or_else(bad_placement)?;
        let block = block_of(tree.at(parent)?);
        ensure_multiline(block, depth, &style);
        replace_with(block, index, node, false, &style);
    }
    Ok(())
}

/// `add-widget`: a widget from the palette, checked like a widget of the
/// file (unknown kind: `edit-issue-unknown-widget`; invalid options: the
/// config's own error).
pub(crate) fn add_widget(
    editor: &mut LayoutEditor,
    widget: &NewWidget,
    to: &LayoutPlacement,
) -> Result<(), EditError> {
    if widgets::find(&widget.kind).is_none() {
        return Err(EditError::Invalid(
            issue(EditIssue::UnknownWidget).arg("kind", widget.kind.as_str()),
        ));
    }
    let target_ref = placement_target(to);
    let win = Win::open(editor, target_ref)?;
    let style = editor.style.clone();
    let mut tree = win.tree(editor);
    let target = tree.resolve(target_ref)?;
    let id = match &widget.id {
        Some(id) => id.clone(),
        None => unique_id(&widget.kind, &tree.ids()),
    };
    let line = widget_line(widget, &id)?;
    check_widget_line(&line)?;
    let node = parse_one(&format!("{line}{}", style.nl)).ok_or_else(not_plain)?;
    tree.place(&node, 0, &target, to, None)
}

/// `<kind>` (`-2`, `-3`, … when taken) as a node id.
fn unique_id(kind: &str, taken: &[String]) -> String {
    let base: String = kind
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let base = if is_kebab(&base) {
        base
    } else {
        "widget".to_owned()
    };
    if !taken.contains(&base) {
        return base;
    }
    // One of `taken.len() + 1` suffixes is free.
    (2..=taken.len() + 2)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or(base)
}

/// The KDL line of a new widget (no indentation, no line break).
fn widget_line(widget: &NewWidget, id: &str) -> Result<String, EditError> {
    let mut line = widget.kind.clone();
    let panel = widget.kind == PLUGIN_PANEL_WIDGET;
    if panel && let Some(value) = widget.options.get("panel") {
        line.push(' ');
        line.push_str(&json_text(value).ok_or_else(not_plain)?);
    }
    line = with_id(line, Some(id));
    if let Some(label) = &widget.label {
        line.push_str(" label=");
        line.push_str(&quote(label));
    }
    for (key, value) in &widget.options {
        if panel && key == "panel" {
            continue;
        }
        if !is_kebab(key) {
            return Err(EditError::Invalid(
                msg("config-option-unknown-none")
                    .arg("key", key.as_str())
                    .arg("node", widget.kind.as_str()),
            ));
        }
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&json_text(value).ok_or_else(not_plain)?);
    }
    Ok(line)
}

/// Reads a widget line on its own: the config's first error, if any.
fn check_widget_line(line: &str) -> Result<(), EditError> {
    let text = format!(
        "layout \"check\" extends=\"default\" {{\n    window \"nail-edit-check\" {{\n        {line}\n    }}\n}}\n"
    );
    match crate::parse_layout_all(&text, "layout.kdl") {
        Ok(_) => Ok(()),
        Err(errors) => Err(errors.into_iter().next().map_or_else(
            || invalid(EditIssue::UnknownWidget),
            |e| EditError::Invalid((*e.localized).clone()),
        )),
    }
}

/// `remove-node`: removes a node with its subtree (never the root).
pub(crate) fn remove_node(
    editor: &mut LayoutEditor,
    node: &LayoutNodeRef,
) -> Result<(), EditError> {
    let win = Win::open(editor, node)?;
    let mut tree = win.tree(editor);
    let path = tree.resolve(node)?;
    let Some((&index, parent)) = path.split_last() else {
        return Err(bad_placement());
    };
    if parent.is_empty() {
        return Err(bad_placement());
    }
    tree.take(parent, index)?;
    tree.tidy(parent)
}

/// `set-hidden`: writes or removes `hidden=#true`.
pub(crate) fn set_hidden(
    editor: &mut LayoutEditor,
    node: &LayoutNodeRef,
    hidden: bool,
) -> Result<(), EditError> {
    let win = Win::open(editor, node)?;
    let mut tree = win.tree(editor);
    let path = tree.resolve(node)?;
    let target = tree.at(&path)?;
    if hidden {
        set_prop(target, "hidden", KdlValue::Bool(true));
    } else {
        remove_prop(target, "hidden");
    }
    Ok(())
}

/// `apply-preset`: arranges the breakpoint's panes as the preset says.
/// Panes it names are shown; panes it does not name stay, hidden, after
/// its tree in a stack at the root. The root keeps its id.
pub(crate) fn apply_preset(
    editor: &mut LayoutEditor,
    preset_id: &str,
    target: &LayoutNodeRef,
) -> Result<(), EditError> {
    let layout =
        crate::parse_layout_all(&editor.text(), "layout.kdl").map_err(EditError::Config)?;
    let preset = layout
        .presets
        .iter()
        .find(|p| p.id == preset_id && p.window == target.window)
        .ok_or_else(|| {
            EditError::Invalid(issue(EditIssue::UnknownPreset).arg("preset", preset_id))
        })?;
    let defaults = editor
        .default_window_text(&target.window)?
        .map(|text| parse_rendered(&text))
        .unwrap_or_default();
    let win = Win::open(editor, target)?;
    let tree = win.tree(editor);
    let style = tree.style.clone();
    let root_depth = tree.depth_of(&[0]);

    // The panes the preset names, and those it leaves out.
    let mut named: Vec<(String, Vec<usize>)> = Vec::new();
    for id in preset.root.pane_ids() {
        let path = tree.find_id(id).ok_or_else(|| {
            EditError::Invalid(issue(EditIssue::PresetPaneMissing).arg("pane", id))
        })?;
        named.push((id.to_owned(), path));
    }
    let paths: Vec<&[usize]> = named.iter().map(|(_, p)| p.as_slice()).collect();
    if paths
        .iter()
        .any(|a| paths.iter().any(|b| a != b && b.starts_with(a)))
    {
        // A named pane inside another one.
        return Err(bad_placement());
    }
    let mut left_out = Vec::new();
    collect_left_out(&tree, vec![0], &paths, &mut left_out);

    let copy = |path: &[usize], hidden: bool| -> Result<(KdlNode, usize), EditError> {
        let mut node = extracted(tree.get(path).ok_or_else(bad_placement)?, &style);
        if hidden {
            set_prop(&mut node, "hidden", KdlValue::Bool(true));
        } else {
            remove_prop(&mut node, "hidden");
        }
        Ok((node, tree.depth_of(path)))
    };
    let mut panes: HashMap<String, (KdlNode, usize)> = HashMap::new();
    let mut limits = Limits::new();
    for (id, path) in &named {
        panes.insert(id.clone(), copy(path, false)?);
        if let Some(found) = tree.limits_of(path) {
            limits.entry(id.clone()).or_default().push(found);
        }
    }
    for (id, found) in default_limits(&defaults, &named) {
        limits.entry(id).or_default().push(found);
    }
    let mut hidden = Vec::new();
    for path in &left_out {
        hidden.push(copy(path, true)?);
    }
    let root_id = tree
        .get(&[0])
        .filter(|root| is_container(root) && !paths.contains(&&[0][..]))
        .and_then(|root| str_prop(root, "id"))
        .map(str::to_owned);

    let root = if hidden.is_empty() {
        build_preset(
            &preset.root,
            root_depth,
            root_id.as_deref(),
            &mut panes,
            &limits,
            &style,
        )?
    } else {
        let arranged = build_preset(
            &preset.root,
            root_depth + 1,
            None,
            &mut panes,
            &limits,
            &style,
        )?;
        let mut children = vec![arranged];
        let mut sizes = vec!["1fr"];
        for (node, depth) in hidden {
            children.push(reindent(&node, depth, root_depth + 1, &style).ok_or_else(not_plain)?);
            sizes.push("auto");
        }
        let header = with_sizes(with_id("stack".to_owned(), root_id.as_deref()), &sizes);
        container(&header, root_depth, children, &style)?
    };
    let window_block = block_of(&mut *tree.window);
    ensure_multiline(window_block, root_depth, &style);
    replace_with(window_block, 0, root, true, &style);
    Ok(())
}

/// The panes a preset does not name: walking down from the root through
/// the containers that hold a named pane, every other node.
fn collect_left_out(
    tree: &Tree<'_>,
    path: Vec<usize>,
    named: &[&[usize]],
    out: &mut Vec<Vec<usize>>,
) {
    if named.contains(&path.as_slice()) {
        return;
    }
    let Some(node) = tree.get(&path) else {
        return;
    };
    let holds_named = named.iter().any(|p| p.starts_with(&path));
    if !holds_named || !is_container(node) {
        out.push(path);
        return;
    }
    for i in 0..child_count(node) {
        let mut child = path.clone();
        child.push(i);
        collect_left_out(tree, child, named, out);
    }
}

/// Per pane id the limits it had (current tree first, then the defaults).
type Limits = HashMap<String, Vec<(ListKind, Entries)>>;

/// The limit lists a pane carries into a preset's split (sizes come from
/// the preset).
const LIMIT_LISTS: &[(&str, &str)] = &[
    ("min-sizes", "none"),
    ("max-sizes", "none"),
    ("collapsible", "false"),
];

/// The limit entries of child `index` of a container, with its kind.
fn limits_in(container: &KdlNode, index: usize) -> Option<(ListKind, Entries)> {
    let kind = list_kind(container)?;
    let entries: Entries = LIMIT_LISTS
        .iter()
        .filter_map(|&(key, _)| {
            let items = full_list(container, key)?;
            Some((key.to_owned(), items.get(index)?.clone()))
        })
        .collect();
    Some((kind, entries))
}

/// The limits the default layout gives the named panes (every breakpoint
/// of the window; the first found per kind wins).
fn default_limits(
    defaults: &[KdlNode],
    named: &[(String, Vec<usize>)],
) -> Vec<(String, (ListKind, Entries))> {
    fn walk(node: &KdlNode, wanted: &[&str], out: &mut Vec<(String, (ListKind, Entries))>) {
        if !is_container(node) && node.name().value() != "window" {
            return;
        }
        for (index, child) in children(node).iter().enumerate() {
            if let Some(id) = str_prop(child, "id")
                && wanted.contains(&id)
                && let Some(found) = limits_in(node, index)
            {
                out.push((id.to_owned(), found));
            }
            walk(child, wanted, out);
        }
    }
    let wanted: Vec<&str> = named.iter().map(|(id, _)| id.as_str()).collect();
    let mut out = Vec::new();
    for top in defaults {
        match top.name().value() {
            "window" => walk(top, &wanted, &mut out),
            "breakpoint" => {
                for window in children(top) {
                    walk(window, &wanted, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// A preset's arrangement as a node at `depth`, its panes taken out of
/// `panes` (the config checks that each is named once).
fn build_preset(
    preset: &PresetNode,
    depth: usize,
    id: Option<&str>,
    panes: &mut HashMap<String, (KdlNode, usize)>,
    limits: &Limits,
    style: &Style,
) -> Result<KdlNode, EditError> {
    let (header, children) = match preset {
        PresetNode::Pane { id: pane } => {
            let (node, from) = panes.remove(pane).ok_or_else(bad_placement)?;
            return reindent(&node, from, depth, style).ok_or_else(not_plain);
        }
        PresetNode::Split {
            orientation,
            sizes,
            children,
        } => {
            let split = format!("split {}", quote(Orientation::as_str(*orientation)));
            let mut header = with_sizes(with_id(split, id), sizes);
            let kind = ListKind::Split(*orientation);
            for &(key, default) in LIMIT_LISTS {
                let values: Vec<Option<String>> = children
                    .iter()
                    .map(|child| {
                        let PresetNode::Pane { id: pane } = child else {
                            return None;
                        };
                        limits
                            .get(pane)?
                            .iter()
                            .filter(|(k, _)| *k == kind)
                            .find_map(|(_, entries)| {
                                entries
                                    .iter()
                                    .find(|(k, _)| k == key)
                                    .map(|(_, v)| v.clone())
                            })
                    })
                    .collect();
                if values.iter().any(Option::is_some) {
                    let values: Vec<String> = values
                        .into_iter()
                        .map(|v| v.unwrap_or_else(|| default.to_owned()))
                        .collect();
                    header = with_list(header, key, &values);
                }
            }
            (header, children)
        }
        PresetNode::Stack { sizes, children } => {
            (with_sizes(with_id("stack".to_owned(), id), sizes), children)
        }
        PresetNode::Tabs { children } => (with_id("tabs".to_owned(), id), children),
    };
    let mut built = Vec::with_capacity(children.len());
    for child in children {
        built.push(build_preset(child, depth + 1, None, panes, limits, style)?);
    }
    container(&header, depth, built, style)
}

/// `reset-window`: the file's definitions of the window go; a full file
/// gets the default definitions back in their place.
pub(crate) fn reset_window(editor: &mut LayoutEditor, window: &str) -> Result<(), EditError> {
    let at = LayoutEditor::window_sites(editor.body(), window)
        .first()
        .map(|s| s.top);
    editor.remove_window(window);
    if editor.sparse {
        return Ok(());
    }
    let Some(text) = editor.default_window_text(window)? else {
        return Ok(());
    };
    let style = editor.style.clone();
    let body = editor.body();
    let first = at.unwrap_or(body.nodes().len());
    for (index, node) in (first..).zip(parse_rendered(&text)) {
        put(body, index, node, 1, &style);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Toolbar patches

/// Where a `toolbar` node is (`(block, index)` as in
/// [`LayoutEditor::named_sites`]) and its depth.
#[derive(Debug, Clone, Copy)]
struct BarSite {
    block: Option<usize>,
    index: usize,
    depth: usize,
}

/// The toolbar's (last) definition in the file. A toolbar only the
/// defaults define is copied into a sparse file; `create` makes an empty
/// one when there is none at all.
fn toolbar_site(editor: &mut LayoutEditor, name: &str, create: bool) -> Result<BarSite, EditError> {
    if !is_kebab(name) {
        return Err(bad_slot(name));
    }
    if LayoutEditor::named_sites(editor.body(), "toolbar", name).is_empty() {
        let defaults = default_layout();
        let style = editor.style.clone();
        let text = match defaults.toolbars.get(name) {
            Some(items) if editor.sparse => {
                let settings = defaults
                    .toolbar_settings
                    .get(name)
                    .copied()
                    .unwrap_or_default();
                render::toolbar(name, items, settings, 1, &style)
            }
            _ if create => {
                render::toolbar(name, &[], nail_api::ToolbarSettings::default(), 1, &style)
            }
            _ => None,
        }
        .ok_or_else(|| bad_slot(name))?;
        editor.append(&text);
    }
    let sites = LayoutEditor::named_sites(editor.body(), "toolbar", name);
    let &(block, index) = sites.last().ok_or_else(|| bad_slot(name))?;
    Ok(BarSite {
        block,
        index,
        depth: if block.is_some() { 2 } else { 1 },
    })
}

fn toolbar_node(editor: &mut LayoutEditor, site: BarSite) -> &mut KdlNode {
    let body = editor.body();
    match site.block {
        None => &mut body.nodes_mut()[site.index],
        Some(top) => &mut body.nodes_mut()[top].ensure_children().nodes_mut()[site.index],
    }
}

fn is_more(node: &KdlNode) -> bool {
    node.name().value() == "more"
}

/// The child indexes of the bar's items (every child but `more`).
fn bar_items(toolbar: &KdlNode) -> Vec<usize> {
    children(toolbar)
        .iter()
        .enumerate()
        .filter(|(_, n)| !is_more(n))
        .map(|(i, _)| i)
        .collect()
}

fn more_index(toolbar: &KdlNode) -> Option<usize> {
    children(toolbar).iter().position(is_more)
}

/// Whether an item node can go into an area: no spacer in "More", no menu
/// entries (submenus, headers) on the bar.
fn fits(node: &KdlNode, area: ToolbarArea) -> bool {
    match area {
        ToolbarArea::Bar => !matches!(node.name().value(), "submenu" | "header" | "more"),
        ToolbarArea::More => !matches!(node.name().value(), "spacer" | "more"),
    }
}

/// The item at a slot.
fn item_at(toolbar: &KdlNode, slot: ToolbarSlot) -> Option<&KdlNode> {
    let index = to_index(slot.index);
    match slot.area {
        ToolbarArea::Bar => children(toolbar).get(*bar_items(toolbar).get(index)?),
        ToolbarArea::More => children(&children(toolbar)[more_index(toolbar)?]).get(index),
    }
}

/// Takes the item at `slot` off the toolbar (detached, with its depth).
fn take_item(
    toolbar: &mut KdlNode,
    depth: usize,
    slot: ToolbarSlot,
    style: &Style,
    name: &str,
) -> Result<(KdlNode, usize), EditError> {
    if item_at(toolbar, slot).is_none() {
        return Err(bad_slot(name));
    }
    let index = to_index(slot.index);
    match slot.area {
        ToolbarArea::Bar => {
            let child = bar_items(toolbar)[index];
            Ok((
                detach(block_of(toolbar), child, depth + 1, style),
                depth + 1,
            ))
        }
        ToolbarArea::More => {
            let more = more_index(toolbar).ok_or_else(|| bad_slot(name))?;
            let more_node = &mut block_of(toolbar).nodes_mut()[more];
            Ok((
                detach(block_of(more_node), index, depth + 2, style),
                depth + 2,
            ))
        }
    }
}

/// Puts an item (detached, indented for `from`) at `slot`; a "More" menu is
/// added at the end of the bar when the toolbar has none. An item going
/// into "More" loses the bar-only `rank` / `display`.
fn put_item(
    toolbar: &mut KdlNode,
    depth: usize,
    slot: ToolbarSlot,
    mut item: KdlNode,
    from: usize,
    style: &Style,
) -> Result<(), EditError> {
    let index = to_index(slot.index);
    match slot.area {
        ToolbarArea::Bar => {
            let items = bar_items(toolbar);
            let at = match items.get(index) {
                Some(&child) => child,
                None => items.last().map_or(0, |&last| last + 1),
            };
            let item = reindent(&item, from, depth + 1, style).ok_or_else(not_plain)?;
            put(block_of(toolbar), at, item, depth + 1, style);
        }
        ToolbarArea::More => {
            remove_prop(&mut item, "rank");
            remove_prop(&mut item, "display");
            let more = if let Some(more) = more_index(toolbar) {
                more
            } else {
                let node = parse_one(&format!("{}more{}", style.pad(depth + 1), style.nl))
                    .ok_or_else(not_plain)?;
                let block = block_of(toolbar);
                let end = block.nodes().len();
                put(block, end, node, depth + 1, style);
                end
            };
            let item = reindent(&item, from, depth + 2, style).ok_or_else(not_plain)?;
            let menu = block_of(&mut block_of(toolbar).nodes_mut()[more]);
            let at = index.min(menu.nodes().len());
            put(menu, at, item, depth + 2, style);
        }
    }
    Ok(())
}

/// `move-toolbar-item`: within the bar, within "More" or between them.
pub(crate) fn move_toolbar_item(
    editor: &mut LayoutEditor,
    name: &str,
    from: ToolbarSlot,
    to: ToolbarSlot,
) -> Result<(), EditError> {
    let site = toolbar_site(editor, name, false)?;
    let style = editor.style.clone();
    let toolbar = toolbar_node(editor, site);
    if !item_at(toolbar, from).is_some_and(|item| fits(item, to.area)) {
        return Err(bad_slot(name));
    }
    let (item, item_depth) = take_item(toolbar, site.depth, from, &style, name)?;
    put_item(toolbar, site.depth, to, item, item_depth, &style)
}

/// `add-toolbar-item`: an action, separator or spacer from the palette.
pub(crate) fn add_toolbar_item(
    editor: &mut LayoutEditor,
    name: &str,
    item: &ToolbarItem,
    to: ToolbarSlot,
) -> Result<(), EditError> {
    if matches!(item, ToolbarItem::More { .. })
        || (to.area == ToolbarArea::More && matches!(item, ToolbarItem::Spacer))
    {
        return Err(bad_slot(name));
    }
    let style = editor.style.clone();
    let text = render::toolbar_item(item, 0, &style).ok_or_else(not_plain)?;
    let node = parse_one(&text).ok_or_else(not_plain)?;
    let site = toolbar_site(editor, name, true)?;
    let toolbar = toolbar_node(editor, site);
    put_item(toolbar, site.depth, to, node, 0, &style)
}

/// `remove-toolbar-item`: takes an item off the bar or "More".
pub(crate) fn remove_toolbar_item(
    editor: &mut LayoutEditor,
    name: &str,
    from: ToolbarSlot,
) -> Result<(), EditError> {
    let site = toolbar_site(editor, name, false)?;
    let style = editor.style.clone();
    let toolbar = toolbar_node(editor, site);
    take_item(toolbar, site.depth, from, &style, name).map(drop)
}
