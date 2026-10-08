//! Writing `rules.kdl`: a canonical renderer and a format-preserving
//! updater.
//!
//! [`update_text`] turns an existing file plus the desired rules into the
//! new file text:
//!
//! * a rule whose meaning did not change keeps its text byte for byte,
//!   together with the comment lines directly above it (it moves with them
//!   when rules are reordered);
//! * the places rules occupy in the file stay where they are: reordering
//!   permutes rules over those places, so free-standing comments (separated
//!   from a rule by a blank line) and blank lines stay put;
//! * a changed rule is rewritten node by node: every node (header, `when`,
//!   `then`, a condition, an action) that is structurally unchanged keeps
//!   its text and the comments before it; changed nodes are written in the
//!   canonical form, recursing into child blocks;
//! * a removed rule takes the comment block directly above it along;
//! * new rules go after their predecessor in the desired order, in the
//!   canonical form, with the file's indentation and line endings.
//!
//! [`crate::rules::RulesDocument::set_rules`] re-reads the result and falls
//! back to the canonical form of the whole file should it ever not mean
//! the desired rules.

use kdl::{KdlDocument, KdlNode};
use nail_api::rules::{
    Comparison, Condition, DateComparison, FolderRef, Rule, RuleAction, RuleScope, RuleTrigger,
    TextMatch,
};

use super::read::{
    ADDRESS_FIELDS, AUTH_METHODS, AUTH_RESULTS, FLAGS, ROLES, SIMPLE_TRIGGERS, TEXT_OPS, name_of,
};
use super::{normalize_rule, rule_id_base, same_rule};
use crate::kdl_util::children;
pub(crate) use crate::kdl_write::detect_indent;
use crate::kdl_write::{Line, same_header, same_node};

// ---------------------------------------------------------------------------
// Canonical rendering

impl Line {
    fn text_match(self, m: &TextMatch) -> Self {
        let line = self.prop(name_of(TEXT_OPS, &m.op), &m.value);
        if m.case_sensitive {
            line.raw_prop("case-sensitive", "#true")
        } else {
            line
        }
    }

    fn account(self, account: Option<&nail_api::AccountId>) -> Self {
        self.prop_opt("account", account.map(nail_api::AccountId::as_str))
    }
}

fn simple_trigger_name(trigger: &RuleTrigger) -> Option<&'static str> {
    SIMPLE_TRIGGERS
        .iter()
        .find(|(_, t)| t == trigger)
        .map(|(k, _)| *k)
}

fn scope_spec(scope: &RuleScope) -> (String, Option<&nail_api::AccountId>) {
    match scope {
        RuleScope::Account { account_id } => (format!("account:{account_id}"), None),
        RuleScope::Folder {
            folder: FolderRef::Path { path, account_id },
        } => (format!("folder:{path}"), account_id.as_ref()),
        RuleScope::Folder {
            folder: FolderRef::Role { role, account_id },
        } => (
            format!("role:{}", name_of(ROLES, role)),
            account_id.as_ref(),
        ),
    }
}

fn folder_line(name: &str, folder: &FolderRef) -> Line {
    match folder {
        FolderRef::Path { path, account_id } => {
            Line::new(name).arg(path).account(account_id.as_ref())
        }
        FolderRef::Role { role, account_id } => Line::new(name)
            .prop("role", name_of(ROLES, role))
            .account(account_id.as_ref()),
    }
}

