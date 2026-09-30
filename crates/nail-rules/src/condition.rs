//! Conditions: compiled once per rule (regular expressions, queries,
//! dates), then evaluated against messages, optionally with an explanation
//! of which condition matched and on what value.

use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef,
    MessageFlag,
};
use nail_api::{AccountId, LocalizedMessage};
use serde::{Deserialize, Serialize};

use crate::message::{Env, MessageView};
use crate::text::{Matcher, excerpt};
use crate::time::parse_day;

/// Where a condition is in a rule: the child indexes from the root of
/// [`nail_api::Rule::conditions`] (empty = the root itself).
pub type ConditionPath = Vec<u32>;

/// One step of an explanation: a condition that was evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionHit {
    /// The condition.
    pub path: ConditionPath,
    /// Whether it matched.
    pub matched: bool,
    /// The message value that decided it (a header value, the subject, an
    /// address, a file name …), shortened; `null` for groups and for
    /// conditions without a single value.
    pub value: Option<String>,
}

/// What is wrong with a rule's conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionError {
    /// The broken condition.
    pub path: ConditionPath,
    /// Diagnostic (English).
    pub message: String,
    /// The user-facing text (`rule-issue-*` in `locales/<lang>/core.ftl`),
    /// naming the condition like the diagnostic does.
    pub localized: LocalizedMessage,
}

impl ConditionError {
    fn new(path: &[u32], message: String, localized: LocalizedMessage) -> Self {
        Self {
            path: path.to_vec(),
            message,
            localized: localized.arg("condition", condition_label(path)),
        }
    }
}

/// The `condition` argument of `rule-issue-*` messages: the 1-based path
/// (`2.1`), or `root` for the condition tree itself.
pub(crate) fn condition_label(path: &[u32]) -> String {
    if path.is_empty() {
        "root".to_owned()
    } else {
        path.iter()
            .map(|i| (i + 1).to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl std::fmt::Display for ConditionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            write!(f, "conditions: {}", self.message)
        } else {
            let path: Vec<String> = self.path.iter().map(|i| (i + 1).to_string()).collect();
            write!(f, "condition {}: {}", path.join("."), self.message)
        }
    }
}

#[derive(Debug, Clone)]
enum Node {
    All(Vec<Node>),
    Any(Vec<Node>),
    Not(Box<Node>),
    Header(String, Option<Matcher>),
    Address(AddressField, Matcher),
    Subject(Matcher),
    Body(Matcher),
    Query(Box<nail_search::Query>),
    Attachment(Option<Matcher>, Option<Matcher>),
    Size(Comparison, u64),
    /// Before the start / after the end of a local day (days since epoch).
    Date(DateComparison, i64),
    Age(Comparison, u32),
    Flag(MessageFlag, bool),
    Tag(String),
    Account(AccountId),
    Folder(FolderRef),
    Auth(AuthMethod, AuthResult),
    ListId(Option<Matcher>),
}

/// A compiled condition tree.
#[derive(Debug, Clone)]
pub struct CompiledCondition {
    root: Node,
}

impl CompiledCondition {
    /// Compiles a condition tree; every broken condition is reported.
    pub fn new(condition: &Condition) -> Result<Self, Vec<ConditionError>> {
        let mut errors = Vec::new();
        let root = compile(condition, &mut Vec::new(), &mut errors);
        if errors.is_empty() {
            Ok(Self { root })
        } else {
            Err(errors)
        }
    }

    /// Whether the message matches (stops at the first deciding child).
    pub fn matches(&self, message: &dyn MessageView, env: &Env<'_>) -> bool {
        eval(&self.root, message, env, &mut Vec::new(), None)
    }

