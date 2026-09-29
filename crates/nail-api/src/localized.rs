//! Localized messages built by the core (M2c, ADR 0043).
//!
//! Nothing the core, `nail-config`, the MCP server or the CLI shows to a
//! user may be English text built in Rust (design.md, "Languages"). Such
//! a message travels as a [`LocalizedMessage`]: a Fluent message id plus
//! named arguments. The UI renders it from the catalogs in
//! `locales/<lang>/*.ftl` with the per-key English fallback; Rust code
//! that has to print or show text itself (CLI output, OS notifications)
//! renders it from the same catalogs.
//!
//! Every contract type that carries an English *diagnostic* (`message`,
//! `detail`, `error` strings — for logs and developer tools) also carries
//! an optional `localized` sibling. When it is present the UI shows it;
//! when it is absent the UI falls back to the text chosen by the error
//! code (`error-<code>`) or the type's own kind / state message. The
//! diagnostic stays for logs and is never shown as the primary text.
//!
//! Message ids are Fluent identifiers: lowercase ASCII letters, digits and
//! `-`, starting with a letter ([`is_message_id`]). Argument names follow
//! the same rule with `_` allowed (Fluent variables, `{ $name }`).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A value of a [`LocalizedMessage`] argument.
///
/// JSON: a string or a number. Numbers are integers (counts, sizes,
/// ports, line numbers); Fluent selects plural forms from them. Dates are
/// passed as RFC 3339 strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum MessageArg {
    /// An integer.
    Number(i64),
    /// Text (inserted as is; never interpreted as markup).
    Text(String),
}

impl From<&str> for MessageArg {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for MessageArg {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

macro_rules! number_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for MessageArg {
            fn from(value: $t) -> Self {
                Self::Number(i64::from(value))
            }
        }
    )*};
}
number_arg!(i8, i16, i32, i64, u8, u16, u32);

impl From<usize> for MessageArg {
    fn from(value: usize) -> Self {
        Self::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<u64> for MessageArg {
    fn from(value: u64) -> Self {
        Self::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl std::fmt::Display for MessageArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(t) => f.write_str(t),
        }
    }
}

/// A user-facing message: a Fluent id and its arguments.
///
/// JSON: `{"id": "config-unknown-node", "args": {"name": "windw"}}`
/// (`args` is omitted when empty).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LocalizedMessage {
    /// Fluent message id, e.g. `"sieve-not-expressible-body"`.
    pub id: String,
    /// Named arguments (`{ $name }` in the message).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(as = "Option<IndexMap<String, MessageArg>>", optional)]
    pub args: IndexMap<String, MessageArg>,
}

impl LocalizedMessage {
    /// A message without arguments.
    ///
    /// Debug builds assert that `id` is a valid Fluent id.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(is_message_id(&id), "invalid Fluent message id {id:?}");
        Self {
            id,
            args: IndexMap::new(),
        }
    }

    /// Adds (or replaces) an argument.
    #[must_use]
    pub fn arg(mut self, name: &str, value: impl Into<MessageArg>) -> Self {
        debug_assert!(is_arg_name(name), "invalid Fluent variable {name:?}");
        self.args.insert(name.to_owned(), value.into());
        self
    }

    /// The Fluent message for an [`crate::ErrorCode`] (`error-<code>`),
    /// without arguments.
    pub fn for_code(code: crate::ErrorCode) -> Self {
        Self::new(code.message_id())
    }

    /// Whether the id and every argument name are well formed.
    pub fn is_valid(&self) -> bool {
        is_message_id(&self.id) && self.args.keys().all(|k| is_arg_name(k))
    }

    /// Renders the message with a lookup function that returns the
    /// pattern of an id (e.g. from a catalog), replacing `{ $name }`
    /// placeables with the arguments. Falls back to the id itself when the
    /// lookup has nothing. Only simple placeables are supported (no
    /// selectors); full Fluent rendering belongs to the UI and to a
    /// Fluent bundle in Rust.
    pub fn render_with(&self, lookup: impl Fn(&str) -> Option<String>) -> String {
        let Some(pattern) = lookup(&self.id) else {
            return self.id.clone();
        };
        let mut out = String::with_capacity(pattern.len());
        let mut rest = pattern.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let inner = after[..end].trim();
            match inner
                .strip_prefix('$')
                .and_then(|name| self.args.get(name.trim()))
            {
                Some(value) => out.push_str(&value.to_string()),
                None => out.push_str(&rest[start..start + end + 2]),
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }
}

impl std::fmt::Display for LocalizedMessage {
    /// `id` or `id(name=value, …)`, for logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)?;
        if !self.args.is_empty() {
            f.write_str("(")?;
            for (i, (name, value)) in self.args.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{name}={value}")?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

/// Whether `id` is a Fluent message id as Nail writes them: lowercase
/// ASCII letters, digits and single `-` between them, starting with a
/// letter.
pub fn is_message_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.ends_with('-')
        && !id.contains("--")
}

/// Whether `name` is a Fluent variable name as Nail writes them.
pub fn is_arg_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_compactly() {
        let plain = LocalizedMessage::new("sieve-uploaded");
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"id": "sieve-uploaded"})
        );
        let with = LocalizedMessage::new("config-unknown-node")
            .arg("name", "windw")
            .arg("line", 3u32);
        let value = serde_json::to_value(&with).unwrap();
        assert_eq!(
            value,
            json!({"id": "config-unknown-node", "args": {"name": "windw", "line": 3}})
        );
        assert_eq!(
            serde_json::from_value::<LocalizedMessage>(value).unwrap(),
            with
        );
        let parsed: LocalizedMessage = serde_json::from_value(json!({"id": "x"})).unwrap();
        assert!(parsed.args.is_empty());
    }

    #[test]
    fn validates_ids_and_names() {
        assert!(is_message_id("error-not-found"));
        assert!(is_message_id("a1"));
        for bad in ["", "Error", "-a", "a-", "a--b", "a.b", "a_b", "1a"] {
            assert!(!is_message_id(bad), "{bad}");
        }
        assert!(is_arg_name("count") && is_arg_name("file_name"));
        assert!(!is_arg_name("Count") && !is_arg_name("") && !is_arg_name("$x"));
        let mut msg = LocalizedMessage::new("ok");
        assert!(msg.is_valid());
        msg.args.insert("Bad".into(), MessageArg::Number(1));
        assert!(!msg.is_valid());
    }

    #[test]
    fn renders_simple_placeables() {
        let msg = LocalizedMessage::new("mail-new")
            .arg("count", 3usize)
            .arg("folder", "Inbox");
        let text = msg.render_with(|id| {
            (id == "mail-new").then(|| "{ $count } new in { $folder } { $missing }".to_owned())
        });
        assert_eq!(text, "3 new in Inbox { $missing }");
        assert_eq!(msg.render_with(|_| None), "mail-new");
        assert_eq!(msg.to_string(), "mail-new(count=3, folder=Inbox)");
        assert_eq!(
            LocalizedMessage::for_code(crate::ErrorCode::NotFound).id,
            "error-not-found"
        );
    }
}
