//! The search query language (design.md, "Threading and search").
//!
//! ```text
//! query    := or-expr?                       (empty = every message)
//! or-expr  := and-expr ("OR" and-expr)*
//! and-expr := unary (["AND"] unary)*         (juxtaposition = AND)
//! unary    := ("NOT" | "-") unary | "(" or-expr ")" | term
//! term     := word | "quoted text" | field ":" value | "size" cmp size
//! value    := word | "quoted text"
//! cmp      := ">" | ">=" | "<" | "<="
//! ```
//!
//! * Operators are upper case (`and` / `or` / `not` are words). `AND` binds
//!   tighter than `OR`.
//! * Fields (case-insensitive): `from:` `to:` `cc:` `subject:` `body:`
//!   `filename:` (attachment names), `has:attachment`, `is:` (`unread`,
//!   `read`, `flagged`, `unflagged`, `answered`, `unanswered`, `draft`;
//!   aliases `seen` = read, `starred` = flagged, `replied` = answered),
//!   `tag:`, `account:`, `folder:`, `category:` (Gmail inbox category, M3b),
//!   `before:` / `after:` (a day
//!   `2026-09-01` or a relative age `7d`: `h` hours, `d` days, `w` weeks,
//!   `m` months of 30 days, `y` years of 365 days), and `size>` / `size>=`
//!   / `size<` / `size<=` with an optional unit (`k`, `m`, `g`, 1024-based;
//!   a trailing `b` is allowed: `10kb`).
//! * Quoted text allows `\"` and `\\` escapes.
//! * A word `prefix:rest` whose prefix is letters only must name a known
//!   field (quote it to search the text, e.g. `"http://x"`).
//!
//! [`parse_query`] reports the first error as a [`SearchError`] with UTF-16
//! offsets; the AST keeps byte offsets ([`Span`]).

use std::fmt;

use nail_api::search::{SearchError, SearchErrorKind, utf16_offset};

/// A byte range in the query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// The empty query: every message.
    All,
    /// One search term.
    Term(Term),
    /// Messages not matching the inner query.
    Not(Box<Query>),
    /// Messages matching all (at least two) sub-queries.
    And(Vec<Query>),
    /// Messages matching any (at least two) sub-queries.
    Or(Vec<Query>),
}

/// A search term and where it is in the query text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// What it matches.
    pub kind: TermKind,
    /// Its bytes in the query text (including field name and quotes).
    pub span: Span,
}

/// The kinds of search terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermKind {
    /// Full text in a field (or all text fields).
    Text {
        /// Where to look.
        field: TextField,
        /// The text; matched as a phrase when it has several words.
        value: String,
        /// Whether it was quoted in the query.
        quoted: bool,
    },
    /// A flag state (`is:`).
    Is(FlagTerm),
    /// `has:attachment`.
    HasAttachment,
    /// `tag:` (case-insensitive).
    Tag(Value),
    /// `category:` — a Gmail inbox category (`primary`, `social`,
    /// `promotions`, `updates`, `forums`; case-insensitive; M3b).
    Category(Value),
    /// `account:` — an account's id, name or address, or a virtual account.
    Account(Value),
    /// `folder:` — a folder's name, path or role.
    Folder(Value),
    /// `before:` — dated strictly before.
    Before(DateValue),
    /// `after:` — dated on or after.
    After(DateValue),
    /// `size>` and friends.
    Size(SizeOp, u64),
}

/// A (possibly quoted) field value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// The text.
    pub text: String,
    /// Whether it was quoted.
    pub quoted: bool,
}

/// Text fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    /// All text fields (free text).
    Any,
    /// `from:` sender names and addresses.
    From,
    /// `to:` recipients.
    To,
    /// `cc:` copy recipients.
    Cc,
    /// `subject:`.
    Subject,
    /// `body:` decoded body text.
    Body,
    /// `filename:` attachment names.
    Filename,
}

impl TextField {
    fn keyword(self) -> Option<&'static str> {
        Some(match self {
            Self::Any => return None,
            Self::From => "from",
            Self::To => "to",
            Self::Cc => "cc",
            Self::Subject => "subject",
            Self::Body => "body",
            Self::Filename => "filename",
        })
    }
}

/// Flag states for `is:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlagTerm {
    /// Without `\Seen`.
    Unread,
    /// With `\Seen`.
    Read,
    /// With `\Flagged`.
    Flagged,
    /// Without `\Flagged`.
    Unflagged,
    /// With `\Answered`.
    Answered,
    /// Without `\Answered`.
    Unanswered,
    /// With `\Draft`.
    Draft,
}

