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

use crate::accounts::write::{detect_indent, quote, split_leading, uses_crlf};

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

fn node_format(node: &mut KdlNode) -> &mut KdlNodeFormat {
    if node.format().is_none() {
        node.set_format(KdlNodeFormat::default());
    }
    node.format_mut().expect("format was just set")
}

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
            fmt.leading = style.nl.to_owned();
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
    let last_ends_line = ends_line(last);
    if !last_ends_line {
        let fmt = node_format(last);
        if fmt.terminator == ";" {
            fmt.terminator.clear();
        }
        fmt.before_terminator = fmt.before_terminator.trim_end().to_owned();
        fmt.trailing = format!("{}{}", fmt.trailing.trim_end(), style.nl);
        let doc = doc_format(block);
        doc.trailing = parent_pad;
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
    if index == 0 && leading.starts_with('\n') && !keep.starts_with('\n') {
        keep.insert_str(0, style.nl);
    }
    if let Some(next) = block.nodes_mut().get_mut(index) {
        let fmt = node_format(next);
        let next_leading = if index == 0 && fmt.leading.starts_with('\n') && keep.contains('\n') {
            // The line break after `{` is already in `keep`.
            fmt.leading.trim_start_matches(['\r', '\n']).to_owned()
        } else {
            std::mem::take(&mut fmt.leading)
        };
        fmt.leading = format!("{keep}{next_leading}");
    } else if block.nodes().is_empty() {
        let fmt = doc_format(block);
        if !fmt.leading.contains('\n') {
            fmt.leading = keep.trim_end_matches([' ', '\t']).to_owned();
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

/// Renders a document back to text, ending with a line break.
pub(crate) fn to_text(doc: &KdlDocument, style: &Style) -> String {
    let mut out = doc.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(style.nl);
    }
    out
}

/// Structural equality ignoring formatting (name, arguments in order,
/// properties, children).
pub(crate) fn same_node(a: &KdlNode, b: &KdlNode) -> bool {
    fn props(n: &KdlNode) -> Vec<(&str, &KdlValue)> {
        let mut props: Vec<_> = n
            .entries()
            .iter()
            .filter_map(|e| e.name().map(|k| (k.value(), e.value())))
            .collect();
        props.sort_by(|x, y| x.0.cmp(y.0));
        props
    }
    fn args(n: &KdlNode) -> Vec<&KdlValue> {
        n.entries()
            .iter()
            .filter(|e| e.name().is_none())
            .map(KdlEntry::value)
            .collect()
    }
    fn kids(n: &KdlNode) -> &[KdlNode] {
        n.children().map_or(&[], KdlDocument::nodes)
    }
    a.name().value() == b.name().value()
        && args(a) == args(b)
        && props(a) == props(b)
        && kids(a).len() == kids(b).len()
        && kids(a).iter().zip(kids(b)).all(|(x, y)| same_node(x, y))
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
}