    /// Whether the message matches, with every evaluated condition in
    /// `hits` (groups evaluate all their children, so the explanation shows
    /// each one).
    pub fn explain(&self, message: &dyn MessageView, env: &Env<'_>) -> (bool, Vec<ConditionHit>) {
        let mut hits = Vec::new();
        let matched = eval(&self.root, message, env, &mut Vec::new(), Some(&mut hits));
        // Tree order: a group before its children.
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        (matched, hits)
    }
}

fn matcher(
    m: &nail_api::rules::TextMatch,
    path: &[u32],
    errors: &mut Vec<ConditionError>,
) -> Option<Matcher> {
    match Matcher::new(m) {
        Ok(m) => Some(m),
        Err(e) => {
            errors.push(ConditionError::new(
                path,
                format!("invalid regular expression: {e}"),
                LocalizedMessage::new("rule-issue-regex").arg("detail", e.to_string()),
            ));
            None
        }
    }
}

fn compile(condition: &Condition, path: &mut Vec<u32>, errors: &mut Vec<ConditionError>) -> Node {
    let group = |children: &[Condition], path: &mut Vec<u32>, errors: &mut Vec<ConditionError>| {
        children
            .iter()
            .enumerate()
            .map(|(i, c)| {
                path.push(u32::try_from(i).unwrap_or(u32::MAX));
                let node = compile(c, path, errors);
                path.pop();
                node
            })
            .collect::<Vec<_>>()
    };
    // A placeholder that keeps the tree shape when a child is broken (the
    // whole tree is refused anyway).
    let never = || Node::Any(Vec::new());
    match condition {
        Condition::All { conditions } => Node::All(group(conditions, path, errors)),
        Condition::Any { conditions } => Node::Any(group(conditions, path, errors)),
        Condition::Not { condition } => {
            path.push(0);
            let node = compile(condition, path, errors);
            path.pop();
            Node::Not(Box::new(node))
        }
        Condition::Header { name, matches } => {
            if name.trim().is_empty() {
                errors.push(ConditionError::new(
                    path,
                    "the header name is empty".into(),
                    LocalizedMessage::new("rule-issue-header-empty"),
                ));
            }
            match matches {
                None => Node::Header(name.trim().to_owned(), None),
                Some(m) => matcher(m, path, errors)
                    .map_or_else(never, |m| Node::Header(name.trim().to_owned(), Some(m))),
            }
        }
        Condition::Address { field, matches } => {
            matcher(matches, path, errors).map_or_else(never, |m| Node::Address(*field, m))
        }
        Condition::Subject { matches } => {
            matcher(matches, path, errors).map_or_else(never, Node::Subject)
        }
        Condition::Body { matches } => {
            matcher(matches, path, errors).map_or_else(never, Node::Body)
        }
        Condition::Query { query } => match nail_search::parse_query(query) {
            Ok(q) => Node::Query(Box::new(q)),
            Err(e) => {
                errors.push(ConditionError::new(
                    path,
                    format!(
                        "invalid search query: {} (at {}–{})",
                        e.message, e.start, e.end
                    ),
                    LocalizedMessage::new("rule-issue-query")
                        .arg("detail", e.message.as_str())
                        .arg("start", e.start)
                        .arg("end", e.end),
                ));
                never()
            }
        },
        Condition::Attachment { name, mime } => {
            let name = name.as_ref().map(|m| matcher(m, path, errors));
            let mime = mime.as_ref().map(|m| matcher(m, path, errors));
            match (name, mime) {
                (Some(None), _) | (_, Some(None)) => never(),
                (name, mime) => Node::Attachment(name.flatten(), mime.flatten()),
            }
        }
        Condition::Size { cmp, bytes } => Node::Size(*cmp, *bytes),
        Condition::Date { cmp, date } => match parse_day(date) {
            Some(day) => Node::Date(*cmp, day),
            None => {
                errors.push(ConditionError::new(
                    path,
                    format!("{date:?} is not a date (expected YYYY-MM-DD)"),
                    LocalizedMessage::new("rule-issue-date").arg("found", date.as_str()),
                ));
                never()
            }
        },
        Condition::Age { cmp, days } => Node::Age(*cmp, *days),
        Condition::Flag { flag, set } => Node::Flag(*flag, *set),
        Condition::Tag { tag } => Node::Tag(tag.clone()),
        Condition::Account { account_id } => Node::Account(account_id.clone()),
        Condition::Folder { folder } => Node::Folder(folder.clone()),
        Condition::Auth { method, result } => Node::Auth(*method, *result),
        Condition::ListId { matches } => match matches {
            None => Node::ListId(None),
            Some(m) => matcher(m, path, errors).map_or_else(never, |m| Node::ListId(Some(m))),
        },
    }
}

