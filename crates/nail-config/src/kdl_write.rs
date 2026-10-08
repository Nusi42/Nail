//! Writing KDL text: the pieces every writer shares (`accounts.kdl`,
//! `rules.kdl`, the layout write-back, the merge dialog, plug-in
//! placements): quoting, the canonical node-line builder [`Line`], a
//! file's style (indentation, line endings), the comment block a removed
//! node takes along, and structural node equality (wave Q2, ADR 0171).

use std::collections::HashMap;
use std::fmt::Write as _;

use kdl::{KdlEntry, KdlNode, KdlValue};

use crate::kdl_util::children;

/// Quotes a string as a KDL v2 quoted string (control characters and
/// bidirectional overrides escaped).
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control()
                || matches!(
                    c,
                    '\u{200E}'
                        | '\u{200F}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{FEFF}'
                ) =>
            {
                // Writing into a `String` cannot fail.
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One node line in the canonical form, built up front to back:
/// `name arg… key=value…`.
pub(crate) struct Line(pub(crate) String);

impl Line {
    pub(crate) fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    /// A quoted string argument.
    pub(crate) fn arg(mut self, value: &str) -> Self {
        self.0.push(' ');
        self.0.push_str(&quote(value));
        self
    }

    /// An argument written as it is (a number, `#true`, …).
    pub(crate) fn raw_arg(mut self, value: impl std::fmt::Display) -> Self {
        let _ = write!(self.0, " {value}");
        self
    }

    /// A quoted string property.
    pub(crate) fn prop(mut self, key: &str, value: &str) -> Self {
        let _ = write!(self.0, " {key}={}", quote(value));
        self
    }

    /// [`Self::prop`] when there is a value.
    pub(crate) fn prop_opt(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.prop(key, v),
            None => self,
        }
    }

    /// A property written as it is (a number, `#true`, …).
    pub(crate) fn raw_prop(mut self, key: &str, value: impl std::fmt::Display) -> Self {
        let _ = write!(self.0, " {key}={value}");
        self
    }

    /// A number property when there is a value.
    pub(crate) fn num_opt(self, key: &str, value: Option<u32>) -> Self {
        match value {
            Some(v) => self.raw_prop(key, v),
            None => self,
        }
    }

    /// A `#true` / `#false` property when there is a value.
    pub(crate) fn bool_opt(self, key: &str, value: Option<bool>) -> Self {
        match value {
            Some(b) => self.raw_prop(key, format_args!("#{b}")),
            None => self,
        }
    }

    /// The node at `depth`, with a children block when `children` is not
    /// empty (each child already rendered, ending with a newline).
    pub(crate) fn finish(self, depth: usize, indent: &str, children: &[String]) -> String {
        let pad = indent.repeat(depth);
        if children.is_empty() {
            format!("{pad}{}\n", self.0)
        } else {
            let mut out = format!("{pad}{} {{\n", self.0);
            for child in children {
                out.push_str(child);
            }
            out.push_str(&pad);
            out.push_str("}\n");
            out
        }
    }
}

/// The indentation unit of a file: the leading whitespace of its first
/// indented line, else four spaces.
pub(crate) fn detect_indent(text: &str) -> String {
    for line in text.lines() {
        let ws: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if !ws.is_empty() && line.len() > ws.len() && !line[ws.len()..].starts_with("//") {
            return ws;
        }
    }
    "    ".to_owned()
}

/// Whether the file uses `\r\n` line endings.
pub(crate) fn uses_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Splits a removed node's leading text into the part that stays in the
/// file (everything up to the last blank line) and the part that goes with
/// the node (the comment block directly above it).
pub(crate) fn split_leading(leading: &str) -> (&str, &str) {
    let bytes = leading.as_bytes();
    let mut keep_until = 0;
    let mut line_start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            let line = &leading[line_start..i];
            if line.trim().is_empty() && i > 0 && line_start > 0 {
                // A whitespace-only line that is preceded by another line
                // break: a blank line. Keep everything up to it.
                keep_until = i + 1;
            }
            line_start = i + 1;
        }
    }
    leading.split_at(keep_until)
}

