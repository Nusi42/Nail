//! Splits an IMAP-like line (IMAP, ManageSieve) into atoms, quoted strings
//! and literals, keeping the text between them verbatim, so the protocol
//! rules can decide piece by piece what stays.

use std::borrow::Cow;

use crate::Part;

/// One piece of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    /// A run of characters without spaces, parentheses or quotes
    /// (`UID`, `151:200`, `BODY[]`, `\Seen`).
    Atom(&'a str),
    /// A quoted string: as written and its unescaped value.
    Quoted {
        /// The string with its quotes.
        raw: &'a str,
        /// The content.
        value: String,
    },
    /// A literal's bytes (never available, only their number).
    Literal(usize),
    /// Spaces and parentheses.
    Gap(&'a str),
}

impl Piece<'_> {
    /// The atom, upper-cased.
    pub(crate) fn upper_atom(&self) -> Option<String> {
        match self {
            Piece::Atom(atom) => Some(atom.to_ascii_uppercase()),
            _ => None,
        }
    }
}

/// The pieces of `parts`. A `{n}` / `{n+}` announcement at the end of a
/// text part followed by [`Part::Bytes`] is dropped (the literal stands for
/// it); line breaks are dropped.
pub(crate) fn pieces<'a>(parts: &'a [Part<'a>]) -> Vec<Piece<'a>> {
    let mut out = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        match part {
            Part::Bytes(n) => out.push(Piece::Literal(*n)),
            Part::Text(text) => {
                let literal_follows = matches!(parts.get(index + 1), Some(Part::Bytes(_)));
                let text = if literal_follows {
                    strip_announcement(text)
                } else {
                    text
                };
                lex(text, &mut out);
            }
        }
    }
    out
}

/// `text` without a trailing `{123}` / `{123+}` (and line break).
fn strip_announcement(text: &str) -> &str {
    let trimmed = text.trim_end_matches(['\r', '\n']);
    let Some(body) = trimmed.strip_suffix('}') else {
        return text;
    };
    let Some(open) = body.rfind('{') else {
        return text;
    };
    let digits = body[open + 1..].trim_end_matches('+');
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return text;
    }
    &trimmed[..open]
}

fn lex<'a>(text: &'a str, out: &mut Vec<Piece<'a>>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' | b'\n' => i += 1,
            b' ' | b'(' | b')' => {
                let start = i;
                while i < bytes.len() && matches!(bytes[i], b' ' | b'(' | b')') {
                    i += 1;
                }
                out.push(Piece::Gap(&text[start..i]));
            }
            b'"' => {
                let start = i;
                let mut value = Vec::new();
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if i + 1 < bytes.len() => {
                            value.push(bytes[i + 1]);
                            i += 2;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b => {
                            value.push(b);
                            i += 1;
                        }
                    }
                }
                out.push(Piece::Quoted {
                    raw: &text[start..i],
                    value: String::from_utf8_lossy(&value).into_owned(),
                });
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'(' | b')' | b'"' | b'\r' | b'\n')
                {
                    i += 1;
                }
                out.push(Piece::Atom(&text[start..i]));
            }
        }
    }
}

/// The placeholder of a literal or payload.
pub(crate) fn bytes_placeholder(n: impl std::fmt::Display) -> String {
    format!("{{{n} bytes}}")
}

/// Renders pieces; `choose` gives each piece's text (`None` = as written,
/// literals as [`bytes_placeholder`]).
pub(crate) fn render<'p>(
    pieces: &'p [Piece<'p>],
    mut choose: impl FnMut(usize, &Piece<'p>) -> Option<Cow<'p, str>>,
) -> String {
    let mut out = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        match choose(index, piece) {
            Some(text) => out.push_str(&text),
            None => match piece {
                Piece::Atom(text) | Piece::Gap(text) => out.push_str(text),
                Piece::Quoted { raw, .. } => out.push_str(raw),
                Piece::Literal(n) => out.push_str(&bytes_placeholder(*n)),
            },
        }
    }
    out.trim_end().to_owned()
}

/// Renders `pieces` with every quoted string as `value` (the protocol's
/// value placeholder).
pub(crate) fn render_masked(pieces: &[Piece<'_>], value: &'static str) -> String {
    render(pieces, |_, piece| match piece {
        Piece::Quoted { .. } => Some(Cow::Borrowed(value)),
        _ => None,
    })
}

/// The atoms of `pieces`, upper-cased, in order (for finding the command).
pub(crate) fn atoms(pieces: &[Piece<'_>]) -> Vec<String> {
    pieces.iter().filter_map(Piece::upper_atom).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn lines_split_into_pieces() {
        let parts = [
            Part::Text("* 1 FETCH (UID 7 ENVELOPE (\"a \\\"b\\\"\" NIL) BODY[] {11}\r\n"),
            Part::Bytes(11),
            Part::Text(")\r\n"),
        ];
        let pieces = pieces(&parts);
        assert_eq!(pieces[0], Piece::Atom("*"));
        assert!(pieces.contains(&Piece::Quoted {
            raw: "\"a \\\"b\\\"\"",
            value: "a \"b\"".into()
        }));
        assert!(pieces.contains(&Piece::Literal(11)));
        assert_eq!(
            render(&pieces, |_, _| None),
            "* 1 FETCH (UID 7 ENVELOPE (\"a \\\"b\\\"\" NIL) BODY[] {11 bytes})"
        );
        assert_eq!(atoms(&pieces)[..3], ["*", "1", "FETCH"]);
    }

    #[test]
    fn announcements_only_before_literals() {
        assert_eq!(strip_announcement("A1 APPEND x {12+}\r\n"), "A1 APPEND x ");
        assert_eq!(strip_announcement("A1 OK {x}"), "A1 OK {x}");
        let parts = [Part::Text("* OK {5}")];
        assert_eq!(render(&pieces(&parts), |_, _| None), "* OK {5}");
        let unterminated = [Part::Text("A1 LOGIN \"ab")];
        assert!(pieces(&unterminated).iter().any(|p| matches!(
            p,
            Piece::Quoted { value, .. } if value == "ab"
        )));
    }
}