/// One rule in the canonical layout, ending with a newline.
pub(crate) fn render_rule(rule: &Rule, indent: &str) -> String {
    let mut line = Line::new("rule").arg(&rule.name);
    if rule.id.as_str() != rule_id_base(&rule.name) {
        line = line.prop("id", rule.id.as_str());
    }
    if !rule.enabled {
        line = line.raw_prop("enabled", "#false");
    }
    let mut children = Vec::new();
    match rule.triggers.as_slice() {
        [single] if simple_trigger_name(single).is_some() => {
            line = line.prop("trigger", simple_trigger_name(single).unwrap_or_default());
        }
        triggers => {
            for trigger in triggers {
                let t = match trigger {
                    RuleTrigger::Schedule { interval_minutes } => Line::new("trigger")
                        .arg("schedule")
                        .raw_prop("every", interval_minutes),
                    RuleTrigger::OlderThan { days } => Line::new("trigger")
                        .arg("older-than")
                        .raw_prop("days", days),
                    other => {
                        Line::new("trigger").arg(simple_trigger_name(other).unwrap_or_default())
                    }
                };
                children.push(t.finish(1, indent, &[]));
            }
        }
    }
    match rule.scope.as_slice() {
        [single @ RuleScope::Account { .. }] => {
            line = line.prop("scope", &scope_spec(single).0);
        }
        scopes => {
            for scope in scopes {
                let (spec, account) = scope_spec(scope);
                children.push(Line::new("scope").arg(&spec).account(account).finish(
                    1,
                    indent,
                    &[],
                ));
            }
        }
    }
    let (mode, conditions) = match &rule.conditions {
        Condition::All { conditions } => (None, conditions.as_slice()),
        Condition::Any { conditions } => (Some("any"), conditions.as_slice()),
        other => (None, std::slice::from_ref(other)),
    };
    if mode.is_some() || !conditions.is_empty() {
        let mut when = Line::new("when");
        if let Some(mode) = mode {
            when = when.raw_arg(mode);
        }
        let rendered: Vec<String> = conditions
            .iter()
            .map(|c| render_condition(c, 2, indent))
            .collect();
        children.push(if rendered.is_empty() {
            format!("{indent}{} {{\n{indent}}}\n", when.0)
        } else {
            when.finish(1, indent, &rendered)
        });
    }
    if !rule.actions.is_empty() {
        let rendered: Vec<String> = rule
            .actions
            .iter()
            .map(|a| render_action(a, 2, indent))
            .collect();
        children.push(Line::new("then").finish(1, indent, &rendered));
    }
    if children.is_empty() {
        // Keep the block so the rule reads as a rule, not a stray line.
        return format!("{} {{\n}}\n", line.0);
    }
    line.finish(0, indent, &children)
}

fn render_group(name: &str, conditions: &[Condition], depth: usize, indent: &str) -> String {
    let rendered: Vec<String> = conditions
        .iter()
        .map(|c| render_condition(c, depth + 1, indent))
        .collect();
    if rendered.is_empty() {
        format!(
            "{}{name} {{\n{}}}\n",
            indent.repeat(depth),
            indent.repeat(depth)
        )
    } else {
        Line::new(name).finish(depth, indent, &rendered)
    }
}

