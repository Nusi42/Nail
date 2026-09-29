//! Helpers for reading KDL documents with precise error positions.

use std::borrow::Cow;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use nail_api::SourceLocation;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::Value;

use crate::error::{ConfigError, ConfigErrorKind};

/// A value together with the place in a file where it was written.
///
/// Serializes and describes itself (JSON Schema) as the bare value; the
/// location is only used for error messages and the inspector.
#[derive(Debug, Clone, PartialEq)]
pub struct Located<T> {
    /// The value.
    pub value: T,
    /// Where it was written.
    pub loc: SourceLocation,
}

impl<T> Located<T> {
    pub(crate) fn new(value: T, loc: SourceLocation) -> Self {
        Self { value, loc }
    }
}

impl<T: JsonSchema> JsonSchema for Located<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}

/// One source file being read: maps byte offsets to lines and columns.
pub(crate) struct Source<'a> {
    file: &'a str,
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> Source<'a> {
    pub(crate) fn new(file: &'a str, text: &'a str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            file,
            text,
            line_starts,
        }
    }

    /// Parses the text as a KDL v2 document.
    pub(crate) fn parse(&self) -> Result<KdlDocument, ConfigError> {
        KdlDocument::parse_v2(self.text).map_err(|err| {
            let Some(diag) = err.diagnostics.first() else {
                return ConfigError::at(
                    ConfigErrorKind::Syntax,
                    &self.loc_at(0),
                    "invalid KDL document",
                );
            };
            let mut message = diag
                .message
                .clone()
                .unwrap_or_else(|| "invalid KDL".to_owned());
            if let Some(help) = &diag.help {
                message.push_str(" (");
                message.push_str(help);
                message.push(')');
            }
            ConfigError::at(
                ConfigErrorKind::Syntax,
                &self.loc_at(diag.span.offset()),
                format!("syntax error: {message}"),
            )
        })
    }

    /// The location of a byte offset.
    pub(crate) fn loc_at(&self, offset: usize) -> SourceLocation {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let start = self.line_starts[line];
        let column = self
            .text
            .get(start..offset)
            .map_or(offset - start, |s| s.chars().count());
        SourceLocation {
            file: self.file.to_owned(),
            line: u32::try_from(line + 1).unwrap_or(u32::MAX),
            column: u32::try_from(column + 1).unwrap_or(u32::MAX),
        }
    }

    /// Location of a node (its name).
    pub(crate) fn node_loc(&self, node: &KdlNode) -> SourceLocation {
        self.loc_at(node.name().span().offset())
    }

    /// Location of an entry (argument value or property name).
    pub(crate) fn entry_loc(&self, entry: &KdlEntry) -> SourceLocation {
        self.loc_at(entry.span().offset())
    }

    /// An "invalid" error at a node.
    pub(crate) fn node_err(&self, node: &KdlNode, message: impl Into<String>) -> ConfigError {
        ConfigError::invalid(&self.node_loc(node), message)
    }

    /// An "invalid" error at an entry.
    pub(crate) fn entry_err(&self, entry: &KdlEntry, message: impl Into<String>) -> ConfigError {
        ConfigError::invalid(&self.entry_loc(entry), message)
    }

    /// A located copy of a string.
    pub(crate) fn located<T>(&self, entry: &KdlEntry, value: T) -> Located<T> {
        Located::new(value, self.entry_loc(entry))
    }
}

/// Short human description of a KDL value, for error messages.
pub(crate) fn describe(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) => format!("the string {s:?}"),
        KdlValue::Integer(i) => format!("the number {i}"),
        KdlValue::Float(f) => format!("the number {f}"),
        KdlValue::Bool(b) => format!("#{b}"),
        KdlValue::Null => "#null".to_owned(),
    }
}

/// The node's name.
pub(crate) fn name(node: &KdlNode) -> &str {
    node.name().value()
}

/// The node's positional arguments.
pub(crate) fn args(node: &KdlNode) -> Vec<&KdlEntry> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .collect()
}

/// The node's properties as `(key, entry)`, in source order.
pub(crate) fn props(node: &KdlNode) -> impl Iterator<Item = (&str, &KdlEntry)> {
    node.entries()
        .iter()
        .filter_map(|e| e.name().map(|n| (n.value(), e)))
}

/// The node's children (empty when it has no block).
pub(crate) fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map_or(&[], KdlDocument::nodes)
}

/// Checks that every property of the node is in `allowed`.
pub(crate) fn check_props(
    src: &Source<'_>,
    node: &KdlNode,
    allowed: &[&str],
) -> Result<(), ConfigError> {
    for (key, entry) in props(node) {
        if !allowed.contains(&key) {
            let hint = if allowed.is_empty() {
                format!("`{}` takes no properties", name(node))
            } else {
                format!("allowed: {}", allowed.join(", "))
            };
            return Err(src.entry_err(
                entry,
                format!("unknown property `{key}` on `{}` ({hint})", name(node)),
            ));
        }
    }
    Ok(())
}

/// Checks that the node has at most `max` arguments.
pub(crate) fn check_max_args(
    src: &Source<'_>,
    node: &KdlNode,
    max: usize,
) -> Result<(), ConfigError> {
    if let Some(extra) = args(node).get(max) {
        return Err(src.entry_err(
            extra,
            format!(
                "too many arguments for `{}` (expected at most {max})",
                name(node)
            ),
        ));
    }
    Ok(())
}

