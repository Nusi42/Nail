//! Format-preserving edits of KDL documents (layout write-back and the
//! merge dialog).
//!
//! The `kdl` crate keeps every byte of whitespace and every comment in the
//! `format` of documents, nodes and entries, so an untouched node prints
//! exactly as it was read. The helpers here change only what an edit needs:
//!
//! * a node's leading text holds the line break after `{` (first child
//!   only), blank lines, comments above it and its indentation; its
//!   terminator holds a trailing comment and the line break;
//! * [`append_rendered`] adds nodes rendered in the canonical style at the
//!   end of a block (turning a one-line block into a multi-line one first);
//! * [`remove_node`] drops a node with the comment block directly above it
//!   but keeps comments separated from it by a blank line;
//! * [`set_prop`] changes a property value in place (or appends it).

use kdl::{KdlDocument, KdlDocumentFormat, KdlEntry, KdlNode, KdlNodeFormat, KdlValue};

pub(crate) use crate::kdl_write::same_node;
use crate::kdl_write::{detect_indent, quote, split_leading, uses_crlf};

/// The indentation and line-ending style of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Style {
    /// One level of indentation, e.g. four spaces.
    pub(crate) indent: String,
    /// `"\n"` or `"\r\n"`.
    pub(crate) nl: &'static str,
}

impl Style {
    /// The style of an existing file (four spaces and `\n` for new files).
    pub(crate) fn of(text: &str) -> Self {
        Self {
            indent: detect_indent(text),
            nl: if uses_crlf(text) { "\r\n" } else { "\n" },
        }
    }

    /// Indentation for `depth` levels.
    pub(crate) fn pad(&self, depth: usize) -> String {
        self.indent.repeat(depth)
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            indent: "    ".to_owned(),
            nl: "\n",
        }
    }
}

/// The children of a node, creating an empty block (` {}`) when it has none.
pub(crate) fn block_of(node: &mut KdlNode) -> &mut KdlDocument {
    if node.children().is_none()
        && let Some(format) = node.format_mut()
        && format.before_children.is_empty()
    {
        " ".clone_into(&mut format.before_children);
    }
    node.ensure_children()
}

/// A KDL value written as source text: strings quoted, numbers plain,
/// `#true` / `#false` / `#null`.
pub(crate) fn value_text(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) => quote(s),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => crate::kdl_util::format_float(*f),
        KdlValue::Bool(true) => "#true".to_owned(),
        KdlValue::Bool(false) => "#false".to_owned(),
        KdlValue::Null => "#null".to_owned(),
    }
}

/// A JSON scalar as KDL source text; `None` for arrays and objects, which
/// KDL values cannot hold.
pub(crate) fn json_text(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    Some(match value {
        Value::String(s) => quote(s),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "#true".to_owned(),
        Value::Bool(false) => "#false".to_owned(),
        Value::Null => "#null".to_owned(),
        Value::Array(_) | Value::Object(_) => return None,
    })
}

/// The first string argument of a node.
pub(crate) fn first_arg(node: &KdlNode) -> Option<&str> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
}

/// A string property of a node.
pub(crate) fn str_prop<'n>(node: &'n KdlNode, key: &str) -> Option<&'n str> {
    node.get(key).and_then(KdlValue::as_string)
}

/// Sets property `key` to `value`, keeping its position and surrounding
/// formatting when it exists, else appending it.
pub(crate) fn set_prop(node: &mut KdlNode, key: &str, value: KdlValue) {
    let text = value_text(&value);
    if let Some(entry) = node.entry_mut(key) {
        entry.set_value(value);
        if let Some(format) = entry.format_mut() {
            format.value_repr = text;
        }
    } else {
        node.push(formatted(KdlEntry::new_prop(key, value), text));
    }
}

/// An entry that prints its value as `text` (quoted strings stay quoted).
fn formatted(mut entry: KdlEntry, text: String) -> KdlEntry {
    entry.set_format(kdl::KdlEntryFormat {
        value_repr: text,
        leading: " ".to_owned(),
        ..Default::default()
    });
    entry
}

/// Removes property `key` (all occurrences).
pub(crate) fn remove_prop(node: &mut KdlNode, key: &str) {
    node.entries_mut()
        .retain(|e| e.name().is_none_or(|n| n.value() != key));
}

