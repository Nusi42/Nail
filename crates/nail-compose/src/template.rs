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
    expand_with(template, vars, ToOwned::to_owned)
}

/// Expands an HTML template; values are HTML-escaped.
pub fn expand_template_html(
    template: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    expand_with(template, vars, crate::escape_html)
}

/// A placeholder token no template contains: ASCII letters and digits
/// only, so Markdown leaves it alone in text, code and link targets.
fn token_prefix(template: &str) -> String {
    let mut n: u64 = 0x6e61_696c;
    loop {
        let prefix = format!("NAILVAR{n:x}Q");
        if !template.contains(&prefix) {
            return prefix;
        }
        n = n.wrapping_mul(31).wrapping_add(7);
    }
}

/// Renders a Markdown template to a sanitized HTML fragment. The values
/// are inserted as **text** after rendering (HTML-escaped), so a value
/// cannot add Markdown or HTML: a sender-controlled subject such as
/// `[Verify](https://evil.example) <img src=…>` stays literal text in an
/// automatic reply.
pub fn render_markdown_template(
    template: &str,
    vars: &BTreeMap<String, String>,
    style: &crate::MailStyle,
) -> Result<String, TemplateError> {
    let prefix = token_prefix(template);
    let names: Vec<&String> = vars.keys().collect();
    let tokens: BTreeMap<String, String> = names
        .iter()
        .enumerate()
        .map(|(i, name)| ((*name).clone(), format!("{prefix}{i}Z")))
        .collect();
    let markdown = expand_template(template, &tokens)?;
    let html = crate::render_markdown(&markdown, style);
    // One left-to-right pass: inserted values are never scanned again.
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(at) = rest.find(&prefix) {
        out.push_str(&rest[..at]);
        let after = &rest[at + prefix.len()..];
        let digits = after
            .find('Z')
            .filter(|&end| end > 0 && after[..end].bytes().all(|b| b.is_ascii_digit()));
        match digits.and_then(|end| Some((end, after[..end].parse::<usize>().ok()?))) {
            Some((end, i)) if i < names.len() => {
                let value = vars[names[i]].replace(['\r', '\n'], " ");
                out.push_str(&crate::escape_html(&value));
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&prefix);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(crate::sanitize_outgoing(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_templates_insert_values_as_text() {
        let vars = BTreeMap::from([
            (
                "subject".to_owned(),
                "[Verify your account](https://evil.example) <img src=https://evil.example/t.png> **now**"
                    .to_owned(),
            ),
            ("sender.name".to_owned(), "Eve <script>x</script>".to_owned()),
        ]);
        let html = render_markdown_template(
            "Hello **{sender.name}**, thanks for “{subject}”. [Help](https://help.example)",
            &vars,
            &crate::MailStyle::default(),
        )
        .unwrap();
        // The template's own Markdown is rendered…
        assert!(
            html.contains("<strong>Eve &lt;script&gt;x&lt;/script&gt;</strong>"),
            "{html}"
        );
        assert!(html.contains("href=\"https://help.example\""), "{html}");
        // …the values are not.
        assert!(!html.contains("href=\"https://evil.example\""), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(
            html.contains("[Verify your account](https://evil.example)"),
            "{html}"
        );
        assert!(html.contains("**now**"), "{html}");
        assert!(!html.contains("NAILVAR"), "{html}");
        // Unknown variables are still refused.
        assert_eq!(
            render_markdown_template("{nope}", &vars, &crate::MailStyle::default()),
            Err(TemplateError::Unknown(vec!["nope".into()]))
        );
    }

    #[test]
    fn markdown_template_tokens_avoid_the_template_text() {
        let first = token_prefix("plain");
        let clash = token_prefix(&format!("x {first}0Z"));
        assert_ne!(first, clash);
        let vars = BTreeMap::from([("a".to_owned(), "A".to_owned())]);
        let html = render_markdown_template(
            &format!("{first}0Z {{a}} {{{{a}}}}"),
            &vars,
            &crate::MailStyle::default(),
        )
        .unwrap();
        assert!(html.contains(&format!("{first}0Z A {{a}}")), "{html}");
    }

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
