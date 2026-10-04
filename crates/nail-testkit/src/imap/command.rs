//! Parsing of client commands: tokens, sequence sets and fetch items.

/// One argument token of a client command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    /// An atom (may contain `[...]` sections, e.g. `BODY.PEEK[HEADER]`).
    Atom(String),
    /// A quoted string or literal.
    Str(Vec<u8>),
    /// A parenthesized list.
    List(Vec<Token>),
}

impl Token {
    /// The token as text: atoms and strings (lossy UTF-8); `None` for lists.
    pub(crate) fn as_text(&self) -> Option<String> {
        match self {
            Token::Atom(atom) => Some(atom.clone()),
            Token::Str(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            Token::List(_) => None,
        }
    }
}

/// Splits a complete command (literals inlined as `{n}\r\n<bytes>`) into tokens.
pub(crate) fn tokenize(input: &[u8]) -> Result<Vec<Token>, String> {
    let mut pos = 0;
    let tokens = parse_sequence(input, &mut pos, false)?;
    if pos < input.len() {
        return Err("unbalanced parenthesis".to_owned());
    }
    Ok(tokens)
}

fn parse_sequence(input: &[u8], pos: &mut usize, in_list: bool) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    loop {
        while *pos < input.len() && input[*pos] == b' ' {
            *pos += 1;
        }
        let Some(&byte) = input.get(*pos) else {
            return if in_list {
                Err("unterminated list".to_owned())
            } else {
                Ok(tokens)
            };
        };
        match byte {
            b'(' => {
                *pos += 1;
                tokens.push(Token::List(parse_sequence(input, pos, true)?));
            }
            b')' => {
                if !in_list {
                    return Ok(tokens);
                }
                *pos += 1;
                return Ok(tokens);
            }
            b'"' => tokens.push(Token::Str(parse_quoted(input, pos)?)),
            b'{' => tokens.push(Token::Str(parse_literal(input, pos)?)),
            _ => tokens.push(Token::Atom(parse_atom(input, pos))),
        }
    }
}

fn parse_quoted(input: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    *pos += 1; // opening quote
    let mut out = Vec::new();
    while let Some(&byte) = input.get(*pos) {
        *pos += 1;
        match byte {
            b'"' => return Ok(out),
            b'\\' => {
                let escaped = *input.get(*pos).ok_or("unterminated escape")?;
                *pos += 1;
                out.push(escaped);
            }
            _ => out.push(byte),
        }
    }
    Err("unterminated quoted string".to_owned())
}

fn parse_literal(input: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    let close = input[*pos..]
        .iter()
        .position(|&b| b == b'}')
        .ok_or("unterminated literal length")?;
    let spec = std::str::from_utf8(&input[*pos + 1..*pos + close]).map_err(|e| e.to_string())?;
    let len: usize = spec
        .trim_end_matches('+')
        .parse()
        .map_err(|_| format!("bad literal length {spec:?}"))?;
    *pos += close + 1;
    if input.get(*pos..*pos + 2) != Some(b"\r\n") {
        return Err("literal length not followed by CRLF".to_owned());
    }
    *pos += 2;
    let data = input
        .get(*pos..*pos + len)
        .ok_or("literal shorter than announced")?;
    *pos += len;
    Ok(data.to_vec())
}

fn parse_atom(input: &[u8], pos: &mut usize) -> String {
    let start = *pos;
    let mut depth = 0u32;
    while let Some(&byte) = input.get(*pos) {
        match byte {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b' ' | b'(' | b')' if depth == 0 => break,
            _ => {}
        }
        *pos += 1;
    }
    String::from_utf8_lossy(&input[start..*pos]).into_owned()
}