impl FlagTerm {
    const ALL: [(&'static str, FlagTerm); 10] = [
        ("unread", Self::Unread),
        ("read", Self::Read),
        ("seen", Self::Read),
        ("flagged", Self::Flagged),
        ("starred", Self::Flagged),
        ("unflagged", Self::Unflagged),
        ("answered", Self::Answered),
        ("replied", Self::Answered),
        ("unanswered", Self::Unanswered),
        ("draft", Self::Draft),
    ];

    fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(s))
            .map(|(_, f)| *f)
    }

    /// The canonical keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Read => "read",
            Self::Flagged => "flagged",
            Self::Unflagged => "unflagged",
            Self::Answered => "answered",
            Self::Unanswered => "unanswered",
            Self::Draft => "draft",
        }
    }
}

/// A date in `before:` / `after:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateValue {
    /// A calendar day (local midnight).
    Day {
        /// Year.
        year: i32,
        /// Month 1–12.
        month: u8,
        /// Day 1–31 (valid for the month).
        day: u8,
    },
    /// An age relative to now.
    Ago {
        /// How many units.
        amount: u32,
        /// The unit.
        unit: AgeUnit,
    },
}

/// Units of relative dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgeUnit {
    /// `h`
    Hours,
    /// `d`
    Days,
    /// `w`
    Weeks,
    /// `m` (30 days)
    Months,
    /// `y` (365 days)
    Years,
}

impl AgeUnit {
    /// Length in seconds.
    pub fn seconds(self) -> i64 {
        match self {
            Self::Hours => 3_600,
            Self::Days => 86_400,
            Self::Weeks => 7 * 86_400,
            Self::Months => 30 * 86_400,
            Self::Years => 365 * 86_400,
        }
    }

    fn suffix(self) -> char {
        match self {
            Self::Hours => 'h',
            Self::Days => 'd',
            Self::Weeks => 'w',
            Self::Months => 'm',
            Self::Years => 'y',
        }
    }
}

/// Size comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeOp {
    /// `>`
    Greater,
    /// `>=`
    GreaterOrEqual,
    /// `<`
    Less,
    /// `<=`
    LessOrEqual,
}

impl SizeOp {
    fn symbol(self) -> &'static str {
        match self {
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
        }
    }
}

// ---------------------------------------------------------------------------
// Errors

/// A parse error with byte offsets (converted to UTF-16 at the API edge).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParseError {
    kind: SearchErrorKind,
    message: String,
    span: Span,
}

fn err<T>(kind: SearchErrorKind, span: Span, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        kind,
        message: message.into(),
        span,
    })
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    And,
    Or,
    Not,
    Minus,
    Term(Term),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    span: Span,
}