/// Sets the first argument, keeping its formatting.
pub(crate) fn set_first_arg(node: &mut KdlNode, value: KdlValue) {
    let text = value_text(&value);
    if let Some(entry) = node.entries_mut().iter_mut().find(|e| e.name().is_none()) {
        entry.set_value(value);
        if let Some(format) = entry.format_mut() {
            format.value_repr = text;
        }
    } else {
        node.entries_mut()
            .insert(0, formatted(KdlEntry::new(value), text));
    }
}

/// Sets the positional argument `index` (0-based), keeping its formatting;
/// a missing one is appended after the last argument.
pub(crate) fn set_arg(node: &mut KdlNode, index: usize, value: KdlValue) {
    let text = value_text(&value);
    if let Some(entry) = node
        .entries_mut()
        .iter_mut()
        .filter(|e| e.name().is_none())
        .nth(index)
    {
        entry.set_value(value);
        if let Some(format) = entry.format_mut() {
            format.value_repr = text;
        }
        return;
    }
    let after = node
        .entries()
        .iter()
        .rposition(|e| e.name().is_none())
        .map_or(0, |i| i + 1);
    node.entries_mut()
        .insert(after, formatted(KdlEntry::new(value), text));
}

// `kdl` has no `get_or_insert` for formats; `set_format` right before
// makes the `expect` infallible.
#[expect(clippy::expect_used, reason = "the format was set on the line before")]
pub(crate) fn node_format(node: &mut KdlNode) -> &mut KdlNodeFormat {
    if node.format().is_none() {
        node.set_format(KdlNodeFormat::default());
    }
    node.format_mut().expect("format was just set")
}

#[expect(clippy::expect_used, reason = "the format was set on the line before")]
fn doc_format(doc: &mut KdlDocument) -> &mut KdlDocumentFormat {
    if doc.format().is_none() {
        doc.set_format(KdlDocumentFormat::default());
    }
    doc.format_mut().expect("format was just set")
}

fn ends_line(node: &KdlNode) -> bool {
    node.format()
        .is_some_and(|f| f.terminator.contains('\n') || f.trailing.contains('\n'))
}

/// Parses rendered node text (already indented for its target depth).
///
/// # Panics
///
/// When the text is not valid KDL: the renderers produce it, so that is a
/// bug.
pub(crate) fn parse_rendered(text: &str) -> Vec<KdlNode> {
    KdlDocument::parse_v2(text)
        .unwrap_or_else(|e| panic!("rendered invalid KDL ({e}):\n{text}"))
        .nodes()
        .to_vec()
}

/// Makes a block (the children of a node at `depth - 1`, children at
/// `depth`) multi-line: every child on its own line, the closing brace on
/// its own line. Blocks that already are multi-line are not touched.
pub(crate) fn ensure_multiline(block: &mut KdlDocument, depth: usize, style: &Style) {
    let pad = style.pad(depth);
    let parent_pad = style.pad(depth.saturating_sub(1));
    let count = block.nodes().len();
    if count == 0 {
        let fmt = doc_format(block);
        if !fmt.leading.contains('\n') && !fmt.trailing.contains('\n') {
            style.nl.clone_into(&mut fmt.leading);
            fmt.trailing = parent_pad;
        }
        return;
    }
    let doc_leading_breaks = block.format().is_some_and(|f| f.leading.contains('\n'));
    for i in 0..count {
        let prev_ends_line = if i == 0 {
            doc_leading_breaks
        } else {
            ends_line(&block.nodes()[i - 1])
        };
        let node = &mut block.nodes_mut()[i];
        let fmt = node_format(node);
        if !prev_ends_line && !fmt.leading.contains('\n') {
            fmt.leading = format!("{}{pad}", style.nl);
        }
    }
    let last = &mut block.nodes_mut()[count - 1];
    if !ends_line(last) {
        end_line(last, style);
        doc_format(block).trailing = parent_pad;
    }
}