pub(crate) fn render_condition(condition: &Condition, depth: usize, indent: &str) -> String {
    let leaf = |line: Line| line.finish(depth, indent, &[]);
    match condition {
        Condition::All { conditions } => render_group("all", conditions, depth, indent),
        Condition::Any { conditions } => render_group("any", conditions, depth, indent),
        Condition::Not { condition } => match condition.as_ref() {
            Condition::All { conditions } if conditions.len() != 1 => {
                render_group("not", conditions, depth, indent)
            }
            inner => Line::new("not").finish(
                depth,
                indent,
                &[render_condition(inner, depth + 1, indent)],
            ),
        },
        Condition::Header { name, matches } => {
            let line = Line::new("header").arg(name);
            leaf(match matches {
                Some(m) => line.text_match(m),
                None => line.raw_prop("exists", "#true"),
            })
        }
        Condition::Address { field, matches } => {
            leaf(Line::new(name_of(ADDRESS_FIELDS, field)).text_match(matches))
        }
        Condition::Subject { matches } => leaf(Line::new("subject").text_match(matches)),
        Condition::Body { matches } => leaf(Line::new("body").text_match(matches)),
        Condition::Query { query } => leaf(Line::new("query").arg(query)),
        Condition::Attachment { name, mime } => {
            let mut kids = Vec::new();
            if let Some(m) = name {
                kids.push(
                    Line::new("name")
                        .text_match(m)
                        .finish(depth + 1, indent, &[]),
                );
            }
            if let Some(m) = mime {
                kids.push(
                    Line::new("type")
                        .text_match(m)
                        .finish(depth + 1, indent, &[]),
                );
            }
            Line::new("attachment").finish(depth, indent, &kids)
        }
        Condition::Size { cmp, bytes } => leaf(Line::new("size").raw_prop(
            match cmp {
                Comparison::Greater => "larger",
                Comparison::Less => "smaller",
            },
            bytes,
        )),
        Condition::Date { cmp, date } => leaf(Line::new("date").prop(
            match cmp {
                DateComparison::Before => "before",
                DateComparison::After => "after",
            },
            date,
        )),
        Condition::Age { cmp, days } => leaf(Line::new("age").raw_prop(
            match cmp {
                Comparison::Greater => "older-than",
                Comparison::Less => "newer-than",
            },
            days,
        )),
        Condition::Flag { flag, set } => {
            let line = Line::new("flag").arg(name_of(FLAGS, flag));
            leaf(if *set {
                line
            } else {
                line.raw_prop("set", "#false")
            })
        }
        Condition::Tag { tag } => leaf(Line::new("tag").arg(tag)),
        Condition::Account { account_id } => leaf(Line::new("account").arg(account_id.as_str())),
        Condition::Folder { folder } => leaf(folder_line("folder", folder)),
        Condition::Auth { method, result } => {
            leaf(Line::new(name_of(AUTH_METHODS, method)).arg(name_of(AUTH_RESULTS, result)))
        }
        Condition::ListId { matches } => {
            let line = Line::new("list-id");
            leaf(match matches {
                Some(m) => line.text_match(m),
                None => line,
            })
        }
    }
}

fn bool_arg(line: Line, value: bool) -> Line {
    if value { line } else { line.raw_arg("#false") }
}

pub(crate) fn render_action(action: &RuleAction, depth: usize, indent: &str) -> String {
    let line = match action {
        RuleAction::Move { folder } => folder_line("move", folder),
        RuleAction::Copy { folder } => folder_line("copy", folder),
        RuleAction::Delete { permanent } => {
            let line = Line::new("delete");
            if *permanent {
                line.raw_prop("permanent", "#true")
            } else {
                line
            }
        }
        RuleAction::Archive => Line::new("archive"),
        RuleAction::Tag { tag } => Line::new("tag").arg(tag),
        RuleAction::Untag { tag } => Line::new("untag").arg(tag),
        RuleAction::Flag { flagged } => bool_arg(Line::new("flag"), *flagged),
        RuleAction::MarkRead { read } => bool_arg(Line::new("mark-read"), *read),
        RuleAction::Notify { notify } => bool_arg(Line::new("notify"), *notify),
        RuleAction::Forward { to, identity_id } => to
            .iter()
            .fold(Line::new("forward"), |l, a| l.arg(a))
            .prop_opt(
                "identity",
                identity_id.as_ref().map(nail_api::IdentityId::as_str),
            ),
        RuleAction::Redirect { to } => to.iter().fold(Line::new("redirect"), |l, a| l.arg(a)),
        RuleAction::Reply {
            template,
            identity_id,
        } => Line::new("reply").arg(template).prop_opt(
            "identity",
            identity_id.as_ref().map(nail_api::IdentityId::as_str),
        ),
        RuleAction::Stop => Line::new("stop"),
    };
    line.finish(depth, indent, &[])
}

/// A whole file: the rules separated by blank lines.
pub(crate) fn render_all(rules: &[Rule], indent: &str) -> String {
    rules
        .iter()
        .map(|r| render_rule(&normalize_rule(r.clone()), indent))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Lexical helpers

/// Where a node is in its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Extent {
    /// First byte of the node (its name).
    start: usize,
    /// One past the node's last byte (its closing `}` or last entry), not
    /// including a terminator or trailing comment.
    end: usize,
    /// The `{` opening its children block.
    open: Option<usize>,
}

