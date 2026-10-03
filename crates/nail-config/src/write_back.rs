//! Write-back of UI edits into `layout.kdl` ([`LayoutPatch`]es): divider
//! positions, column order, toolbar contents and key bindings.
//!
//! The user's file is edited in place with [`crate::kdl_edit`], so comments,
//! blank lines, indentation and property order of everything the patch does
//! not touch stay as they were. A missing file becomes a sparse file that
//! extends the defaults; a window that only exists in the defaults is copied
//! into the file before it is changed (a window defined in a sparse file
//! replaces all breakpoints of that window in the defaults). Removing a
//! binding that the defaults define writes an `unbind`.

use kdl::{KdlDocument, KdlNode, KdlValue};
use nail_api::{
    ApiError, ErrorCode, LayoutNodeRef, LayoutPatch, LocalizedMessage, ToolbarItem,
    ToolbarOverflow, ToolbarSettings,
};
use serde_json::Value;

use crate::error::ConfigError;
use crate::kdl_edit::{
    Style, append_rendered, ensure_multiline, first_arg, parse_rendered, remove_node,
    replace_rendered, same_node, set_prop, str_prop, to_text,
};
use crate::kdl_util::is_kebab;
use crate::l10n::msg;
use crate::layout::normalize_key;
use crate::render;
use crate::{DEFAULT_LAYOUT_KDL, LayoutFile, default_layout, parse_layout_all};

/// The file written when there is no `layout.kdl` yet.
pub(crate) const NEW_LAYOUT: &str = "// Changes made in Nail. See the layout reference in the user documentation.\nlayout \"custom\" extends=\"default\" {\n}\n";

/// Why a write-back or merge could not be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The file on disk (or the file the change would produce) is invalid.
    Config(Vec<ConfigError>),
    /// The request does not fit the file (unknown window, wrong number of
    /// sizes, …).
    Invalid(LocalizedMessage),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(errors) => {
                let shown: Vec<String> = errors.iter().take(5).map(ToString::to_string).collect();
                write!(f, "{}", shown.join("; "))?;
                if errors.len() > 5 {
                    write!(f, " (and {} more)", errors.len() - 5)?;
                }
                Ok(())
            }
            Self::Invalid(message) => f.write_str(&crate::l10n::english(message)),
        }
    }
}

impl std::error::Error for EditError {}

impl From<EditError> for ApiError {
    fn from(error: EditError) -> Self {
        match &error {
            EditError::Config(errors) => {
                let api = ApiError::new(ErrorCode::Config, error.to_string());
                match errors.first() {
                    Some(first) => api.with_localized((*first.localized).clone()),
                    None => api,
                }
            }
            EditError::Invalid(message) => {
                ApiError::invalid_request(error.to_string()).with_localized(message.clone())
            }
        }
    }
}

fn invalid(message: LocalizedMessage) -> EditError {
    EditError::Invalid(message)
}

/// Applies `patches` (in order, as one edit) to the text of `layout.kdl`
/// (`None` = no file yet) and returns the new text. Fails with
/// [`EditError::Config`] when the current file is invalid, and with
/// [`EditError::Invalid`] when a patch does not fit the file or the result
/// would be invalid (e.g. an unknown action).
pub fn apply_layout_patches(
    current: Option<&str>,
    patches: &[LayoutPatch],
    file: &str,
) -> Result<String, EditError> {
    let text = current.unwrap_or(NEW_LAYOUT);
    let parsed = LayoutFile::parse_all(text, file).map_err(EditError::Config)?;
    parse_layout_all(text, file).map_err(EditError::Config)?;
    let mut editor = LayoutEditor::new(text, parsed.extends.is_some())?;
    for patch in patches {
        editor.apply(patch)?;
    }
    let out = editor.finish();
    if let Err(errors) = parse_layout_all(&out, file) {
        return Err(invalid(
            msg("config-edit-makes-invalid")
                .arg("file", file)
                .arg("error", errors[0].to_string()),
        ));
    }
    Ok(out)
}

/// A `layout.kdl` document being edited.
pub(crate) struct LayoutEditor {
    doc: KdlDocument,
    root: usize,
    sparse: bool,
    pub(crate) style: Style,
}