/// Appends nodes rendered for `depth` (each line indented, ending with a
/// line break) at the end of `block`.
pub(crate) fn append_rendered(block: &mut KdlDocument, depth: usize, text: &str, style: &Style) {
    ensure_multiline(block, depth, style);
    let was_empty = block.nodes().is_empty();
    let doc_leading_breaks = block.format().is_some_and(|f| f.leading.contains('\n'));
    let mut nodes = parse_rendered(text);
    if was_empty
        && !doc_leading_breaks
        && let Some(first) = nodes.first_mut()
    {
        let fmt = node_format(first);
        fmt.leading.insert_str(0, style.nl);
    }
    block.nodes_mut().extend(nodes);
}

/// Replaces the node at `index` with nodes rendered for `depth`, keeping
/// the old node's leading text (line break, comments, indentation) and its
/// trailing comment.
pub(crate) fn replace_rendered(block: &mut KdlDocument, index: usize, text: &str) {
    let old = block.nodes()[index].format().cloned().unwrap_or_default();
    let mut nodes = parse_rendered(text);
    if let Some(first) = nodes.first_mut() {
        node_format(first).leading = old.leading;
    }
    if let Some(last) = nodes.last_mut() {
        let fmt = node_format(last);
        if old.terminator.contains("//") || old.terminator.contains("/*") {
            fmt.before_terminator = old.before_terminator;
            fmt.terminator = old.terminator;
        }
        fmt.trailing = old.trailing;
    }
    block.nodes_mut().splice(index..=index, nodes);
}

/// Removes the node at `index` together with the comment block directly
/// above it; comments separated from it by a blank line stay.
pub(crate) fn remove_node(block: &mut KdlDocument, index: usize, style: &Style) {
    let removed = block.nodes_mut().remove(index);
    let leading = removed
        .format()
        .map(|f| f.leading.clone())
        .unwrap_or_default();
    let (keep, _) = split_leading(&leading);
    let mut keep = keep.to_owned();
    if index == 0 && strip_break(&leading).is_some() && strip_break(&keep).is_none() {
        keep.insert_str(0, style.nl);
    }
    if let Some(next) = block.nodes_mut().get_mut(index) {
        let fmt = node_format(next);
        // A blank line above the removed node (a later node's leading text
        // starts with a line break) separates what follows from what is
        // above unless the next node has one of its own.
        if index > 0
            && keep.is_empty()
            && strip_break(&leading).is_some()
            && strip_break(&fmt.leading).is_none()
        {
            keep.push_str(style.nl);
        }
        let next_leading =
            if index == 0 && strip_break(&fmt.leading).is_some() && keep.contains('\n') {
                // The line break after `{` is already in `keep`.
                fmt.leading.trim_start_matches(['\r', '\n']).to_owned()
            } else {
                std::mem::take(&mut fmt.leading)
            };
        fmt.leading = format!("{keep}{next_leading}");
    } else if block.nodes().is_empty() {
        let fmt = doc_format(block);
        if !fmt.leading.contains('\n') {
            keep.trim_end_matches([' ', '\t'])
                .clone_into(&mut fmt.leading);
        }
    } else {
        // The last node was removed: comments above it that stay move in
        // front of the closing brace.
        let kept = keep.trim_start_matches(['\r', '\n']);
        if !kept.trim().is_empty() {
            let fmt = doc_format(block);
            fmt.trailing = format!("{kept}{}", fmt.trailing.trim_start_matches([' ', '\t']));
        }
    }
}

/// Strips one line break (`\n` or `\r\n`) from the start of `text`.
fn strip_break(text: &str) -> Option<&str> {
    text.strip_prefix("\r\n")
        .or_else(|| text.strip_prefix('\n'))
}

/// Makes a node end its line: a `;` terminator or a node followed by
/// another on the same line gets a line break.
fn end_line(node: &mut KdlNode, style: &Style) {
    let fmt = node_format(node);
    if fmt.terminator.contains('\n') {
        if fmt.trailing.trim().is_empty() {
            fmt.trailing.clear();
        }
        return;
    }
    if fmt.trailing.contains('\n') {
        return;
    }
    if fmt.terminator == ";" {
        fmt.terminator.clear();
    }
    fmt.before_terminator = fmt.before_terminator.trim_end().to_owned();
    fmt.trailing = format!("{}{}", fmt.trailing.trim_end(), style.nl);
}

