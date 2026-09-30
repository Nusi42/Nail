//! Rust `regex` syntax → POSIX extended regular expressions (ERE), the
//! dialect of the Sieve `regex` extension (draft-ietf-sieve-regex; Dovecot,
//! Cyrus and Stalwart use POSIX `regcomp` semantics).
//!
//! Rules store patterns in Rust syntax ([`nail_api::rules::TextOp::Regex`]).
//! The pattern is parsed with `regex-syntax` and rewritten when every part
//! has a POSIX equivalent: literals, `.`, `^` / `$`, alternation, groups
//! (non-capturing and named groups become plain groups), greedy
//! repetition, bracket classes with ranges and `[:ascii:]` classes, and the
//! Perl classes `\d` `\s` `\w` (as `[0-9]`, `[[:space:]]`, `[[:alnum:]_]`).
//! Everything else — flags, lazy repetition, word boundaries, Unicode
//! classes, class set operations, empty alternatives — makes the pattern
//! not expressible. Case-insensitivity is the comparator's job, not the
//! pattern's.
//!
//! The output is itself valid Rust syntax and translates to itself
//! (idempotent), which the round-trip harness relies on.

use regex_syntax::ast::{
    AssertionKind, Ast, ClassAsciiKind, ClassPerl, ClassPerlKind, ClassSet, ClassSetItem,
    GroupKind, LiteralKind, RepetitionKind, RepetitionRange, SpecialLiteralKind,
};

/// Translates a Rust regular expression to POSIX ERE. `None` when the
/// pattern is invalid or uses a feature ERE lacks.
pub fn to_posix_ere(pattern: &str) -> Option<String> {
    let ast = regex_syntax::ast::parse::Parser::new()
        .parse(pattern)
        .ok()?;
    // Reject patterns the full regex crate refuses (e.g. size limits).
    regex::Regex::new(pattern).ok()?;
    let mut out = String::new();
    write(&ast, &mut out)?;
    Some(out)
}

/// Characters that are special outside bracket expressions in ERE.
fn escape_char(c: char, out: &mut String) {
    if matches!(
        c,
        '.' | '[' | ']' | '(' | ')' | '*' | '+' | '?' | '{' | '}' | '|' | '^' | '$' | '\\'
    ) {
        out.push('\\');
    }
    out.push(c);
}

fn literal_char(kind: &LiteralKind, c: char) -> Option<char> {
    match kind {
        LiteralKind::Special(SpecialLiteralKind::Bell) => Some('\u{7}'),
        LiteralKind::Special(SpecialLiteralKind::FormFeed) => Some('\u{c}'),
        LiteralKind::Special(SpecialLiteralKind::VerticalTab) => Some('\u{b}'),
        LiteralKind::Special(SpecialLiteralKind::Space) => Some(' '),
        _ => Some(c),
    }
    // A literal tab / newline stays a literal character; ERE has no
    // escapes for them.
    .filter(|c| *c != '\0')
}