/// If a command line ends with a literal announcement `{n}` / `{n+}`, returns
/// `(n, synchronizing)`.
pub(crate) fn trailing_literal(line: &[u8]) -> Option<(usize, bool)> {
    let line = line.strip_suffix(b"}")?;
    let open = line.iter().rposition(|&b| b == b'{')?;
    let spec = std::str::from_utf8(&line[open + 1..]).ok()?;
    let (digits, sync) = match spec.strip_suffix('+') {
        Some(digits) => (digits, false),
        None => (spec, true),
    };
    Some((digits.parse().ok()?, sync))
}

/// A parsed IMAP sequence set (`1:*`, `3,5:7`, `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequenceSet(Vec<(SeqBound, SeqBound)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeqBound {
    Num(u32),
    Star,
}

impl SequenceSet {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut ranges = Vec::new();
        for part in text.split(',') {
            let (a, b) = if let Some((a, b)) = part.split_once(':') {
                (parse_bound(a)?, parse_bound(b)?)
            } else {
                let bound = parse_bound(part)?;
                (bound, bound)
            };
            ranges.push((a, b));
        }
        Some(Self(ranges))
    }

    /// Whether `value` is in the set, where `*` stands for `max` (the largest
    /// UID or sequence number in use). As in RFC 3501, `n:*` with `n > max`
    /// still contains `max`.
    pub(crate) fn contains(&self, value: u32, max: u32) -> bool {
        if max == 0 {
            // An empty mailbox: `*` matches nothing.
            return self.0.iter().any(|&(a, b)| match (a, b) {
                (SeqBound::Num(a), SeqBound::Num(b)) => (a.min(b)..=a.max(b)).contains(&value),
                _ => false,
            });
        }
        self.0.iter().any(|&(a, b)| {
            let resolve = |bound| match bound {
                SeqBound::Num(n) => n,
                SeqBound::Star => max,
            };
            let (a, b) = (resolve(a), resolve(b));
            (a.min(b)..=a.max(b)).contains(&value)
        })
    }
}

fn parse_bound(text: &str) -> Option<SeqBound> {
    if text == "*" {
        return Some(SeqBound::Star);
    }
    match text.parse::<u32>() {
        Ok(0) | Err(_) => None,
        Ok(n) => Some(SeqBound::Num(n)),
    }
}

/// A requested fetch data item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchItem {
    Uid,
    Flags,
    InternalDate,
    Size,
    Envelope,
    /// `MODSEQ` (RFC 7162).
    ModSeq,
    /// A body section; `peek` avoids setting `\Seen`.
    Body {
        section: Section,
        peek: bool,
    },
    /// `RFC822`, `RFC822.HEADER`, `RFC822.TEXT` (named differently in the response).
    Rfc822(Section),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Full,
    Header,
    Text,
}

/// Parses the fetch attribute argument (an atom, macro or list).
pub(crate) fn parse_fetch_items(token: &Token) -> Result<Vec<FetchItem>, String> {
    let atoms: Vec<String> = match token {
        Token::Atom(atom) => match atom.to_ascii_uppercase().as_str() {
            // `FULL` adds `BODY` (the structure), which the mock does not
            // serve; it answers like `ALL`.
            "ALL" | "FULL" => {
                return Ok(vec![
                    FetchItem::Flags,
                    FetchItem::InternalDate,
                    FetchItem::Size,
                    FetchItem::Envelope,
                ]);
            }
            "FAST" => {
                return Ok(vec![
                    FetchItem::Flags,
                    FetchItem::InternalDate,
                    FetchItem::Size,
                ]);
            }
            _ => vec![atom.clone()],
        },
        Token::List(items) => items
            .iter()
            .map(|item| match item {
                Token::Atom(atom) => Ok(atom.clone()),
                _ => Err("fetch items must be atoms".to_owned()),
            })
            .collect::<Result<_, _>>()?,
        Token::Str(_) => return Err("fetch items must be atoms".to_owned()),
    };
    atoms.iter().map(|atom| parse_fetch_atom(atom)).collect()
}