/// A copy of a node as it would be moved elsewhere: the comment block
/// directly above it goes along (its leading text is then that comment
/// block plus its indentation, without line breaks in front); it ends its
/// line.
pub(crate) fn extracted(node: &KdlNode, style: &Style) -> KdlNode {
    let mut node = node.clone();
    let leading = node.format().map(|f| f.leading.clone()).unwrap_or_default();
    let (_, attached) = split_leading(&leading);
    let attached = attached.trim_start_matches(['\r', '\n']).to_owned();
    node_format(&mut node).leading = attached;
    end_line(&mut node, style);
    node
}

/// Takes the node at `index` out of `block` (children at `depth`) for
/// moving it elsewhere ([`extracted`]); comments separated from it by a
/// blank line stay ([`remove_node`]).
pub(crate) fn detach(
    block: &mut KdlDocument,
    index: usize,
    depth: usize,
    style: &Style,
) -> KdlNode {
    ensure_multiline(block, depth, style);
    let node = extracted(&block.nodes()[index], style);
    remove_node(block, index, style);
    node
}

/// Inserts a detached node ([`detach`], [`reindent`]: leading text without
/// line breaks in front, indented for `depth`) into `block` (children at
/// `depth`) at `index` (clamped to the end).
pub(crate) fn put(
    block: &mut KdlDocument,
    index: usize,
    mut node: KdlNode,
    depth: usize,
    style: &Style,
) {
    ensure_multiline(block, depth, style);
    end_line(&mut node, style);
    let count = block.nodes().len();
    if count == 0 {
        // An empty block's leading text runs up to `}`: the indentation of
        // the closing brace moves behind the new node.
        let fmt = doc_format(block);
        if let Some(last) = fmt.leading.rfind('\n') {
            let closing = fmt.leading.split_off(last + 1);
            fmt.trailing.insert_str(0, &closing);
        }
    }
    let index = index.min(count);
    let doc_breaks = block.format().is_some_and(|f| f.leading.contains('\n'));
    if index == 0 && !doc_breaks {
        // The line break after `{` lives in the first child's leading text:
        // it moves to the new first node.
        if let Some(first) = block.nodes_mut().first_mut() {
            let fmt = node_format(first);
            if let Some(rest) = strip_break(&fmt.leading) {
                fmt.leading = rest.to_owned();
            }
        }
        node_format(&mut node).leading.insert_str(0, style.nl);
    }
    block.nodes_mut().insert(index, node);
}

/// Replaces the node at `index` with a detached node. The old node's
/// leading text stays in front of the new one: only what is separated from
/// it by a blank line (`keep_comments` false: the comments directly above
/// the old node go with it), or all of it (`keep_comments` true).
pub(crate) fn replace_with(
    block: &mut KdlDocument,
    index: usize,
    mut node: KdlNode,
    keep_comments: bool,
    style: &Style,
) {
    let old = block.nodes()[index]
        .format()
        .map(|f| f.leading.clone())
        .unwrap_or_default();
    let mut prefix = if keep_comments {
        old.rfind('\n')
            .map_or_else(String::new, |i| old[..=i].to_owned())
    } else {
        split_leading(&old).0.to_owned()
    };
    if prefix.is_empty() && strip_break(&old).is_some() {
        prefix.push_str(style.nl);
    }
    end_line(&mut node, style);
    node_format(&mut node).leading.insert_str(0, &prefix);
    block.nodes_mut()[index] = node;
}

/// Parses text holding exactly one node (with comments and indentation in
/// front). `None` when it is not valid KDL.
pub(crate) fn parse_one(text: &str) -> Option<KdlNode> {
    let doc = KdlDocument::parse_v2(text).ok()?;
    let [node] = doc.nodes() else {
        return None;
    };
    let mut node = node.clone();
    if let Some(lead) = doc.format().map(|f| f.leading.clone())
        && !lead.is_empty()
    {
        node_format(&mut node).leading.insert_str(0, &lead);
    }
    Some(node)
}