fn write(ast: &Ast, out: &mut String) -> Option<()> {
    match ast {
        // `a|` or `()`: POSIX leaves empty branches undefined.
        Ast::Empty(_) | Ast::Flags(_) | Ast::ClassUnicode(_) => None,
        Ast::Literal(lit) => {
            escape_char(literal_char(&lit.kind, lit.c)?, out);
            Some(())
        }
        Ast::Dot(_) => {
            out.push('.');
            Some(())
        }
        Ast::Assertion(a) => {
            match a.kind {
                AssertionKind::StartLine | AssertionKind::StartText => out.push('^'),
                AssertionKind::EndLine | AssertionKind::EndText => out.push('$'),
                _ => return None,
            }
            Some(())
        }
        Ast::ClassPerl(p) => {
            out.push_str(perl_class(p));
            Some(())
        }
        Ast::ClassBracketed(b) => {
            let ClassSet::Item(item) = &b.kind else {
                return None;
            };
            let mut items = Vec::new();
            flatten(item, &mut items)?;
            bracket(&items, b.negated, out)
        }
        Ast::Repetition(r) => {
            if !r.greedy {
                return None;
            }
            // `a**` is an error in Rust already; a repeated repetition such as
            // `(a*)*` stays grouped.
            write_atom(&r.ast, out)?;
            match &r.op.kind {
                RepetitionKind::ZeroOrOne => out.push('?'),
                RepetitionKind::ZeroOrMore => out.push('*'),
                RepetitionKind::OneOrMore => out.push('+'),
                RepetitionKind::Range(RepetitionRange::Exactly(n)) => {
                    out.push_str(&format!("{{{n}}}"));
                }
                RepetitionKind::Range(RepetitionRange::AtLeast(n)) => {
                    out.push_str(&format!("{{{n},}}"));
                }
                RepetitionKind::Range(RepetitionRange::Bounded(a, b)) => {
                    out.push_str(&format!("{{{a},{b}}}"));
                }
            }
            // POSIX limits bounds to RE_DUP_MAX (255).
            match &r.op.kind {
                RepetitionKind::Range(
                    RepetitionRange::Exactly(n)
                    | RepetitionRange::AtLeast(n)
                    | RepetitionRange::Bounded(_, n),
                ) if *n > 255 => None,
                _ => Some(()),
            }
        }
        Ast::Group(g) => {
            if let GroupKind::NonCapturing(flags) = &g.kind
                && !flags.items.is_empty()
            {
                return None;
            }
            out.push('(');
            write(&g.ast, out)?;
            out.push(')');
            Some(())
        }
        Ast::Alternation(alt) => {
            for (i, branch) in alt.asts.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                write(branch, out)?;
            }
            Some(())
        }
        Ast::Concat(c) => {
            for part in &c.asts {
                write(part, out)?;
            }
            Some(())
        }
    }
}

/// Writes the operand of a repetition; an operand that is not a single
/// atom is grouped.
fn write_atom(ast: &Ast, out: &mut String) -> Option<()> {
    match ast {
        Ast::Literal(_)
        | Ast::Dot(_)
        | Ast::ClassPerl(_)
        | Ast::ClassBracketed(_)
        | Ast::Group(_) => write(ast, out),
        Ast::Assertion(_) => None,
        _ => {
            out.push('(');
            write(ast, out)?;
            out.push(')');
            Some(())
        }
    }
}

fn perl_class(p: &ClassPerl) -> &'static str {
    match (&p.kind, p.negated) {
        (ClassPerlKind::Digit, false) => "[0-9]",
        (ClassPerlKind::Digit, true) => "[^0-9]",
        (ClassPerlKind::Space, false) => "[[:space:]]",
        (ClassPerlKind::Space, true) => "[^[:space:]]",
        (ClassPerlKind::Word, false) => "[[:alnum:]_]",
        (ClassPerlKind::Word, true) => "[^[:alnum:]_]",
    }
}

/// One member of a bracket expression.
#[derive(Debug, Clone, PartialEq)]
enum Member {
    Char(char),
    Range(char, char),
    Named(&'static str),
}

fn flatten(item: &ClassSetItem, out: &mut Vec<Member>) -> Option<()> {
    match item {
        ClassSetItem::Empty(_) => {}
        ClassSetItem::Literal(l) => out.push(Member::Char(literal_char(&l.kind, l.c)?)),
        ClassSetItem::Range(r) => {
            let (a, b) = (
                literal_char(&r.start.kind, r.start.c)?,
                literal_char(&r.end.kind, r.end.c)?,
            );
            // A range bounded by a bracket-special character is hard to
            // write portably.
            if [a, b].iter().any(|c| matches!(c, ']' | '^' | '-' | '[')) {
                return None;
            }
            out.push(Member::Range(a, b));
        }
        ClassSetItem::Ascii(a) => {
            if a.negated {
                return None;
            }
            out.push(Member::Named(ascii_name(&a.kind)?));
        }
        ClassSetItem::Perl(p) => {
            if p.negated {
                return None;
            }
            match p.kind {
                ClassPerlKind::Digit => out.push(Member::Range('0', '9')),
                ClassPerlKind::Space => out.push(Member::Named("space")),
                ClassPerlKind::Word => {
                    out.push(Member::Named("alnum"));
                    out.push(Member::Char('_'));
                }
            }
        }
        ClassSetItem::Unicode(_) | ClassSetItem::Bracketed(_) => return None,
        ClassSetItem::Union(u) => {
            for i in &u.items {
                flatten(i, out)?;
            }
        }
    }
    Some(())
}

fn ascii_name(kind: &ClassAsciiKind) -> Option<&'static str> {
    Some(match kind {
        ClassAsciiKind::Alnum => "alnum",
        ClassAsciiKind::Alpha => "alpha",
        ClassAsciiKind::Blank => "blank",
        ClassAsciiKind::Cntrl => "cntrl",
        ClassAsciiKind::Digit => "digit",
        ClassAsciiKind::Graph => "graph",
        ClassAsciiKind::Lower => "lower",
        ClassAsciiKind::Print => "print",
        ClassAsciiKind::Punct => "punct",
        ClassAsciiKind::Space => "space",
        ClassAsciiKind::Upper => "upper",
        ClassAsciiKind::Xdigit => "xdigit",
        // `[:ascii:]` and `[:word:]` are not POSIX.
        ClassAsciiKind::Ascii | ClassAsciiKind::Word => return None,
    })
}

