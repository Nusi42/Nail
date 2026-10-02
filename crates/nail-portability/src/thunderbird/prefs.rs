//! `prefs.js` / `user.js`: `user_pref("name", value);` lines with string,
//! integer and boolean values (JavaScript string escapes).

use std::collections::HashMap;

/// A preference value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefValue {
    /// A string.
    Str(String),
    /// An integer.
    Int(i64),
    /// A boolean.
    Bool(bool),
}

/// The preferences of a profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    values: HashMap<String, PrefValue>,
}

impl Prefs {
    /// Parses `prefs.js` text; lines that are not preferences are ignored.
    pub fn parse(text: &str) -> Self {
        let mut values = HashMap::new();
        let mut rest = text;
        while let Some(at) = find_pref_call(rest) {
            let (_, after) = at;
            if let Some((name, value, used)) = parse_call(after) {
                values.insert(name, value);
                rest = &after[used..];
            } else {
                rest = after;
            }
        }
        Self { values }
    }

    /// A value.
    pub fn get(&self, name: &str) -> Option<&PrefValue> {
        self.values.get(name)
    }

    /// A string value (integers are given as text too).
    pub fn str(&self, name: &str) -> Option<&str> {
        match self.values.get(name)? {
            PrefValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// A non-empty string value.
    pub fn text(&self, name: &str) -> Option<String> {
        self.str(name)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    /// An integer value (strings holding integers too).
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.values.get(name)? {
            PrefValue::Int(i) => Some(*i),
            PrefValue::Str(s) => s.trim().parse().ok(),
            PrefValue::Bool(_) => None,
        }
    }

    /// A boolean value.
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.values.get(name)? {
            PrefValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// A comma-separated list.
    pub fn list(&self, name: &str) -> Vec<String> {
        self.str(name)
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Names starting with `prefix`, sorted.
    pub fn names_with_prefix(&self, prefix: &str) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .values
            .keys()
            .filter(|k| k.starts_with(prefix))
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }
}

/// Finds the next `user_pref(` / `pref(` call: returns the offset just
/// after the parenthesis and the rest.
fn find_pref_call(text: &str) -> Option<(usize, &str)> {
    let mut search = 0;
    while let Some(found) = text[search..].find("pref(") {
        let at = search + found;
        let before = &text[..at];
        let valid = before.is_empty()
            || before.ends_with("user_")
            || before.ends_with("sticky_")
            || before.ends_with("lock")
            || before.ends_with(|c: char| c.is_whitespace() || c == ';');
        let end = at + "pref(".len();
        if valid {
            return Some((end, &text[end..]));
        }
        search = end;
    }
    None
}

/// Parses `"name", value)`; returns the name, the value and the bytes used.
fn parse_call(text: &str) -> Option<(String, PrefValue, usize)> {
    let mut chars = Scanner { text, pos: 0 };
    chars.skip_ws();
    let name = chars.string()?;
    chars.skip_ws();
    if !chars.eat(',') {
        return None;
    }
    chars.skip_ws();
    let value = if chars.peek() == Some('"') || chars.peek() == Some('\'') {
        PrefValue::Str(chars.string()?)
    } else {
        let start = chars.pos;
        while let Some(c) = chars.peek() {
            if c == ')' || c == ',' || c.is_whitespace() {
                break;
            }
            chars.pos += c.len_utf8();
        }
        let word = &text[start..chars.pos];
        match word {
            "true" => PrefValue::Bool(true),
            "false" => PrefValue::Bool(false),
            _ => PrefValue::Int(word.parse().ok()?),
        }
    };
    chars.skip_ws();
    // `pref("x", 1, locked)` style extra arguments are ignored.
    while let Some(c) = chars.peek() {
        chars.pos += c.len_utf8();
        if c == ')' {
            return Some((name, value, chars.pos));
        }
    }
    None
}

struct Scanner<'a> {
    text: &'a str,
    pos: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    /// A JavaScript string literal.
    fn string(&mut self) -> Option<String> {
        let quote = self.peek().filter(|c| *c == '"' || *c == '\'')?;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += c.len_utf8();
            match c {
                c if c == quote => return Some(out),
                '\\' => {
                    let e = self.peek()?;
                    self.pos += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'u' => {
                            let hex = self.text.get(self.pos..self.pos + 4)?;
                            let unit = u16::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            // Surrogate pairs.
                            if (0xD800..0xDC00).contains(&unit)
                                && self.text[self.pos..].starts_with("\\u")
                                && let Some(low) = self
                                    .text
                                    .get(self.pos + 2..self.pos + 6)
                                    .and_then(|h| u16::from_str_radix(h, 16).ok())
                            {
                                self.pos += 6;
                                out.extend(
                                    char::decode_utf16([unit, low])
                                        .map(|r| r.unwrap_or('\u{fffd}')),
                                );
                            } else {
                                out.push(char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}'));
                            }
                        }
                        'x' => {
                            let hex = self.text.get(self.pos..self.pos + 2)?;
                            let byte = u8::from_str_radix(hex, 16).ok()?;
                            self.pos += 2;
                            out.push(char::from(byte));
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        let prefs = Prefs::parse(
            r#"// Mozilla User Preferences
/* comment user_pref("commented", 1); is still read by Firefox too */
user_pref("mail.accountmanager.accounts", "account1,account2");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.leave_on_server", true);
user_pref("mail.identity.id1.fullName", "Ada \"Countess\" Lovelace ü 😀");
user_pref("negative", -5);
user_pref("broken", );
pref("plain", "x\\y");
user_pref("mail.identity.id1.htmlSigText", "line1\nline2");
"#,
        );
        assert_eq!(
            prefs.list("mail.accountmanager.accounts"),
            vec!["account1", "account2"]
        );
        assert_eq!(prefs.int("mail.server.server1.port"), Some(993));
        assert_eq!(
            prefs.bool("mail.server.server1.leave_on_server"),
            Some(true)
        );
        assert_eq!(
            prefs.str("mail.identity.id1.fullName"),
            Some("Ada \"Countess\" Lovelace ü 😀")
        );
        assert_eq!(prefs.int("negative"), Some(-5));
        assert!(prefs.get("broken").is_none());
        assert_eq!(prefs.str("plain"), Some("x\\y"));
        assert_eq!(
            prefs.str("mail.identity.id1.htmlSigText"),
            Some("line1\nline2")
        );
        assert_eq!(prefs.text("missing"), None);
        assert_eq!(
            prefs.names_with_prefix("mail.server."),
            vec![
                "mail.server.server1.leave_on_server",
                "mail.server.server1.port"
            ]
        );
        // Not a call: `xpref(` is not `pref(`.
        assert!(Prefs::parse(r#"xpref("a", 1);"#).get("a").is_none());
    }
}