/// Characters that end a bare word.
fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '"')
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    /// Reads a quoted string starting at `self.pos` (on the `"`).
    fn quoted(&mut self) -> Result<(String, Span), ParseError> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        let mut chars = self.text[self.pos..].char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.pos += i + 1;
                    return Ok((out, Span::new(start, self.pos)));
                }
                '\\' => match chars.next() {
                    Some((_, e @ ('"' | '\\'))) => out.push(e),
                    Some((_, other)) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => break,
                },
                c => out.push(c),
            }
        }
        err(
            SearchErrorKind::UnterminatedQuote,
            Span::new(start, self.text.len()),
            "missing closing quote",
        )
    }

    /// Reads a bare word; returns its text and span.
    fn word(&mut self) -> (&'a str, Span) {
        let start = self.pos;
        let len = self.text[start..]
            .char_indices()
            .find(|(_, c)| ends_word(*c))
            .map_or(self.text.len() - start, |(i, _)| i);
        self.pos = start + len;
        (&self.text[start..self.pos], Span::new(start, self.pos))
    }

    fn tokens(mut self) -> Result<Vec<Token>, ParseError> {
        let mut out = Vec::new();
        loop {
            let rest = &self.text[self.pos..];
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();
            let Some(c) = self.peek() else { break };
            let start = self.pos;
            let simple = match c {
                '(' => Some(Tok::LParen),
                ')' => Some(Tok::RParen),
                _ => None,
            };
            if let Some(tok) = simple {
                self.pos += 1;
                out.push(Token {
                    tok,
                    span: Span::new(start, self.pos),
                });
                continue;
            }
            if c == '"' {
                let (value, span) = self.quoted()?;
                out.push(Token {
                    tok: Tok::Term(Term {
                        kind: TermKind::Text {
                            field: TextField::Any,
                            value,
                            quoted: true,
                        },
                        span,
                    }),
                    span,
                });
                continue;
            }
            // `-` directly before something is negation.
            if c == '-'
                && self.text[start + 1..]
                    .chars()
                    .next()
                    .is_some_and(|n| !n.is_whitespace() && n != ')')
            {
                self.pos += 1;
                out.push(Token {
                    tok: Tok::Minus,
                    span: Span::new(start, self.pos),
                });
                continue;
            }
            let (word, span) = self.word();
            let tok = match word {
                "AND" => Tok::And,
                "OR" => Tok::Or,
                "NOT" => Tok::Not,
                _ => Tok::Term(self.term(word, span)?),
            };
            let span = match &tok {
                Tok::Term(t) => t.span,
                _ => span,
            };
            out.push(Token { tok, span });
        }
        Ok(out)
    }

    /// Interprets a bare word (and, after `field:`, a quoted value).
    fn term(&mut self, word: &'a str, span: Span) -> Result<Term, ParseError> {
        // size>10k, size>=1M, size<5k, size<=5k
        if word.len() > 4 && word.is_char_boundary(4) && word[..4].eq_ignore_ascii_case("size") {
            let rest = &word[4..];
            let op = [
                (">=", SizeOp::GreaterOrEqual),
                ("<=", SizeOp::LessOrEqual),
                (">", SizeOp::Greater),
                ("<", SizeOp::Less),
            ]
            .into_iter()
            .find_map(|(symbol, op)| rest.strip_prefix(symbol).map(|r| (op, r)));
            if let Some((op, value)) = op {
                let value_span = Span::new(span.end - value.len(), span.end);
                if value.is_empty() {
                    return self.missing_value(span, "size");
                }
                let bytes = parse_size(value).ok_or_else(|| ParseError {
                    kind: SearchErrorKind::InvalidValue,
                    message: format!("invalid size {value:?} (examples: 500k, 2m)"),
                    span: value_span,
                })?;
                return Ok(Term {
                    kind: TermKind::Size(op, bytes),
                    span,
                });
            }
        }

        let Some(colon) = word.find(':') else {
            return Ok(text_term(TextField::Any, word.to_owned(), false, span));
        };
        let name = &word[..colon];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphabetic()) {
            return Ok(text_term(TextField::Any, word.to_owned(), false, span));
        }
        let field_span = Span::new(span.start, span.start + colon + 1);
        let lower = name.to_ascii_lowercase();
        let known = matches!(
            lower.as_str(),
            "from"
                | "to"
                | "cc"
                | "subject"
                | "body"
                | "filename"
                | "has"
                | "is"
                | "tag"
                | "category"
                | "account"
                | "folder"
                | "before"
                | "after"
                | "size"
        );
        if !known {
            return err(
                SearchErrorKind::UnknownField,
                field_span,
                format!("unknown field {name:?}"),
            );
        }
        // The value: the rest of the word, or a quoted string right after
        // the colon.
        let mut value = word[colon + 1..].to_owned();
        let mut quoted = false;
        let mut value_span = Span::new(field_span.end, span.end);
        let mut span = span;
        if value.is_empty() && self.peek() == Some('"') {
            let (q, qspan) = self.quoted()?;
            value = q;
            quoted = true;
            value_span = qspan;
            span.end = qspan.end;
        }
        if value.is_empty() && !quoted {
            return self.missing_value(span, &lower);
        }
        let invalid = |message: String| ParseError {
            kind: SearchErrorKind::InvalidValue,
            message,
            span: value_span,
        };
        let v = || Value {
            text: value.clone(),
            quoted,
        };
        let kind = match lower.as_str() {
            "from" => text_kind(TextField::From, &value, quoted),
            "to" => text_kind(TextField::To, &value, quoted),
            "cc" => text_kind(TextField::Cc, &value, quoted),
            "subject" => text_kind(TextField::Subject, &value, quoted),
            "body" => text_kind(TextField::Body, &value, quoted),
            "filename" => text_kind(TextField::Filename, &value, quoted),
            "has" => {
                if value.eq_ignore_ascii_case("attachment")
                    || value.eq_ignore_ascii_case("attachments")
                {
                    TermKind::HasAttachment
                } else {
                    return Err(invalid(format!(
                        "has:{value} is not supported (use has:attachment)"
                    )));
                }
            }
            "is" => TermKind::Is(
                FlagTerm::parse(&value)
                    .ok_or_else(|| invalid(format!("unknown flag {value:?}")))?,
            ),
            "tag" => TermKind::Tag(v()),
            "category" => TermKind::Category(v()),
            "account" => TermKind::Account(v()),
            "folder" => TermKind::Folder(v()),
            "before" | "after" => {
                let date = parse_date(&value).ok_or_else(|| {
                    invalid(format!("invalid date {value:?} (examples: 2026-09-01, 7d)"))
                })?;
                if lower == "before" {
                    TermKind::Before(date)
                } else {
                    TermKind::After(date)
                }
            }
            // "size:" without a comparison.
            _ => return Err(invalid("size needs a comparison, e.g. size>1m".into())),
        };
        Ok(Term { kind, span })
    }

    fn missing_value<T>(&self, span: Span, field: &str) -> Result<T, ParseError> {
        if span.end == self.text.len() {
            err(
                SearchErrorKind::UnexpectedEnd,
                Span::new(span.end, span.end),
                format!("{field} needs a value"),
            )
        } else {
            err(
                SearchErrorKind::InvalidValue,
                span,
                format!("{field} needs a value"),
            )
        }
    }
}