/// A detached node moved from nesting depth `from` to `to`: every line of
/// it (comments above it, its children, the closing brace) is indented by
/// the difference. `None` when the result does not parse (a multi-line
/// string the indentation change would alter).
pub(crate) fn reindent(node: &KdlNode, from: usize, to: usize, style: &Style) -> Option<KdlNode> {
    if from == to {
        return Some(node.clone());
    }
    let text = node.to_string();
    let add = style.pad(to.saturating_sub(from));
    let cut = style.pad(from.saturating_sub(to));
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let end = &line[body.len()..];
        if body.trim().is_empty() {
            out.push_str(end);
            continue;
        }
        if to > from {
            out.push_str(&add);
            out.push_str(body);
        } else if let Some(rest) = body.strip_prefix(cut.as_str()) {
            out.push_str(rest);
        } else {
            out.push_str(body.trim_start_matches([' ', '\t']));
        }
        out.push_str(end);
    }
    parse_one(&out)
}

/// Renders a document back to text, ending with a line break.
pub(crate) fn to_text(doc: &KdlDocument, style: &Style) -> String {
    let mut out = doc.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(style.nl);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> KdlDocument {
        KdlDocument::parse_v2(text).unwrap()
    }

    fn block(doc: &mut KdlDocument) -> &mut KdlDocument {
        doc.nodes_mut()[0].children_mut().as_mut().unwrap()
    }

    #[test]
    fn style_detection() {
        assert_eq!(Style::of("a {\n  b\n}\n").indent, "  ");
        assert_eq!(Style::of("a {\r\n\tb\r\n}\r\n").nl, "\r\n");
        assert_eq!(Style::of("").pad(2), "        ");
    }

    #[test]
    fn values_render_as_kdl() {
        assert_eq!(
            value_text(&KdlValue::String("a \"b\"".into())),
            "\"a \\\"b\\\"\""
        );
        assert_eq!(value_text(&KdlValue::Integer(3)), "3");
        assert_eq!(value_text(&KdlValue::Float(1.5)), "1.5");
        assert_eq!(value_text(&KdlValue::Bool(true)), "#true");
        assert_eq!(value_text(&KdlValue::Null), "#null");
        assert_eq!(json_text(&serde_json::json!("x")).as_deref(), Some("\"x\""));
        assert_eq!(json_text(&serde_json::json!(2)).as_deref(), Some("2"));
        assert_eq!(
            json_text(&serde_json::json!(false)).as_deref(),
            Some("#false")
        );
        assert_eq!(
            json_text(&serde_json::json!(null)).as_deref(),
            Some("#null")
        );
        assert_eq!(json_text(&serde_json::json!([1])), None);
    }

    #[test]
    fn props_change_in_place() {
        let mut d = doc("split \\\n    sizes=\"1fr 2fr\" /* why */ id=\"x\" { a; b }\n");
        let node = &mut d.nodes_mut()[0];
        set_prop(node, "sizes", KdlValue::String("200px 1fr".into()));
        set_prop(node, "min-sizes", KdlValue::String("none none".into()));
        assert_eq!(
            d.to_string(),
            "split \\\n    sizes=\"200px 1fr\" /* why */ id=\"x\" min-sizes=\"none none\" { a; b }\n"
        );
        let node = &mut d.nodes_mut()[0];
        remove_prop(node, "min-sizes");
        set_first_arg(node, KdlValue::String("vertical".into()));
        assert_eq!(
            d.to_string(),
            "split \"vertical\" \\\n    sizes=\"200px 1fr\" /* why */ id=\"x\" { a; b }\n"
        );
    }

    #[test]
    fn appending_to_a_multiline_block() {
        let style = Style::default();
        let mut d = doc("keys {\n    // j\n    bind \"j\" \"a.b\" // down\n}\n");
        append_rendered(block(&mut d), 1, "    bind \"k\" \"a.c\"\n", &style);
        assert_eq!(
            d.to_string(),
            "keys {\n    // j\n    bind \"j\" \"a.b\" // down\n    bind \"k\" \"a.c\"\n}\n"
        );
    }

    #[test]
    fn appending_to_empty_and_inline_blocks() {
        let style = Style::default();
        let mut d = doc("keys {}\n");
        append_rendered(block(&mut d), 1, "    bind \"k\" \"a.c\"\n", &style);
        assert_eq!(d.to_string(), "keys {\n    bind \"k\" \"a.c\"\n}\n");

        let mut d = doc("keys {\n}\n");
        append_rendered(block(&mut d), 1, "    bind \"k\" \"a.c\"\n", &style);
        assert_eq!(d.to_string(), "keys {\n    bind \"k\" \"a.c\"\n}\n");

        let mut d = doc("layout \"x\" {\n    toolbar \"t\" { item \"a.b\"; spacer }\n}\n");
        let toolbar = &mut block(&mut d).nodes_mut()[0];
        append_rendered(
            toolbar.children_mut().as_mut().unwrap(),
            2,
            "        separator\n",
            &style,
        );
        assert_eq!(
            d.to_string(),
            "layout \"x\" {\n    toolbar \"t\" {\n        item \"a.b\";\n        spacer\n        separator\n    }\n}\n"
        );
    }

    #[test]
    fn removing_keeps_separated_comments() {
        let style = Style::default();
        let text = "keys {\n    // section\n\n    // about j\n    bind \"j\" \"a.b\"\n    bind \"k\" \"a.c\" // up\n\n    // about l\n    bind \"l\" \"a.d\"\n}\n";
        let mut d = doc(text);
        remove_node(block(&mut d), 0, &style);
        assert_eq!(
            d.to_string(),
            "keys {\n    // section\n\n    bind \"k\" \"a.c\" // up\n\n    // about l\n    bind \"l\" \"a.d\"\n}\n"
        );
        remove_node(block(&mut d), 1, &style);
        assert_eq!(
            d.to_string(),
            "keys {\n    // section\n\n    bind \"k\" \"a.c\" // up\n}\n"
        );
        remove_node(block(&mut d), 0, &style);
        assert_eq!(d.to_string(), "keys {\n    // section\n\n}\n");

        // The first node without comments.
        let mut d = doc("keys {\n    bind \"j\" \"a.b\"\n    bind \"k\" \"a.c\"\n}\n");
        remove_node(block(&mut d), 0, &style);
        assert_eq!(d.to_string(), "keys {\n    bind \"k\" \"a.c\"\n}\n");
        remove_node(block(&mut d), 0, &style);
        assert_eq!(d.to_string(), "keys {\n}\n");
    }

    #[test]
    fn replacing_keeps_comments_around_the_node() {
        let mut d =
            doc("keys {\n    // mine\n    bind \"j\" \"a.b\" // note\n    bind \"k\" \"a.c\"\n}\n");
        replace_rendered(block(&mut d), 0, "    bind \"j\" \"x.y\"\n");
        assert_eq!(
            d.to_string(),
            "keys {\n    // mine\n    bind \"j\" \"x.y\" // note\n    bind \"k\" \"a.c\"\n}\n"
        );
    }

    #[test]
    fn structural_equality() {
        let a = doc("item \"a.b\" label=\"x\" icon=\"y\"\n");
        let b = doc("item   \"a.b\" icon=\"y\"   label=\"x\";");
        assert!(same_node(&a.nodes()[0], &b.nodes()[0]));
        let c = doc("item \"a.c\"");
        assert!(!same_node(&a.nodes()[0], &c.nodes()[0]));
    }

    #[test]
    fn detached_nodes_take_their_comments_and_move_between_depths() {
        let style = Style::default();
        let mut d =
            doc("a {\n    // kept\n\n    // about x\n    x {\n        y // why\n    }\n    z\n}\n");
        let x = detach(block(&mut d), 0, 1, &style);
        assert_eq!(d.to_string(), "a {\n    // kept\n\n    z\n}\n");
        assert_eq!(
            x.to_string(),
            "    // about x\n    x {\n        y // why\n    }\n"
        );
        let deeper = reindent(&x, 1, 3, &style).unwrap();
        assert_eq!(
            deeper.to_string(),
            "            // about x\n            x {\n                y // why\n            }\n"
        );
        let back = reindent(&deeper, 3, 1, &style).unwrap();
        assert_eq!(back.to_string(), x.to_string());
        // At the front, behind the comments that stayed, and at the end.
        put(block(&mut d), 0, x.clone(), 1, &style);
        assert_eq!(
            d.to_string(),
            "a {\n    // about x\n    x {\n        y // why\n    }\n    // kept\n\n    z\n}\n"
        );
        let mut e = doc("a {\n    z\n}\n");
        put(block(&mut e), 9, x, 1, &style);
        assert_eq!(
            e.to_string(),
            "a {\n    z\n    // about x\n    x {\n        y // why\n    }\n}\n"
        );
    }

    #[test]
    fn putting_into_empty_and_inline_blocks() {
        let style = Style::default();
        let node = || parse_one("    b\n").unwrap();
        for (text, expected) in [
            ("a {\n}\n", "a {\n    b\n}\n"),
            ("a {}\n", "a {\n    b\n}\n"),
            ("x {\n    a {\n    }\n}\n", ""),
            ("a { c; }\n", "a {\n    b\n    c;\n}\n"),
        ] {
            if expected.is_empty() {
                // An empty nested block: the closing brace keeps its indentation.
                let mut d = doc(text);
                let inner = &mut block(&mut d).nodes_mut()[0];
                put(
                    inner.children_mut().as_mut().unwrap(),
                    0,
                    parse_one("        b\n").unwrap(),
                    2,
                    &style,
                );
                assert_eq!(d.to_string(), "x {\n    a {\n        b\n    }\n}\n");
                continue;
            }
            let mut d = doc(text);
            put(block(&mut d), 0, node(), 1, &style);
            let out = d.to_string();
            assert!(KdlDocument::parse_v2(&out).is_ok(), "{out}");
            if expected.contains("c;") {
                assert!(out.starts_with("a {\n    b\n"), "{out}");
            } else {
                assert_eq!(out, expected);
            }
        }
    }

    #[test]
    fn replacing_keeps_the_slot_or_all_comments() {
        let style = Style::default();
        let new = || parse_one("    // new\n    n\n").unwrap();
        let text = "a {\n    // section\n\n    // old\n    o\n    p\n}\n";
        let mut d = doc(text);
        replace_with(block(&mut d), 0, new(), false, &style);
        assert_eq!(
            d.to_string(),
            "a {\n    // section\n\n    // new\n    n\n    p\n}\n"
        );
        let mut d = doc(text);
        replace_with(block(&mut d), 0, new(), true, &style);
        assert_eq!(
            d.to_string(),
            "a {\n    // section\n\n    // old\n    // new\n    n\n    p\n}\n"
        );
    }

    #[test]
    fn crlf_blocks_keep_their_line_breaks() {
        let style = Style {
            indent: "\t".into(),
            nl: "\r\n",
        };
        let mut d = doc("a {\r\n\tx\r\n\ty\r\n}\r\n");
        let x = detach(block(&mut d), 0, 1, &style);
        assert_eq!(d.to_string(), "a {\r\n\ty\r\n}\r\n");
        put(block(&mut d), 1, x, 1, &style);
        assert_eq!(d.to_string(), "a {\r\n\ty\r\n\tx\r\n}\r\n");
    }

    #[test]
    fn a_blank_line_above_a_removed_node_stays() {
        let style = Style::default();
        let mut d = doc("a {\n    x\n\n    // about y\n    y\n    z\n}\n");
        remove_node(block(&mut d), 1, &style);
        assert_eq!(d.to_string(), "a {\n    x\n\n    z\n}\n");
        // The next node has its own blank line: no second one.
        let mut d = doc("a {\n    x\n\n    y\n\n    z\n}\n");
        remove_node(block(&mut d), 1, &style);
        assert_eq!(d.to_string(), "a {\n    x\n\n    z\n}\n");
    }

    #[test]
    fn arguments_are_set_in_place_or_added() {
        let mut d = doc("color \"bg\" \"#fff\" /* base */ dark=\"#000\"\n");
        let node = &mut d.nodes_mut()[0];
        set_arg(node, 1, KdlValue::String("#eee".into()));
        assert_eq!(
            d.to_string(),
            "color \"bg\" \"#eee\" /* base */ dark=\"#000\"\n"
        );
        let mut d = doc("color \"bg\" dark=\"#000\"\n");
        set_arg(&mut d.nodes_mut()[0], 1, KdlValue::Integer(4));
        assert_eq!(d.to_string(), "color \"bg\" 4 dark=\"#000\"\n");
    }
}