/// Skips a string, comment or other token that starts at `i` and returns
/// the index after it, or `None` when `b[i]` is an ordinary character.
fn skip_token(b: &[u8], i: usize) -> Option<usize> {
    let at = |k: usize| b.get(k).copied();
    match b[i] {
        b'/' if at(i + 1) == Some(b'*') => {
            let mut level = 0usize;
            let mut k = i;
            while k < b.len() {
                if b[k] == b'/' && at(k + 1) == Some(b'*') {
                    level += 1;
                    k += 2;
                } else if b[k] == b'*' && at(k + 1) == Some(b'/') {
                    level -= 1;
                    k += 2;
                    if level == 0 {
                        return Some(k);
                    }
                } else {
                    k += 1;
                }
            }
            Some(b.len())
        }
        b'#' => {
            let hashes = b[i..].iter().take_while(|&&c| c == b'#').count();
            let mut k = i + hashes;
            if at(k) != Some(b'"') {
                return Some(k); // a keyword like #true
            }
            let quotes = if b[k..].starts_with(b"\"\"\"") { 3 } else { 1 };
            k += quotes;
            let mut end = vec![b'"'; quotes];
            end.extend(std::iter::repeat_n(b'#', hashes));
            Some(
                b.get(k..)
                    .and_then(|rest| rest.windows(end.len()).position(|w| w == end.as_slice()))
                    .map_or(b.len(), |p| k + p + end.len()),
            )
        }
        b'"' => {
            let quotes = if b[i..].starts_with(b"\"\"\"") { 3 } else { 1 };
            let mut k = i + quotes;
            while k < b.len() {
                if b[k] == b'\\' {
                    k += 2;
                } else if b[k..].starts_with(&b"\"\"\""[..quotes]) {
                    return Some(k + quotes);
                } else {
                    k += 1;
                }
            }
            Some(b.len())
        }
        _ => None,
    }
}

/// The extent of the node whose name starts at `start`.
fn extent(text: &str, start: usize) -> Extent {
    let b = text.as_bytes();
    let mut depth = 0usize;
    let mut open = None;
    let mut i = start;
    let mut last = start; // one past the last significant byte at depth 0
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            if depth == 0 {
                break;
            }
            i = b[i..]
                .iter()
                .position(|&x| x == b'\n')
                .map_or(b.len(), |p| i + p);
            continue;
        }
        if let Some(next) = skip_token(b, i) {
            i = next;
            if depth == 0 && c != b'/' {
                last = i;
            }
            continue;
        }
        match c {
            b'{' => {
                if depth == 0 && open.is_none() {
                    open = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                if depth == 0 {
                    return Extent {
                        start,
                        end: i + 1,
                        open,
                    };
                }
            }
            b'\\' if depth == 0 => {
                // Line continuation: skip to the next line.
                i = b[i..]
                    .iter()
                    .position(|&x| x == b'\n')
                    .map_or(b.len(), |p| i + p + 1);
                continue;
            }
            b'\n' | b';' if depth == 0 => break,
            b' ' | b'\t' | b'\r' => {}
            _ if depth == 0 => last = i + 1,
            _ => {}
        }
        i += 1;
    }
    Extent {
        start,
        end: last,
        open,
    }
}

/// Past spaces and a `// comment` that follow `end` on the same line (not
/// the line break).
fn trailing_end(text: &str, end: usize) -> usize {
    let b = text.as_bytes();
    let mut i = end;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b';') {
        i += 1;
    }
    if b[i..].starts_with(b"//") {
        let eol = b[i..]
            .iter()
            .position(|&x| x == b'\n')
            .map_or(b.len(), |p| i + p);
        return if eol > i && b[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };
    }
    end
}

/// The start of `pos`'s line if only whitespace precedes it there, else
/// `pos`.
fn line_start(text: &str, pos: usize) -> usize {
    let start = text[..pos].rfind('\n').map_or(0, |p| p + 1);
    if text[start..pos].trim().is_empty() {
        start
    } else {
        pos
    }
}

/// The start of the comment lines (`// …`) directly above the line that
/// starts at `start` (no blank line in between).
fn attached_comments_start(text: &str, start: usize) -> usize {
    let mut begin = start;
    while begin > 0 {
        let prev_end = begin - 1; // the '\n' ending the previous line
        let prev_start = text[..prev_end].rfind('\n').map_or(0, |p| p + 1);
        let line = text[prev_start..prev_end].trim();
        if line.starts_with("//") {
            begin = prev_start;
        } else {
            break;
        }
    }
    begin
}