fn text_kind(field: TextField, value: &str, quoted: bool) -> TermKind {
    TermKind::Text {
        field,
        value: value.to_owned(),
        quoted,
    }
}

fn text_term(field: TextField, value: String, quoted: bool, span: Span) -> Term {
    Term {
        kind: TermKind::Text {
            field,
            value,
            quoted,
        },
        span,
    }
}

/// `500`, `10k`, `10kb`, `2M`, `1g` (1024-based).
fn parse_size(s: &str) -> Option<u64> {
    let lower = s.to_ascii_lowercase();
    let digits_end = lower
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(lower.len());
    if digits_end == 0 {
        return None;
    }
    let n: u64 = lower[..digits_end].parse().ok()?;
    let unit = match &lower[digits_end..] {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(unit)
}

/// `2026-09-01` or `7d` / `12h` / `2w` / `3m` / `1y`.
fn parse_date(s: &str) -> Option<DateValue> {
    if let Some(unit) = s.chars().last().and_then(|u| match u.to_ascii_lowercase() {
        'h' => Some(AgeUnit::Hours),
        'd' => Some(AgeUnit::Days),
        'w' => Some(AgeUnit::Weeks),
        'm' => Some(AgeUnit::Months),
        'y' => Some(AgeUnit::Years),
        _ => None,
    }) {
        let digits = &s[..s.len() - 1];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            let amount: u32 = digits.parse().ok()?;
            // Keep the seconds representable.
            if i64::from(amount) * unit.seconds() > 1_000 * 365 * 86_400 {
                return None;
            }
            return Some(DateValue::Ago { amount, unit });
        }
        return None;
    }
    let mut parts = s.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    if ![y, m, d]
        .iter()
        .all(|p| p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day): (i32, u8, u8) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    (crate::date::is_valid_day(year, month, day)).then_some(DateValue::Day { year, month, day })
}

// ---------------------------------------------------------------------------
// Parser

/// Longest query text accepted, in bytes.
pub const MAX_QUERY_BYTES: usize = 4096;
/// Deepest nesting of parentheses and `NOT` / `-` accepted. The parser
/// recurses once per level, so this bounds its stack use: a pasted
/// `((((…` must be an error, not a stack overflow that aborts the core.
pub const MAX_NESTING: usize = 64;

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    len: usize,
    /// Open parentheses.
    depth: usize,
    /// Open parentheses and pending negations (recursion depth).
    nesting: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn end_span(&self) -> Span {
        Span::new(self.len, self.len)
    }

    fn starts_unary(tok: &Tok) -> bool {
        matches!(tok, Tok::Term(_) | Tok::LParen | Tok::Not | Tok::Minus)
    }

    fn or_expr(&mut self) -> Result<Query, ParseError> {
        let mut items = vec![self.and_expr()?];
        while let Some(Token { tok: Tok::Or, .. }) = self.peek() {
            self.pos += 1;
            items.push(self.and_expr()?);
        }
        Ok(if items.len() == 1 {
            items.pop().expect("one item")
        } else {
            Query::Or(flatten(items, true))
        })
    }

    fn and_expr(&mut self) -> Result<Query, ParseError> {
        let mut items = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Token { tok: Tok::And, .. }) => {
                    self.pos += 1;
                    items.push(self.unary()?);
                }
                Some(t) if Self::starts_unary(&t.tok) => items.push(self.unary()?),
                _ => break,
            }
        }
        Ok(if items.len() == 1 {
            items.pop().expect("one item")
        } else {
            Query::And(flatten(items, false))
        })
    }

    fn unary(&mut self) -> Result<Query, ParseError> {
        let Some(token) = self.peek().cloned() else {
            return err(
                SearchErrorKind::UnexpectedEnd,
                self.end_span(),
                "the query ends too early",
            );
        };
        self.pos += 1;
        match token.tok {
            Tok::Not | Tok::Minus | Tok::LParen => {
                if self.nesting >= MAX_NESTING {
                    return err(
                        SearchErrorKind::TooComplex,
                        token.span,
                        format!("nested more than {MAX_NESTING} levels deep"),
                    );
                }
                self.nesting += 1;
                let result = self.nested(token);
                self.nesting -= 1;
                result
            }
            Tok::Term(term) => Ok(Query::Term(term)),
            Tok::RParen if self.depth == 0 => err(
                SearchErrorKind::UnbalancedParenthesis,
                token.span,
                "closing parenthesis without an opening one",
            ),
            Tok::RParen => err(
                SearchErrorKind::UnexpectedToken,
                token.span,
                "expected a search term before the closing parenthesis",
            ),
            Tok::And | Tok::Or => err(
                SearchErrorKind::UnexpectedToken,
                token.span,
                "expected a search term before this operator",
            ),
        }
    }

    /// A negation or a parenthesized group (one nesting level).
    fn nested(&mut self, token: Token) -> Result<Query, ParseError> {
        match token.tok {
            Tok::Not | Tok::Minus => Ok(Query::Not(Box::new(self.unary()?))),
            Tok::Term(term) => Ok(Query::Term(term)),
            Tok::LParen => {
                if let Some(Token {
                    tok: Tok::RParen,
                    span,
                }) = self.peek()
                {
                    return err(SearchErrorKind::UnexpectedToken, *span, "empty parentheses");
                }
                if self.peek().is_none() {
                    return err(
                        SearchErrorKind::UnbalancedParenthesis,
                        token.span,
                        "missing closing parenthesis",
                    );
                }
                self.depth += 1;
                let inner = self.or_expr()?;
                self.depth -= 1;
                match self.peek() {
                    Some(Token {
                        tok: Tok::RParen, ..
                    }) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    None => err(
                        SearchErrorKind::UnbalancedParenthesis,
                        token.span,
                        "missing closing parenthesis",
                    ),
                    Some(other) => err(
                        SearchErrorKind::UnexpectedToken,
                        other.span,
                        "unexpected token",
                    ),
                }
            }
            _ => err(
                SearchErrorKind::UnexpectedToken,
                token.span,
                "unexpected token",
            ),
        }
    }
}

