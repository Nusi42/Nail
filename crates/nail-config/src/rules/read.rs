//! Reading `rules.kdl` into [`Rule`]s, validating every node, property and
//! value with precise positions. Errors are collected, not returned at the
//! first one: a broken condition, action, trigger or scope is reported and
//! reading goes on with its next sibling, so the file shows all its errors
//! at once.

use std::cell::RefCell;
use std::collections::HashSet;

use kdl::{KdlEntry, KdlNode, KdlValue};
use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef,
    MessageFlag, Rule, RuleAction, RuleScope, RuleTrigger, TextMatch, TextOp,
};
use nail_api::{AccountId, FolderRole, IdentityId, LocalizedMessage, RuleId};

use super::{derive_rule_id, is_valid_rule_id};
use crate::error::ConfigError;
use crate::kdl_util::{Source, args, children, describe, name, props};
use crate::l10n::msg;

/// Text comparison properties, in the order they are listed in messages.
pub(crate) const TEXT_OPS: &[(&str, TextOp)] = &[
    ("equals", TextOp::Equals),
    ("contains", TextOp::Contains),
    ("starts-with", TextOp::StartsWith),
    ("ends-with", TextOp::EndsWith),
    ("regex", TextOp::Regex),
];

pub(crate) const ADDRESS_FIELDS: &[(&str, AddressField)] = &[
    ("from", AddressField::From),
    ("to", AddressField::To),
    ("cc", AddressField::Cc),
    ("recipients", AddressField::Recipients),
    ("reply-to", AddressField::ReplyTo),
    ("sender", AddressField::Sender),
];

pub(crate) const FLAGS: &[(&str, MessageFlag)] = &[
    ("seen", MessageFlag::Seen),
    ("flagged", MessageFlag::Flagged),
    ("answered", MessageFlag::Answered),
    ("draft", MessageFlag::Draft),
];

pub(crate) const AUTH_METHODS: &[(&str, AuthMethod)] = &[
    ("dkim", AuthMethod::Dkim),
    ("spf", AuthMethod::Spf),
    ("dmarc", AuthMethod::Dmarc),
];

pub(crate) const AUTH_RESULTS: &[(&str, AuthResult)] = &[
    ("pass", AuthResult::Pass),
    ("fail", AuthResult::Fail),
    ("softfail", AuthResult::Softfail),
    ("neutral", AuthResult::Neutral),
    ("none", AuthResult::None),
    ("temperror", AuthResult::Temperror),
    ("permerror", AuthResult::Permerror),
];

pub(crate) const ROLES: &[(&str, FolderRole)] = &[
    ("inbox", FolderRole::Inbox),
    ("sent", FolderRole::Sent),
    ("drafts", FolderRole::Drafts),
    ("archive", FolderRole::Archive),
    ("junk", FolderRole::Junk),
    ("trash", FolderRole::Trash),
];

/// Triggers without parameters, usable in `trigger="…"`.
pub(crate) const SIMPLE_TRIGGERS: &[(&str, RuleTrigger)] = &[
    ("arrival", RuleTrigger::Arrival),
    ("on-send", RuleTrigger::OnSend),
    ("manual", RuleTrigger::Manual),
];

/// Condition node names (for "unknown condition" messages).
const CONDITIONS: &str = "all, any, not, header, from, to, cc, recipients, reply-to, sender, subject, body, query, attachment, size, date, age, flag, tag, account, folder, dkim, spf, dmarc, list-id";

/// Action node names.
const ACTIONS: &str = "move, copy, delete, archive, tag, untag, flag, mark-read, forward, redirect, reply, notify, stop";

pub(crate) fn lookup<T: Clone>(table: &[(&str, T)], key: &str) -> Option<T> {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
}