/// A node's properties as a map (the last of a repeated key wins, as in
/// KDL).
pub(crate) fn prop_map(node: &KdlNode) -> HashMap<&str, &KdlValue> {
    node.entries()
        .iter()
        .filter_map(|e| e.name().map(|k| (k.value(), e.value())))
        .collect()
}

/// A node's argument values in order.
pub(crate) fn arg_values(node: &KdlNode) -> Vec<&KdlValue> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(KdlEntry::value)
        .collect()
}

/// Same name, type annotation, arguments in order and properties
/// (ignoring formatting).
pub(crate) fn same_header(a: &KdlNode, b: &KdlNode) -> bool {
    a.name().value() == b.name().value()
        && a.ty().map(kdl::KdlIdentifier::value) == b.ty().map(kdl::KdlIdentifier::value)
        && arg_values(a) == arg_values(b)
        && prop_map(a) == prop_map(b)
}

/// Structural equality ignoring formatting: [`same_header`] and the same
/// children in order.
pub(crate) fn same_node(a: &KdlNode, b: &KdlNode) -> bool {
    same_header(a, b)
        && children(a).len() == children(b).len()
        && children(a)
            .iter()
            .zip(children(b))
            .all(|(x, y)| same_node(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdl::KdlDocument;

    #[test]
    fn quoting_escapes_what_kdl_needs() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        assert_eq!(quote("\u{202E}"), "\"\\u{202e}\"");
        assert_eq!(quote("tab\there\r"), "\"tab\\there\\r\"");
        assert_eq!(quote("\u{feff}x\u{7}"), "\"\\u{feff}x\\u{7}\"");
        for s in ["a\"b", "x\\y", "l1\nl2", "\u{202e}rtl", "tab\t", "é😀"] {
            let doc = KdlDocument::parse_v2(&format!("n {}", quote(s))).unwrap();
            assert_eq!(doc.nodes()[0].entries()[0].value().as_string(), Some(s));
        }
    }

    #[test]
    fn lines() {
        let leaf = Line::new("n")
            .arg("a b")
            .raw_arg(3)
            .prop("k", "v")
            .prop_opt("none", None)
            .num_opt("n", Some(2))
            .bool_opt("on", Some(true))
            .bool_opt("off", None);
        assert_eq!(
            leaf.finish(1, "  ", &[]),
            "  n \"a b\" 3 k=\"v\" n=2 on=#true\n"
        );
        let parent = Line::new("p").finish(0, "  ", &["  c\n".to_owned()]);
        assert_eq!(parent, "p {\n  c\n}\n");
    }

    #[test]
    fn indentation_and_line_endings() {
        assert_eq!(detect_indent("a {\n  b\n}\n"), "  ");
        assert_eq!(detect_indent("a {\n\tb\n}\n"), "\t");
        assert_eq!(detect_indent("a\n"), "    ");
        assert_eq!(detect_indent(""), "    ");
        assert_eq!(detect_indent("a {\n // x\n    b\n}\n"), "    ");
        assert!(uses_crlf("a\r\nb") && !uses_crlf("a\nb"));
    }

    #[test]
    fn leading_comments() {
        assert_eq!(split_leading(""), ("", ""));
        assert_eq!(split_leading("    "), ("", "    "));
        assert_eq!(
            split_leading("// header\n\n// about x\n"),
            ("// header\n\n", "// about x\n")
        );
        assert_eq!(
            split_leading("    // about x\n    "),
            ("", "    // about x\n    ")
        );
        assert_eq!(
            split_leading("\n    // a\n\n    // b\n    "),
            ("\n    // a\n\n", "    // b\n    ")
        );
    }

    #[test]
    fn structural_equality_ignores_formatting() {
        let parse = |text: &str| KdlDocument::parse_v2(text).unwrap().nodes()[0].clone();
        let a = parse("n 1 x=\"a\"   y=2 {\n  c\n}\n");
        assert!(same_node(&a, &parse("n   1 y=2 x=a { c; }")));
        assert!(same_node(&parse("n x=1 x=2"), &parse("n x=2")));
        assert!(!same_node(&a, &parse("n 1 x=\"a\" y=3 { c }")));
        assert!(!same_node(&a, &parse("n 1 x=\"a\" y=2 { d }")));
        assert!(!same_node(&parse("(t)n"), &parse("n")));
        assert!(same_header(&a, &parse("n 1 x=a y=2")));
    }
}