/// The results for `method` in the topmost `Authentication-Results` header
/// (the one the receiving server added; lower ones may come from anyone).
pub(crate) fn auth_results(message: &dyn MessageView, method: AuthMethod) -> Vec<String> {
    let key = match method {
        AuthMethod::Dkim => "dkim",
        AuthMethod::Spf => "spf",
        AuthMethod::Dmarc => "dmarc",
    };
    let Some(header) = message
        .header_values("Authentication-Results")
        .into_iter()
        .next()
    else {
        return Vec::new();
    };
    header
        .split(';')
        .skip(1) // the authserv-id
        .filter_map(|part| {
            let part = part.trim();
            let (name, rest) = part.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case(key) {
                return None;
            }
            let result: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            Some(result.to_lowercase())
        })
        .collect()
}

fn result_name(result: AuthResult) -> &'static str {
    match result {
        AuthResult::Pass => "pass",
        AuthResult::Fail => "fail",
        AuthResult::Softfail => "softfail",
        AuthResult::Neutral => "neutral",
        AuthResult::None => "none",
        AuthResult::Temperror => "temperror",
        AuthResult::Permerror => "permerror",
    }
}

/// `Some(value)`: matched on that value; `None`: did not match. The flag is
/// whether it matched (for tests with no single value).
type Decision = (bool, Option<String>);

fn first(m: &Matcher, values: &[String]) -> Decision {
    match m.first_match(values.iter().map(String::as_str)) {
        Some(v) => (true, Some(excerpt(v))),
        None => (false, values.first().map(|v| excerpt(v))),
    }
}