/// Past the line break at `pos` (if any).
fn past_newline(text: &str, pos: usize) -> usize {
    let rest = &text[pos..];
    if rest.starts_with("\r\n") {
        pos + 2
    } else if rest.starts_with('\n') {
        pos + 1
    } else {
        pos
    }
}

// ---------------------------------------------------------------------------
// Structural comparison

/// What identifies a child among its siblings when it changed: its name
/// and first argument.
fn match_key(n: &KdlNode) -> (String, Option<String>) {
    (
        n.name().value().to_owned(),
        n.entries()
            .iter()
            .find(|e| e.name().is_none())
            .map(|e| e.value().to_string()),
    )
}

// ---------------------------------------------------------------------------
// Preserving merge of one node

struct Side<'a> {
    text: &'a str,
}

impl Side<'_> {
    fn extent(&self, node: &KdlNode) -> Extent {
        extent(self.text, node.name().span().offset())
    }

    /// The node's own text (from its name to its end).
    fn node_text(&self, node: &KdlNode) -> &str {
        let e = self.extent(node);
        &self.text[e.start..e.end]
    }

    /// The header text: from the name up to (not including) ` {`.
    fn header_text(&self, node: &KdlNode) -> &str {
        let e = self.extent(node);
        self.text[e.start..e.open.unwrap_or(e.end)].trim_end()
    }
}

/// Emits `new` (a node of the canonical text) at `depth`, reusing `old`
/// (a node of the original text) where it is unchanged.
fn merge_node(
    old_side: &Side<'_>,
    old: &KdlNode,
    new_side: &Side<'_>,
    new: &KdlNode,
    depth: usize,
    indent: &str,
) -> String {
    if same_node(old, new) {
        return old_side.node_text(old).to_owned();
    }
    let header = if same_header(old, new) {
        old_side.header_text(old)
    } else {
        new_side.header_text(new)
    };
    let new_kids = children(new);
    if new_kids.is_empty() {
        return header.to_owned();
    }
    let old_kids = children(old);
    let old_extent = old_side.extent(old);
    let text = old_side.text;
    // Each old child: its leading text (line break, blank lines, comments
    // and indentation before it) and its text through a trailing comment.
    let mut pieces: Vec<(&str, &str)> = Vec::new();
    let mut boundary = old_extent.open.map_or(old_extent.end, |o| o + 1);
    // A comment on the line of `{` stays there.
    let opening = &text[boundary..trailing_end(text, boundary)];
    boundary += opening.len();
    for child in old_kids {
        let e = old_side.extent(child);
        let tail = trailing_end(text, e.end);
        let leading_end = if text[boundary..e.start].contains('\n') {
            // Keep the child's indentation as part of its text.
            line_start(text, e.start)
        } else {
            e.start
        };
        pieces.push((&text[boundary..leading_end], &text[leading_end..tail]));
        boundary = tail;
    }
    let closing = if old_kids.is_empty() || old_extent.open.is_none() {
        None
    } else {
        // Text between the last child and the closing brace.
        Some(&text[boundary..old_extent.end - 1])
    };

    let mut used = vec![false; old_kids.len()];
    let mut out = format!("{header} {{{opening}");
    for child in new_kids {
        let exact = (0..old_kids.len()).find(|&i| !used[i] && same_node(&old_kids[i], child));
        let similar = exact.or_else(|| {
            (0..old_kids.len()).find(|&i| !used[i] && match_key(&old_kids[i]) == match_key(child))
        });
        if let Some(i) = similar {
            used[i] = true;
            let (leading, own) = pieces[i];
            let leading = if leading.contains('\n') {
                leading
            } else {
                "\n"
            };
            out.push_str(leading);
            if exact.is_some() {
                out.push_str(own);
            } else {
                let own_start = own.len() - own.trim_start().len();
                out.push_str(&own[..own_start]);
                if own_start == 0 {
                    out.push_str(&indent.repeat(depth + 1));
                }
                out.push_str(&merge_node(
                    old_side,
                    &old_kids[i],
                    new_side,
                    child,
                    depth + 1,
                    indent,
                ));
            }
        } else {
            out.push('\n');
            out.push_str(&indent.repeat(depth + 1));
            out.push_str(new_side.node_text(child));
        }
    }
    match closing {
        Some(closing) if closing.contains('\n') => out.push_str(closing),
        _ => {
            out.push('\n');
            out.push_str(&indent.repeat(depth));
        }
    }
    out.push('}');
    out
}

