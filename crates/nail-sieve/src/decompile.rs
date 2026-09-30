//! The round-trip harness: a script Nail generated → rules and vacation
//! settings again ([`decompile`]), to be compared with the originals in
//! Nail's canonical form ([`normalize_rule`]).
//!
//! `compile → to_crlf → parse → decompile` must give back every server
//! rule's conditions and actions (normalized) and the vacation settings;
//! the tests run this over a corpus of rules. Only the shapes
//! [`crate::compile`] writes are understood — this is not a general Sieve
//! decompiler.
//!
//! Folders come back as mailbox names (`FolderRef::Path` without an
//! account), so [`normalize_rule`] maps the original's folders through the
//! same [`FolderResolver`](crate::FolderResolver).

use std::collections::BTreeSet;

use nail_api::domain::FolderRole;
use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef, Rule,
    RuleAction, TextMatch, TextOp,
};
use nail_api::sieve::VacationSettings;

use crate::compile::{
    CompileOptions, DELIVER_VAR, FOLDER_VAR, PREVIOUS_MARKER, RULE_MARKER, address_fields,
    auth_method, auth_result,
};
use crate::ere::to_posix_ere;
use crate::parse::{Argument, Command, Test, parse};
use crate::writer::{DOLLAR_VAR, unmatches_literal};

/// A rule read back from a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompiledRule {
    /// The rule id from its `# rule "<id>"` marker.
    pub id: String,
    /// The condition.
    pub condition: Condition,
    /// The actions (folders as mailbox names).
    pub actions: Vec<RuleAction>,
}

/// A script read back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decompiled {
    /// Starts with Nail's marker.
    pub managed: bool,
    /// The recorded previously active script.
    pub previous_active: Option<String>,
    /// Required extensions.
    pub required: BTreeSet<String>,
    /// The script run through `include`.
    pub included: Option<String>,
    /// The vacation reply, if the script has one.
    pub vacation: Option<VacationSettings>,
    /// The rules, in order.
    pub rules: Vec<DecompiledRule>,
    /// Whether moves are deferred to the final delivery block.
    pub deferred_delivery: bool,
}

/// Reads a script written by [`crate::compile`].
pub fn decompile(script: &str) -> Result<Decompiled, String> {
    let parsed = parse(script).map_err(|e| e.to_string())?;
    parsed.check_requires()?;
    let mut out = Decompiled {
        required: parsed.required(),
        ..Decompiled::default()
    };
    let unvar = out.required.contains("variables");
    let lit = |s: &str| {
        if unvar {
            s.replace(&format!("${{{DOLLAR_VAR}}}"), "$")
        } else {
            s.to_owned()
        }
    };
    let mut after_flush = false;
    for (index, command) in parsed.commands.iter().enumerate() {
        // The `elsif` of the final delivery block.
        if std::mem::take(&mut after_flush) && command.name == "elsif" {
            continue;
        }
        for comment in &command.comments {
            if format!("# {comment}") == crate::compile::MANAGED_MARKER && index == 0 {
                out.managed = true;
            }
            if let Some(rest) = format!("# {comment}").strip_prefix(PREVIOUS_MARKER) {
                out.previous_active = Some(unquote(rest)?);
            }
        }
        let marker = |prefix: &str| {
            command
                .comments
                .iter()
                .find_map(|c| format!("# {c}").strip_prefix(prefix).map(str::to_owned))
        };
        match command.name.as_str() {
            "require" => {}
            "set"
                if strings(&command.arguments).first().map(String::as_str) == Some(DOLLAR_VAR) => {}
            "include" => out.included = strings(&command.arguments).first().cloned(),
            _ if command.comments.iter().any(|c| c == "nail:vacation") => {
                out.vacation = Some(vacation(command, &lit)?);
            }
            _ if command.comments.iter().any(|c| c == "nail:deliver") => {
                is_flush(command)
                    .then_some(())
                    .ok_or("unexpected delivery block")?;
                out.deferred_delivery = true;
                after_flush = true;
            }
            "if" => {
                let id = marker(RULE_MARKER)
                    .ok_or_else(|| format!("line {}: rule block without marker", command.line))?;
                let id = unquote(&id)?;
                let [test] = command.tests.as_slice() else {
                    return Err(format!("line {}: if without one test", command.line));
                };
                let condition = condition(test, &lit)?;
                let block = command.block.as_deref().unwrap_or_default();
                let actions = actions(block, &lit)?;
                out.rules.push(DecompiledRule {
                    id,
                    condition,
                    actions,
                });
            }
            other => return Err(format!("line {}: unexpected command {other}", command.line)),
        }
    }
    Ok(out)
}