fn leaf(node: &Node, message: &dyn MessageView, env: &Env<'_>) -> Decision {
    match node {
        Node::All(_) | Node::Any(_) | Node::Not(_) => unreachable!("groups are not leaves"),
        Node::Header(name, None) => {
            let values = message.header_values(name);
            (!values.is_empty(), values.first().map(|v| excerpt(v)))
        }
        Node::Header(name, Some(m)) => first(m, &message.header_values(name)),
        Node::Address(field, m) => {
            let candidates: Vec<String> = message
                .addresses(*field)
                .into_iter()
                .flat_map(|a| {
                    let mut v = vec![a.email.clone()];
                    if let Some(name) = a.name {
                        v.push(format!("{name} <{}>", a.email));
                        v.push(name);
                    }
                    v
                })
                .collect();
            first(m, &candidates)
        }
        Node::Subject(m) => first(m, &[message.subject()]),
        Node::Body(m) => match message.body_text() {
            None => (false, None),
            Some(body) => (m.is_match(&body), None),
        },
        Node::Query(q) => (crate::query::matches(q, message, env), None),
        Node::Attachment(name, mime) => {
            let attachments = message.attachments();
            let found = attachments.iter().find(|a| {
                name.as_ref()
                    .is_none_or(|m| a.filename.as_deref().is_some_and(|f| m.is_match(f)))
                    && mime.as_ref().is_none_or(|m| m.is_match(&a.mime))
            });
            match found {
                Some(a) => (
                    true,
                    Some(excerpt(a.filename.as_deref().unwrap_or(&a.mime))),
                ),
                None => (false, None),
            }
        }
        Node::Size(cmp, bytes) => {
            let size = message.size();
            let ok = match cmp {
                Comparison::Greater => size > *bytes,
                Comparison::Less => size < *bytes,
            };
            (ok, Some(size.to_string()))
        }
        Node::Date(cmp, day) => {
            let start = day * 86_400 - i64::from(env.utc_offset_secs);
            let date = message.date();
            let ok = match cmp {
                DateComparison::Before => date < start,
                DateComparison::After => date >= start + 86_400,
            };
            (ok, None)
        }
        Node::Age(cmp, days) => {
            let age = env.now - message.date();
            let limit = i64::from(*days) * 86_400;
            let ok = match cmp {
                Comparison::Greater => age > limit,
                Comparison::Less => age < limit,
            };
            (ok, Some((age.max(0) / 86_400).to_string()))
        }
        Node::Flag(flag, set) => {
            let f = message.flags();
            let state = match flag {
                MessageFlag::Seen => f.seen,
                MessageFlag::Flagged => f.flagged,
                MessageFlag::Answered => f.answered,
                MessageFlag::Draft => f.draft,
            };
            (state == *set, None)
        }
        Node::Tag(tag) => {
            let tags = message.tags();
            let found = tags.iter().find(|t| t.to_lowercase() == tag.to_lowercase());
            (found.is_some(), found.cloned())
        }
        Node::Account(account) => (
            env.account_matches(message.account_id(), account),
            Some(message.account_id().to_string()),
        ),
        Node::Folder(folder) => {
            let resolved = env.resolve_folder(folder, message.account_id());
            let current = env.mailbox.folder(message.folder_id()).map(|f| f.path);
            (
                resolved.is_some_and(|f| &f.id == message.folder_id()),
                current,
            )
        }
        Node::Auth(method, result) => {
            let results = auth_results(message, *method);
            let wanted = result_name(*result);
            let ok = if results.is_empty() {
                *result == AuthResult::None
            } else {
                results.iter().any(|r| r == wanted)
            };
            (ok, results.first().cloned())
        }
        Node::ListId(matcher) => {
            let values = message.header_values("List-Id");
            match matcher {
                None => (!values.is_empty(), values.first().map(|v| excerpt(v))),
                Some(m) => {
                    // Also test the id inside the angle brackets.
                    let mut candidates = values.clone();
                    candidates.extend(values.iter().filter_map(|v| {
                        let start = v.rfind('<')?;
                        let end = v[start..].find('>')? + start;
                        Some(v[start + 1..end].to_owned())
                    }));
                    first(m, &candidates)
                }
            }
        }
    }
}

fn eval(
    node: &Node,
    message: &dyn MessageView,
    env: &Env<'_>,
    path: &mut Vec<u32>,
    mut hits: Option<&mut Vec<ConditionHit>>,
) -> bool {
    let explain = hits.is_some();
    let mut children = |items: &[Node], want: bool, hits: &mut Option<&mut Vec<ConditionHit>>| {
        // `want`: the result that decides the group (false for all, true
        // for any).
        let mut decided = false;
        for (i, child) in items.iter().enumerate() {
            path.push(u32::try_from(i).unwrap_or(u32::MAX));
            let r = eval(child, message, env, path, hits.as_deref_mut());
            path.pop();
            if r == want {
                decided = true;
                if !explain {
                    break;
                }
            }
        }
        decided
    };
    let (matched, value) = match node {
        Node::All(items) => (!children(items, false, &mut hits), None),
        Node::Any(items) => (children(items, true, &mut hits), None),
        Node::Not(inner) => {
            path.push(0);
            let r = eval(inner, message, env, path, hits.as_deref_mut());
            path.pop();
            (!r, None)
        }
        leaf_node => leaf(leaf_node, message, env),
    };
    if let Some(hits) = hits {
        hits.push(ConditionHit {
            path: path.clone(),
            matched,
            value,
        });
    }
    matched
}