/// Writes a POSIX bracket expression: `]` first, `-` last, `^` not first;
/// a backslash is an ordinary character inside brackets in POSIX but an
/// escape in Rust, so classes containing one are refused.
fn bracket(members: &[Member], negated: bool, out: &mut String) -> Option<()> {
    if members.is_empty() {
        return None;
    }
    let has = |c: char| members.contains(&Member::Char(c));
    if has('\\') || has('[') {
        return None;
    }
    let mut body = String::new();
    if has(']') {
        body.push(']');
    }
    for m in members {
        match m {
            Member::Char(']' | '-' | '^') => {}
            Member::Char(c) => body.push(*c),
            Member::Range(a, b) => {
                body.push(*a);
                body.push('-');
                body.push(*b);
            }
            Member::Named(name) => {
                body.push_str("[:");
                body.push_str(name);
                body.push_str(":]");
            }
        }
    }
    if has('^') {
        if body.is_empty() {
            // `[^]` would be read as a negation.
            return None;
        }
        body.push('^');
    }
    if has('-') {
        body.push('-');
    }
    // Rust reads `[]…]` and `[^]…]` differently from POSIX: a leading `]`
    // is only a literal in POSIX. Refuse it so the output stays valid Rust
    // syntax with the same meaning.
    if body.starts_with(']') {
        return None;
    }
    out.push('[');
    if negated {
        out.push('^');
    }
    out.push_str(&body);
    out.push(']');
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_the_portable_subset() {
        let cases = [
            (r"^\[list\] .*$", r"^\[list\] .*$"),
            (r"invoice\s+#\d+", r"invoice[[:space:]]+#[0-9]+"),
            (r"\w+@example\.com", r"[[:alnum:]_]+@example\.com"),
            (r"(?:a|b)c", "(a|b)c"),
            (r"(?P<x>ab)+", "(ab)+"),
            (r"a{2,3}b{4}c{5,}", "a{2,3}b{4}c{5,}"),
            (r"[a-z0-9._-]+", "[a-z0-9._-]+"),
            (r"[\d\s]", "[0-9[:space:]]"),
            (r"[[:alpha:]]x", "[[:alpha:]]x"),
            (r"[^^a]", "[^a^]"),
            (r"\D\S\W", "[^0-9][^[:space:]][^[:alnum:]_]"),
            (r"\x41\u{e4}", "Aä"),
            (r"a\.b\*c\?", r"a\.b\*c\?"),
            (r"\-\#", "-#"),
            ("über", "über"),
            (r"\Aab\z", "^ab$"),
            (r"(ab|cd)*", "(ab|cd)*"),
            (r"[^\d]", "[^0-9]"),
        ];
        for (rust, ere) in cases {
            assert_eq!(to_posix_ere(rust).as_deref(), Some(ere), "{rust}");
            // Idempotent, and the output is valid Rust syntax.
            assert_eq!(to_posix_ere(ere).as_deref(), Some(ere), "{ere}");
        }
    }

    #[test]
    fn refuses_what_posix_lacks() {
        for rust in [
            r"(?i)abc",
            r"(?i:abc)",
            r"a*?",
            r"a+?",
            r"\bword\b",
            r"\pL+",
            r"[\p{Greek}]",
            r"[a-z&&[^aeiou]]",
            r"a|",
            r"()",
            r"[\\]",
            r"[\]a]",
            r"[[:ascii:]]",
            r"[[:^alpha:]]",
            r"[\D]",
            r"a{300}",
            r"(",
            r"[z-a]",
            r"\x00",
        ] {
            assert_eq!(to_posix_ere(rust), None, "{rust}");
        }
    }
}