pub(crate) fn name_of<T: PartialEq>(table: &[(&'static str, T)], value: &T) -> &'static str {
    table
        .iter()
        .find(|(_, v)| v == value)
        .map_or("", |(k, _)| k)
}

fn names<T>(table: &[(&str, T)]) -> String {
    table.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
}

type R<T> = Result<T, ConfigError>;

/// Reads every `rule` node. Returns the valid rules and every error.
pub(crate) fn read(src: &Source<'_>) -> (Vec<Rule>, Vec<ConfigError>) {
    let doc = match src.parse() {
        Ok(doc) => doc,
        Err(e) => return (Vec::new(), vec![e]),
    };
    let reader = Reader {
        src,
        errors: RefCell::new(Vec::new()),
    };
    // Explicit ids first, so derived ids never take them.
    let mut explicit: HashSet<String> = HashSet::new();
    for node in doc.nodes().iter().filter(|n| name(n) == "rule") {
        if let Some(entry) = node.entry("id")
            && let Some(id) = entry.value().as_string()
            && is_valid_rule_id(id)
            && !explicit.insert(id.to_owned())
        {
            reader.push(src.entry_err(
                entry,
                msg("config-rule-id-duplicate").arg("id", id.to_string()),
            ));
        }
    }
    let mut taken: HashSet<String> = explicit.clone();
    let mut rules = Vec::new();
    for node in doc.nodes() {
        if name(node) != "rule" {
            reader.push(src.node_err(
                node,
                msg("config-rules-unknown-node").arg("node", name(node)),
            ));
            continue;
        }
        let before = reader.error_count();
        let rule = reader.rule(node, &mut taken);
        if let Some(rule) = rule
            && reader.error_count() == before
        {
            rules.push(rule);
        }
    }
    (rules, reader.errors.into_inner())
}

struct Reader<'s, 'a> {
    src: &'s Source<'a>,
    errors: RefCell<Vec<ConfigError>>,
}

impl Reader<'_, '_> {
    fn push(&self, error: ConfigError) {
        self.errors.borrow_mut().push(error);
    }

    fn error_count(&self) -> usize {
        self.errors.borrow().len()
    }

    /// Records the error of `result`, if any.
    fn ok<T>(&self, result: R<T>) -> Option<T> {
        result.map_err(|e| self.push(e)).ok()
    }

    // --- Small readers -----------------------------------------------------

    fn check_props(&self, node: &KdlNode, allowed: &[&str]) -> R<()> {
        let mut seen = HashSet::new();
        for (key, entry) in props(node) {
            if !allowed.contains(&key) {
                return Err(self.src.entry_err(
                    entry,
                    crate::kdl_util::unknown_property(key, name(node), allowed),
                ));
            }
            if !seen.insert(key) {
                return Err(self
                    .src
                    .entry_err(entry, msg("config-duplicate-property").arg("key", key)));
            }
        }
        Ok(())
    }

    fn check_args(&self, node: &KdlNode, min: usize, max: usize, what: &str) -> R<()> {
        let args = args(node);
        if let Some(extra) = args.get(max) {
            return Err(self.src.entry_err(
                extra,
                if max == 0 {
                    msg("config-no-arguments").arg("node", name(node))
                } else {
                    msg("config-too-many-arguments")
                        .arg("node", name(node))
                        .arg("max", max)
                },
            ));
        }
        if args.len() < min {
            return Err(self.src.node_err(node, msg(what).arg("node", name(node))));
        }
        Ok(())
    }

    fn check_no_children(&self, node: &KdlNode) -> R<()> {
        if let Some(child) = children(node).first() {
            return Err(self
                .src
                .node_err(child, msg("config-no-children").arg("node", name(node))));
        }
        Ok(())
    }

    fn string<'e>(&self, entry: &'e KdlEntry, what: &str) -> R<&'e str> {
        entry.value().as_string().ok_or_else(|| {
            self.src.entry_err(
                entry,
                msg("config-expected-string")
                    .arg("what", what)
                    .arg("found", describe(entry.value())),
            )
        })
    }

    fn nonempty<'e>(&self, entry: &'e KdlEntry, what: &str) -> R<&'e str> {
        let s = self.string(entry, what)?;
        if s.trim().is_empty() {
            return Err(self
                .src
                .entry_err(entry, msg("config-empty").arg("what", what)));
        }
        Ok(s)
    }

    fn arg_string<'n>(&self, node: &'n KdlNode, what: &str) -> R<Option<&'n str>> {
        match args(node).first() {
            None => Ok(None),
            Some(entry) => self.nonempty(entry, what).map(Some),
        }
    }

    fn prop_string<'n>(&self, node: &'n KdlNode, key: &str) -> R<Option<&'n str>> {
        match node.entry(key) {
            None => Ok(None),
            Some(entry) => self.nonempty(entry, &format!("`{key}`")).map(Some),
        }
    }

    fn bool_entry(&self, entry: &KdlEntry, what: &str) -> R<bool> {
        match entry.value() {
            KdlValue::Bool(b) => Ok(*b),
            other => Err(self.src.entry_err(
                entry,
                msg("config-expected-bool")
                    .arg("what", what)
                    .arg("found", describe(other)),
            )),
        }
    }

    fn prop_bool(&self, node: &KdlNode, key: &str) -> R<Option<bool>> {
        node.entry(key)
            .map(|e| self.bool_entry(e, &format!("`{key}`")))
            .transpose()
    }

    /// An optional `#true` / `#false` argument (default `#true`).
    fn arg_bool(&self, node: &KdlNode) -> R<bool> {
        match args(node).first() {
            None => Ok(true),
            Some(entry) => self.bool_entry(entry, &format!("`{}`", name(node))),
        }
    }

    fn uint(&self, entry: &KdlEntry, what: &str, min: u64) -> R<u64> {
        entry
            .value()
            .as_integer()
            .and_then(|i| u64::try_from(i).ok())
            .filter(|n| *n >= min)
            .ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    msg("config-expected-at-least")
                        .arg("what", what)
                        .arg("min", min)
                        .arg("found", describe(entry.value())),
                )
            })
    }

    fn uint32(&self, entry: &KdlEntry, what: &str, min: u64) -> R<u32> {
        let n = self.uint(entry, what, min)?;
        u32::try_from(n).map_err(|_| {
            self.src
                .entry_err(entry, msg("config-number-too-large").arg("what", what))
        })
    }

    fn account_id(&self, entry: &KdlEntry, what: &str) -> R<AccountId> {
        let id = self.nonempty(entry, what)?;
        if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(self
                .src
                .entry_err(entry, msg("config-no-spaces").arg("what", what)));
        }
        Ok(AccountId::new(id))
    }

    fn role(&self, entry: &KdlEntry) -> R<FolderRole> {
        let s = self.string(entry, "`role`")?;
        lookup(ROLES, s).ok_or_else(|| {
            self.src.entry_err(
                entry,
                msg("config-role-unknown")
                    .arg("found", s)
                    .arg("expected", names(ROLES)),
            )
        })
    }

    /// `"<path>"` or `role="<role>"`, plus an optional `account="<id>"`.
    fn folder_ref(&self, node: &KdlNode) -> R<FolderRef> {
        self.check_props(node, &["role", "account"])?;
        self.check_args(node, 0, 1, "")?;
        self.check_no_children(node)?;
        let account_id = node
            .entry("account")
            .map(|e| self.account_id(e, "`account`"))
            .transpose()?;
        match (args(node).first(), node.entry("role")) {
            (Some(path), None) => Ok(FolderRef::Path {
                path: self
                    .nonempty(path, &format!("`{}`", name(node)))?
                    .to_owned(),
                account_id,
            }),
            (None, Some(role)) => Ok(FolderRef::Role {
                role: self.role(role)?,
                account_id,
            }),
            (Some(_), Some(role)) => Err(self.src.entry_err(
                role,
                msg("config-rule-folder-path-or-role").arg("node", name(node)),
            )),
            (None, None) => Err(self.src.node_err(
                node,
                msg("config-rule-needs-folder").arg("node", name(node)),
            )),
        }
    }

    /// Reads the text comparison of a node: exactly one of `equals=`,
    /// `contains=`, `starts-with=`, `ends-with=`, `regex=`, plus
    /// `case-sensitive=`. `None` when the node has none and `required` is
    /// false.
    /// [`Self::text_match`] for a node that must have a comparison.
    fn required_text_match(&self, node: &KdlNode) -> R<TextMatch> {
        match self.text_match(node, true)? {
            Some(matches) => Ok(matches),
            None => Err(self.src.node_err(
                node,
                msg("config-rule-needs-comparison")
                    .arg("node", name(node))
                    .arg("ops", names(TEXT_OPS)),
            )),
        }
    }

    fn text_match(&self, node: &KdlNode, required: bool) -> R<Option<TextMatch>> {
        let mut found: Option<(TextOp, &KdlEntry)> = None;
        for (key, entry) in props(node) {
            if let Some(op) = lookup(TEXT_OPS, key) {
                if found.is_some() {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-rule-one-comparison").arg("node", name(node)),
                    ));
                }
                found = Some((op, entry));
            }
        }
        let case_sensitive = self.prop_bool(node, "case-sensitive")?;
        let Some((op, entry)) = found else {
            if required {
                return Err(self.src.node_err(
                    node,
                    msg("config-rule-needs-comparison")
                        .arg("node", name(node))
                        .arg("ops", names(TEXT_OPS)),
                ));
            }
            if let Some(cs) = node.entry("case-sensitive") {
                return Err(self
                    .src
                    .entry_err(cs, msg("config-rule-case-without-comparison")));
            }
            return Ok(None);
        };
        let value = self.string(entry, &format!("`{}`", name_of(TEXT_OPS, &op)))?;
        if value.is_empty() && op != TextOp::Equals {
            return Err(self
                .src
                .entry_err(entry, msg("config-rule-comparison-empty")));
        }
        Ok(Some(TextMatch {
            op,
            value: value.to_owned(),
            case_sensitive: case_sensitive.unwrap_or(false),
        }))
    }

    fn text_props() -> Vec<&'static str> {
        let mut v: Vec<&str> = TEXT_OPS.iter().map(|(k, _)| *k).collect();
        v.push("case-sensitive");
        v
    }

    // --- Rules -------------------------------------------------------------

    fn rule(&self, node: &KdlNode, taken: &mut HashSet<String>) -> Option<Rule> {
        let header = (|| -> R<(String, Option<RuleId>, bool)> {
            self.check_props(node, &["id", "enabled", "trigger", "scope"])?;
            self.check_args(node, 1, 1, "config-rule-needs-name")?;
            let name = self
                .nonempty(args(node)[0], "the rule name")?
                .trim()
                .to_owned();
            let id = match node.entry("id") {
                None => None,
                Some(entry) => {
                    let id = self.string(entry, "`id`")?;
                    if !is_valid_rule_id(id) {
                        return Err(self
                            .src
                            .entry_err(entry, msg("config-rule-id-invalid").arg("id", id)));
                    }
                    Some(RuleId::new(id))
                }
            };
            let enabled = self.prop_bool(node, "enabled")?.unwrap_or(true);
            Ok((name, id, enabled))
        })();
        let (rule_name, id, enabled) = self.ok(header)?;
        let id = id.unwrap_or_else(|| {
            let id = derive_rule_id(&rule_name, |candidate| taken.contains(candidate));
            taken.insert(id.to_string());
            id
        });

        let mut triggers = Vec::new();
        if let Some(entry) = node.entry("trigger") {
            triggers.extend(self.ok(self.simple_trigger(entry)));
        }
        let mut scope = Vec::new();
        if let Some(entry) = node.entry("scope")
            && let Some(spec) = self.ok(self.string(entry, "`scope`"))
        {
            scope.extend(self.ok(self.scope_spec(entry, spec, None)));
        }
        let mut conditions = None;
        let mut actions = None;
        for child in children(node) {
            match name(child) {
                "trigger" => {
                    if let Some(trigger) = self.ok(self.trigger(child)) {
                        if triggers.contains(&trigger) {
                            self.push(
                                self.src
                                    .node_err(child, msg("config-rule-trigger-duplicate")),
                            );
                        } else {
                            triggers.push(trigger);
                        }
                    }
                }
                "scope" => scope.extend(self.ok(self.scope(child))),
                "when" => {
                    if conditions.is_some() {
                        self.push(
                            self.src
                                .node_err(child, msg("config-rule-one-block").arg("node", "when")),
                        );
                    } else {
                        conditions = Some(self.when(child));
                    }
                }
                "then" => {
                    if actions.is_some() {
                        self.push(
                            self.src
                                .node_err(child, msg("config-rule-one-block").arg("node", "then")),
                        );
                    } else {
                        actions = Some(self.then(child));
                    }
                }
                other => self.push(
                    self.src.node_err(
                        child,
                        msg("config-unknown-child")
                            .arg("node", other)
                            .arg("parent", "rule")
                            .arg("expected", "trigger, scope, when, then"),
                    ),
                ),
            }
        }
        Some(Rule {
            id,
            name: rule_name,
            enabled,
            triggers,
            scope,
            conditions: conditions.unwrap_or_else(Condition::always),
            actions: actions.unwrap_or_default(),
            source: Some(self.src.node_loc(node)),
        })
    }

    fn simple_trigger(&self, entry: &KdlEntry) -> R<RuleTrigger> {
        let s = self.string(entry, "`trigger`")?;
        lookup(SIMPLE_TRIGGERS, s).ok_or_else(|| {
            self.src.entry_err(
                entry,
                if matches!(s, "schedule" | "older-than") {
                    msg("config-rule-trigger-needs-params")
                        .arg("found", s)
                        .arg("expected", names(SIMPLE_TRIGGERS))
                } else {
                    msg("config-rule-trigger-unknown")
                        .arg("found", s)
                        .arg("expected", names(SIMPLE_TRIGGERS))
                },
            )
        })
    }

    /// `trigger "arrival" | "on-send" | "manual"`,
    /// `trigger "schedule" every=<minutes | "30m" | "2h" | "1d">`,
    /// `trigger "older-than" days=<n>`.
    fn trigger(&self, node: &KdlNode) -> R<RuleTrigger> {
        self.check_args(node, 1, 1, "config-rule-needs-trigger-kind")?;
        self.check_no_children(node)?;
        let entry = args(node)[0];
        let kind = self.string(entry, "`trigger`")?;
        match kind {
            "schedule" => {
                self.check_props(node, &["every"])?;
                let every = node.entry("every").ok_or_else(|| {
                    self.src
                        .node_err(node, msg("config-rule-schedule-needs-every"))
                })?;
                Ok(RuleTrigger::Schedule {
                    interval_minutes: self.interval(every)?,
                })
            }
            "older-than" => {
                self.check_props(node, &["days"])?;
                let days = node.entry("days").ok_or_else(|| {
                    self.src
                        .node_err(node, msg("config-rule-older-than-needs-days"))
                })?;
                Ok(RuleTrigger::OlderThan {
                    days: self.uint32(days, "`days`", 1)?,
                })
            }
            _ => {
                self.check_props(node, &[])?;
                self.simple_trigger(entry)
            }
        }
    }

    fn interval(&self, entry: &KdlEntry) -> R<u32> {
        if entry.value().as_integer().is_some() {
            return self.uint32(entry, "`every`", 1);
        }
        let bad = || {
            self.src.entry_err(
                entry,
                msg("config-rule-every-invalid").arg("found", describe(entry.value())),
            )
        };
        let s = entry.value().as_string().ok_or_else(bad)?.trim();
        let (digits, factor) = match s.chars().last() {
            Some('m') => (&s[..s.len() - 1], 1),
            Some('h') => (&s[..s.len() - 1], 60),
            Some('d') => (&s[..s.len() - 1], 1440),
            _ => return Err(bad()),
        };
        digits
            .trim()
            .parse::<u32>()
            .ok()
            .and_then(|n| n.checked_mul(factor))
            .filter(|n| *n >= 1)
            .ok_or_else(bad)
    }

    /// `scope "<spec>" [account="<id>"]`.
    fn scope(&self, node: &KdlNode) -> R<RuleScope> {
        self.check_props(node, &["account"])?;
        self.check_args(node, 1, 1, "config-rule-needs-scope")?;
        self.check_no_children(node)?;
        let entry = args(node)[0];
        let spec = self.string(entry, "`scope`")?;
        let account = node
            .entry("account")
            .map(|e| self.account_id(e, "`account`"))
            .transpose()?;
        self.scope_spec(entry, spec, account)
    }

    fn scope_spec(&self, entry: &KdlEntry, spec: &str, account: Option<AccountId>) -> R<RuleScope> {
        let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
        let rest = rest.trim();
        let bad = |message: LocalizedMessage| Err(self.src.entry_err(entry, message));
        if rest.is_empty() {
            return bad(msg("config-rule-scope-invalid").arg("found", format!("{spec:?}")));
        }
        match kind {
            "account" | "virtual" => {
                if account.is_some() {
                    return bad(
                        msg("config-rule-scope-account-option").arg("kind", format!("{kind:?}"))
                    );
                }
                if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return bad(msg("config-no-spaces").arg("what", "`account`"));
                }
                Ok(RuleScope::Account {
                    account_id: AccountId::new(rest),
                })
            }
            "folder" => Ok(RuleScope::Folder {
                folder: FolderRef::Path {
                    path: rest.to_owned(),
                    account_id: account,
                },
            }),
            "role" => match lookup(ROLES, rest) {
                Some(role) => Ok(RuleScope::Folder {
                    folder: FolderRef::Role {
                        role,
                        account_id: account,
                    },
                }),
                None => bad(msg("config-role-unknown")
                    .arg("found", rest)
                    .arg("expected", names(ROLES))),
            },
            _ => bad(msg("config-rule-scope-kind-unknown").arg("found", format!("{kind:?}"))),
        }
    }

    // --- Conditions --------------------------------------------------------

    /// `when [all|any] { … }`.
    fn when(&self, node: &KdlNode) -> Condition {
        let mode = (|| -> R<&str> {
            self.check_props(node, &[])?;
            self.check_args(node, 0, 1, "")?;
            match args(node).first() {
                None => Ok("all"),
                Some(entry) => {
                    let s = self.string(entry, "`when`")?;
                    if matches!(s, "all" | "any") {
                        Ok(s)
                    } else {
                        Err(self.src.entry_err(
                            entry,
                            msg("config-rule-when-mode").arg("found", format!("{s:?}")),
                        ))
                    }
                }
            }
        })();
        let conditions = self.conditions(node);
        match self.ok(mode) {
            Some("any") => Condition::Any { conditions },
            _ => Condition::All { conditions },
        }
    }

    fn conditions(&self, parent: &KdlNode) -> Vec<Condition> {
        children(parent)
            .iter()
            .filter_map(|child| self.ok(self.condition(child)))
            .collect()
    }

    fn condition(&self, node: &KdlNode) -> R<Condition> {
        let kind = name(node);
        if let Some(field) = lookup(ADDRESS_FIELDS, kind) {
            self.check_props(node, &Self::text_props())?;
            self.check_args(node, 0, 0, "")?;
            self.check_no_children(node)?;
            let matches = self.required_text_match(node)?;
            return Ok(Condition::Address { field, matches });
        }
        if let Some(method) = lookup(AUTH_METHODS, kind) {
            self.check_props(node, &[])?;
            self.check_args(node, 1, 1, "config-rule-needs-result")?;
            self.check_no_children(node)?;
            let entry = args(node)[0];
            let s = self.string(entry, &format!("`{}`", name(node)))?;
            let result = lookup(AUTH_RESULTS, s).ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    msg("config-auth-result-unknown")
                        .arg("found", s)
                        .arg("expected", names(AUTH_RESULTS)),
                )
            })?;
            return Ok(Condition::Auth { method, result });
        }
        match kind {
            "all" | "any" | "not" => {
                self.check_props(node, &[])?;
                self.check_args(node, 0, 0, "")?;
                let before = self.error_count();
                let mut conditions = self.conditions(node);
                if self.error_count() != before {
                    // Keep reading siblings, but do not build a group
                    // that silently lost a child.
                    return Ok(Condition::always());
                }
                Ok(match kind {
                    "all" => Condition::All { conditions },
                    "any" => Condition::Any { conditions },
                    _ if conditions.len() == 1 => Condition::Not {
                        condition: Box::new(conditions.remove(0)),
                    },
                    _ => Condition::Not {
                        condition: Box::new(Condition::All { conditions }),
                    },
                })
            }
            "header" => {
                let mut allowed = Self::text_props();
                allowed.push("exists");
                self.check_props(node, &allowed)?;
                self.check_args(node, 1, 1, "config-rule-needs-header")?;
                self.check_no_children(node)?;
                let header = self.nonempty(args(node)[0], "`header`")?;
                if header.contains(':') || header.chars().any(char::is_whitespace) {
                    return Err(self.src.entry_err(
                        args(node)[0],
                        msg("config-rule-header-name-invalid").arg("found", format!("{header:?}")),
                    ));
                }
                let exists = self.prop_bool(node, "exists")?;
                let matches = self.text_match(node, false)?;
                match (exists, matches) {
                    (Some(_), Some(_)) => Err(self
                        .src
                        .node_err(node, msg("config-rule-header-exists-or-comparison"))),
                    (Some(false), None) => Ok(Condition::Not {
                        condition: Box::new(Condition::Header {
                            name: header.to_owned(),
                            matches: None,
                        }),
                    }),
                    (_, matches) => Ok(Condition::Header {
                        name: header.to_owned(),
                        matches,
                    }),
                }
            }
            "subject" | "body" => {
                self.check_props(node, &Self::text_props())?;
                self.check_args(node, 0, 0, "")?;
                self.check_no_children(node)?;
                let matches = self.required_text_match(node)?;
                Ok(if kind == "subject" {
                    Condition::Subject { matches }
                } else {
                    Condition::Body { matches }
                })
            }
            "query" => {
                self.check_props(node, &[])?;
                self.check_args(node, 1, 1, "config-rule-needs-query")?;
                self.check_no_children(node)?;
                let query = self.nonempty(args(node)[0], "`query`")?;
                Ok(Condition::Query {
                    query: query.to_owned(),
                })
            }
            "attachment" => {
                self.check_props(node, &[])?;
                self.check_args(node, 0, 0, "")?;
                let mut name_match = None;
                let mut mime = None;
                for child in children(node) {
                    let slot = match name(child) {
                        "name" => &mut name_match,
                        "type" => &mut mime,
                        other => {
                            return Err(self.src.node_err(
                                child,
                                msg("config-unknown-child")
                                    .arg("node", other)
                                    .arg("parent", "attachment")
                                    .arg("expected", "name, type"),
                            ));
                        }
                    };
                    if slot.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", name(child))));
                    }
                    self.check_props(child, &Self::text_props())?;
                    self.check_args(child, 0, 0, "")?;
                    self.check_no_children(child)?;
                    *slot = self.text_match(child, true)?;
                }
                Ok(Condition::Attachment {
                    name: name_match,
                    mime,
                })
            }
            "size" => {
                self.check_props(node, &["larger", "smaller"])?;
                self.check_args(node, 0, 0, "")?;
                self.check_no_children(node)?;
                let (cmp, entry) = self.one_of(
                    node,
                    &[
                        ("larger", Comparison::Greater),
                        ("smaller", Comparison::Less),
                    ],
                    "config-rule-needs-size",
                )?;
                Ok(Condition::Size {
                    cmp,
                    bytes: self.size(entry)?,
                })
            }
            "date" => {
                self.check_props(node, &["before", "after"])?;
                self.check_args(node, 0, 0, "")?;
                self.check_no_children(node)?;
                let (cmp, entry) = self.one_of(
                    node,
                    &[
                        ("before", DateComparison::Before),
                        ("after", DateComparison::After),
                    ],
                    "config-rule-needs-date",
                )?;
                let date = self.string(entry, "`date`")?;
                if !is_valid_date(date) {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-rule-date-invalid").arg("found", format!("{date:?}")),
                    ));
                }
                Ok(Condition::Date {
                    cmp,
                    date: date.to_owned(),
                })
            }
            "age" => {
                self.check_props(node, &["older-than", "newer-than"])?;
                self.check_args(node, 0, 0, "")?;
                self.check_no_children(node)?;
                let (cmp, entry) = self.one_of(
                    node,
                    &[
                        ("older-than", Comparison::Greater),
                        ("newer-than", Comparison::Less),
                    ],
                    "config-rule-needs-age",
                )?;
                Ok(Condition::Age {
                    cmp,
                    days: self.uint32(entry, "`age`", 0)?,
                })
            }
            "flag" => {
                self.check_props(node, &["set"])?;
                self.check_args(node, 1, 1, "config-rule-needs-flag")?;
                self.check_no_children(node)?;
                let entry = args(node)[0];
                let s = self.string(entry, "`flag`")?;
                let flag = lookup(FLAGS, s).ok_or_else(|| {
                    self.src.entry_err(
                        entry,
                        msg("config-flag-unknown")
                            .arg("found", s)
                            .arg("expected", names(FLAGS)),
                    )
                })?;
                Ok(Condition::Flag {
                    flag,
                    set: self.prop_bool(node, "set")?.unwrap_or(true),
                })
            }
            "tag" => {
                self.check_props(node, &[])?;
                self.check_args(node, 1, 1, "config-rule-needs-tag")?;
                self.check_no_children(node)?;
                Ok(Condition::Tag {
                    tag: self.tag(args(node)[0])?,
                })
            }
            "account" => {
                self.check_props(node, &[])?;
                self.check_args(node, 1, 1, "config-rule-needs-account")?;
                self.check_no_children(node)?;
                Ok(Condition::Account {
                    account_id: self.account_id(args(node)[0], "`account`")?,
                })
            }
            "folder" => Ok(Condition::Folder {
                folder: self.folder_ref(node)?,
            }),
            "list-id" => {
                self.check_props(node, &Self::text_props())?;
                self.check_args(node, 0, 0, "")?;
                self.check_no_children(node)?;
                Ok(Condition::ListId {
                    matches: self.text_match(node, false)?,
                })
            }
            other => Err(self.src.node_err(
                node,
                msg("config-rule-condition-unknown")
                    .arg("node", other)
                    .arg("expected", CONDITIONS),
            )),
        }
    }

    /// Exactly one of the listed properties.
    fn one_of<'n, T: Copy>(
        &self,
        node: &'n KdlNode,
        options: &[(&str, T)],
        what: &str,
    ) -> R<(T, &'n KdlEntry)> {
        let mut found = None;
        for (key, entry) in props(node) {
            if let Some(v) = lookup(options, key) {
                if found.is_some() {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-rule-only-one-of")
                            .arg("node", name(node))
                            .arg("options", names(options)),
                    ));
                }
                found = Some((v, entry));
            }
        }
        found.ok_or_else(|| self.src.node_err(node, msg(what).arg("node", name(node))))
    }

    /// Bytes: a whole number, or a string with a unit (`k`, `m`, `g`,
    /// 1024-based, optional trailing `b`, any case): `"10kb"`, `"5MB"`.
    fn size(&self, entry: &KdlEntry) -> R<u64> {
        if entry.value().as_integer().is_some() {
            return self.uint(entry, "`size`", 0);
        }
        let bad = || {
            self.src.entry_err(
                entry,
                msg("config-rule-size-invalid").arg("found", describe(entry.value())),
            )
        };
        let s = entry.value().as_string().ok_or_else(bad)?;
        parse_size(s).ok_or_else(bad)
    }

    fn tag(&self, entry: &KdlEntry) -> R<String> {
        let tag = self.nonempty(entry, "`tag`")?;
        if tag
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "(){%*\"\\]".contains(c))
        {
            return Err(self.src.entry_err(
                entry,
                msg("config-rule-tag-invalid").arg("found", format!("{tag:?}")),
            ));
        }
        Ok(tag.to_owned())
    }

    // --- Actions -----------------------------------------------------------

    fn then(&self, node: &KdlNode) -> Vec<RuleAction> {
        if self.ok(self.check_props(node, &[])).is_some() {
            self.ok(self.check_args(node, 0, 0, ""));
        }
        children(node)
            .iter()
            .filter_map(|child| self.ok(self.action(child)))
            .collect()
    }

    fn action(&self, node: &KdlNode) -> R<RuleAction> {
        let kind = name(node);
        let leaf = |props: &[&str], min: usize, max: usize, what: &str| -> R<()> {
            self.check_props(node, props)?;
            self.check_args(node, min, max, what)?;
            self.check_no_children(node)
        };
        match kind {
            "move" | "copy" => {
                let folder = self.folder_ref(node)?;
                Ok(if kind == "move" {
                    RuleAction::Move { folder }
                } else {
                    RuleAction::Copy { folder }
                })
            }
            "delete" => {
                leaf(&["permanent"], 0, 0, "")?;
                Ok(RuleAction::Delete {
                    permanent: self.prop_bool(node, "permanent")?.unwrap_or(false),
                })
            }
            "archive" => {
                leaf(&[], 0, 0, "")?;
                Ok(RuleAction::Archive)
            }
            "stop" => {
                leaf(&[], 0, 0, "")?;
                Ok(RuleAction::Stop)
            }
            "tag" | "untag" => {
                leaf(&[], 1, 1, "config-rule-needs-tag")?;
                let tag = self.tag(args(node)[0])?;
                Ok(if kind == "tag" {
                    RuleAction::Tag { tag }
                } else {
                    RuleAction::Untag { tag }
                })
            }
            "flag" => {
                leaf(&[], 0, 1, "")?;
                Ok(RuleAction::Flag {
                    flagged: self.arg_bool(node)?,
                })
            }
            "mark-read" => {
                leaf(&[], 0, 1, "")?;
                Ok(RuleAction::MarkRead {
                    read: self.arg_bool(node)?,
                })
            }
            "notify" => {
                leaf(&[], 0, 1, "")?;
                Ok(RuleAction::Notify {
                    notify: self.arg_bool(node)?,
                })
            }
            "forward" | "redirect" => {
                let allowed: &[&str] = if kind == "forward" {
                    &["identity"]
                } else {
                    &[]
                };
                leaf(allowed, 1, usize::MAX, "config-rule-needs-address")?;
                let mut to = Vec::new();
                for entry in args(node) {
                    let address = self.nonempty(entry, &format!("`{}`", name(node)))?.trim();
                    if !is_plausible_address(address) {
                        return Err(self.src.entry_err(
                            entry,
                            msg("config-rule-address-invalid").arg("found", format!("{address:?}")),
                        ));
                    }
                    to.push(address.to_owned());
                }
                if kind == "forward" {
                    Ok(RuleAction::Forward {
                        to,
                        identity_id: self.prop_string(node, "identity")?.map(IdentityId::new),
                    })
                } else {
                    Ok(RuleAction::Redirect { to })
                }
            }
            "reply" => {
                leaf(&["identity"], 1, 1, "config-rule-needs-template")?;
                let template = self
                    .arg_string(node, "`reply`")?
                    .unwrap_or_default()
                    .to_owned();
                Ok(RuleAction::Reply {
                    template,
                    identity_id: self.prop_string(node, "identity")?.map(IdentityId::new),
                })
            }
            other => Err(self.src.node_err(
                node,
                msg("config-rule-action-unknown")
                    .arg("node", other)
                    .arg("expected", ACTIONS),
            )),
        }
    }
}

