//! Text comparisons ([`TextMatch`] compiled into a [`Matcher`]) and the
//! simple `*` wildcard used by the query language's address terms.

use nail_api::rules::{TextMatch, TextOp};
use regex::{Regex, RegexBuilder};

/// Largest compiled regular expression (bytes of the automaton), so a rule
/// cannot make evaluation slow or eat memory.
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// A compiled [`TextMatch`].
#[derive(Debug, Clone)]
pub struct Matcher {
    op: TextOp,
    /// The comparison text, lowercased unless case-sensitive.
    value: String,
    case_sensitive: bool,
    regex: Option<Regex>,
}

impl Matcher {
    /// Compiles a text comparison. Fails only for an invalid regular
    /// expression (the message is the `regex` crate's).
    pub fn new(m: &TextMatch) -> Result<Self, String> {
        let regex = if m.op == TextOp::Regex {
            Some(
                RegexBuilder::new(&m.value)
                    .case_insensitive(!m.case_sensitive)
                    .size_limit(REGEX_SIZE_LIMIT)
                    .build()
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        Ok(Self {
            op: m.op,
            value: if m.case_sensitive {
                m.value.clone()
            } else {
                m.value.to_lowercase()
            },
            case_sensitive: m.case_sensitive,
            regex,
        })
    }

    /// Whether `text` passes.
    pub fn is_match(&self, text: &str) -> bool {
        if let Some(regex) = &self.regex {
            return regex.is_match(text);
        }
        let lowered;
        let text = if self.case_sensitive {
            text
        } else {
            lowered = text.to_lowercase();
            &lowered
        };
        let text = text.trim();
        match self.op {
            TextOp::Equals => text == self.value.trim(),
            TextOp::Contains => text.contains(&self.value),
            TextOp::StartsWith => text.starts_with(&self.value),
            TextOp::EndsWith => text.ends_with(&self.value),
            TextOp::Regex => false,
        }
    }

    /// The first of `values` that passes.
    pub fn first_match<'v>(&self, values: impl IntoIterator<Item = &'v str>) -> Option<&'v str> {
        values.into_iter().find(|v| self.is_match(v))
    }
}

/// Case-insensitive `*` wildcard match of the whole text (`*@example.com`).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    // Iterative matcher with backtracking to the last star.
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Shortens a value for an explanation.
pub fn excerpt(text: &str) -> String {
    const MAX: usize = 120;
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        flat
    } else {
        let mut s: String = flat.chars().take(MAX).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(op: TextOp, value: &str, case_sensitive: bool) -> Matcher {
        Matcher::new(&TextMatch {
            op,
            value: value.into(),
            case_sensitive,
        })
        .unwrap()
    }

    #[test]
    fn every_operator() {
        assert!(m(TextOp::Equals, "Hello", false).is_match(" hello "));
        assert!(!m(TextOp::Equals, "Hello", true).is_match("hello"));
        assert!(m(TextOp::Contains, "LLO w", false).is_match("hello world"));
        assert!(m(TextOp::StartsWith, "[list]", false).is_match("[LIST] news"));
        assert!(!m(TextOp::StartsWith, "[list]", true).is_match("[LIST] news"));
        assert!(m(TextOp::EndsWith, ".PDF", false).is_match("report.pdf"));
        assert!(m(TextOp::Regex, r"^\[list\]", false).is_match("[LIST] x"));
        assert!(!m(TextOp::Regex, r"^\[list\]", true).is_match("[LIST] x"));
        assert!(m(TextOp::Regex, r"invoice\s+#\d+", false).is_match("Your Invoice #42"));
        assert_eq!(
            m(TextOp::Contains, "b", false).first_match(["a", "abc", "b"]),
            Some("abc")
        );
    }

    #[test]
    fn invalid_or_huge_regexes_are_refused() {
        let bad = Matcher::new(&TextMatch {
            op: TextOp::Regex,
            value: "(unclosed".into(),
            case_sensitive: false,
        });
        assert!(bad.is_err());
        let huge = Matcher::new(&TextMatch {
            op: TextOp::Regex,
            value: r"\w{1000}\w{1000}\w{1000}".into(),
            case_sensitive: false,
        });
        assert!(huge.is_err());
    }

    #[test]
    fn globs() {
        assert!(glob_match("*@substack.com", "Writer@Substack.com"));
        assert!(!glob_match("*@substack.com", "a@substack.com.evil"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a*", "ba"));
        assert!(glob_match("news*@*.example", "newsletter@mail.example"));
    }

    #[test]
    fn excerpts_are_flat_and_short() {
        assert_eq!(excerpt(" a \n b "), "a b");
        let long = "x".repeat(300);
        assert_eq!(excerpt(&long).chars().count(), 121);
    }
}
