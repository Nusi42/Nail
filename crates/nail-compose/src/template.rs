//! Templates with `{variable}` placeholders (composer templates, rule
//! "reply with template").
//!
//! Names are letters, digits, `-`, `_` and `.` (`{sender.name}`); `{{` and
//! `}}` write literal braces. Unknown variables are an error listing every
//! unknown name, so a template is never sent half filled.

use std::collections::BTreeMap;

/// Why a template could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// Placeholders without a value.
    #[error("unknown template variables: {}", .0.join(", "))]
    Unknown(Vec<String>),
    /// A `{` without its `}` (or a stray `}`), at this byte offset.
    #[error("unbalanced brace at offset {0}")]
    Unbalanced(usize),
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn expand_with(
    template: &str,
    vars: &BTreeMap<String, String>,
    escape: fn(&str) -> String,
) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(template.len());
    let mut unknown: Vec<String> = Vec::new();
    let mut chars = template.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|&(_, n)| n) == Some('{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek().map(|&(_, n)| n) == Some('}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let rest = &template[at + 1..];
                let end = rest.find('}').ok_or(TemplateError::Unbalanced(at))?;
                let name = rest[..end].trim();
                if !valid_name(name) {
                    return Err(TemplateError::Unbalanced(at));
                }
                match vars.get(name) {
                    Some(value) => out.push_str(&escape(value)),
                    None => {
                        if !unknown.iter().any(|u| u == name) {
                            unknown.push(name.to_owned());
                        }
                    }
                }
                // Skip the name and the closing brace.
                for _ in 0..rest[..=end].chars().count() {
                    chars.next();
                }
            }
            '}' => return Err(TemplateError::Unbalanced(at)),
            _ => out.push(c),
        }
    }
    if unknown.is_empty() {
        Ok(out)
    } else {
        Err(TemplateError::Unknown(unknown))
    }
}

/// Expands a plain-text or Markdown template.
pub fn expand_template(
    template: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    expand_with(template, vars, |v| v.to_owned())
}

/// Expands an HTML template; values are HTML-escaped.
pub fn expand_template_html(
    template: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    expand_with(template, vars, crate::escape_html)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("sender.name".to_owned(), "Ada <Lovelace>".to_owned()),
            ("subject".to_owned(), "Plans".to_owned()),
        ])
    }

    #[test]
    fn expands_variables_and_escapes_braces() {
        assert_eq!(
            expand_template("Hi {sender.name}, re {subject} {{literal}}", &vars()).unwrap(),
            "Hi Ada <Lovelace>, re Plans {literal}"
        );
        assert_eq!(
            expand_template_html("<p>{ sender.name }</p>", &vars()).unwrap(),
            "<p>Ada &lt;Lovelace&gt;</p>"
        );
        assert_eq!(
            expand_template("Grüße {subject}!", &vars()).unwrap(),
            "Grüße Plans!"
        );
    }

    #[test]
    fn reports_unknown_and_unbalanced() {
        assert_eq!(
            expand_template("{a} {subject} {b} {a}", &vars()),
            Err(TemplateError::Unknown(vec!["a".into(), "b".into()]))
        );
        assert_eq!(
            expand_template("x {open", &vars()),
            Err(TemplateError::Unbalanced(2))
        );
        assert_eq!(
            expand_template("x } y", &vars()),
            Err(TemplateError::Unbalanced(2))
        );
        assert_eq!(
            expand_template("{a b}", &vars()),
            Err(TemplateError::Unbalanced(0))
        );
        assert_eq!(
            TemplateError::Unknown(vec!["a".into(), "b".into()]).to_string(),
            "unknown template variables: a, b"
        );
    }
}