/// Checks that the node has no children block.
pub(crate) fn check_no_children(src: &Source<'_>, node: &KdlNode) -> Result<(), ConfigError> {
    if let Some(child) = children(node).first() {
        return Err(src.node_err(
            child,
            format!("`{}` does not take a children block", name(node)),
        ));
    }
    Ok(())
}

/// Reads a string value.
pub(crate) fn entry_str<'e>(
    src: &Source<'_>,
    entry: &'e KdlEntry,
    what: &str,
) -> Result<&'e str, ConfigError> {
    entry.value().as_string().ok_or_else(|| {
        src.entry_err(
            entry,
            format!("{what} must be a string, found {}", describe(entry.value())),
        )
    })
}

/// Reads the required string argument at `index`.
pub(crate) fn arg_str<'n>(
    src: &Source<'_>,
    node: &'n KdlNode,
    index: usize,
    what: &str,
) -> Result<(&'n str, &'n KdlEntry), ConfigError> {
    let entry = *args(node).get(index).ok_or_else(|| {
        src.node_err(
            node,
            format!("`{}` needs {what} as argument {}", name(node), index + 1),
        )
    })?;
    Ok((entry_str(src, entry, what)?, entry))
}

/// Reads an optional string property.
pub(crate) fn prop_str<'n>(
    src: &Source<'_>,
    node: &'n KdlNode,
    key: &str,
) -> Result<Option<(&'n str, &'n KdlEntry)>, ConfigError> {
    match node.entry(key) {
        None => Ok(None),
        Some(entry) => Ok(Some((entry_str(src, entry, &format!("`{key}`"))?, entry))),
    }
}

/// Reads an optional non-negative integer property.
pub(crate) fn prop_u32(
    src: &Source<'_>,
    node: &KdlNode,
    key: &str,
) -> Result<Option<u32>, ConfigError> {
    match node.entry(key) {
        None => Ok(None),
        Some(entry) => entry
            .value()
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .map(Some)
            .ok_or_else(|| {
                src.entry_err(
                    entry,
                    format!(
                        "`{key}` must be a non-negative whole number, found {}",
                        describe(entry.value())
                    ),
                )
            }),
    }
}

/// Formats a number without a trailing `.0`.
pub(crate) fn format_float(f: f64) -> String {
    let s = f.to_string();
    s.strip_suffix(".0").map_or(s.clone(), str::to_owned)
}

/// Converts a scalar KDL value to a CSS value string (strings verbatim,
/// numbers formatted). Booleans and null are rejected.
pub(crate) fn scalar_to_string(
    src: &Source<'_>,
    entry: &KdlEntry,
    what: &str,
) -> Result<String, ConfigError> {
    match entry.value() {
        KdlValue::String(s) => Ok(s.clone()),
        KdlValue::Integer(i) => Ok(i.to_string()),
        KdlValue::Float(f) => Ok(format_float(*f)),
        other => Err(src.entry_err(
            entry,
            format!(
                "{what} must be a string or a number, found {}",
                describe(other)
            ),
        )),
    }
}

/// Converts a KDL value to JSON (for widget options and action arguments).
pub(crate) fn to_json(src: &Source<'_>, entry: &KdlEntry) -> Result<Value, ConfigError> {
    Ok(match entry.value() {
        KdlValue::String(s) => Value::String(s.clone()),
        KdlValue::Integer(i) => i64::try_from(*i)
            .map(Value::from)
            .map_err(|_| src.entry_err(entry, "number is out of range"))?,
        KdlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .ok_or_else(|| src.entry_err(entry, "number must be finite"))?,
        KdlValue::Bool(b) => Value::Bool(*b),
        KdlValue::Null => Value::Null,
    })
}

/// Whether `s` is a lowercase kebab-case identifier (`[a-z0-9]` words joined
/// by single hyphens, starting with a letter).
pub(crate) fn is_kebab(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `s` is a dotted name of kebab-case segments (`message-list.row`).
pub(crate) fn is_dotted_kebab(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_kebab)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_one_based_and_count_chars() {
        let src = Source::new("t.kdl", "a\nthéme \"é\" x=1\n");
        assert_eq!(src.loc_at(0).line, 1);
        let loc = src.loc_at(src.text.find('"').unwrap());
        assert_eq!((loc.line, loc.column), (2, 7));
        let loc = src.loc_at(src.text.find('x').unwrap());
        assert_eq!((loc.line, loc.column), (2, 11));
        assert_eq!(loc.file, "t.kdl");
        // Past the end is clamped.
        assert_eq!(src.loc_at(10_000).line, 3);
    }

    #[test]
    fn syntax_errors_carry_positions() {
        let src = Source::new("t.kdl", "node {\n  child 1.\n}\n");
        let err = src.parse().unwrap_err();
        assert_eq!(err.kind, ConfigErrorKind::Syntax);
        assert_eq!((err.line, err.column), (2, 9));
        assert!(err.message.starts_with("syntax error:"), "{}", err.message);
    }

    #[test]
    fn kebab_names() {
        assert!(is_kebab("focus-visible") && is_kebab("quote-1") && is_kebab("a"));
        assert!(!is_kebab("Focus") && !is_kebab("-a") && !is_kebab("a-") && !is_kebab("a--b"));
        assert!(!is_kebab("1a") && !is_kebab(""));
        assert!(is_dotted_kebab("message-list.row.subject"));
        assert!(!is_dotted_kebab("message-list..row") && !is_dotted_kebab(".row"));
    }

    #[test]
    fn floats_format_without_trailing_zero() {
        assert_eq!(format_float(1.0), "1");
        assert_eq!(format_float(1.25), "1.25");
        assert_eq!(format_float(0.45), "0.45");
    }
}