/// Where a `window "<name>"` node is: `body[top]`, or the `inner`-th child
/// of the breakpoint `body[top]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Site {
    pub(crate) top: usize,
    pub(crate) inner: Option<usize>,
}

impl LayoutEditor {
    /// Starts editing valid layout text.
    pub(crate) fn new(text: &str, sparse: bool) -> Result<Self, EditError> {
        Self::open(text, "layout", sparse)
    }

    /// Starts editing valid text whose single top-level node is `root`
    /// (`layout` or `theme`).
    pub(crate) fn open(text: &str, root: &str, sparse: bool) -> Result<Self, EditError> {
        let doc = KdlDocument::parse_v2(text)
            .map_err(|e| invalid(msg("config-syntax").arg("detail", e.to_string())))?;
        let root = doc
            .nodes()
            .iter()
            .position(|n| n.name().value() == root)
            .ok_or_else(|| invalid(msg("config-edit-no-root").arg("root", root)))?;
        Ok(Self {
            doc,
            root,
            sparse,
            style: Style::of(text),
        })
    }

    /// The edited text.
    pub(crate) fn finish(self) -> String {
        to_text(&self.doc, &self.style)
    }

    pub(crate) fn root_node(&mut self) -> &mut KdlNode {
        &mut self.doc.nodes_mut()[self.root]
    }

    /// The children of the `layout` node (depth 1).
    pub(crate) fn body(&mut self) -> &mut KdlDocument {
        self.doc.nodes_mut()[self.root].ensure_children()
    }

    fn apply(&mut self, patch: &LayoutPatch) -> Result<(), EditError> {
        match patch {
            LayoutPatch::SplitSizes { node, sizes } => {
                for size in sizes {
                    if size.is_empty() || size.contains(char::is_whitespace) {
                        return Err(invalid(
                            msg("config-edit-size-invalid").arg("size", format!("{size:?}")),
                        ));
                    }
                }
                let target = self.node_mut(node)?;
                let name = target.name().value().to_owned();
                if name != "split" && name != "stack" {
                    return Err(invalid(
                        msg("config-edit-not-split")
                            .arg("node", format!("{node:?}"))
                            .arg("name", name),
                    ));
                }
                let children = target.children().map_or(0, |c| c.nodes().len());
                if children != sizes.len() {
                    return Err(invalid(
                        msg("config-edit-sizes-count")
                            .arg("count", sizes.len())
                            .arg("name", name)
                            .arg("children", children),
                    ));
                }
                set_prop(target, "sizes", KdlValue::String(sizes.join(" ")));
                Ok(())
            }
            LayoutPatch::ColumnOrder { node, columns } => {
                if columns.is_empty() || columns.iter().any(|c| !is_kebab(c)) {
                    return Err(invalid(
                        msg("config-edit-columns-invalid").arg("columns", format!("{columns:?}")),
                    ));
                }
                let target = self.node_mut(node)?;
                let name = target.name().value();
                if matches!(name, "split" | "stack" | "tabs") {
                    return Err(invalid(
                        msg("config-edit-not-list")
                            .arg("node", format!("{node:?}"))
                            .arg("name", name),
                    ));
                }
                set_prop(target, "columns", KdlValue::String(columns.join(" ")));
                Ok(())
            }
            LayoutPatch::ToolbarOrder { toolbar, items } => self.set_toolbar(toolbar, items),
            LayoutPatch::AddKeyBinding {
                context,
                key,
                action,
                args,
            } => self.set_binding(context, key, Some((action, args))),
            LayoutPatch::RemoveKeyBinding { context, key } => self.set_binding(context, key, None),
            LayoutPatch::ResetToolbar { toolbar } => self.reset_toolbar(toolbar),
            LayoutPatch::ResetKeyBinding { context, key } => self.reset_binding(context, key),
        }
    }

    // --- windows -----------------------------------------------------------