// ---------------------------------------------------------------------------
// File update

#[derive(Debug)]
enum Piece {
    /// Text kept as it is.
    Gap(String),
    /// The place of old rule `.0`, to be filled.
    Slot(usize),
    /// Desired rule `.0`, written from old rule `.1` if any.
    Rule(usize, Option<usize>),
}

/// The new text of a file that held `old` (as read from `text`) and should
/// hold `wanted`.
pub(crate) fn update_text(text: &str, old: &[Rule], wanted: &[Rule]) -> String {
    let crlf = text.contains("\r\n");
    let indent = detect_indent(text);
    let render = |rule: &Rule| {
        let s = render_rule(rule, &indent);
        if crlf { s.replace('\n', "\r\n") } else { s }
    };
    let newline = if crlf { "\r\n" } else { "\n" };

    let doc = match KdlDocument::parse_v2(text) {
        Ok(doc) if doc.nodes().len() == old.len() => doc,
        _ => return render_all(wanted, &indent),
    };

    // Segments of the old rules: attached comments + node + rest of line.
    let mut segments: Vec<(usize, usize, usize)> = Vec::new(); // (seg start, node start, seg end)
    for node in doc.nodes() {
        let e = extent(text, node.name().span().offset());
        let start = attached_comments_start(text, line_start(text, e.start));
        let end = past_newline(text, trailing_end(text, e.end));
        segments.push((start.max(segments.last().map_or(0, |s| s.2)), e.start, end));
    }

    let mut pieces: Vec<Piece> = Vec::new();
    let mut pos = 0;
    for (i, (start, _, end)) in segments.iter().enumerate() {
        pieces.push(Piece::Gap(text[pos..*start].to_owned()));
        pieces.push(Piece::Slot(i));
        pos = *end;
    }
    pieces.push(Piece::Gap(text[pos..].to_owned()));

    // Removed rules leave with one adjacent whitespace-only gap.
    let is_blank = |p: &Piece| matches!(p, Piece::Gap(g) if g.trim().is_empty());
    let mut i = 0;
    while i < pieces.len() {
        let removed =
            matches!(pieces[i], Piece::Slot(o) if !wanted.iter().any(|w| w.id == old[o].id));
        if !removed {
            i += 1;
            continue;
        }
        pieces.remove(i);
        if i < pieces.len() && is_blank(&pieces[i]) && i + 1 < pieces.len() {
            pieces.remove(i);
        } else if i > 0 && is_blank(&pieces[i - 1]) && i > 1 {
            pieces.remove(i - 1);
        }
    }

    // Kept rules fill the remaining places in their new order.
    let mut kept = wanted
        .iter()
        .enumerate()
        .filter_map(|(w, rule)| old.iter().position(|o| o.id == rule.id).map(|o| (w, o)));
    for piece in &mut pieces {
        if let Piece::Slot(_) = piece
            && let Some((w, o)) = kept.next()
        {
            *piece = Piece::Rule(w, Some(o));
        }
    }

    // New rules go after their predecessor (or before the first rule).
    for (w, rule) in wanted.iter().enumerate() {
        if old.iter().any(|o| o.id == rule.id) {
            continue;
        }
        let after = w.checked_sub(1).and_then(|p| {
            pieces
                .iter()
                .position(|x| matches!(x, Piece::Rule(pw, _) if *pw == p))
        });
        match after {
            Some(at) => {
                pieces.insert(at + 1, Piece::Rule(w, None));
                pieces.insert(at + 1, Piece::Gap(newline.to_owned()));
            }
            None => {
                if let Some(at) = pieces.iter().position(|x| matches!(x, Piece::Rule(..))) {
                    pieces.insert(at, Piece::Gap(newline.to_owned()));
                    pieces.insert(at, Piece::Rule(w, None));
                } else {
                    let last_text: String = pieces
                        .iter()
                        .map(|p| match p {
                            Piece::Gap(g) => g.as_str(),
                            _ => "",
                        })
                        .collect();
                    if !last_text.is_empty() {
                        if !last_text.ends_with('\n') {
                            pieces.push(Piece::Gap(newline.to_owned()));
                        }
                        if !last_text.trim().is_empty() || last_text.ends_with('\n') {
                            pieces.push(Piece::Gap(newline.to_owned()));
                        }
                    }
                    pieces.push(Piece::Rule(w, None));
                }
            }
        }
    }

    let old_side = Side { text };
    let mut out = String::with_capacity(text.len() + 256);
    let count = pieces.len();
    for (index, piece) in pieces.iter().enumerate() {
        let written = match piece {
            Piece::Gap(g) => g.clone(),
            Piece::Slot(_) => String::new(),
            Piece::Rule(w, None) => render(&wanted[*w]),
            Piece::Rule(w, Some(o)) => {
                let (seg_start, node_start, seg_end) = segments[*o];
                if same_rule(&wanted[*w], &old[*o]) {
                    text[seg_start..seg_end].to_owned()
                } else {
                    let canonical = render_rule(&normalize_rule(wanted[*w].clone()), &indent);
                    let merged = KdlDocument::parse_v2(&canonical)
                        .ok()
                        .and_then(|d| d.nodes().first().cloned())
                        .map(|new_node| {
                            let new_side = Side { text: &canonical };
                            merge_node(
                                &old_side,
                                &doc.nodes()[*o],
                                &new_side,
                                &new_node,
                                0,
                                &indent,
                            )
                        });
                    match merged {
                        Some(node_text) => {
                            let node_end = extent(text, node_start).end;
                            let rest = &text[node_end..seg_end];
                            let mut s = String::new();
                            s.push_str(&text[seg_start..node_start]);
                            s.push_str(&if crlf {
                                node_text.replace("\r\n", "\n").replace('\n', "\r\n")
                            } else {
                                node_text
                            });
                            s.push_str(rest);
                            s
                        }
                        None => render(&wanted[*w]),
                    }
                }
            }
        };
        out.push_str(&written);
        // A rule that ended the file without a line break may move.
        if matches!(piece, Piece::Rule(..)) && index + 1 < count && !out.ends_with('\n') {
            out.push_str(newline);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extents_cover_nodes_and_blocks() {
        let text = "rule \"a\" x=1 { when { tag \"}\" } } // c\nnext\n";
        let e = extent(text, 0);
        assert_eq!(
            &text[e.start..e.end],
            "rule \"a\" x=1 { when { tag \"}\" } }"
        );
        assert_eq!(e.open, Some(13));
        assert_eq!(trailing_end(text, e.end), text.find('\n').unwrap());

        let text = "tag \"x\" // note\n";
        let e = extent(text, 0);
        assert_eq!(&text[e.start..e.end], "tag \"x\"");
        assert!(e.open.is_none());

        let text = "a 1 /* {{ */ 2; b";
        assert_eq!(&text[..extent(text, 0).end], "a 1 /* {{ */ 2");

        let text = "a #\"raw } {\"# \\\n  b=1\nc";
        assert_eq!(&text[..extent(text, 0).end], "a #\"raw } {\"# \\\n  b=1");

        let text = "{ inner x }";
        assert_eq!(&text[2..extent(text, 2).end], "inner x");
    }

    #[test]
    fn comment_attachment() {
        let text = "// free\n\n// one\n// two\nrule \"a\"\n";
        let start = text.find("rule").unwrap();
        assert_eq!(
            &text[attached_comments_start(text, start)..start],
            "// one\n// two\n"
        );
        assert_eq!(line_start("  x", 2), 0);
        assert_eq!(line_start("a x", 2), 2);
    }
}
