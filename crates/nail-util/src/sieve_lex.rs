//! The lexer of Sieve scripts (RFC 5228 §2: identifiers, tags, numbers
//! with quantifiers, quoted and `text:` multi-line strings, comments,
//! punctuation): one definition for the Sieve compiler's round-trip parser
//! and the mock ManageSieve server's script check (wave Q2, ADR 0171).

/// A token of a ManageSieve protocol line (RFC 5804 §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireTok {
    /// An atom (`OK`, `PUTSCRIPT`, response code names).
    Atom(String),
    /// A quoted string (escapes undone; bytes, not checked as UTF-8).
    Str(Vec<u8>),
    /// `(`.
    Open,
    /// `)`.
    Close,
}

/// Splits one ManageSieve line into tokens: atoms, quoted strings,
/// parentheses; a trailing literal marker `{n}` / `{n+}` is returned
/// separately (its data follows on the wire). The error says what is
/// wrong (English).
pub fn wire_tokens(line: &str) -> Result<(Vec<WireTok>, Option<usize>), &'static str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'(' => {
                out.push(WireTok::Open);
                i += 1;
            }
            b')' => {
                out.push(WireTok::Close);
                i += 1;
            }
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                loop {
                    match bytes.get(i) {
                        None => return Err("unterminated string"),
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            let c = bytes.get(i + 1).ok_or("unterminated string")?;
                            s.push(*c);
                            i += 2;
                        }
                        Some(c) => {
                            s.push(*c);
                            i += 1;
                        }
                    }
                }
                out.push(WireTok::Str(s));
            }
            b'{' => {
                let end = line[i..].find('}').ok_or("unterminated literal")? + i;
                if end + 1 != bytes.len() {
                    return Err("literal marker not at the end");
                }
                let n = line[i + 1..end].trim_end_matches('+');
                let n: usize = n.parse().map_err(|_| "invalid literal length")?;
                return Ok((out, Some(n)));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'\t' | b'(' | b')' | b'"' | b'{')
                {
                    i += 1;
                }
                out.push(WireTok::Atom(line[start..i].to_owned()));
            }
        }
    }
    Ok((out, None))
}

/// A lexical error: the (1-based) line and what is wrong (English).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// Line (1-based).
    pub line: usize,
    /// What is wrong (English, for logs and tests).
    pub message: String,
}

/// A token of a Sieve script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// An identifier (lowercased): a command or test name.
    Ident(String),
    /// A tag (`:is`, lowercased).
    Tag(String),
    /// A number, with its `K` / `M` / `G` quantifier applied.
    Number(u64),
    /// A quoted or multi-line (`text:`) string.
    Str(String),
    /// A `#` comment (its trimmed text); bracketed comments are dropped.
    Comment(String),
    /// One of `[ ] ( ) { } , ;`.
    Punct(char),
}

