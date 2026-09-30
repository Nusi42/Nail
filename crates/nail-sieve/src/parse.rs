//! A small Sieve parser (RFC 5228 section 8 grammar): commands, blocks,
//! tests and test lists, tagged / numeric / string-list arguments, quoted
//! and multi-line strings, `#` and `/* */` comments.
//!
//! It checks syntax only (no semantics beyond [`Script::check_requires`]).
//! Nail uses it for the round-trip test harness ([`crate::decompile()`]) and
//! to read the markers of its own script on the server. Hash comments
//! directly before a command are kept on it ([`Command::comments`]), so the
//! `# rule "<id>"` markers survive.

use std::collections::BTreeSet;

/// A parsed script.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    /// Top-level commands.
    pub commands: Vec<Command>,
}

/// A command: `name arguments [test / test-list] (";" / block)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Lowercased identifier (`if`, `fileinto`, …).
    pub name: String,
    /// Positional and tagged arguments.
    pub arguments: Vec<Argument>,
    /// The test (`if`, `elsif`) or tests of the command.
    pub tests: Vec<Test>,
    /// The block, when the command has one.
    pub block: Option<Vec<Command>>,
    /// Hash comments on the lines directly before the command (text after
    /// `#`, trimmed).
    pub comments: Vec<String>,
    /// Line of the identifier (1-based).
    pub line: usize,
}

/// A test: `name arguments [test / test-list]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    /// Lowercased identifier (`header`, `allof`, …).
    pub name: String,
    /// Positional and tagged arguments.
    pub arguments: Vec<Argument>,
    /// Nested tests (`not`, `allof`, `anyof`).
    pub tests: Vec<Test>,
}

/// An argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// A string or a string list (a single string is a list of one).
    Strings(Vec<String>),
    /// A number (quantifiers `K`, `M`, `G` applied).
    Number(u64),
    /// A tag (`:is`), lowercased, with the colon.
    Tag(String),
}

/// A syntax error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
    /// Line (1-based).
    pub line: usize,
    /// What is wrong (English, for logs and tests).
    pub message: String,
}

