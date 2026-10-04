//! Helpers for reading KDL documents with precise error positions.

use std::borrow::Cow;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use nail_api::{LocalizedMessage, SourceLocation};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::Value;

use crate::error::{ConfigError, ConfigErrorKind};
use crate::l10n::msg;

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
    ///
    /// Documents nested deeper than [`MAX_NESTING`] are refused before the
    /// `kdl` parser sees them: it recurses per child block (and per nested
    /// block comment), and a crafted file would overflow the stack, which
    /// aborts the process instead of returning an error.
    pub(crate) fn parse(&self) -> Result<KdlDocument, ConfigError> {
        self.parse_all().map_err(|mut errors| errors.swap_remove(0))
    }

    /// Like [`Self::parse`], but returns every syntax error the KDL parser
    /// reports (in file order), not only the first.
    pub(crate) fn parse_all(&self) -> Result<KdlDocument, Vec<ConfigError>> {
        if let Some(offset) = nesting_violation(self.text) {
            return Err(vec![ConfigError::at(
                ConfigErrorKind::Syntax,
                &self.loc_at(offset),
                msg("config-syntax-nesting").arg("max", MAX_NESTING),
            )]);
        }
        KdlDocument::parse_v2(self.text).map_err(|err| {
            let mut errors: Vec<ConfigError> = err
                .diagnostics
                .iter()
                .map(|diag| {
                    // The parser's own (English) description is an argument.
                    let localized = match (&diag.message, &diag.help) {
                        (Some(detail), Some(help)) => msg("config-syntax-help")
                            .arg("detail", detail.as_str())
                            .arg("help", help.as_str()),
                        (Some(detail), None) => msg("config-syntax").arg("detail", detail.as_str()),
                        (None, _) => msg("config-syntax-invalid"),
                    };
                    ConfigError::at(
                        ConfigErrorKind::Syntax,
                        &self.loc_at(diag.span.offset()),
                        localized,
                    )
                })
                .collect();
            if errors.is_empty() {
                errors.push(ConfigError::at(
                    ConfigErrorKind::Syntax,
                    &self.loc_at(0),
                    msg("config-syntax-invalid"),
                ));
            }
            // The parser's own order: its first diagnostic is the primary one.
            errors
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
    pub(crate) fn node_err(&self, node: &KdlNode, message: LocalizedMessage) -> ConfigError {
        ConfigError::invalid(&self.node_loc(node), message)
    }

    /// An "invalid" error at an entry.
    pub(crate) fn entry_err(&self, entry: &KdlEntry, message: LocalizedMessage) -> ConfigError {
        ConfigError::invalid(&self.entry_loc(entry), message)
    }

    /// A located copy of a string.
    pub(crate) fn located<T>(&self, entry: &KdlEntry, value: T) -> Located<T> {
        Located::new(value, self.entry_loc(entry))
    }
}

/// Collects errors while reading a file, so that one bad item does not hide
/// the errors of the others (every error of a file is reported at once).
#[derive(Debug, Default)]
pub(crate) struct Errors(std::cell::RefCell<Vec<ConfigError>>);

impl Errors {
    /// Records an error.
    pub(crate) fn push(&self, error: ConfigError) {
        self.0.borrow_mut().push(error);
    }

    /// The value of `result`, recording its error.
    pub(crate) fn ok<T>(&self, result: Result<T, ConfigError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.push(error);
                None
            }
        }
    }

    /// `value` when no error was recorded, else every error in file order.
    pub(crate) fn finish<T>(self, value: T) -> Result<T, Vec<ConfigError>> {
        let mut errors = self.0.into_inner();
        if errors.is_empty() {
            Ok(value)
        } else {
            sort_errors(&mut errors);
            Err(errors)
        }
    }
}

/// Sorts errors into file order (file, line, column) and drops exact
/// duplicates.
pub(crate) fn sort_errors(errors: &mut Vec<ConfigError>) {
    errors.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
    errors.dedup();
}