fn unquote(text: &str) -> Result<String, String> {
    let inner = text
        .trim()
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .ok_or_else(|| format!("not a quoted string: {text}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn strings(args: &[Argument]) -> Vec<String> {
    args.iter()
        .filter_map(|a| match a {
            Argument::Strings(s) => Some(s.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The positional string lists and the tags (with a following value) of a
/// test.
#[derive(Debug, Default)]
struct Args {
    tags: Vec<String>,
    comparator: Option<String>,
    zone: Option<String>,
    index: Option<u64>,
    relational: Option<String>,
    lists: Vec<Vec<String>>,
    numbers: Vec<u64>,
}

fn args(arguments: &[Argument]) -> Args {
    let mut out = Args::default();
    let mut iter = arguments.iter().peekable();
    while let Some(a) = iter.next() {
        match a {
            Argument::Tag(t) if t == ":comparator" || t == ":zone" || t == ":value" => {
                let value = match iter.peek() {
                    Some(Argument::Strings(s)) => s.first().cloned(),
                    _ => None,
                };
                iter.next();
                match t.as_str() {
                    ":comparator" => out.comparator = value,
                    ":zone" => out.zone = value,
                    _ => out.relational = value,
                }
            }
            Argument::Tag(t) if t == ":index" => {
                if let Some(Argument::Number(n)) = iter.peek() {
                    out.index = Some(*n);
                }
                iter.next();
            }
            Argument::Tag(t) => out.tags.push(t.clone()),
            Argument::Strings(s) => out.lists.push(s.clone()),
            Argument::Number(n) => out.numbers.push(*n),
        }
    }
    out
}

fn text_match(a: &Args, key: &str) -> Result<TextMatch, String> {
    let case_sensitive = a.comparator.as_deref() == Some("i;octet");
    let m = |op, value: String| TextMatch {
        op,
        value,
        case_sensitive,
    };
    let has = |t: &str| a.tags.iter().any(|x| x == t);
    if has(":is") {
        Ok(m(TextOp::Equals, key.to_owned()))
    } else if has(":contains") {
        Ok(m(TextOp::Contains, key.to_owned()))
    } else if has(":regex") {
        Ok(m(TextOp::Regex, key.to_owned()))
    } else if has(":matches") {
        // The final `*` is a wildcard when an even number of backslashes
        // precedes it.
        if let Some(prefix) = key.strip_suffix('*')
            && (prefix.len() - prefix.trim_end_matches('\\').len()) % 2 == 0
            && let Some(text) = unmatches_literal(prefix)
        {
            return Ok(m(TextOp::StartsWith, text));
        }
        if let Some(text) = key.strip_prefix('*').and_then(unmatches_literal) {
            return Ok(m(TextOp::EndsWith, text));
        }
        Err(format!("unexpected :matches pattern {key:?}"))
    } else {
        Err("text test without match type".to_owned())
    }
}

fn one<'a>(list: Option<&'a Vec<String>>, what: &str) -> Result<&'a str, String> {
    match list.map(Vec::as_slice) {
        Some([one]) => Ok(one),
        _ => Err(format!("expected one {what}")),
    }
}

fn condition(test: &Test, lit: &impl Fn(&str) -> String) -> Result<Condition, String> {
    let a = args(&test.arguments);
    let children = |tests: &[Test]| -> Result<Vec<Condition>, String> {
        tests.iter().map(|t| condition(t, lit)).collect()
    };
    Ok(match test.name.as_str() {
        "true" => Condition::always(),
        "false" => Condition::Any {
            conditions: Vec::new(),
        },
        "allof" => Condition::All {
            conditions: children(&test.tests)?,
        },
        "anyof" => {
            if let Some(c) = address_pattern(test, lit)? {
                return Ok(c);
            }
            if let Some(c) = list_id_pattern(test, lit)? {
                return Ok(c);
            }
            Condition::Any {
                conditions: children(&test.tests)?,
            }
        }
        "not" => {
            let [inner] = test.tests.as_slice() else {
                return Err("not without one test".to_owned());
            };
            Condition::Not {
                condition: Box::new(condition(inner, lit)?),
            }
        }
        "exists" => Condition::Header {
            name: lit(one(a.lists.first(), "header name")?),
            matches: None,
        },
        "header" => {
            let name = lit(one(a.lists.first(), "header name")?);
            let key = lit(one(a.lists.get(1), "key")?);
            if a.index == Some(1) && name == "authentication-results" {
                let (method, result) = key.split_once('=').ok_or("bad auth key")?;
                return Ok(Condition::Auth {
                    method: parse_auth_method(method)?,
                    result: parse_auth_result(result)?,
                });
            }
            let matches = text_match(&a, &key)?;
            if name == "subject" {
                Condition::Subject { matches }
            } else {
                Condition::Header {
                    name,
                    matches: Some(matches),
                }
            }
        }
        "body" => {
            if !a.tags.iter().any(|t| t == ":text") {
                return Err("body test without :text".to_owned());
            }
            Condition::Body {
                matches: text_match(&a, &lit(one(a.lists.first(), "key")?))?,
            }
        }
        "size" => {
            let bytes = *a.numbers.first().ok_or("size without limit")?;
            let cmp = if a.tags.iter().any(|t| t == ":over") {
                Comparison::Greater
            } else {
                Comparison::Less
            };
            Condition::Size { cmp, bytes }
        }
        "date" => {
            let cmp = match a.relational.as_deref() {
                Some("lt") => DateComparison::Before,
                Some("gt") => DateComparison::After,
                other => return Err(format!("unexpected date relation {other:?}")),
            };
            Condition::Date {
                cmp,
                date: one(a.lists.get(2), "date")?.to_owned(),
            }
        }
        other => return Err(format!("unexpected test {other}")),
    })
}

/// `anyof(address :all … F K, header … F K)` → `Address`.
fn address_pattern(
    test: &Test,
    lit: &impl Fn(&str) -> String,
) -> Result<Option<Condition>, String> {
    let [address, header] = test.tests.as_slice() else {
        return Ok(None);
    };
    if address.name != "address" || header.name != "header" {
        return Ok(None);
    }
    let a = args(&address.arguments);
    let fields = a.lists.first().ok_or("address without fields")?;
    let field = [
        AddressField::From,
        AddressField::To,
        AddressField::Cc,
        AddressField::Recipients,
        AddressField::ReplyTo,
        AddressField::Sender,
    ]
    .into_iter()
    .find(|f| address_fields(*f) == fields.as_slice())
    .ok_or("unknown address fields")?;
    let key = lit(one(a.lists.get(1), "key")?);
    let h = args(&header.arguments);
    if h.lists != a.lists {
        return Err("address pattern with different header test".to_owned());
    }
    Ok(Some(Condition::Address {
        field,
        matches: text_match(&a, &key)?,
    }))
}

/// `anyof(header … "list-id" K, header :matches "list-id" "*<…>*")` →
/// `ListId`.
fn list_id_pattern(
    test: &Test,
    lit: &impl Fn(&str) -> String,
) -> Result<Option<Condition>, String> {
    let [plain, bracket] = test.tests.as_slice() else {
        return Ok(None);
    };
    if plain.name != "header" || bracket.name != "header" {
        return Ok(None);
    }
    let a = args(&plain.arguments);
    let b = args(&bracket.arguments);
    let is_list_id = |x: &Args| x.lists.first().map(Vec::as_slice) == Some(&["list-id".to_owned()]);
    let bracket_key = b.lists.get(1).and_then(|k| k.first());
    if !is_list_id(&a)
        || !is_list_id(&b)
        || !bracket_key.is_some_and(|k| k.starts_with("*<") && k.ends_with(">*"))
    {
        return Ok(None);
    }
    Ok(Some(Condition::ListId {
        matches: Some(text_match(&a, &lit(one(a.lists.get(1), "key")?))?),
    }))
}

fn parse_auth_method(text: &str) -> Result<AuthMethod, String> {
    [AuthMethod::Dkim, AuthMethod::Spf, AuthMethod::Dmarc]
        .into_iter()
        .find(|m| auth_method(*m) == text)
        .ok_or_else(|| format!("unknown method {text}"))
}

fn parse_auth_result(text: &str) -> Result<AuthResult, String> {
    [
        AuthResult::Pass,
        AuthResult::Fail,
        AuthResult::Softfail,
        AuthResult::Neutral,
        AuthResult::None,
        AuthResult::Temperror,
        AuthResult::Permerror,
    ]
    .into_iter()
    .find(|r| auth_result(*r) == text)
    .ok_or_else(|| format!("unknown result {text}"))
}

fn is_flush(command: &Command) -> bool {
    command.name == "if"
        && command.tests.first().is_some_and(|t| {
            t.name == "string"
                && strings(&t.arguments).first() == Some(&format!("${{{DELIVER_VAR}}}"))
        })
}

fn path(mailbox: String) -> FolderRef {
    FolderRef::Path {
        path: mailbox,
        account_id: None,
    }
}

fn actions(block: &[Command], lit: &impl Fn(&str) -> String) -> Result<Vec<RuleAction>, String> {
    let mut out: Vec<RuleAction> = Vec::new();
    let mut iter = block.iter().peekable();
    while let Some(c) = iter.next() {
        let s: Vec<String> = strings(&c.arguments).iter().map(|x| lit(x)).collect();
        let tags: Vec<&str> = c
            .arguments
            .iter()
            .filter_map(|a| match a {
                Argument::Tag(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        match (c.name.as_str(), s.as_slice()) {
            ("set", [var, kind]) if var == DELIVER_VAR && kind == "fileinto" => {
                let Some(next) = iter.next() else {
                    return Err("fileinto without folder".to_owned());
                };
                match strings(&next.arguments).as_slice() {
                    [var, folder] if next.name == "set" && var == FOLDER_VAR => {
                        out.push(RuleAction::Move {
                            folder: path(lit(folder)),
                        });
                    }
                    _ => return Err("fileinto without folder".to_owned()),
                }
            }
            ("set", [var, kind]) if var == DELIVER_VAR && kind == "discard" => {
                out.push(RuleAction::Delete { permanent: true });
            }
            ("fileinto", [mailbox]) if tags == [":copy"] => out.push(RuleAction::Copy {
                folder: path(mailbox.clone()),
            }),
            ("addflag" | "removeflag", [flag]) => {
                let add = c.name == "addflag";
                out.push(match flag.as_str() {
                    "\\Flagged" => RuleAction::Flag { flagged: add },
                    "\\Seen" => RuleAction::MarkRead { read: add },
                    _ if add => RuleAction::Tag { tag: flag.clone() },
                    _ => RuleAction::Untag { tag: flag.clone() },
                });
            }
            ("redirect", [address]) if tags == [":copy"] => match out.last_mut() {
                Some(RuleAction::Redirect { to }) => to.push(address.clone()),
                _ => out.push(RuleAction::Redirect {
                    to: vec![address.clone()],
                }),
            },
            ("if", _) if is_flush(c) => {
                if iter.peek().is_some_and(|n| n.name == "elsif") {
                    iter.next();
                }
                match iter.next() {
                    Some(stop) if stop.name == "stop" => out.push(RuleAction::Stop),
                    _ => return Err("delivery block without stop".to_owned()),
                }
            }
            ("stop", []) => out.push(RuleAction::Stop),
            (other, _) => return Err(format!("line {}: unexpected action {other}", c.line)),
        }
    }
    Ok(out)
}

fn vacation(command: &Command, lit: &impl Fn(&str) -> String) -> Result<VacationSettings, String> {
    let mut settings = VacationSettings {
        enabled: true,
        skip_lists: false,
        ..VacationSettings::default()
    };
    let reply = if command.name == "if" {
        let [test] = command.tests.as_slice() else {
            return Err("vacation block without test".to_owned());
        };
        let tests: &[Test] = if test.name == "allof" {
            &test.tests
        } else {
            std::slice::from_ref(test)
        };
        let mut skips = 0;
        for t in tests {
            let a = args(&t.arguments);
            match t.name.as_str() {
                "currentdate" => {
                    let date = one(a.lists.get(1), "date")?.to_owned();
                    match a.relational.as_deref() {
                        Some("ge") => settings.start_date = Some(date),
                        Some("le") => settings.end_date = Some(date),
                        other => return Err(format!("unexpected relation {other:?}")),
                    }
                }
                "not" => skips += 1,
                other => return Err(format!("unexpected vacation test {other}")),
            }
        }
        settings.skip_lists = match skips {
            0 => false,
            3 => true,
            _ => return Err("partial list filter".to_owned()),
        };
        match command.block.as_deref() {
            Some([reply]) => reply,
            _ => return Err("vacation block without one command".to_owned()),
        }
    } else {
        command
    };
    if reply.name != "vacation" {
        return Err(format!("expected vacation, found {}", reply.name));
    }
    let mut iter = reply.arguments.iter();
    let mut body = None;
    while let Some(a) = iter.next() {
        match a {
            Argument::Tag(t) => {
                let value = iter.next();
                match (t.as_str(), value) {
                    (":days", Some(Argument::Number(n))) => {
                        settings.days = u32::try_from(*n).map_err(|_| "days too large")?;
                    }
                    (":subject", Some(Argument::Strings(s))) => settings.subject = lit(&s[0]),
                    (":from", Some(Argument::Strings(s))) => settings.from = Some(lit(&s[0])),
                    (":addresses", Some(Argument::Strings(s))) => {
                        settings.addresses = s.iter().map(|x| lit(x)).collect();
                    }
                    _ => return Err(format!("unexpected vacation argument {t}")),
                }
            }
            Argument::Strings(s) => body = Some(lit(&s[0])),
            Argument::Number(_) => return Err("unexpected number".to_owned()),
        }
    }
    let body = body.ok_or("vacation without reason")?;
    settings.body = body.strip_suffix('\n').unwrap_or(&body).to_owned();
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Canonical form of the originals

/// A rule's conditions and actions in the form [`decompile`] returns for
/// it: account and Inbox conditions evaluated, `List-Id` / `Subject`
/// aliases folded, text tests normalized (trimmed `equals`, regular
/// expressions as POSIX ERE), folders as mailbox names, archive and trash
/// as moves, actions cut after `stop`, adjacent redirects merged.
pub fn normalize_rule(rule: &Rule, options: &CompileOptions<'_>) -> (Condition, Vec<RuleAction>) {
    (
        normalize_condition(&rule.conditions, options),
        normalize_actions(&rule.actions, options),
    )
}

fn normalize_match(m: &TextMatch) -> TextMatch {
    let mut m = m.clone();
    match m.op {
        TextOp::Equals => m.value = m.value.trim().to_owned(),
        TextOp::StartsWith | TextOp::EndsWith if m.value.is_empty() => m.op = TextOp::Contains,
        TextOp::Regex => m.value = to_posix_ere(&m.value).unwrap_or(m.value),
        _ => {}
    }
    m
}

fn normalize_condition(condition: &Condition, options: &CompileOptions<'_>) -> Condition {
    let this = |id: &nail_api::ids::AccountId| {
        *id == options.account_id || options.account_aliases.contains(id)
    };
    match condition {
        Condition::All { conditions } => Condition::All {
            conditions: conditions
                .iter()
                .map(|c| normalize_condition(c, options))
                .collect(),
        },
        Condition::Any { conditions } => Condition::Any {
            conditions: conditions
                .iter()
                .map(|c| normalize_condition(c, options))
                .collect(),
        },
        Condition::Not { condition } => Condition::Not {
            condition: Box::new(normalize_condition(condition, options)),
        },
        Condition::Header { name, matches } => {
            let name = name.trim().to_ascii_lowercase();
            match matches {
                Some(m) if name == "subject" => Condition::Subject {
                    matches: normalize_match(m),
                },
                m => Condition::Header {
                    name,
                    matches: m.as_ref().map(normalize_match),
                },
            }
        }
        Condition::Address { field, matches } => Condition::Address {
            field: *field,
            matches: normalize_match(matches),
        },
        Condition::Subject { matches } => Condition::Subject {
            matches: normalize_match(matches),
        },
        Condition::Body { matches } => Condition::Body {
            matches: normalize_match(matches),
        },
        Condition::Account { account_id } => {
            if this(account_id) {
                Condition::always()
            } else {
                Condition::Any {
                    conditions: Vec::new(),
                }
            }
        }
        Condition::Folder { .. } => Condition::always(),
        Condition::ListId { matches: None } => Condition::Header {
            name: "list-id".to_owned(),
            matches: None,
        },
        Condition::ListId { matches: Some(m) } => {
            let m = normalize_match(m);
            match m.op {
                TextOp::Contains | TextOp::Regex => Condition::Header {
                    name: "list-id".to_owned(),
                    matches: Some(m),
                },
                _ => Condition::ListId { matches: Some(m) },
            }
        }
        other => other.clone(),
    }
}

fn normalize_actions(actions: &[RuleAction], options: &CompileOptions<'_>) -> Vec<RuleAction> {
    let mailbox = |folder: &FolderRef| {
        path(
            options
                .folders
                .mailbox(folder)
                .unwrap_or_else(|| "<unknown>".to_owned()),
        )
    };
    let role = |role| FolderRef::Role {
        role,
        account_id: None,
    };
    let mut out: Vec<RuleAction> = Vec::new();
    for action in actions {
        let normalized = match action {
            RuleAction::Move { folder } => RuleAction::Move {
                folder: mailbox(folder),
            },
            RuleAction::Archive => RuleAction::Move {
                folder: mailbox(&role(FolderRole::Archive)),
            },
            RuleAction::Delete { permanent: false } => RuleAction::Move {
                folder: mailbox(&role(FolderRole::Trash)),
            },
            RuleAction::Copy { folder } => RuleAction::Copy {
                folder: mailbox(folder),
            },
            RuleAction::Redirect { to } => {
                let to: Vec<String> = to.iter().map(|a| a.trim().to_owned()).collect();
                if to.is_empty() {
                    continue;
                }
                if let Some(RuleAction::Redirect { to: previous }) = out.last_mut() {
                    previous.extend(to);
                    continue;
                }
                RuleAction::Redirect { to }
            }
            other => other.clone(),
        };
        let stop = normalized == RuleAction::Stop;
        out.push(normalized);
        if stop {
            break;
        }
    }
    out
}