fn parse_fetch_atom(atom: &str) -> Result<FetchItem, String> {
    let upper = atom.to_ascii_uppercase();
    let item = match upper.as_str() {
        "UID" => FetchItem::Uid,
        "FLAGS" => FetchItem::Flags,
        "INTERNALDATE" => FetchItem::InternalDate,
        "RFC822.SIZE" => FetchItem::Size,
        "ENVELOPE" => FetchItem::Envelope,
        "MODSEQ" => FetchItem::ModSeq,
        "RFC822" => FetchItem::Rfc822(Section::Full),
        "RFC822.HEADER" => FetchItem::Rfc822(Section::Header),
        "RFC822.TEXT" => FetchItem::Rfc822(Section::Text),
        _ => {
            let (peek, rest) = if let Some(rest) = upper.strip_prefix("BODY.PEEK[") {
                (true, rest)
            } else if let Some(rest) = upper.strip_prefix("BODY[") {
                (false, rest)
            } else {
                return Err(format!("unsupported fetch item {atom}"));
            };
            let section = match rest {
                "]" => Section::Full,
                "HEADER]" => Section::Header,
                "TEXT]" => Section::Text,
                _ => return Err(format!("unsupported body section {atom}")),
            };
            FetchItem::Body { section, peek }
        }
    };
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(s: &str) -> Token {
        Token::Atom(s.to_owned())
    }

    #[test]
    fn tokenizes_strings_lists_and_literals() {
        let tokens = tokenize(b"a1 LOGIN \"sam\\\"q\" {3}\r\nabc").unwrap();
        assert_eq!(
            tokens,
            vec![
                atom("a1"),
                atom("LOGIN"),
                Token::Str(b"sam\"q".to_vec()),
                Token::Str(b"abc".to_vec())
            ]
        );
        let tokens =
            tokenize(b"a2 UID FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (FROM TO)] FLAGS)").unwrap();
        assert_eq!(
            tokens[4],
            Token::List(vec![
                atom("UID"),
                atom("BODY.PEEK[HEADER.FIELDS (FROM TO)]"),
                atom("FLAGS")
            ])
        );
        assert!(tokenize(b"a3 X (unterminated").is_err());
        assert!(tokenize(b"a4 X \"open").is_err());
        assert!(tokenize(b"a5 X {10}\r\nshort").is_err());
    }

    #[test]
    fn detects_trailing_literals() {
        assert_eq!(trailing_literal(b"a LOGIN {4}"), Some((4, true)));
        assert_eq!(trailing_literal(b"a LOGIN {4+}"), Some((4, false)));
        assert_eq!(trailing_literal(b"a NOOP"), None);
        assert_eq!(trailing_literal(b"a X {abc}"), None);
    }

    #[test]
    fn sequence_sets() {
        let set = SequenceSet::parse("1:3,7,10:*").unwrap();
        assert!(set.contains(2, 12));
        assert!(!set.contains(5, 12));
        assert!(set.contains(7, 12));
        assert!(set.contains(12, 12));
        // `n:*` beyond the last UID still matches the last message.
        let tail = SequenceSet::parse("20:*").unwrap();
        assert!(tail.contains(12, 12));
        assert!(!tail.contains(11, 12));
        assert!(SequenceSet::parse("0").is_none());
        assert!(SequenceSet::parse("a:b").is_none());
    }

    #[test]
    fn fetch_items() {
        let items = parse_fetch_items(
            &tokenize(b"(UID FLAGS RFC822.SIZE ENVELOPE BODY.PEEK[])").unwrap()[0],
        )
        .unwrap();
        assert_eq!(
            items,
            vec![
                FetchItem::Uid,
                FetchItem::Flags,
                FetchItem::Size,
                FetchItem::Envelope,
                FetchItem::Body {
                    section: Section::Full,
                    peek: true
                }
            ]
        );
        assert_eq!(parse_fetch_items(&atom("fast")).unwrap().len(), 3);
        assert!(parse_fetch_items(&atom("BODY[1.2]")).is_err());
        assert!(parse_fetch_items(&atom("BODYSTRUCTURE")).is_err());
    }
}
