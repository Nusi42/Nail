//! Writing Sieve syntax (RFC 5228 section 2): quoted and multi-line
//! strings, string lists, `:matches` wildcards, and the `variables`
//! escape for literal `${`.

/// The variable that holds a literal `$` when the script uses `variables`
/// (RFC 5229 expands `${name}` in every string; substitution is single
/// pass, so `${nail_dollar}{x}` yields the literal text `${x}`).
pub(crate) const DOLLAR_VAR: &str = "nail_dollar";

/// How literal text is written into the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Escaper {
    /// The script requires `variables`: literal `${` must be protected.
    pub variables: bool,
}

impl Escaper {
    /// Protects `${` when variables are on.
    pub(crate) fn text(self, text: &str) -> String {
        if self.variables {
            text.replace("${", &format!("${{{DOLLAR_VAR}}}{{"))
        } else {
            text.to_owned()
        }
    }

    /// A quoted string of literal text.
    pub(crate) fn quoted(self, text: &str) -> String {
        quote(&self.text(text))
    }

    /// A string list of literal texts: one string or `["a", "b"]`.
    pub(crate) fn list<S: AsRef<str>>(self, items: &[S]) -> String {
        if let [one] = items {
            return self.quoted(one.as_ref());
        }
        let parts: Vec<String> = items.iter().map(|s| self.quoted(s.as_ref())).collect();
        format!("[{}]", parts.join(", "))
    }
}

/// A quoted string: `"` and `\` escaped; line breaks are allowed inside
/// quoted strings (RFC 5228 2.4.2) and kept as CRLF.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The text of a quoted string written by [`quote`] (`"…"` with `\`
/// escapes), `None` when `text` is not one.
pub(crate) fn unquote(text: &str) -> Option<String> {
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// A multi-line string (`text:` … `.`), dot-stuffed, for long texts such
/// as the vacation body; ends with the line break after the final `.`.
/// Line ends are `\n` here; the client sends CRLF.
pub(crate) fn multiline(text: &str) -> String {
    let mut out = String::from("text:\n");
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.split('\n') {
        if line.starts_with('.') {
            out.push('.');
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(".\n");
    out
}

/// Escapes the `:matches` wildcards `*` and `?` (and `\`) so `text` is
/// matched literally.
pub(crate) fn matches_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Reverses [`matches_literal`]: `None` when `pattern` has an unescaped
/// wildcard.
pub(crate) fn unmatches_literal(pattern: &str) -> Option<String> {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '*' | '?' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// Converts `\n` / bare `\r` line ends to CRLF, as RFC 5228 requires for
/// scripts on the wire.
pub fn to_crlf(script: &str) -> String {
    let normalized = script.replace("\r\n", "\n").replace('\r', "\n");
    normalized.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquoting_reverses_quoting() {
        for text in ["plain", "a \"quote\"", "back\\slash", ""] {
            assert_eq!(unquote(&quote(text)).as_deref(), Some(text));
        }
        assert_eq!(unquote("bare"), None);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert_eq!(quote("x\ny"), "\"x\ny\"");
        let plain = Escaper::default();
        assert_eq!(plain.list(&["a"]), "\"a\"");
        assert_eq!(plain.list(&["a", "b"]), "[\"a\", \"b\"]");
        assert_eq!(plain.quoted("${x}"), "\"${x}\"");
        let vars = Escaper { variables: true };
        assert_eq!(vars.quoted("cost ${x} $y"), "\"cost ${nail_dollar}{x} $y\"");
    }

    #[test]
    fn multiline_strings_are_dot_stuffed() {
        assert_eq!(
            multiline("Away.\n.hidden\r\nend"),
            "text:\nAway.\n..hidden\nend\n.\n"
        );
        assert_eq!(to_crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn matches_escaping_round_trips() {
        let text = r"50% off* now? \o/";
        let escaped = matches_literal(text);
        assert_eq!(escaped, r"50% off\* now\? \\o/");
        assert_eq!(unmatches_literal(&escaped).as_deref(), Some(text));
        assert_eq!(unmatches_literal("a*"), None);
        assert_eq!(unmatches_literal("a\\"), None);
    }
}