/// `YYYY-MM-DD`, a real calendar day.
pub(crate) fn is_valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|p| p.parse::<u32>().ok());
    let (Some(year), Some(month), Some(day)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    if !s[0..4].bytes().all(|c| c.is_ascii_digit())
        || !s[5..7].bytes().all(|c| c.is_ascii_digit())
        || !s[8..10].bytes().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// Parses `"500"`, `"10k"`, `"10kb"`, `"5MB"`, `"1g"` (1024-based).
pub(crate) fn parse_size(s: &str) -> Option<u64> {
    let lower = s.trim().to_ascii_lowercase();
    let lower = lower.strip_suffix('b').unwrap_or(&lower);
    let (digits, factor) = match lower.chars().last()? {
        'k' => (&lower[..lower.len() - 1], 1024),
        'm' => (&lower[..lower.len() - 1], 1024 * 1024),
        'g' => (&lower[..lower.len() - 1], 1024 * 1024 * 1024),
        _ => (lower, 1),
    };
    let digits = digits.trim();
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(factor)
}

/// A loose address check: one `@` with text on both sides, no spaces.
fn is_plausible_address(s: &str) -> bool {
    let mut parts = s.split('@');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None)
            if !local.is_empty()
                && !domain.is_empty()
                && !s.chars().any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The KDL names are the contract's wire names (one fact): every table
    /// matches serde, and the simple triggers are `RuleTrigger::SIMPLE`.
    #[test]
    fn name_tables_are_the_wire_names() {
        fn wire<T: serde::Serialize>(value: &T) -> String {
            match serde_json::to_value(value).unwrap() {
                serde_json::Value::String(s) => s,
                serde_json::Value::Object(o) => o["type"].as_str().unwrap().to_owned(),
                other => panic!("{other}"),
            }
        }
        fn check<T: serde::Serialize>(table: &[(&str, T)]) {
            for (name, value) in table {
                assert_eq!(*name, wire(value));
            }
        }
        check(TEXT_OPS);
        check(ADDRESS_FIELDS);
        check(FLAGS);
        check(AUTH_METHODS);
        check(AUTH_RESULTS);
        check(ROLES);
        check(SIMPLE_TRIGGERS);
        assert_eq!(
            SIMPLE_TRIGGERS
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<Vec<_>>(),
            RuleTrigger::SIMPLE.to_vec()
        );
    }

    #[test]
    fn dates() {
        assert!(is_valid_date("2026-09-29"));
        assert!(is_valid_date("2028-02-29"));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-1-01",
            "26-09-29",
            "2026-09-3x",
            "2026/09/29",
        ] {
            assert!(!is_valid_date(bad), "{bad}");
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("500"), Some(500));
        assert_eq!(parse_size("10k"), Some(10 * 1024));
        assert_eq!(parse_size("10KB"), Some(10 * 1024));
        assert_eq!(parse_size("5MB"), Some(5 * 1024 * 1024));
        assert_eq!(parse_size(" 1g "), Some(1024 * 1024 * 1024));
        for bad in ["", "k", "1.5m", "-1", "5 tb", "x"] {
            assert_eq!(parse_size(bad), None, "{bad}");
        }
    }

    #[test]
    fn addresses() {
        assert!(is_plausible_address("a@b.example"));
        for bad in ["a", "@b", "a@", "a b@c", "a@b@c", "<a@b>"] {
            assert!(!is_plausible_address(bad), "{bad}");
        }
    }
}