/// Flattens nested ANDs (or ORs) into one level.
fn flatten(items: Vec<Query>, or: bool) -> Vec<Query> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Query::Or(inner) if or => out.extend(inner),
            Query::And(inner) if !or => out.extend(inner),
            other => out.push(other),
        }
    }
    out
}

fn parse_bytes(text: &str) -> Result<Query, ParseError> {
    if text.len() > MAX_QUERY_BYTES {
        return err(
            SearchErrorKind::TooComplex,
            Span::new(0, text.len()),
            format!("longer than {MAX_QUERY_BYTES} bytes"),
        );
    }
    let tokens = Lexer { text, pos: 0 }.tokens()?;
    if tokens.is_empty() {
        return Ok(Query::All);
    }
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        len: text.len(),
        depth: 0,
        nesting: 0,
    };
    let query = parser.or_expr()?;
    match parser.peek() {
        None => Ok(query),
        Some(Token {
            tok: Tok::RParen,
            span,
        }) => err(
            SearchErrorKind::UnbalancedParenthesis,
            *span,
            "closing parenthesis without an opening one",
        ),
        Some(other) => err(
            SearchErrorKind::UnexpectedToken,
            other.span,
            "unexpected token",
        ),
    }
}

/// Parses a query. Errors carry UTF-16 offsets into `text`.
pub fn parse_query(text: &str) -> Result<Query, SearchError> {
    parse_bytes(text).map_err(|e| SearchError {
        kind: e.kind,
        message: e.message,
        start: utf16_offset(text, e.span.start),
        end: utf16_offset(text, e.span.end),
    })
}

// ---------------------------------------------------------------------------
// Canonical text form

/// Whether `s` can be written as a bare word and parse back to the same
/// plain text term.
fn is_bare_word(s: &str) -> bool {
    // A lone `-` is text; `-x` would be a negation.
    if s.is_empty() || matches!(s, "AND" | "OR" | "NOT") || (s.starts_with('-') && s != "-") {
        return false;
    }
    if s.chars().any(|c| ends_word(c) || c == '\\') {
        return false;
    }
    if let Some(colon) = s.find(':') {
        let name = &s[..colon];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
    }
    // `size>…` would parse as a size term.
    !(s.len() > 4
        && s.is_char_boundary(4)
        && s[..4].eq_ignore_ascii_case("size")
        && s[4..].starts_with(['<', '>']))
}

fn write_quoted(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            f.write_str("\\")?;
        }
        write!(f, "{c}")?;
    }
    f.write_str("\"")
}

/// A field value: bare when possible (and not quoted originally).
fn write_value(f: &mut fmt::Formatter<'_>, s: &str, quoted: bool) -> fmt::Result {
    let bare = !quoted && !s.is_empty() && !s.chars().any(|c| ends_word(c) || c == '\\');
    if bare {
        f.write_str(s)
    } else {
        write_quoted(f, s)
    }
}