/// The tokens of `script` with their (1-based) line.
pub fn lex(script: &str) -> Result<Vec<(usize, Tok)>, LexError> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    let fail = |line: usize, message: &str| LexError {
        line,
        message: message.to_owned(),
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '#' => {
                let start = i + 1;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                out.push((line, Tok::Comment(text.trim().to_owned())));
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let start = line;
                i += 2;
                loop {
                    if i + 1 >= chars.len() {
                        return Err(fail(start, "unterminated comment"));
                    }
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2;
                        break;
                    }
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            '"' => {
                let start = line;
                i += 1;
                let mut s = String::new();
                loop {
                    match chars.get(i) {
                        None => return Err(fail(start, "unterminated string")),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            let Some(next) = chars.get(i + 1) else {
                                return Err(fail(start, "unterminated string"));
                            };
                            s.push(*next);
                            i += 2;
                        }
                        Some('\r') if chars.get(i + 1) == Some(&'\n') => i += 1,
                        Some(ch) => {
                            if *ch == '\n' {
                                line += 1;
                            }
                            s.push(*ch);
                            i += 1;
                        }
                    }
                }
                out.push((start, Tok::Str(s)));
            }
            '[' | ']' | '(' | ')' | '{' | '}' | ',' | ';' => {
                out.push((line, Tok::Punct(c)));
                i += 1;
            }
            ':' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                if i == start + 1 || chars[start + 1].is_ascii_digit() {
                    return Err(fail(line, "invalid tag"));
                }
                let tag: String = chars[start..i].iter().collect();
                out.push((line, Tok::Tag(tag.to_ascii_lowercase())));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let digits: String = chars[start..i].iter().collect();
                let mut n: u64 = digits.parse().map_err(|_| fail(line, "number too large"))?;
                let factor = match chars.get(i) {
                    Some('K' | 'k') => 1 << 10,
                    Some('M' | 'm') => 1 << 20,
                    Some('G' | 'g') => 1 << 30,
                    _ => 1,
                };
                if factor > 1 {
                    i += 1;
                    n = n
                        .checked_mul(factor)
                        .ok_or_else(|| fail(line, "number too large"))?;
                }
                out.push((line, Tok::Number(n)));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word = chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase();
                if word == "text" && chars.get(i) == Some(&':') {
                    let start_line = line;
                    i += 1;
                    // Rest of the line: whitespace, optionally a comment.
                    while i < chars.len() && chars[i] != '\n' {
                        if chars[i] == '#' {
                            while i < chars.len() && chars[i] != '\n' {
                                i += 1;
                            }
                            break;
                        }
                        if !chars[i].is_whitespace() {
                            return Err(fail(line, "text after \"text:\""));
                        }
                        i += 1;
                    }
                    let mut s = String::new();
                    loop {
                        if i >= chars.len() {
                            return Err(fail(start_line, "unterminated multi-line string"));
                        }
                        i += 1; // the '\n'
                        line += 1;
                        let begin = i;
                        while i < chars.len() && chars[i] != '\n' {
                            i += 1;
                        }
                        let text: String = chars[begin..i].iter().collect();
                        let text = text.strip_suffix('\r').unwrap_or(&text);
                        if text == "." {
                            break;
                        }
                        s.push_str(text.strip_prefix('.').unwrap_or(text));
                        s.push('\n');
                    }
                    out.push((start_line, Tok::Str(s)));
                } else {
                    out.push((line, Tok::Ident(word)));
                }
            }
            other => return Err(fail(line, &format!("unexpected character {other:?}"))),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_every_token_kind() {
        let script = "require [\"fileinto\"]; # hi
/* x
 */ if size :over 1K {
  fileinto text:
.a
..b
.
;
}";
        let tokens = lex(script).unwrap();
        let kinds: Vec<&Tok> = tokens.iter().map(|(_, t)| t).collect();
        assert_eq!(kinds[0], &Tok::Ident("require".into()));
        assert_eq!(kinds[2], &Tok::Str("fileinto".into()));
        assert_eq!(kinds[5], &Tok::Comment("hi".into()));
        assert!(kinds.contains(&&Tok::Tag(":over".into())));
        assert!(kinds.contains(&&Tok::Number(1024)));
        assert!(
            kinds.contains(&&Tok::Str(
                "a
.b
"
                .into()
            ))
        );
        assert_eq!(tokens.last().unwrap(), &(9, Tok::Punct('}')));
    }

    #[test]
    fn wire_lines() {
        assert_eq!(
            wire_tokens(r#"PUTSCRIPT "a \"b\"" {12+}"#),
            Ok((
                vec![
                    WireTok::Atom("PUTSCRIPT".into()),
                    WireTok::Str(br#"a "b""#.to_vec())
                ],
                Some(12)
            ))
        );
        assert_eq!(
            wire_tokens("OK (TAG \"x\")"),
            Ok((
                vec![
                    WireTok::Atom("OK".into()),
                    WireTok::Open,
                    WireTok::Atom("TAG".into()),
                    WireTok::Str(b"x".to_vec()),
                    WireTok::Close
                ],
                None
            ))
        );
        assert_eq!(wire_tokens("X \"open"), Err("unterminated string"));
        assert_eq!(wire_tokens("X {3} y"), Err("literal marker not at the end"));
        assert_eq!(wire_tokens("X {z}"), Err("invalid literal length"));
        assert_eq!(wire_tokens("X {3"), Err("unterminated literal"));
    }

    #[test]
    fn reports_the_line_of_errors() {
        for (script, line, message) in [
            (
                "
\"open",
                2,
                "unterminated string",
            ),
            ("/* never", 1, "unterminated comment"),
            (": x", 1, "invalid tag"),
            ("99999999999999999999", 1, "number too large"),
            ("text: x", 1, "text after \"text:\""),
            (
                "text:
no end",
                1,
                "unterminated multi-line string",
            ),
            (
                "

@",
                3,
                "unexpected character '@'",
            ),
        ] {
            let error = lex(script).unwrap_err();
            assert_eq!(
                (error.line, error.message.as_str()),
                (line, message),
                "{script:?}"
            );
        }
    }
}