    pub(crate) fn window_sites(body: &KdlDocument, name: &str) -> Vec<Site> {
        let mut sites = Vec::new();
        for (top, node) in body.nodes().iter().enumerate() {
            match node.name().value() {
                "window" if first_arg(node) == Some(name) => sites.push(Site { top, inner: None }),
                "breakpoint" => {
                    for (inner, child) in node
                        .children()
                        .map_or(&[][..], KdlDocument::nodes)
                        .iter()
                        .enumerate()
                    {
                        if child.name().value() == "window" && first_arg(child) == Some(name) {
                            sites.push(Site {
                                top,
                                inner: Some(inner),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        sites
    }

    /// Removes every definition of a window (a breakpoint left empty goes
    /// too).
    pub(crate) fn remove_window(&mut self, name: &str) {
        let style = self.style.clone();
        let sites = Self::window_sites(self.body(), name);
        for site in sites.into_iter().rev() {
            let body = self.body();
            match site.inner {
                None => remove_node(body, site.top, &style),
                Some(inner) => {
                    let bp = &mut body.nodes_mut()[site.top];
                    let block = bp.ensure_children();
                    remove_node(block, inner, &style);
                    if block.nodes().is_empty() {
                        remove_node(body, site.top, &style);
                    }
                }
            }
        }
    }

    /// Copies the default definitions of a window into a sparse file.
    pub(crate) fn copy_default_window(&mut self, name: &str) -> Result<(), EditError> {
        if !self.sparse {
            return Err(invalid(msg("config-edit-no-window").arg("name", name)));
        }
        let defaults = KdlDocument::parse_v2(DEFAULT_LAYOUT_KDL).map_err(|e| {
            invalid(msg("config-edit-default-invalid").arg("detail", e.to_string()))
        })?;
        let default_body = defaults.nodes()[0]
            .children()
            .ok_or_else(|| invalid(msg("config-edit-default-empty")))?;
        let sites = Self::window_sites(default_body, name);
        if sites.is_empty() {
            return Err(invalid(msg("config-edit-no-window").arg("name", name)));
        }
        let pad = self.style.pad(1);
        let mut text = String::new();
        for site in sites {
            let mut node = default_body.nodes()[site.top].clone();
            if let Some(inner) = site.inner {
                // Keep only this window inside the breakpoint.
                let block = node.ensure_children();
                let keep = block.nodes()[inner].clone();
                let count = block.nodes().len();
                for i in (0..count).rev() {
                    if i != inner {
                        remove_node(block, i, &Style::default());
                    }
                }
                debug_assert!(same_node(&block.nodes()[0], &keep));
            }
            if let Some(fmt) = node.format_mut() {
                fmt.leading.clone_from(&pad);
                fmt.trailing.clear();
                if !fmt.terminator.ends_with('\n') {
                    fmt.terminator = "\n".to_owned();
                }
            }
            text.push_str(&node.to_string());
        }
        if self.style.nl == "\r\n" {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        let style = self.style.clone();
        append_rendered(self.body(), 1, &text, &style);
        Ok(())
    }

    /// The node a [`LayoutNodeRef`] addresses, copying the window from the
    /// defaults first when the file does not define it.
    fn node_mut(&mut self, node: &LayoutNodeRef) -> Result<&mut KdlNode, EditError> {
        if Self::window_sites(self.body(), &node.window).is_empty() {
            self.copy_default_window(&node.window)?;
        }
        let sites = Self::window_sites(self.body(), &node.window);
        let site = *sites.get(node.breakpoint as usize).ok_or_else(|| {
            invalid(
                msg("config-edit-no-breakpoint")
                    .arg("name", node.window.as_str())
                    .arg("breakpoint", node.breakpoint),
            )
        })?;
        let body = self.body();
        let window = match site.inner {
            None => &mut body.nodes_mut()[site.top],
            Some(inner) => &mut body.nodes_mut()[site.top].ensure_children().nodes_mut()[inner],
        };
        let mut current = window
            .children_mut()
            .as_mut()
            .and_then(|c| c.nodes_mut().first_mut())
            .ok_or_else(|| {
                invalid(msg("config-edit-window-empty").arg("name", node.window.as_str()))
            })?;
        for &index in &node.path {
            current = current
                .children_mut()
                .as_mut()
                .and_then(|c| c.nodes_mut().get_mut(index as usize))
                .ok_or_else(|| {
                    invalid(msg("config-edit-no-node").arg("path", format!("{:?}", node.path)))
                })?;
        }
        Ok(current)
    }

    // --- toolbars and menus ------------------------------------------------

    /// `(block path, index)` of every top-level `<kind> "<name>"` node and
    /// those inside `<kind>s { … }` blocks: `None` = the body itself.
    pub(crate) fn named_sites(
        body: &KdlDocument,
        kind: &str,
        name: &str,
    ) -> Vec<(Option<usize>, usize)> {
        let plural = format!("{kind}s");
        let mut sites = Vec::new();
        for (top, node) in body.nodes().iter().enumerate() {
            let node_name = node.name().value();
            if node_name == kind && first_arg(node) == Some(name) {
                sites.push((None, top));
            } else if node_name == plural {
                for (inner, child) in node
                    .children()
                    .map_or(&[][..], KdlDocument::nodes)
                    .iter()
                    .enumerate()
                {
                    if child.name().value() == kind && first_arg(child) == Some(name) {
                        sites.push((Some(top), inner));
                    }
                }
            }
        }
        sites
    }

    pub(crate) fn set_toolbar(
        &mut self,
        name: &str,
        items: &[ToolbarItem],
    ) -> Result<(), EditError> {
        if !is_kebab(name) {
            return Err(invalid(
                msg("config-toolbar-name-invalid").arg("name", name),
            ));
        }
        let style = self.style.clone();
        let sites = Self::named_sites(self.body(), "toolbar", name);
        let Some(&(block, index)) = sites.last() else {
            let text = render::toolbar(name, items, &ToolbarSettings::default(), 1, &style)
                .ok_or_else(|| invalid(msg("config-edit-plain-values")))?;
            append_rendered(self.body(), 1, &text, &style);
            return Ok(());
        };
        let depth = if block.is_some() { 2 } else { 1 };
        let body = self.body();
        let (parent, index) = match block {
            None => (body, index),
            Some(top) => (body.nodes_mut()[top].ensure_children(), index),
        };
        let node = &mut parent.nodes_mut()[index];
        let multiline = node
            .children()
            .and_then(|c| c.nodes().first())
            .is_some_and(|first| first.format().is_some_and(|f| f.leading.contains('\n')));
        if !multiline {
            // The node is rendered anew: keep its M5b properties.
            let settings = toolbar_settings_of(node);
            let text = render::toolbar(name, items, &settings, depth, &style)
                .ok_or_else(|| invalid(msg("config-edit-plain-values")))?;
            replace_rendered(parent, index, &text);
            return Ok(());
        }
        // Reuse the existing item nodes (with their comments) where the item
        // stays; render new ones.
        let block = node.ensure_children();
        let mut old: Vec<Option<KdlNode>> = block.nodes_mut().drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(items.len());
        for item in items {
            let text = render::toolbar_item(item, depth + 1, &style)
                .ok_or_else(|| invalid(msg("config-edit-plain-values")))?;
            let fresh = parse_rendered(&text).remove(0);
            let reused = old
                .iter_mut()
                .enumerate()
                .find(|(_, slot)| slot.as_ref().is_some_and(|n| same_node(n, &fresh)))
                .and_then(|(i, slot)| {
                    slot.take().map(|mut n| {
                        // The first child's leading starts with the line break
                        // after `{`; elsewhere a leading break is a blank line.
                        if i == 0
                            && let Some(fmt) = n.format_mut()
                        {
                            let cut = if fmt.leading.starts_with("\r\n") {
                                2
                            } else {
                                1
                            };
                            if fmt.leading.starts_with(['\r', '\n']) {
                                fmt.leading.drain(..cut);
                            }
                        }
                        n
                    })
                });
            nodes.push(reused.unwrap_or(fresh));
        }
        for (i, node) in nodes.iter_mut().enumerate() {
            let fmt = node.format().cloned().unwrap_or_default();
            let mut fmt = fmt;
            if i == 0 {
                fmt.leading.insert_str(0, style.nl);
            }
            if !fmt.terminator.contains('\n') && !fmt.trailing.contains('\n') {
                fmt.terminator = style.nl.to_owned();
            }
            node.set_format(fmt);
        }
        *block.nodes_mut() = nodes;
        if block.nodes().is_empty() {
            let fmt = block.format().cloned().unwrap_or_default();
            let mut fmt = fmt;
            fmt.leading = style.nl.to_owned();
            fmt.trailing = style.pad(depth);
            block.set_format(fmt);
        }
        Ok(())
    }

    /// Removes the file's definitions of a toolbar; a full (non-sparse)
    /// file gets the default items written back.
    fn reset_toolbar(&mut self, name: &str) -> Result<(), EditError> {
        if !is_kebab(name) {
            return Err(invalid(
                msg("config-toolbar-name-invalid").arg("name", name),
            ));
        }
        let style = self.style.clone();
        let sites = Self::named_sites(self.body(), "toolbar", name);
        let mut emptied = Vec::new();
        for &(block, index) in sites.iter().rev() {
            let body = self.body();
            match block {
                None => remove_node(body, index, &style),
                Some(top) => {
                    let inner = body.nodes_mut()[top].ensure_children();
                    remove_node(inner, index, &style);
                    if inner.nodes().is_empty() && !emptied.contains(&top) {
                        emptied.push(top);
                    }
                }
            }
        }
        emptied.sort_unstable_by(|a, b| b.cmp(a));
        for top in emptied {
            remove_node(self.body(), top, &style);
        }
        if !self.sparse
            && let Some(items) = default_layout().toolbars.get(name)
        {
            self.set_toolbar(name, items)?;
        }
        Ok(())
    }

    /// Removes the file's `bind` / `unbind` lines for a key; a full
    /// (non-sparse) file gets the default binding written back.
    fn reset_binding(&mut self, context: &str, key: &str) -> Result<(), EditError> {
        if context != "global" && !is_kebab(context) {
            return Err(invalid(
                msg("config-key-context-invalid").arg("context", context),
            ));
        }
        let key = normalize_key(key).map_err(invalid)?;
        let style = self.style.clone();
        let blocks = Self::keys_blocks(self.body(), context);
        let mut emptied = Vec::new();
        for &b in blocks.iter().rev() {
            let block = self.body().nodes_mut()[b].ensure_children();
            let matching: Vec<usize> = block
                .nodes()
                .iter()
                .enumerate()
                .filter(|(_, child)| {
                    matches!(child.name().value(), "bind" | "unbind")
                        && first_arg(child).and_then(|k| normalize_key(k).ok()) == Some(key.clone())
                })
                .map(|(i, _)| i)
                .collect();
            if matching.is_empty() {
                continue;
            }
            for &i in matching.iter().rev() {
                remove_node(block, i, &style);
            }
            if block.nodes().is_empty() {
                emptied.push(b);
            }
        }
        for b in emptied {
            remove_node(self.body(), b, &style);
        }
        if !self.sparse
            && let Some(binding) = default_layout()
                .keys
                .iter()
                .rev()
                .find(|k| k.context == context && k.key == key)
        {
            self.set_binding(context, &key, Some((&binding.action, &binding.args)))?;
        }
        Ok(())
    }

    // --- key bindings ------------------------------------------------------

    fn keys_blocks(body: &KdlDocument, context: &str) -> Vec<usize> {
        body.nodes()
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.name().value() == "keys" && str_prop(n, "context").unwrap_or("global") == context
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Sets (`Some`) or removes (`None`) the binding of `key` in `context`.
    pub(crate) fn set_binding(
        &mut self,
        context: &str,
        key: &str,
        action: Option<(&String, &Vec<Value>)>,
    ) -> Result<(), EditError> {
        if context != "global" && !is_kebab(context) {
            return Err(invalid(
                msg("config-key-context-invalid").arg("context", context),
            ));
        }
        let key = normalize_key(key).map_err(invalid)?;
        let style = self.style.clone();
        let blocks = Self::keys_blocks(self.body(), context);
        let mut matches: Vec<(usize, usize)> = Vec::new();
        for &b in &blocks {
            let block = &self.body().nodes()[b];
            for (i, child) in block
                .children()
                .map_or(&[][..], KdlDocument::nodes)
                .iter()
                .enumerate()
            {
                let is_binding = matches!(child.name().value(), "bind" | "unbind");
                if is_binding
                    && first_arg(child).and_then(|k| normalize_key(k).ok()) == Some(key.clone())
                {
                    matches.push((b, i));
                }
            }
        }
        match action {
            Some((action, args)) => {
                let text = render::bind(&key, action, args, 2, &style)
                    .ok_or_else(|| invalid(msg("config-edit-plain-values")))?;
                if let Some(&(b, i)) = matches.last() {
                    let block = self.body().nodes_mut()[b].ensure_children();
                    replace_rendered(block, i, &text);
                } else {
                    self.append_binding(&blocks, context, &text);
                }
            }
            None => {
                let defaults_bind = self.sparse
                    && default_layout()
                        .keys
                        .iter()
                        .any(|k| k.context == context && k.key == key);
                let mut emptied = Vec::new();
                for &(b, i) in matches.iter().rev() {
                    let block = self.body().nodes_mut()[b].ensure_children();
                    remove_node(block, i, &style);
                    if block.nodes().is_empty() && !emptied.contains(&b) {
                        emptied.push(b);
                    }
                }
                if defaults_bind {
                    let text = render::unbind(&key, 2, &style);
                    self.append_binding(&blocks, context, &text);
                } else {
                    // Blocks this removal emptied go too (descending order).
                    emptied.sort_unstable_by(|a, b| b.cmp(a));
                    for b in emptied {
                        remove_node(self.body(), b, &style);
                    }
                }
            }
        }
        Ok(())
    }

    fn append_binding(&mut self, blocks: &[usize], context: &str, line: &str) {
        let style = self.style.clone();
        if let Some(&b) = blocks.last() {
            let block = self.body().nodes_mut()[b].ensure_children();
            append_rendered(block, 2, line, &style);
        } else {
            let text = render::keys_block(context, line, 1, &style);
            append_rendered(self.body(), 1, &text, &style);
        }
    }

    /// Appends rendered top-level nodes (depth 1).
    pub(crate) fn append(&mut self, text: &str) {
        let style = self.style.clone();
        ensure_multiline(self.body(), 1, &style);
        append_rendered(self.body(), 1, text, &style);
    }

    /// Appends a rendered line (depth 2) into the last `<section>` block,
    /// creating the block when there is none.
    pub(crate) fn append_in_section(&mut self, section: &str, line: &str) {
        let style = self.style.clone();
        let last = self
            .body()
            .nodes()
            .iter()
            .rposition(|n| n.name().value() == section && n.entries().is_empty());
        match last {
            Some(b) => {
                let block = self.body().nodes_mut()[b].ensure_children();
                append_rendered(block, 2, line, &style);
            }
            None => {
                let text = render::section(section, line, 1, &style);
                append_rendered(self.body(), 1, &text, &style);
            }
        }
    }
}

/// The `hide-disabled=` / `overflow=` properties of a `toolbar` node as
/// written (anything unreadable counts as the default; the file was valid
/// when it was read).
fn toolbar_settings_of(node: &KdlNode) -> ToolbarSettings {
    ToolbarSettings {
        hide_disabled: node
            .entry("hide-disabled")
            .and_then(|e| e.value().as_bool())
            .unwrap_or(false),
        overflow: node
            .entry("overflow")
            .and_then(|e| e.value().as_string())
            .and_then(ToolbarOverflow::parse)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(window: &str, breakpoint: u32, path: &[u32]) -> LayoutNodeRef {
        LayoutNodeRef {
            window: window.into(),
            breakpoint,
            path: path.to_vec(),
        }
    }

    #[test]
    fn errors_are_typed() {
        let err = apply_layout_patches(Some("layout \"x\" {"), &[], "layout.kdl").unwrap_err();
        assert!(matches!(&err, EditError::Config(errors) if !errors.is_empty()));
        assert_eq!(ApiError::from(err).code, ErrorCode::Config);
        let err = apply_layout_patches(
            None,
            &[LayoutPatch::SplitSizes {
                node: at("nope", 0, &[]),
                sizes: vec!["1fr".into()],
            }],
            "layout.kdl",
        )
        .unwrap_err();
        assert_eq!(ApiError::from(err).code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn a_missing_file_becomes_a_sparse_file() {
        let out = apply_layout_patches(
            None,
            &[LayoutPatch::AddKeyBinding {
                context: "list".into(),
                key: "E".into(),
                action: "mail.archive".into(),
                args: vec![],
            }],
            "layout.kdl",
        )
        .unwrap();
        assert_eq!(
            out,
            format!(
                "{}keys context=\"list\" {{\n        bind \"e\" \"mail.archive\"\n    }}\n}}\n",
                NEW_LAYOUT.trim_end_matches("}\n").to_owned() + "    "
            )
        );
        let layout = parse_layout_all(&out, "layout.kdl").unwrap();
        assert!(
            layout
                .keys
                .iter()
                .any(|k| k.context == "list" && k.key == "e")
        );
    }

    #[test]
    fn non_scalar_arguments_are_refused() {
        let err = apply_layout_patches(
            None,
            &[LayoutPatch::AddKeyBinding {
                context: "list".into(),
                key: "x".into(),
                action: "view.toggle".into(),
                args: vec![json!({"a": 1})],
            }],
            "layout.kdl",
        )
        .unwrap_err();
        assert!(matches!(err, EditError::Invalid(_)));
    }

    #[test]
    fn reset_toolbar_and_key_binding_restore_the_defaults() {
        let file = "layout \"mine\" extends=\"default\" {\n    toolbar \"customize\" {\n        item \"customize.merge\"\n    }\n    keys context=\"list\" {\n        bind \"j\" \"mail.archive\"\n    }\n    keys {\n        unbind \"e\"\n    }\n}\n";
        let custom = crate::parse_layout(file, "layout.kdl").unwrap();
        let defaults = default_layout();
        assert_ne!(custom.toolbars["customize"], defaults.toolbars["customize"]);

        let out = apply_layout_patches(
            Some(file),
            &[
                LayoutPatch::ResetToolbar {
                    toolbar: "customize".into(),
                },
                LayoutPatch::ResetKeyBinding {
                    context: "list".into(),
                    key: "J".into(),
                },
                LayoutPatch::ResetKeyBinding {
                    context: "global".into(),
                    key: "e".into(),
                },
            ],
            "layout.kdl",
        )
        .unwrap();
        assert!(!out.contains("toolbar"), "{out}");
        assert!(!out.contains("keys"), "{out}");
        let reset = crate::parse_layout(&out, "layout.kdl").unwrap();
        assert_eq!(reset.toolbars, defaults.toolbars);
        assert_eq!(reset.keys, defaults.keys);

        // Resetting what the file does not change is a no-op.
        let same = apply_layout_patches(
            Some(&out),
            &[LayoutPatch::ResetToolbar {
                toolbar: "customize".into(),
            }],
            "layout.kdl",
        )
        .unwrap();
        assert_eq!(same, out);
        let err = apply_layout_patches(
            Some(&out),
            &[LayoutPatch::ResetToolbar {
                toolbar: "Not Kebab".into(),
            }],
            "layout.kdl",
        )
        .unwrap_err();
        assert!(matches!(err, EditError::Invalid(_)));
    }

    #[test]
    fn reset_in_a_full_file_writes_the_default_back() {
        let file = DEFAULT_LAYOUT_KDL.replacen(
            "bind \"e\" \"mail.archive\"",
            "bind \"e\" \"mail.delete\"",
            1,
        );
        assert!(file.contains("\"mail.delete\""));
        let out = apply_layout_patches(
            Some(&file),
            &[LayoutPatch::ResetKeyBinding {
                context: "global".into(),
                key: "e".into(),
            }],
            "layout.kdl",
        )
        .unwrap();
        let reset = crate::parse_layout(&out, "layout.kdl").unwrap();
        let e = reset
            .keys
            .iter()
            .rev()
            .find(|k| k.context == "global" && k.key == "e")
            .unwrap();
        assert_eq!(e.action, "mail.archive");
    }
}