impl fmt::Display for DateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Day { year, month, day } => write!(f, "{year:04}-{month:02}-{day:02}"),
            Self::Ago { amount, unit } => write!(f, "{amount}{}", unit.suffix()),
        }
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TermKind::Text {
                field,
                value,
                quoted,
            } => match field.keyword() {
                None if !quoted && is_bare_word(value) => f.write_str(value),
                None => write_quoted(f, value),
                Some(k) => {
                    write!(f, "{k}:")?;
                    write_value(f, value, *quoted)
                }
            },
            TermKind::Is(flag) => write!(f, "is:{}", flag.keyword()),
            TermKind::HasAttachment => f.write_str("has:attachment"),
            TermKind::Tag(v) => {
                f.write_str("tag:")?;
                write_value(f, &v.text, v.quoted)
            }
            TermKind::Category(v) => {
                f.write_str("category:")?;
                write_value(f, &v.text, v.quoted)
            }
            TermKind::Account(v) => {
                f.write_str("account:")?;
                write_value(f, &v.text, v.quoted)
            }
            TermKind::Folder(v) => {
                f.write_str("folder:")?;
                write_value(f, &v.text, v.quoted)
            }
            TermKind::Before(d) => write!(f, "before:{d}"),
            TermKind::After(d) => write!(f, "after:{d}"),
            TermKind::Size(op, n) => write!(f, "size{}{n}", op.symbol()),
        }
    }
}

impl fmt::Display for Query {
    /// The canonical form: parses back to an equal query (ignoring spans).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Query::All => Ok(()),
            Query::Term(t) => t.fmt(f),
            Query::Not(inner) => match inner.as_ref() {
                Query::And(_) | Query::Or(_) => write!(f, "NOT ({inner})"),
                _ => write!(f, "NOT {inner}"),
            },
            Query::And(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    match item {
                        Query::Or(_) | Query::And(_) => write!(f, "({item})")?,
                        _ => write!(f, "{item}")?,
                    }
                }
                Ok(())
            }
            Query::Or(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" OR ")?;
                    }
                    match item {
                        Query::Or(_) => write!(f, "({item})")?,
                        _ => write!(f, "{item}")?,
                    }
                }
                Ok(())
            }
        }
    }
}

impl Query {
    /// The same query with every span reset (for comparing ASTs).
    pub fn without_spans(&self) -> Query {
        match self {
            Query::All => Query::All,
            Query::Term(t) => Query::Term(Term {
                kind: t.kind.clone(),
                span: Span::default(),
            }),
            Query::Not(q) => Query::Not(Box::new(q.without_spans())),
            Query::And(v) => Query::And(v.iter().map(Query::without_spans).collect()),
            Query::Or(v) => Query::Or(v.iter().map(Query::without_spans).collect()),
        }
    }

