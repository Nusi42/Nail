//! Part selectors: `"message-list.row:unread .subject"`,
//! `"folder-tree.item[role=\"inbox\"]"`.
//!
//! A selector is one or more compounds separated by whitespace. Each compound
//! is a part name followed by any number of `:state` and `[key]` /
//! `[key="value"]` conditions. The last compound is the target; the others
//! are ancestors. A part name starting with `.` continues the previous
//! compound's name (`.subject` after `message-list.row` means
//! `message-list.row.subject`).

use nail_api::{AncestorSelector, LocalizedMessage, PartCondition};

use crate::kdl_util::{is_dotted_kebab, is_kebab};
use crate::l10n::msg;

/// A parsed selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selector {
    /// Target part name.
    pub part: String,
    /// Target states.
    pub states: Vec<String>,
    /// Target conditions.
    pub conditions: Vec<PartCondition>,
    /// Ancestors, outermost first.
    pub ancestors: Vec<AncestorSelector>,
}

struct Compound {
    name: String,
    states: Vec<String>,
    conditions: Vec<PartCondition>,
}

/// Parses a part selector. Errors are readable messages.
pub(crate) fn parse_selector(input: &str) -> Result<Selector, LocalizedMessage> {
    let mut compounds: Vec<Compound> = Vec::new();
    for raw in input.split_whitespace() {
        let prev = compounds.last().map(|c| c.name.as_str());
        compounds.push(parse_compound(raw, prev)?);
    }
    let target = compounds
        .pop()
        .ok_or_else(|| msg("config-selector-empty"))?;
    Ok(Selector {
        part: target.name,
        states: target.states,
        conditions: target.conditions,
        ancestors: compounds
            .into_iter()
            .map(|c| AncestorSelector {
                selector: c.name,
                states: c.states,
                conditions: c.conditions,
            })
            .collect(),
    })
}

fn parse_compound(raw: &str, prev: Option<&str>) -> Result<Compound, LocalizedMessage> {
    let name_end = raw.find([':', '[']).unwrap_or(raw.len());
    let written = &raw[..name_end];
    let name = match (written.strip_prefix('.'), prev) {
        (Some(rel), Some(prev)) => format!("{prev}.{rel}"),
        (Some(_), None) => {
            return Err(msg("config-selector-relative").arg("part", written));
        }
        (None, _) if written.is_empty() => {
            return Err(msg("config-selector-needs-name").arg("selector", raw));
        }
        (None, _) => written.to_owned(),
    };
    if !is_dotted_kebab(&name) {
        return Err(msg("config-part-name-invalid").arg("name", name.as_str()));
    }

    let mut compound = Compound {
        name,
        states: Vec::new(),
        conditions: Vec::new(),
    };
    let mut rest = &raw[name_end..];
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(':') {
            let end = after.find([':', '[']).unwrap_or(after.len());
            let state = &after[..end];
            if !is_kebab(state) {
                return Err(msg("config-selector-state-invalid").arg("state", state));
            }
            compound.states.push(state.to_owned());
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let end = after
                .find(']')
                .ok_or_else(|| msg("config-selector-bracket").arg("selector", raw))?;
            compound.conditions.push(parse_condition(&after[..end])?);
            rest = &after[end + 1..];
        } else {
            return Err(msg("config-selector-unexpected").arg("text", rest));
        }
    }
    Ok(compound)
}

fn parse_condition(inner: &str) -> Result<PartCondition, LocalizedMessage> {
    let (key, value) = match inner.split_once('=') {
        None => (inner, None),
        Some((key, raw)) => {
            let value = ['"', '\'']
                .iter()
                .find_map(|q| raw.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
                .unwrap_or(raw);
            if value.is_empty() || value.contains(['"', '\'']) {
                return Err(msg("config-selector-condition-value").arg("condition", inner));
            }
            (key, Some(value.to_owned()))
        }
    };
    if !is_kebab(key) {
        return Err(msg("config-selector-condition-key").arg("condition", inner));
    }
    Ok(PartCondition {
        key: key.to_owned(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(key: &str, value: Option<&str>) -> PartCondition {
        PartCondition {
            key: key.into(),
            value: value.map(Into::into),
        }
    }

    #[test]
    fn plain_and_stateful_selectors() {
        let s = parse_selector("message-list.row").unwrap();
        assert_eq!(s.part, "message-list.row");
        assert!(s.states.is_empty() && s.conditions.is_empty() && s.ancestors.is_empty());

        let s = parse_selector("toolbar.button:hover:focus-visible").unwrap();
        assert_eq!(s.states, ["hover", "focus-visible"]);
    }

    #[test]
    fn conditions() {
        let s = parse_selector("message-list.row[tag=\"work\"][encrypted]").unwrap();
        assert_eq!(
            s.conditions,
            [cond("tag", Some("work")), cond("encrypted", None)]
        );
        let s = parse_selector("folder-tree.item[role=inbox]:selected").unwrap();
        assert_eq!(s.conditions, [cond("role", Some("inbox"))]);
        assert_eq!(s.states, ["selected"]);
    }

    #[test]
    fn relative_descendants() {
        let s = parse_selector("message-list.row:unread .subject").unwrap();
        assert_eq!(s.part, "message-list.row.subject");
        assert_eq!(s.ancestors.len(), 1);
        assert_eq!(s.ancestors[0].selector, "message-list.row");
        assert_eq!(s.ancestors[0].states, ["unread"]);

        let s = parse_selector("app[account=\"work\"]  message-list.row .date").unwrap();
        assert_eq!(s.part, "message-list.row.date");
        assert_eq!(s.ancestors[0].conditions, [cond("account", Some("work"))]);
        assert_eq!(s.ancestors[1].selector, "message-list.row");
    }

    #[test]
    fn errors() {
        for (input, needle) in [
            ("", "empty"),
            (".subject", "previous part name"),
            (":hover", "needs a part name"),
            ("Message-List", "invalid part name"),
            ("row:Hover", "invalid state"),
            ("row[tag=\"a", "missing `]`"),
            ("row[Tag]", "invalid condition"),
            ("row[tag=]", "invalid condition value"),
            ("row]x", "invalid part name"),
        ] {
            let err = crate::l10n::english(&parse_selector(input).unwrap_err());
            assert!(err.contains(needle), "{input:?}: {err}");
        }
    }
}