impl Script {
    /// The extensions named by `require` commands (lowercased).
    pub fn required(&self) -> BTreeSet<String> {
        self.commands
            .iter()
            .filter(|c| c.name == "require")
            .flat_map(|c| c.arguments.iter())
            .filter_map(|a| match a {
                Argument::Strings(s) => Some(s.iter().map(|e| e.to_ascii_lowercase())),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Checks that `require` comes first and that every command, test and
    /// tag from an extension used in the script is required.
    pub fn check_requires(&self) -> Result<(), String> {
        let required = self.required();
        let mut seen_other = false;
        for c in &self.commands {
            if c.name == "require" {
                if seen_other {
                    return Err(format!("line {}: require after other commands", c.line));
                }
            } else {
                seen_other = true;
            }
        }
        let mut problems = Vec::new();
        visit(&self.commands, &mut |name, args| {
            if let Some(ext) = extension_of(name)
                && !required.contains(ext)
            {
                problems.push(format!("{name} needs {ext}"));
            }
            for a in args {
                if let Argument::Tag(tag) = a
                    && let Some(ext) = extension_of(tag)
                    && !required.contains(ext)
                {
                    problems.push(format!("{tag} needs {ext}"));
                }
            }
            for pair in args.windows(2) {
                if let [Argument::Tag(t), Argument::Strings(s)] = pair
                    && t == ":comparator"
                    && let Some(name) = s.first()
                    && !matches!(name.as_str(), "i;octet" | "i;ascii-casemap")
                    && !required.contains(&format!("comparator-{name}"))
                {
                    problems.push(format!("comparator {name} not required"));
                }
            }
        });
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

fn extension_of(word: &str) -> Option<&'static str> {
    Some(match word {
        "fileinto" => "fileinto",
        "vacation" => "vacation",
        "addflag" | "removeflag" | "setflag" | "hasflag" | ":flags" => "imap4flags",
        ":copy" => "copy",
        "set" | "string" => "variables",
        "body" => "body",
        ":regex" => "regex",
        ":value" | ":count" => "relational",
        "date" | "currentdate" => "date",
        ":index" | ":last" => "index",
        "include" | "return" | "global" => "include",
        "reject" => "reject",
        "envelope" => "envelope",
        _ => return None,
    })
}

fn visit(commands: &[Command], f: &mut impl FnMut(&str, &[Argument])) {
    fn visit_test(test: &Test, f: &mut impl FnMut(&str, &[Argument])) {
        f(&test.name, &test.arguments);
        for t in &test.tests {
            visit_test(t, f);
        }
    }
    for c in commands {
        f(&c.name, &c.arguments);
        for t in &c.tests {
            visit_test(t, f);
        }
        if let Some(block) = &c.block {
            visit(block, f);
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Tag(String),
    Number(u64),
    Str(String),
    Comment(String),
    Punct(char),
}

fn lex(script: &str) -> Result<Vec<(usize, Tok)>, ParseError> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    let fail = |line: usize, message: &str| ParseError {
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

// ---------------------------------------------------------------------------
// Parser

struct Parser {
    tokens: Vec<(usize, Tok)>,
    pos: usize,
    depth: usize,
}

/// Deepest nesting of blocks and tests accepted (servers limit it too).
const MAX_DEPTH: usize = 64;

/// Parses a script.
pub fn parse(script: &str) -> Result<Script, ParseError> {
    let mut parser = Parser {
        tokens: lex(script)?,
        pos: 0,
        depth: 0,
    };
    let commands = parser.commands(false)?;
    Ok(Script { commands })
}

impl Parser {
    fn line(&self) -> usize {
        self.tokens
            .get(self.pos)
            .or(self.tokens.last())
            .map_or(1, |(l, _)| *l)
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line(),
            message: message.into(),
        })
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|(_, t)| t)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).map(|(_, t)| t.clone());
        self.pos += 1;
        t
    }

    fn comments(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(Tok::Comment(text)) = self.peek() {
            out.push(text.clone());
            self.pos += 1;
        }
        out
    }

    fn skip_comments(&mut self) {
        while let Some(Tok::Comment(_)) = self.peek() {
            self.pos += 1;
        }
    }

    fn commands(&mut self, in_block: bool) -> Result<Vec<Command>, ParseError> {
        let mut out = Vec::new();
        loop {
            let comments = self.comments();
            match self.peek() {
                None if in_block => return self.fail("missing \"}\""),
                None => return Ok(out),
                Some(Tok::Punct('}')) if in_block => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(Tok::Ident(_)) => {
                    let mut command = self.command()?;
                    command.comments = comments;
                    out.push(command);
                }
                Some(other) => return self.fail(format!("expected a command, found {other:?}")),
            }
        }
    }

    fn command(&mut self) -> Result<Command, ParseError> {
        let line = self.line();
        let Some(Tok::Ident(name)) = self.next() else {
            return self.fail("expected a command");
        };
        let arguments = self.arguments()?;
        let tests = self.tests_after_arguments()?;
        self.skip_comments();
        let block = match self.next() {
            Some(Tok::Punct(';')) => None,
            Some(Tok::Punct('{')) => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return self.fail("nesting too deep");
                }
                let block = self.commands(true)?;
                self.depth -= 1;
                Some(block)
            }
            _ => {
                self.pos -= 1;
                return self.fail(format!("expected \";\" or a block after {name}"));
            }
        };
        Ok(Command {
            name,
            arguments,
            tests,
            block,
            comments: Vec::new(),
            line,
        })
    }

    fn arguments(&mut self) -> Result<Vec<Argument>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_comments();
            match self.peek() {
                Some(Tok::Str(_)) => {
                    let Some(Tok::Str(s)) = self.next() else {
                        unreachable!("peeked a string")
                    };
                    out.push(Argument::Strings(vec![s]));
                }
                Some(Tok::Number(n)) => {
                    out.push(Argument::Number(*n));
                    self.pos += 1;
                }
                Some(Tok::Tag(t)) => {
                    out.push(Argument::Tag(t.clone()));
                    self.pos += 1;
                }
                Some(Tok::Punct('[')) => {
                    self.pos += 1;
                    out.push(Argument::Strings(self.string_list()?));
                }
                _ => return Ok(out),
            }
        }
    }

    fn string_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_comments();
            match self.next() {
                Some(Tok::Str(s)) => out.push(s),
                _ => return self.fail("expected a string in the list"),
            }
            self.skip_comments();
            match self.next() {
                Some(Tok::Punct(',')) => {}
                Some(Tok::Punct(']')) => return Ok(out),
                _ => return self.fail("expected \",\" or \"]\""),
            }
        }
    }

    /// A test or a test list, if one follows.
    fn tests_after_arguments(&mut self) -> Result<Vec<Test>, ParseError> {
        self.skip_comments();
        match self.peek() {
            Some(Tok::Ident(_)) => Ok(vec![self.test()?]),
            Some(Tok::Punct('(')) => {
                self.pos += 1;
                self.test_list()
            }
            _ => Ok(Vec::new()),
        }
    }

    fn test(&mut self) -> Result<Test, ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("nesting too deep");
        }
        self.skip_comments();
        let Some(Tok::Ident(name)) = self.next() else {
            return self.fail("expected a test");
        };
        let arguments = self.arguments()?;
        let tests = self.tests_after_arguments()?;
        self.depth -= 1;
        Ok(Test {
            name,
            arguments,
            tests,
        })
    }

    fn test_list(&mut self) -> Result<Vec<Test>, ParseError> {
        let mut out = Vec::new();
        loop {
            out.push(self.test()?);
            self.skip_comments();
            match self.next() {
                Some(Tok::Punct(',')) => {}
                Some(Tok::Punct(')')) => return Ok(out),
                _ => return self.fail("expected \",\" or \")\""),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn s(v: &str) -> Argument {
        Argument::Strings(vec![v.to_owned()])
    }

    #[test]
    fn parses_the_rfc_example_shapes() {
        let script = parse(
            "# comment\r\nrequire [\"fileinto\", \"reject\"];\r\n\
             /* bracket\r\ncomment */\r\n\
             # rule \"x\"\r\n\
             if anyof (not exists [\"From\",\"Date\"], header :contains \"from\" \"cosmic\") {\r\n\
               fileinto \"a \\\"b\\\" \\\\c\";\r\n\
             } elsif size :over 100K { discard; }\r\n\
             else { keep; }\r\n",
        )
        .unwrap();
        assert_eq!(script.commands.len(), 4);
        let require = &script.commands[0];
        assert_eq!(require.comments, ["comment"]);
        assert_eq!(
            require.arguments,
            [Argument::Strings(vec!["fileinto".into(), "reject".into()])]
        );
        let iff = &script.commands[1];
        assert_eq!(iff.name, "if");
        assert_eq!(iff.comments, ["rule \"x\""]);
        assert_eq!(iff.line, 6);
        let anyof = &iff.tests[0];
        assert_eq!(anyof.name, "anyof");
        assert_eq!(anyof.tests[0].name, "not");
        assert_eq!(anyof.tests[0].tests[0].name, "exists");
        assert_eq!(
            anyof.tests[1].arguments,
            [Argument::Tag(":contains".into()), s("from"), s("cosmic")]
        );
        assert_eq!(iff.block.as_ref().unwrap()[0].arguments, [s("a \"b\" \\c")]);
        let elsif = &script.commands[2];
        assert_eq!(
            elsif.tests[0].arguments,
            [Argument::Tag(":over".into()), Argument::Number(100 * 1024)]
        );
        assert_eq!(script.commands[3].name, "else");
        assert_eq!(
            script.required(),
            ["fileinto", "reject"]
                .map(String::from)
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn multiline_strings() {
        let script =
            parse("vacation :days 3 text: # comment\r\nAway.\r\n..dot\r\n\r\n.\r\n;\nkeep;")
                .unwrap();
        assert_eq!(
            script.commands[0].arguments,
            [
                Argument::Tag(":days".into()),
                Argument::Number(3),
                s("Away.\n.dot\n\n")
            ]
        );
        assert_eq!(script.commands[1].line, 7);
    }

    #[test]
    fn syntax_errors_name_the_line() {
        let cases = [
            ("keep", 1, "expected"),
            ("if true {\nkeep;", 2, "missing"),
            ("\"x\";", 1, "expected a command"),
            ("fileinto [\"a\" \"b\"];", 1, "expected"),
            ("if anyof(true, ) {}", 1, "expected a test"),
            ("keep; /* x", 1, "unterminated comment"),
            ("a \"x", 1, "unterminated string"),
            ("vacation text:\nx\n", 1, "multi-line"),
            ("vacation text: x\n.\n;", 1, "after"),
            ("keep :;", 1, "invalid tag"),
            ("keep 99999999999999999999;", 1, "too large"),
            ("keep; @", 1, "unexpected character"),
            ("}", 1, "expected a command"),
        ];
        for (script, line, needle) in cases {
            let err = parse(script).unwrap_err();
            assert!(err.message.contains(needle), "{script:?}: {err}");
            assert_eq!(err.line, line, "{script:?}: {err}");
        }
        let deep = format!("if {}true{} {{}}", "not ".repeat(100), "");
        assert!(parse(&deep).unwrap_err().message.contains("deep"));
    }

    #[test]
    fn checks_requires() {
        let ok = parse("require [\"fileinto\", \"copy\"];\nfileinto :copy \"a\";").unwrap();
        assert_eq!(ok.check_requires(), Ok(()));
        let missing =
            parse("require \"fileinto\";\nif body :contains \"x\" { fileinto :copy \"a\"; }")
                .unwrap();
        let err = missing.check_requires().unwrap_err();
        assert!(
            err.contains("body needs body") && err.contains(":copy needs copy"),
            "{err}"
        );
        let late = parse("keep;\nrequire \"fileinto\";").unwrap();
        assert!(late.check_requires().unwrap_err().contains("line 2"));
        let comparator =
            parse("if header :comparator \"i;unicode-casemap\" :is \"a\" \"b\" {}").unwrap();
        assert!(comparator.check_requires().is_err());
        let octet = parse("if header :comparator \"i;octet\" :is \"a\" \"b\" {}").unwrap();
        assert_eq!(octet.check_requires(), Ok(()));
    }
}