    /// Every term, in query order.
    pub fn terms(&self) -> Vec<&Term> {
        let mut out = Vec::new();
        fn walk<'q>(q: &'q Query, out: &mut Vec<&'q Term>) {
            match q {
                Query::All => {}
                Query::Term(t) => out.push(t),
                Query::Not(inner) => walk(inner, out),
                Query::And(v) | Query::Or(v) => v.iter().for_each(|q| walk(q, out)),
            }
        }
        walk(self, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Query {
        parse_query(s)
            .unwrap_or_else(|e| panic!("{s:?}: {e:?}"))
            .without_spans()
    }

    fn e(s: &str) -> (SearchErrorKind, u32, u32) {
        let err = parse_query(s).expect_err(s);
        (err.kind, err.start, err.end)
    }

    /// Review M1a: deep nesting must be a parse error, never a stack
    /// overflow (which aborts the whole core).
    #[test]
    fn nesting_and_length_are_bounded() {
        let deep = format!("{}a{}", "(".repeat(1000), ")".repeat(1000));
        assert!(deep.len() < MAX_QUERY_BYTES);
        assert_eq!(e(&deep), (SearchErrorKind::TooComplex, 64, 65));
        assert_eq!(e(&"(".repeat(1000)).0, SearchErrorKind::TooComplex);
        let nots = format!("{}a", "NOT ".repeat(500));
        assert_eq!(e(&nots).0, SearchErrorKind::TooComplex);
        let minus = format!("{}a{}", "-(".repeat(40), ")".repeat(40));
        assert_eq!(e(&minus).0, SearchErrorKind::TooComplex);
        let mixed = format!("{}a{}", "NOT (".repeat(40), ")".repeat(40));
        assert_eq!(e(&mixed).0, SearchErrorKind::TooComplex);
        // 20 000 NOTs: refused for their length before parsing.
        let long = format!("{}a", "NOT ".repeat(20_000));
        assert_eq!(
            e(&long),
            (
                SearchErrorKind::TooComplex,
                0,
                u32::try_from(long.len()).unwrap()
            )
        );
        // The limits themselves are fine.
        let ok = format!("{}a{}", "(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
        assert_eq!(p(&ok), text("a"));
        let ok = format!("{}a", "NOT ".repeat(MAX_NESTING));
        assert!(parse_query(&ok).is_ok());
        assert!(parse_query(&"a ".repeat(MAX_QUERY_BYTES / 2)).is_ok());
        assert_eq!(
            SearchErrorKind::TooComplex.message_id(),
            "search-error-too-complex"
        );
    }

    fn text(v: &str) -> Query {
        Query::Term(Term {
            kind: TermKind::Text {
                field: TextField::Any,
                value: v.into(),
                quoted: false,
            },
            span: Span::default(),
        })
    }

    fn term(kind: TermKind) -> Query {
        Query::Term(Term {
            kind,
            span: Span::default(),
        })
    }

    #[test]
    fn empty_and_whitespace_queries_match_everything() {
        assert_eq!(p(""), Query::All);
        assert_eq!(p("   \t "), Query::All);
    }

    #[test]
    fn free_text_and_implicit_and() {
        assert_eq!(p("hello"), text("hello"));
        assert_eq!(
            p("hello world"),
            Query::And(vec![text("hello"), text("world")])
        );
        assert_eq!(
            p("a AND b c"),
            Query::And(vec![text("a"), text("b"), text("c")])
        );
        assert_eq!(
            p("and or not"),
            Query::And(vec![text("and"), text("or"), text("not")])
        );
    }

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(
            p("a b OR c"),
            Query::Or(vec![Query::And(vec![text("a"), text("b")]), text("c")])
        );
        assert_eq!(
            p("a (b OR c)"),
            Query::And(vec![text("a"), Query::Or(vec![text("b"), text("c")])])
        );
        assert_eq!(p("((a))"), text("a"));
        assert_eq!(
            p("a OR (b OR c)"),
            Query::Or(vec![text("a"), text("b"), text("c")])
        );
    }

    #[test]
    fn negation() {
        assert_eq!(p("NOT a"), Query::Not(Box::new(text("a"))));
        assert_eq!(p("-a"), Query::Not(Box::new(text("a"))));
        assert_eq!(
            p("-(a OR b)"),
            Query::Not(Box::new(Query::Or(vec![text("a"), text("b")])))
        );
        assert_eq!(
            p("NOT NOT a"),
            Query::Not(Box::new(Query::Not(Box::new(text("a")))))
        );
        // A lone dash, or one inside a word, is text.
        assert_eq!(
            p("a - b"),
            Query::And(vec![text("a"), text("-"), text("b")])
        );
        assert_eq!(p("a -"), Query::And(vec![text("a"), text("-")]));
        assert_eq!(p("e-mail"), text("e-mail"));
        assert_eq!(
            p("-is:unread"),
            Query::Not(Box::new(term(TermKind::Is(FlagTerm::Unread))))
        );
    }

    #[test]
    fn fields() {
        assert_eq!(
            p("from:ada@example.org"),
            term(TermKind::Text {
                field: TextField::From,
                value: "ada@example.org".into(),
                quoted: false
            })
        );
        assert_eq!(
            p(r#"Subject:"Quarterly report""#),
            term(TermKind::Text {
                field: TextField::Subject,
                value: "Quarterly report".into(),
                quoted: true
            })
        );
        assert_eq!(p("has:attachment"), term(TermKind::HasAttachment));
        assert_eq!(p("is:STARRED"), term(TermKind::Is(FlagTerm::Flagged)));
        assert_eq!(
            p(r#"folder:"Sent Items""#),
            term(TermKind::Folder(Value {
                text: "Sent Items".into(),
                quoted: true
            }))
        );
        assert_eq!(
            p("tag:work account:personal"),
            Query::And(vec![
                term(TermKind::Tag(Value {
                    text: "work".into(),
                    quoted: false
                })),
                term(TermKind::Account(Value {
                    text: "personal".into(),
                    quoted: false
                })),
            ])
        );
        // Colons after non-letters are text.
        assert_eq!(p("10:30"), text("10:30"));
        assert_eq!(p(":x"), text(":x"));
    }

    #[test]
    fn dates_and_sizes() {
        assert_eq!(
            p("before:2026-09-01"),
            term(TermKind::Before(DateValue::Day {
                year: 2026,
                month: 9,
                day: 1
            }))
        );
        assert_eq!(
            p("after:7d"),
            term(TermKind::After(DateValue::Ago {
                amount: 7,
                unit: AgeUnit::Days
            }))
        );
        assert_eq!(
            p("after:2W"),
            term(TermKind::After(DateValue::Ago {
                amount: 2,
                unit: AgeUnit::Weeks
            }))
        );
        assert_eq!(
            p("size>10k"),
            term(TermKind::Size(SizeOp::Greater, 10 << 10))
        );
        assert_eq!(
            p("SIZE<=2MB"),
            term(TermKind::Size(SizeOp::LessOrEqual, 2 << 20))
        );
        assert_eq!(
            p("size>=500"),
            term(TermKind::Size(SizeOp::GreaterOrEqual, 500))
        );
        assert_eq!(p("size<1g"), term(TermKind::Size(SizeOp::Less, 1 << 30)));
        assert_eq!(p("sizeable"), text("sizeable"));
        // Leap days.
        assert!(parse_query("before:2028-02-29").is_ok());
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(
            p(r#""say \"hi\" \\ there""#),
            term(TermKind::Text {
                field: TextField::Any,
                value: r#"say "hi" \ there"#.into(),
                quoted: true
            })
        );
        assert_eq!(
            p(r#"a"b""#),
            Query::And(vec![
                text("a"),
                term(TermKind::Text {
                    field: TextField::Any,
                    value: "b".into(),
                    quoted: true
                })
            ])
        );
        assert_eq!(
            p(r#""""#),
            term(TermKind::Text {
                field: TextField::Any,
                value: String::new(),
                quoted: true
            })
        );
    }

    #[test]
    fn error_positions() {
        use SearchErrorKind::*;
        assert_eq!(e("AND a"), (UnexpectedToken, 0, 3));
        assert_eq!(e("a AND"), (UnexpectedEnd, 5, 5));
        assert_eq!(e("a OR OR b"), (UnexpectedToken, 5, 7));
        assert_eq!(e("NOT"), (UnexpectedEnd, 3, 3));
        assert_eq!(e("(a OR b"), (UnbalancedParenthesis, 0, 1));
        assert_eq!(e("a) b"), (UnbalancedParenthesis, 1, 2));
        assert_eq!(e("()"), (UnexpectedToken, 1, 2));
        assert_eq!(e("(a OR)"), (UnexpectedToken, 5, 6));
        assert_eq!(e("("), (UnbalancedParenthesis, 0, 1));
        assert_eq!(e(r#"x "abc"#), (UnterminatedQuote, 2, 6));
        assert_eq!(e(r#"subject:"abc"#), (UnterminatedQuote, 8, 12));
        assert_eq!(e("frum:ada"), (UnknownField, 0, 5));
        assert_eq!(e("a http://x"), (UnknownField, 2, 7));
        assert_eq!(e("from:"), (UnexpectedEnd, 5, 5));
        assert_eq!(e("from: ada"), (InvalidValue, 0, 5));
        assert_eq!(e("is:purple"), (InvalidValue, 3, 9));
        assert_eq!(e("has:pdf"), (InvalidValue, 4, 7));
        assert_eq!(e("before:2026-02-30"), (InvalidValue, 7, 17));
        assert_eq!(e("after:2026-9-1"), (InvalidValue, 6, 14));
        assert_eq!(e("after:7x"), (InvalidValue, 6, 8));
        assert_eq!(e("size>10q"), (InvalidValue, 5, 8));
        assert_eq!(e("size>"), (UnexpectedEnd, 5, 5));
        assert_eq!(e("size:10"), (InvalidValue, 5, 7));
        // UTF-16 offsets: the emoji counts as two units.
        assert_eq!(e("😀 frum:x"), (UnknownField, 3, 8));
        assert_eq!(e("ü AND"), (UnexpectedEnd, 5, 5));
    }

    #[test]
    fn spans_cover_terms() {
        let q = parse_query(r#"a from:"Ada L" -is:unread"#).unwrap();
        let text = r#"a from:"Ada L" -is:unread"#;
        let spans: Vec<&str> = q
            .terms()
            .iter()
            .map(|t| &text[t.span.start..t.span.end])
            .collect();
        assert_eq!(spans, ["a", r#"from:"Ada L""#, "is:unread"]);
    }

    #[test]
    fn canonical_form_round_trips() {
        for s in [
            "",
            "a",
            "a b OR c",
            "a (b OR c)",
            "NOT (a b)",
            r#"from:"Ada Lovelace" subject:x -has:attachment"#,
            "is:unread OR is:flagged",
            "before:2026-09-01 after:7d size>=1024",
            r#""AND" "-x" "from:y" "a\"b""#,
            "tag:\"two words\" folder:INBOX account:work",
            "category:social -category:promotions",
        ] {
            let q = p(s);
            let printed = q.to_string();
            assert_eq!(p(&printed), q, "{s:?} printed as {printed:?}");
        }
        assert_eq!(p("a AND (b)").to_string(), "a b");
        assert_eq!(p("-a").to_string(), "NOT a");
    }

    #[test]
    fn gmail_categories_are_a_field() {
        let q = p("CATEGORY:Social");
        assert_eq!(q.to_string(), "category:Social");
    }
}