/// A KDL value as written (`"text"`, `5`, `#true`, `#null`), for the
/// `found` argument of error messages (language-neutral).
pub(crate) fn describe(value: &KdlValue) -> String {
    match value {
        KdlValue::String(s) => format!("{s:?}"),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => f.to_string(),
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
            return Err(src.entry_err(entry, unknown_property(key, name(node), allowed)));
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
            msg("config-too-many-arguments")
                .arg("node", name(node))
                .arg("max", max),
        ));
    }
    Ok(())
}

/// Checks that the node has no children block.
pub(crate) fn check_no_children(src: &Source<'_>, node: &KdlNode) -> Result<(), ConfigError> {
    if let Some(child) = children(node).first() {
        return Err(src.node_err(child, msg("config-no-children").arg("node", name(node))));
    }
    Ok(())
}

/// Reads a string value. `what` names it in the message: a property
/// ([`what_key`]) or a node's argument ([`what_arg`]).
pub(crate) fn entry_str<'e>(
    src: &Source<'_>,
    entry: &'e KdlEntry,
    what: &str,
) -> Result<&'e str, ConfigError> {
    entry.value().as_string().ok_or_else(|| {
        src.entry_err(
            entry,
            msg("config-expected-string")
                .arg("what", what)
                .arg("found", describe(entry.value())),
        )
    })
}

/// The `what` of an argument in messages: `` `node` #1 ``.
pub(crate) fn what_arg(node: &KdlNode, index: usize) -> String {
    format!("`{}` #{}", name(node), index + 1)
}

/// The `what` of a property in messages: `` `key` ``.
pub(crate) fn what_key(key: &str) -> String {
    format!("`{key}`")
}

/// "unknown property", with the allowed ones as a hint.
pub(crate) fn unknown_property(key: &str, node: &str, allowed: &[&str]) -> LocalizedMessage {
    if allowed.is_empty() {
        msg("config-unknown-property-none")
            .arg("key", key)
            .arg("node", node)
    } else {
        msg("config-unknown-property-hint")
            .arg("key", key)
            .arg("node", node)
            .arg("allowed", allowed.join(", "))
    }
}

/// Reads the required string argument at `index`.
pub(crate) fn arg_str<'n>(
    src: &Source<'_>,
    node: &'n KdlNode,
    index: usize,
) -> Result<(&'n str, &'n KdlEntry), ConfigError> {
    let entry = *args(node).get(index).ok_or_else(|| {
        src.node_err(
            node,
            msg("config-missing-argument")
                .arg("node", name(node))
                .arg("index", index + 1),
        )
    })?;
    Ok((entry_str(src, entry, &what_arg(node, index))?, entry))
}

/// Reads an optional string property.
pub(crate) fn prop_str<'n>(
    src: &Source<'_>,
    node: &'n KdlNode,
    key: &str,
) -> Result<Option<(&'n str, &'n KdlEntry)>, ConfigError> {
    match node.entry(key) {
        None => Ok(None),
        Some(entry) => Ok(Some((entry_str(src, entry, &what_key(key))?, entry))),
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
                    msg("config-expected-count")
                        .arg("what", what_key(key))
                        .arg("found", describe(entry.value())),
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
            msg("config-expected-string-or-number")
                .arg("what", what)
                .arg("found", describe(other)),
        )),
    }
}

/// Converts a KDL value to JSON (for widget options and action arguments).
pub(crate) fn to_json(src: &Source<'_>, entry: &KdlEntry) -> Result<Value, ConfigError> {
    Ok(match entry.value() {
        KdlValue::String(s) => Value::String(s.clone()),
        KdlValue::Integer(i) => i64::try_from(*i)
            .map(Value::from)
            .map_err(|_| src.entry_err(entry, msg("config-number-out-of-range")))?,
        KdlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .ok_or_else(|| src.entry_err(entry, msg("config-number-not-finite")))?,
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

/// Deepest accepted nesting of `{ … }` child blocks (and of `/* … */` block
/// comments) in a config file. The shipped files use fewer than ten levels.
pub(crate) const MAX_NESTING: usize = 64;

/// The byte offset of the first `{` (or `/*`) that nests deeper than
/// [`MAX_NESTING`], or `None`.
///
/// A cheap lexical pre-scan: it skips quoted, multi-line and raw strings and
/// comments, so braces inside them do not count. It does not validate
/// anything else; unbalanced input is left to the real parser.
fn nesting_violation(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    let at = |i: usize| b.get(i).copied();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Some(i);
                }
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'/' if at(i + 1) == Some(b'/') => {
                i = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |p| i + p);
            }
            b'/' if at(i + 1) == Some(b'*') => {
                // Block comments nest in KDL.
                let mut level = 0usize;
                while i < b.len() {
                    if b[i] == b'/' && at(i + 1) == Some(b'*') {
                        level += 1;
                        if level > MAX_NESTING {
                            return Some(i);
                        }
                        i += 2;
                    } else if b[i] == b'*' && at(i + 1) == Some(b'/') {
                        level -= 1;
                        i += 2;
                        if level == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'#' => {
                // `#"raw"#`, `##"raw"##`, `#"""multi-line raw"""#`, or a
                // keyword like `#true`.
                let hashes = b[i..].iter().take_while(|&&c| c == b'#').count();
                i += hashes;
                if at(i) == Some(b'"') {
                    let quotes = if b[i..].starts_with(b"\"\"\"") { 3 } else { 1 };
                    i += quotes;
                    let mut end = vec![b'"'; quotes];
                    end.extend(std::iter::repeat_n(b'#', hashes));
                    i = find(b, i, &end).map_or(b.len(), |p| p + end.len());
                }
            }
            b'"' => {
                let quotes = if b[i..].starts_with(b"\"\"\"") { 3 } else { 1 };
                i += quotes;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                    } else if b[i..].starts_with(&b"\"\"\""[..quotes]) {
                        i += quotes;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// The first position at or after `from` where `needle` starts.
fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
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
    fn nesting_scan_counts_only_real_blocks() {
        let deep = |n: usize| format!("{}{}", "a {\n".repeat(n), "}\n".repeat(n));
        assert_eq!(nesting_violation(&deep(MAX_NESTING)), None);
        let text = deep(MAX_NESTING + 1);
        assert_eq!(nesting_violation(&text), Some(MAX_NESTING * 4 + 2));

        // Braces in strings and comments do not count.
        let braces = "{".repeat(200);
        for text in [
            format!("a \"{braces}\"\n"),
            format!("a \"\\\"{braces}\"\n"),
            format!("a \"\"\"\n{braces}\n\"\"\"\n"),
            format!("a #\"{braces}\"#\n"),
            format!("a ##\"\"#{braces}\"##\n"),
            format!("a #\"\"\"\n{braces}\n\"\"\"#\n"),
            format!("// {braces}\na\n"),
            format!("/* {braces} /* nested */ {braces} */ a\n"),
            format!("a #true {{ b }} // {braces}"),
        ] {
            assert_eq!(nesting_violation(&text), None, "{text}");
        }

        // Deeply nested block comments are refused too.
        let comments = format!(
            "{}{}",
            "/*".repeat(MAX_NESTING + 1),
            "*/".repeat(MAX_NESTING + 1)
        );
        assert_eq!(nesting_violation(&comments), Some(MAX_NESTING * 2));
        // Unterminated constructs just end the scan.
        assert_eq!(nesting_violation("a \"{{{"), None);
        assert_eq!(nesting_violation("/* {{{"), None);
    }

    #[test]
    fn too_deep_documents_are_a_syntax_error() {
        let text = "a {\n".repeat(10_000);
        let src = Source::new("t.kdl", &text);
        let err = src.parse().unwrap_err();
        assert_eq!(err.kind, ConfigErrorKind::Syntax);
        assert_eq!(
            (err.line, err.column),
            (u32::try_from(MAX_NESTING).unwrap() + 1, 3)
        );
        assert!(
            err.message.contains("nested more than 64"),
            "{}",
            err.message
        );
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
