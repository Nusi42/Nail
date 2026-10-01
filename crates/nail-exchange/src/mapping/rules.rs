//! Nail rules ↔ Exchange inbox rules (ADR 0072, "Server-side rules").
//!
//! For Exchange accounts, inbox rules replace Sieve: a Nail rule is
//! **placed** on the server ([`RulePlacement::Server`]) when an inbox rule
//! means the same thing; otherwise Nail runs it, and the reasons say why
//! (`exchange-rule-reason-*`, with the condition's position or the
//! action's index, like `nail-sieve`).
//!
//! An inbox rule is "all conditions, each a list that matches when any
//! entry matches, unless any exception matches". So a Nail rule maps when
//! its `when` tree is one leaf, an `all` of leaves and `not`s, an `any` of
//! leaves of the same kind, or a `not` of a leaf / an `any` of leaves; each
//! leaf must be a test Exchange has: sender / recipient address equals or
//! contains, subject / body contains, header exists, has attachments, size
//! (whole kilobytes), or "in the Inbox / this account". Texts compare
//! case-insensitively on Exchange, so case-sensitive tests stay in Nail.
//! Actions: move / copy (into this account's folders), delete, archive,
//! mark read, tag (= assign category), forward, redirect, stop.
//!
//! The other direction ([`nail_rule_of`]) shows a server rule as a Nail
//! rule where Nail can express it (for "convert to a Nail rule").

use nail_api::domain::FolderRole;
use nail_api::exchange::ExchangeInboxRule;
use nail_api::exchange::rules::{ExchangeRuleActions, ExchangeRuleConditions};
use nail_api::ids::{AccountId, FolderId};
use nail_api::localized::LocalizedMessage;
use nail_api::pim::is_plausible_email;
use nail_api::rules::{
    AddressField, Comparison, Condition, FolderRef, Rule, RuleAction, RuleScope, RuleTrigger,
    TextMatch, TextOp,
};
use nail_api::sieve::RulePlacement;

use super::categories::is_category_name;
use crate::texts::{self, at_action, at_condition};

/// What placing one Nail rule on an Exchange account gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePlan {
    /// Server, client or not expressible.
    pub placement: RulePlacement,
    /// The inbox rule to write (`server` only): `id` empty, `sequence` 0
    /// (the core numbers them in `rules.kdl` order), `nail_rule_id` set.
    pub inbox_rule: Option<ExchangeInboxRule>,
    /// Why it runs in Nail (empty for `server`); not-expressible reasons
    /// first.
    pub reasons: Vec<LocalizedMessage>,
}

/// What the planner needs to know about the account.
pub struct PlacementContext<'a> {
    /// The Exchange account.
    pub account_id: &'a AccountId,
    /// `server-rules` of the account (`ExchangeOptions::server_rules`).
    pub server_rules: bool,
    /// The account's folder for a reference (`None` = not known / not
    /// synced yet). Only called for references into this account.
    pub resolve_folder: &'a dyn Fn(&FolderRef) -> Option<FolderId>,
}

impl std::fmt::Debug for PlacementContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlacementContext")
            .field("account_id", self.account_id)
            .field("server_rules", &self.server_rules)
            .finish_non_exhaustive()
    }
}

/// Whether a rule's scope covers the account at all (empty scope = every
/// account; virtual accounts are resolved by the core before calling).
pub fn applies_to(rule: &Rule, account: &AccountId) -> bool {
    rule.scope.is_empty()
        || rule.scope.iter().any(|s| match s {
            RuleScope::Account { account_id } => account_id == account,
            RuleScope::Folder { folder } => folder_account(folder).is_none_or(|a| a == account),
        })
}

fn folder_account(folder: &FolderRef) -> Option<&AccountId> {
    match folder {
        FolderRef::Role { account_id, .. } | FolderRef::Path { account_id, .. } => {
            account_id.as_ref()
        }
    }
}

fn is_inbox(folder: &FolderRef, account: &AccountId) -> bool {
    matches!(folder, FolderRef::Role { role: FolderRole::Inbox, account_id }
        if account_id.as_ref().is_none_or(|a| a == account))
        || matches!(folder, FolderRef::Path { path, account_id }
            if path.eq_ignore_ascii_case("Inbox") && account_id.as_ref().is_none_or(|a| a == account))
}

fn folder_label(folder: &FolderRef) -> String {
    match folder {
        FolderRef::Path { path, .. } => path.clone(),
        FolderRef::Role { role, .. } => serde_json::to_value(role)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    }
}

/// Builds a predicate from a value.
type MakePred = fn(String) -> Pred;

/// One Exchange predicate a leaf condition becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pred {
    FromAddress(String),
    SenderContains(String),
    SentTo(String),
    RecipientContains(String),
    Subject(String),
    Body(String),
    Header(String),
    HasAttachments,
    NoAttachments,
    MinKb(u32),
    MaxKb(u32),
    /// Always true for this account's inbox rules.
    Always,
}

impl Pred {
    /// The list a predicate goes to (equal kinds may share an `any`).
    fn kind(&self) -> u8 {
        match self {
            Self::FromAddress(_) => 0,
            Self::SenderContains(_) => 1,
            Self::SentTo(_) => 2,
            Self::RecipientContains(_) => 3,
            Self::Subject(_) => 4,
            Self::Body(_) => 5,
            Self::Header(_) => 6,
            Self::HasAttachments | Self::NoAttachments => 7,
            Self::MinKb(_) => 8,
            Self::MaxKb(_) => 9,
            Self::Always => 10,
        }
    }

    fn add_to(self, c: &mut ExchangeRuleConditions) {
        match self {
            Self::FromAddress(v) => c.from_addresses.push(v),
            Self::SenderContains(v) => c.sender_contains.push(v),
            Self::SentTo(v) => c.sent_to_addresses.push(v),
            Self::RecipientContains(v) => c.recipient_contains.push(v),
            Self::Subject(v) => c.subject_contains.push(v),
            Self::Body(v) => c.body_contains.push(v),
            Self::Header(v) => c.header_contains.push(v),
            Self::HasAttachments => c.has_attachments = Some(true),
            Self::NoAttachments => c.has_attachments = Some(false),
            Self::MinKb(v) => c.min_size_kb = Some(v),
            Self::MaxKb(v) => c.max_size_kb = Some(v),
            Self::Always => {}
        }
    }
}

struct Planner<'a, 'b> {
    ctx: &'a PlacementContext<'b>,
    never: Vec<LocalizedMessage>,
    client: Vec<LocalizedMessage>,
}

/// Contains-only, case-insensitive text tests.
fn contains_value(m: &TextMatch) -> Option<String> {
    (m.op == TextOp::Contains && !m.case_sensitive && !m.value.trim().is_empty())
        .then(|| m.value.clone())
}

impl Planner<'_, '_> {
    fn leaf(&mut self, condition: &Condition, path: &[u32]) -> Option<Pred> {
        let text_test = |p: &mut Self| {
            p.never.push(at_condition(texts::RULE_TEXT_TEST, path));
            None
        };
        match condition {
            Condition::Address { field, matches } => {
                let (equals, contains): (MakePred, MakePred) = match field {
                    AddressField::From => (Pred::FromAddress, Pred::SenderContains),
                    AddressField::Recipients => (Pred::SentTo, Pred::RecipientContains),
                    _ => {
                        self.never
                            .push(at_condition(texts::RULE_ADDRESS_FIELD, path));
                        return None;
                    }
                };
                if matches.case_sensitive {
                    return text_test(self);
                }
                match matches.op {
                    TextOp::Equals if is_plausible_email(matches.value.trim()) => {
                        Some(equals(matches.value.trim().to_ascii_lowercase()))
                    }
                    TextOp::Contains if !matches.value.trim().is_empty() => {
                        Some(contains(matches.value.clone()))
                    }
                    _ => text_test(self),
                }
            }
            Condition::Subject { matches } => match contains_value(matches) {
                Some(v) => Some(Pred::Subject(v)),
                None => text_test(self),
            },
            Condition::Body { matches } => match contains_value(matches) {
                Some(v) => Some(Pred::Body(v)),
                None => text_test(self),
            },
            Condition::Header {
                name,
                matches: None,
            } if !name.trim().is_empty() && !name.contains([':', ' ']) => {
                Some(Pred::Header(format!("{}:", name.trim())))
            }
            Condition::ListId { matches: None } => Some(Pred::Header("List-Id:".to_owned())),
            Condition::Header { .. } | Condition::ListId { .. } => {
                self.never.push(at_condition(texts::RULE_HEADER, path));
                None
            }
            Condition::Attachment {
                name: None,
                mime: None,
            } => Some(Pred::HasAttachments),
            Condition::Attachment { .. } => {
                self.never.push(at_condition(texts::RULE_ATTACHMENT, path));
                None
            }
            Condition::Size { cmp, bytes } => {
                let kb = u32::try_from(bytes / 1024).unwrap_or(u32::MAX);
                Some(match cmp {
                    // "larger than n bytes" → at least the next whole KB.
                    Comparison::Greater => Pred::MinKb(kb.saturating_add(1)),
                    Comparison::Less => {
                        Pred::MaxKb(u32::try_from(bytes.saturating_sub(1) / 1024).unwrap_or(0))
                    }
                })
            }
            Condition::Query { .. } => {
                self.never.push(at_condition(texts::RULE_QUERY, path));
                None
            }
            Condition::Tag { .. } => {
                self.never.push(at_condition(texts::RULE_TAG, path));
                None
            }
            Condition::Folder { folder } if is_inbox(folder, self.ctx.account_id) => {
                Some(Pred::Always)
            }
            Condition::Folder { .. } => {
                self.never.push(at_condition(texts::RULE_FOLDER, path));
                None
            }
            Condition::Account { account_id } if account_id == self.ctx.account_id => {
                Some(Pred::Always)
            }
            Condition::Account { .. }
            | Condition::Flag { .. }
            | Condition::Age { .. }
            | Condition::Date { .. }
            | Condition::Auth { .. } => {
                self.never.push(at_condition(texts::RULE_CONDITION, path));
                None
            }
            Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => {
                self.never.push(at_condition(texts::RULE_STRUCTURE, path));
                None
            }
        }
    }

    /// The predicates of an `any` of leaves (all of one kind), or of one
    /// leaf. `None` on failure (reasons recorded); `Some(empty)` when an
    /// entry is always true.
    fn any_of(&mut self, children: &[Condition], path: &[u32], negated: bool) -> Option<Vec<Pred>> {
        let mut preds = Vec::new();
        let mut ok = true;
        for (i, child) in children.iter().enumerate() {
            let mut child_path = path.to_vec();
            child_path.push(u32::try_from(i).unwrap_or(u32::MAX));
            match self.leaf(child, &child_path) {
                Some(pred) => preds.push(pred),
                None => ok = false,
            }
        }
        if !ok {
            return None;
        }
        if preds.contains(&Pred::Always) {
            if negated {
                // "not (always or …)" never matches.
                self.never.push(at_condition(texts::RULE_STRUCTURE, path));
                return None;
            }
            return Some(Vec::new());
        }
        let first = preds.first().map(Pred::kind);
        let one_list =
            preds.iter().all(|p| Some(p.kind()) == first) && !matches!(first, Some(7..=9));
        if preds.len() > 1 && !one_list && !negated {
            // `any` across different predicates: inbox rules AND them.
            self.never.push(at_condition(texts::RULE_STRUCTURE, path));
            return None;
        }
        Some(preds)
    }

    fn conditions(
        &mut self,
        root: &Condition,
    ) -> Option<(ExchangeRuleConditions, ExchangeRuleConditions)> {
        let mut conditions = ExchangeRuleConditions::default();
        let mut exceptions = ExchangeRuleConditions::default();
        let mut positive: Vec<Vec<Pred>> = Vec::new();
        let mut negative: Vec<Pred> = Vec::new();
        let mut ok = true;
        // (condition, its path, whether it sits under the root `all`)
        let top: Vec<(&Condition, Vec<u32>)> = match root {
            Condition::All { conditions } => conditions
                .iter()
                .enumerate()
                .map(|(i, c)| (c, vec![u32::try_from(i).unwrap_or(u32::MAX)]))
                .collect(),
            other => vec![(other, Vec::new())],
        };
        for (condition, path) in top {
            let result = match condition {
                Condition::Not { condition: inner } => {
                    let mut inner_path = path.clone();
                    inner_path.push(0);
                    match inner.as_ref() {
                        Condition::Any { conditions } => self
                            .any_of(conditions, &inner_path, true)
                            .map(|p| negative.extend(p)),
                        leaf => match self.leaf(leaf, &inner_path) {
                            Some(Pred::Always) => {
                                self.never.push(at_condition(texts::RULE_STRUCTURE, &path));
                                None
                            }
                            // "no attachments" is a condition of its own.
                            Some(Pred::HasAttachments) => {
                                positive.push(vec![Pred::NoAttachments]);
                                Some(())
                            }
                            Some(p) => {
                                negative.push(p);
                                Some(())
                            }
                            None => None,
                        },
                    }
                }
                Condition::Any { conditions } => self
                    .any_of(conditions, &path, false)
                    .map(|p| positive.push(p)),
                leaf => self.leaf(leaf, &path).map(|p| positive.push(vec![p])),
            };
            ok &= result.is_some();
        }
        if !ok {
            return None;
        }
        // Positive lists AND each other: two lists of one kind cannot.
        let mut seen: Vec<u8> = Vec::new();
        for list in positive.into_iter().filter(|l| !l.is_empty()) {
            let kind = list[0].kind();
            if kind == 10 {
                continue;
            }
            if seen.contains(&kind) {
                self.never.push(at_condition(texts::RULE_STRUCTURE, &[]));
                return None;
            }
            seen.push(kind);
            for pred in list {
                pred.add_to(&mut conditions);
            }
        }
        // Exceptions OR anyway; sizes and attachments appear once.
        let mut seen_single: Vec<u8> = Vec::new();
        for pred in negative {
            let kind = pred.kind();
            if matches!(kind, 7..=9) {
                if seen_single.contains(&kind) {
                    self.never.push(at_condition(texts::RULE_STRUCTURE, &[]));
                    return None;
                }
                seen_single.push(kind);
            }
            pred.add_to(&mut exceptions);
        }
        Some((conditions, exceptions))
    }

    fn folder(&mut self, folder: &FolderRef, index: usize) -> Option<FolderId> {
        if folder_account(folder).is_some_and(|a| a != self.ctx.account_id) {
            self.never.push(at_action(texts::RULE_OTHER_ACCOUNT, index));
            return None;
        }
        let found = (self.ctx.resolve_folder)(folder);
        if found.is_none() {
            self.client.push(
                LocalizedMessage::new(texts::RULE_FOLDER_UNKNOWN)
                    .arg("folder", folder_label(folder)),
            );
        }
        found
    }

    fn actions(&mut self, actions: &[RuleAction]) -> ExchangeRuleActions {
        let mut out = ExchangeRuleActions::default();
        let mut targets = 0;
        for (index, action) in actions.iter().enumerate() {
            match action {
                RuleAction::Move { folder } => {
                    targets += 1;
                    out.move_to_folder = self.folder(folder, index);
                }
                RuleAction::Archive => {
                    targets += 1;
                    let archive = FolderRef::Role {
                        role: FolderRole::Archive,
                        account_id: None,
                    };
                    out.move_to_folder = self.folder(&archive, index);
                }
                RuleAction::Copy { folder } => {
                    if out.copy_to_folder.is_some() {
                        self.never.push(at_action(texts::RULE_TARGETS, index));
                    }
                    out.copy_to_folder = self.folder(folder, index);
                }
                RuleAction::Delete { permanent } => {
                    targets += 1;
                    if *permanent {
                        out.permanent_delete = true;
                    } else {
                        out.delete = true;
                    }
                }
                RuleAction::Tag { tag } => {
                    if is_category_name(tag.trim()) {
                        out.assign_categories.push(tag.trim().to_owned());
                    } else {
                        self.never.push(at_action(texts::RULE_CATEGORY_NAME, index));
                    }
                }
                RuleAction::Untag { .. } => self.never.push(at_action(texts::RULE_UNTAG, index)),
                RuleAction::Flag { .. } => self.never.push(at_action(texts::RULE_FLAG, index)),
                RuleAction::MarkRead { read: true } => out.mark_as_read = true,
                RuleAction::MarkRead { read: false } => {
                    self.never.push(at_action(texts::RULE_MARK_UNREAD, index));
                }
                RuleAction::Forward { to, identity_id } => {
                    if identity_id.is_some() {
                        self.never
                            .push(at_action(texts::RULE_FORWARD_IDENTITY, index));
                    }
                    self.addresses(to, index, &mut out.forward_to);
                }
                RuleAction::Redirect { to } => self.addresses(to, index, &mut out.redirect_to),
                RuleAction::Reply { .. } => self.never.push(at_action(texts::RULE_REPLY, index)),
                RuleAction::Notify { .. } => {
                    self.never.push(at_action(texts::RULE_NOTIFY, index));
                }
                RuleAction::Stop => {
                    out.stop_processing_rules = true;
                    // Nothing after `stop` ever runs.
                    break;
                }
            }
        }
        if targets > 1 {
            self.never.push(at_action(
                texts::RULE_TARGETS,
                actions.len().saturating_sub(1),
            ));
        }
        out
    }

    fn addresses(&mut self, to: &[String], index: usize, into: &mut Vec<String>) {
        if to.is_empty() || to.iter().any(|a| !is_plausible_email(a.trim())) {
            self.never.push(at_action(texts::RULE_ADDRESS, index));
            return;
        }
        into.extend(to.iter().map(|a| a.trim().to_owned()));
    }
}

/// Plans where a Nail rule runs for an Exchange account (see the module
/// docs). The rule must apply to the account ([`applies_to`]).
pub fn plan(rule: &Rule, ctx: &PlacementContext<'_>) -> RulePlan {
    let mut planner = Planner {
        ctx,
        never: Vec::new(),
        client: Vec::new(),
    };
    let parts = planner.conditions(&rule.conditions);
    let actions = planner.actions(&rule.actions);
    let only_stop = ExchangeRuleActions {
        stop_processing_rules: false,
        ..actions.clone()
    };
    if planner.never.is_empty() && planner.client.is_empty() && only_stop.is_empty() {
        planner
            .never
            .push(LocalizedMessage::new(texts::RULE_NO_ACTIONS));
    }
    // Expressible but run by Nail.
    if !rule.enabled {
        planner
            .client
            .push(LocalizedMessage::new(texts::RULE_DISABLED));
    }
    if !rule.triggers.contains(&RuleTrigger::Arrival) {
        planner
            .client
            .push(LocalizedMessage::new(texts::RULE_NO_ARRIVAL));
    } else if rule.triggers.iter().any(|t| *t != RuleTrigger::Arrival) {
        planner
            .client
            .push(LocalizedMessage::new(texts::RULE_TRIGGER));
    }
    let scope_ok = rule.scope.iter().all(|s| match s {
        RuleScope::Account { account_id } => account_id == ctx.account_id,
        RuleScope::Folder { folder } => is_inbox(folder, ctx.account_id),
    });
    if !scope_ok {
        planner
            .client
            .push(LocalizedMessage::new(texts::RULE_SCOPE));
    }
    if !ctx.server_rules {
        planner
            .client
            .push(LocalizedMessage::new(texts::RULE_SERVER_RULES_OFF));
    }
    let Planner { never, client, .. } = planner;
    if !never.is_empty() {
        let mut reasons = never;
        reasons.extend(client);
        return RulePlan {
            placement: RulePlacement::NotExpressible,
            inbox_rule: None,
            reasons,
        };
    }
    if !client.is_empty() {
        return RulePlan {
            placement: RulePlacement::Client,
            inbox_rule: None,
            reasons: client,
        };
    }
    let (conditions, exceptions) = parts.unwrap_or_default();
    RulePlan {
        placement: RulePlacement::Server,
        inbox_rule: Some(ExchangeInboxRule {
            id: String::new(),
            name: rule.name.clone(),
            sequence: 0,
            enabled: true,
            conditions,
            exceptions,
            actions,
            has_unsupported: false,
            in_error: false,
            nail_rule_id: Some(rule.id.clone()),
        }),
        reasons: Vec::new(),
    }
}

/// A digest of what a placed inbox rule does (name, state, conditions,
/// exceptions, actions — not its id or sequence): the core stores it with
/// the placement and rewrites the server rule only when it changes.
pub fn digest(rule: &ExchangeInboxRule) -> String {
    let value = serde_json::json!({
        "name": rule.name,
        "enabled": rule.enabled,
        "conditions": rule.conditions,
        "exceptions": rule.exceptions,
        "actions": rule.actions,
    });
    nail_ical::ical::sha256_hex(value.to_string().as_bytes())
}

fn text(op: TextOp, value: &str) -> TextMatch {
    TextMatch {
        op,
        value: value.to_owned(),
        case_sensitive: false,
    }
}

fn any_or_one(mut list: Vec<Condition>) -> Option<Condition> {
    match list.len() {
        0 => None,
        1 => list.pop(),
        _ => Some(Condition::Any { conditions: list }),
    }
}

/// Nail conditions of one side of an inbox rule; `Err` when a part has no
/// Nail condition.
fn nail_conditions(c: &ExchangeRuleConditions) -> Result<Vec<Condition>, ()> {
    let mut out = Vec::new();
    let address = |field: AddressField, op: TextOp, values: &[String]| {
        any_or_one(
            values
                .iter()
                .map(|v| Condition::Address {
                    field,
                    matches: text(op, v),
                })
                .collect(),
        )
    };
    out.extend(address(
        AddressField::From,
        TextOp::Equals,
        &c.from_addresses,
    ));
    out.extend(address(
        AddressField::From,
        TextOp::Contains,
        &c.sender_contains,
    ));
    out.extend(address(
        AddressField::Recipients,
        TextOp::Equals,
        &c.sent_to_addresses,
    ));
    out.extend(address(
        AddressField::Recipients,
        TextOp::Contains,
        &c.recipient_contains,
    ));
    out.extend(any_or_one(
        c.subject_contains
            .iter()
            .map(|v| Condition::Subject {
                matches: text(TextOp::Contains, v),
            })
            .collect(),
    ));
    out.extend(any_or_one(
        c.body_contains
            .iter()
            .map(|v| Condition::Body {
                matches: text(TextOp::Contains, v),
            })
            .collect(),
    ));
    out.extend(any_or_one(
        c.body_or_subject_contains
            .iter()
            .flat_map(|v| {
                [
                    Condition::Subject {
                        matches: text(TextOp::Contains, v),
                    },
                    Condition::Body {
                        matches: text(TextOp::Contains, v),
                    },
                ]
            })
            .collect(),
    ));
    let mut headers = Vec::new();
    for header in &c.header_contains {
        // Only "Name:" (the header exists) has a Nail condition.
        match header.trim().strip_suffix(':') {
            Some(name) if !name.is_empty() && !name.contains([' ', ':']) => {
                headers.push(Condition::Header {
                    name: name.to_owned(),
                    matches: None,
                });
            }
            _ => return Err(()),
        }
    }
    out.extend(any_or_one(headers));
    out.extend(any_or_one(
        c.categories
            .iter()
            .map(|t| Condition::Tag { tag: t.clone() })
            .collect(),
    ));
    match c.has_attachments {
        Some(true) => out.push(Condition::Attachment {
            name: None,
            mime: None,
        }),
        Some(false) => out.push(Condition::Not {
            condition: Box::new(Condition::Attachment {
                name: None,
                mime: None,
            }),
        }),
        None => {}
    }
    if let Some(kb) = c.min_size_kb {
        out.push(Condition::Size {
            cmp: Comparison::Greater,
            bytes: (u64::from(kb) * 1024).saturating_sub(1),
        });
    }
    if let Some(kb) = c.max_size_kb {
        out.push(Condition::Size {
            cmp: Comparison::Less,
            bytes: u64::from(kb) * 1024 + 1,
        });
    }
    let unsupported = c.importance.is_some()
        || c.sent_to_me.is_some()
        || c.sent_only_to_me.is_some()
        || c.sent_cc_me.is_some()
        || c.sent_to_or_cc_me.is_some()
        || c.not_sent_to_me.is_some()
        || c.is_meeting_request.is_some();
    if unsupported { Err(()) } else { Ok(out) }
}

/// A server rule as a Nail rule (arrival, scoped to the account) where
/// Nail can express all of it; `folder_ref` names the account's folders
/// for move / copy targets. `Err` carries
/// `exchange-server-rule-unsupported`.
pub fn nail_rule_of(
    rule: &ExchangeInboxRule,
    account: &AccountId,
    folder_ref: &dyn Fn(&FolderId) -> Option<FolderRef>,
) -> Result<Rule, LocalizedMessage> {
    let unsupported = || LocalizedMessage::new(texts::SERVER_RULE_UNSUPPORTED);
    if rule.has_unsupported {
        return Err(unsupported());
    }
    let mut conditions = nail_conditions(&rule.conditions).map_err(|()| unsupported())?;
    let exceptions = nail_conditions(&rule.exceptions).map_err(|()| unsupported())?;
    if let Some(any) = any_or_one(exceptions) {
        conditions.push(Condition::Not {
            condition: Box::new(any),
        });
    }
    let a = &rule.actions;
    let mut actions = Vec::new();
    let folder = |id: &FolderId| folder_ref(id).ok_or_else(unsupported);
    if let Some(id) = &a.copy_to_folder {
        actions.push(RuleAction::Copy {
            folder: folder(id)?,
        });
    }
    if a.mark_as_read {
        actions.push(RuleAction::MarkRead { read: true });
    }
    actions.extend(
        a.assign_categories
            .iter()
            .map(|t| RuleAction::Tag { tag: t.clone() }),
    );
    if !a.forward_to.is_empty() {
        actions.push(RuleAction::Forward {
            to: a.forward_to.clone(),
            identity_id: None,
        });
    }
    if !a.redirect_to.is_empty() {
        actions.push(RuleAction::Redirect {
            to: a.redirect_to.clone(),
        });
    }
    if let Some(id) = &a.move_to_folder {
        actions.push(RuleAction::Move {
            folder: folder(id)?,
        });
    }
    if a.delete || a.permanent_delete {
        actions.push(RuleAction::Delete {
            permanent: a.permanent_delete,
        });
    }
    if a.stop_processing_rules {
        actions.push(RuleAction::Stop);
    }
    if a.mark_importance.is_some() || !a.forward_as_attachment_to.is_empty() {
        return Err(unsupported());
    }
    Ok(Rule {
        id: "".into(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        triggers: vec![RuleTrigger::Arrival],
        scope: vec![RuleScope::Account {
            account_id: account.clone(),
        }],
        conditions: Condition::All { conditions },
        actions,
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::MessageArg;
    use pretty_assertions::assert_eq;

    fn account() -> AccountId {
        "work".into()
    }

    fn resolve(folder: &FolderRef) -> Option<FolderId> {
        match folder {
            FolderRef::Path { path, .. } if path == "Reading" => Some("f-reading".into()),
            FolderRef::Role {
                role: FolderRole::Archive,
                ..
            } => Some("f-archive".into()),
            _ => None,
        }
    }

    fn plan_of(rule: &Rule) -> RulePlan {
        let account = account();
        let ctx = PlacementContext {
            account_id: &account,
            server_rules: true,
            resolve_folder: &resolve,
        };
        plan(rule, &ctx)
    }

    fn rule(conditions: Condition, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: "r1".into(),
            name: "Newsletters".into(),
            enabled: true,
            triggers: vec![RuleTrigger::Arrival],
            scope: Vec::new(),
            conditions,
            actions,
            source: None,
        }
    }

    fn contains(value: &str) -> TextMatch {
        text(TextOp::Contains, value)
    }

    fn reading() -> RuleAction {
        RuleAction::Move {
            folder: FolderRef::Path {
                path: "Reading".into(),
                account_id: None,
            },
        }
    }

    fn ids(plan: &RulePlan) -> Vec<&str> {
        plan.reasons.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn places_an_expressible_rule() {
        let r = rule(
            Condition::All {
                conditions: vec![
                    Condition::Any {
                        conditions: vec![
                            Condition::Address {
                                field: AddressField::From,
                                matches: contains("@substack.com"),
                            },
                            Condition::Address {
                                field: AddressField::From,
                                matches: contains("@ghost.io"),
                            },
                        ],
                    },
                    Condition::ListId { matches: None },
                    Condition::Folder {
                        folder: FolderRef::Role {
                            role: FolderRole::Inbox,
                            account_id: None,
                        },
                    },
                    Condition::Not {
                        condition: Box::new(Condition::Subject {
                            matches: contains("invoice"),
                        }),
                    },
                    Condition::Size {
                        cmp: Comparison::Less,
                        bytes: 1024 * 1024,
                    },
                ],
            },
            vec![
                RuleAction::Tag {
                    tag: "newsletter".into(),
                },
                RuleAction::MarkRead { read: true },
                reading(),
                RuleAction::Stop,
                RuleAction::Flag { flagged: true },
            ],
        );
        let plan = plan_of(&r);
        assert_eq!(plan.placement, RulePlacement::Server, "{:?}", plan.reasons);
        let inbox = plan.inbox_rule.unwrap();
        assert_eq!(inbox.nail_rule_id, Some("r1".into()));
        assert_eq!(
            inbox.conditions.sender_contains,
            ["@substack.com", "@ghost.io"]
        );
        assert_eq!(inbox.conditions.header_contains, ["List-Id:"]);
        assert_eq!(inbox.conditions.max_size_kb, Some(1023));
        assert_eq!(inbox.exceptions.subject_contains, ["invoice"]);
        assert_eq!(inbox.actions.assign_categories, ["newsletter"]);
        assert!(inbox.actions.mark_as_read && inbox.actions.stop_processing_rules);
        assert_eq!(inbox.actions.move_to_folder, Some("f-reading".into()));
        // The flag after `stop` never runs, so it does not matter.
        assert!(inbox.editable());
        assert_eq!(digest(&inbox), digest(&inbox.clone()));
    }

    #[test]
    fn single_leaf_and_equals() {
        let r = rule(
            Condition::Address {
                field: AddressField::Recipients,
                matches: text(TextOp::Equals, "Team@Corp.Example"),
            },
            vec![RuleAction::Archive],
        );
        let plan = plan_of(&r);
        let inbox = plan.inbox_rule.unwrap();
        assert_eq!(inbox.conditions.sent_to_addresses, ["team@corp.example"]);
        assert_eq!(inbox.actions.move_to_folder, Some("f-archive".into()));
    }

    #[test]
    fn not_expressible_conditions_have_positions() {
        let r = rule(
            Condition::All {
                conditions: vec![
                    Condition::Query {
                        query: "from:x".into(),
                    },
                    Condition::Subject {
                        matches: TextMatch {
                            op: TextOp::Regex,
                            value: "^x".into(),
                            case_sensitive: false,
                        },
                    },
                    Condition::Any {
                        conditions: vec![
                            Condition::Subject {
                                matches: contains("a"),
                            },
                            Condition::Body {
                                matches: contains("b"),
                            },
                        ],
                    },
                    Condition::Address {
                        field: AddressField::ReplyTo,
                        matches: contains("x"),
                    },
                    Condition::Header {
                        name: "X-Spam".into(),
                        matches: Some(contains("yes")),
                    },
                    Condition::Tag { tag: "x".into() },
                ],
            },
            vec![reading()],
        );
        let plan = plan_of(&r);
        assert_eq!(plan.placement, RulePlacement::NotExpressible);
        assert!(plan.inbox_rule.is_none());
        assert_eq!(
            ids(&plan),
            [
                texts::RULE_QUERY,
                texts::RULE_TEXT_TEST,
                texts::RULE_STRUCTURE,
                texts::RULE_ADDRESS_FIELD,
                texts::RULE_HEADER,
                texts::RULE_TAG
            ]
        );
        assert_eq!(
            plan.reasons[0].args["position"],
            MessageArg::Text("1.1".into())
        );
        assert_eq!(
            plan.reasons[2].args["position"],
            MessageArg::Text("1.3".into())
        );
    }

    #[test]
    fn same_list_twice_cannot_be_anded() {
        let r = rule(
            Condition::All {
                conditions: vec![
                    Condition::Subject {
                        matches: contains("a"),
                    },
                    Condition::Subject {
                        matches: contains("b"),
                    },
                ],
            },
            vec![reading()],
        );
        assert_eq!(ids(&plan_of(&r)), [texts::RULE_STRUCTURE]);
        // But exceptions OR, so two negated subjects are fine.
        let r = rule(
            Condition::All {
                conditions: vec![
                    Condition::Not {
                        condition: Box::new(Condition::Subject {
                            matches: contains("a"),
                        }),
                    },
                    Condition::Not {
                        condition: Box::new(Condition::Any {
                            conditions: vec![
                                Condition::Subject {
                                    matches: contains("b"),
                                },
                                Condition::Attachment {
                                    name: None,
                                    mime: None,
                                },
                            ],
                        }),
                    },
                ],
            },
            vec![reading()],
        );
        let plan = plan_of(&r);
        let inbox = plan.inbox_rule.expect("expressible");
        assert_eq!(inbox.exceptions.subject_contains, ["a", "b"]);
        assert_eq!(inbox.exceptions.has_attachments, Some(true));
        assert!(inbox.conditions.is_empty());
    }

    #[test]
    fn not_expressible_actions() {
        let r = rule(
            Condition::always(),
            vec![
                RuleAction::Untag { tag: "x".into() },
                RuleAction::MarkRead { read: false },
                RuleAction::Reply {
                    template: "x".into(),
                    identity_id: None,
                },
                RuleAction::Notify { notify: false },
                RuleAction::Forward {
                    to: vec!["boss@corp.example".into()],
                    identity_id: Some("id1".into()),
                },
                RuleAction::Redirect {
                    to: vec!["not an address".into()],
                },
                RuleAction::Move {
                    folder: FolderRef::Path {
                        path: "x".into(),
                        account_id: Some("home".into()),
                    },
                },
                RuleAction::Tag { tag: "a,b".into() },
                RuleAction::Flag { flagged: true },
            ],
        );
        let plan = plan_of(&r);
        assert_eq!(
            ids(&plan),
            [
                texts::RULE_UNTAG,
                texts::RULE_MARK_UNREAD,
                texts::RULE_REPLY,
                texts::RULE_NOTIFY,
                texts::RULE_FORWARD_IDENTITY,
                texts::RULE_ADDRESS,
                texts::RULE_OTHER_ACCOUNT,
                texts::RULE_CATEGORY_NAME,
                texts::RULE_FLAG
            ]
        );
        assert_eq!(plan.reasons[0].args["action"], MessageArg::Number(1));
        // Two targets.
        let r = rule(
            Condition::always(),
            vec![reading(), RuleAction::Delete { permanent: false }],
        );
        assert_eq!(ids(&plan_of(&r)), [texts::RULE_TARGETS]);
        let r = rule(Condition::always(), vec![RuleAction::Stop]);
        assert_eq!(ids(&plan_of(&r)), [texts::RULE_NO_ACTIONS]);
    }

    #[test]
    fn client_placements() {
        let mut r = rule(
            Condition::Subject {
                matches: contains("x"),
            },
            vec![reading()],
        );
        r.enabled = false;
        r.triggers = vec![RuleTrigger::Arrival, RuleTrigger::Manual];
        r.scope = vec![RuleScope::Folder {
            folder: FolderRef::Path {
                path: "Lists".into(),
                account_id: None,
            },
        }];
        let plan = plan_of(&r);
        assert_eq!(plan.placement, RulePlacement::Client);
        assert_eq!(
            ids(&plan),
            [texts::RULE_DISABLED, texts::RULE_TRIGGER, texts::RULE_SCOPE]
        );
        let mut r = rule(
            Condition::Subject {
                matches: contains("x"),
            },
            vec![RuleAction::Move {
                folder: FolderRef::Path {
                    path: "New folder".into(),
                    account_id: None,
                },
            }],
        );
        r.triggers = vec![RuleTrigger::Manual];
        let plan = plan_of(&r);
        assert_eq!(
            ids(&plan),
            [texts::RULE_FOLDER_UNKNOWN, texts::RULE_NO_ARRIVAL]
        );
        assert_eq!(
            plan.reasons[0].args["folder"],
            MessageArg::Text("New folder".into())
        );
        let account = account();
        let off = PlacementContext {
            account_id: &account,
            server_rules: false,
            resolve_folder: &resolve,
        };
        let r = rule(Condition::always(), vec![reading()]);
        let off_plan = super::plan(&r, &off);
        assert_eq!(ids(&off_plan), [texts::RULE_SERVER_RULES_OFF]);
        assert!(format!("{off:?}").contains("work"));
    }

    #[test]
    fn scope() {
        let mut r = rule(Condition::always(), vec![reading()]);
        assert!(applies_to(&r, &account()));
        r.scope = vec![RuleScope::Account {
            account_id: "home".into(),
        }];
        assert!(!applies_to(&r, &account()));
        r.scope.push(RuleScope::Folder {
            folder: FolderRef::Role {
                role: FolderRole::Inbox,
                account_id: Some("work".into()),
            },
        });
        assert!(applies_to(&r, &account()));
    }

    #[test]
    fn server_rules_as_nail_rules() {
        let server = ExchangeInboxRule {
            id: "AQ".into(),
            name: "From boss".into(),
            sequence: 1,
            enabled: true,
            conditions: ExchangeRuleConditions {
                from_addresses: vec!["boss@corp.example".into()],
                header_contains: vec!["List-Id:".into()],
                has_attachments: Some(false),
                min_size_kb: Some(10),
                ..ExchangeRuleConditions::default()
            },
            exceptions: ExchangeRuleConditions {
                subject_contains: vec!["fyi".into(), "spam".into()],
                ..ExchangeRuleConditions::default()
            },
            actions: ExchangeRuleActions {
                move_to_folder: Some("f-reading".into()),
                mark_as_read: true,
                assign_categories: vec!["boss".into()],
                stop_processing_rules: true,
                ..ExchangeRuleActions::default()
            },
            has_unsupported: false,
            in_error: false,
            nail_rule_id: None,
        };
        let refs = |id: &FolderId| {
            (id.as_str() == "f-reading").then(|| FolderRef::Path {
                path: "Reading".into(),
                account_id: None,
            })
        };
        let nail = nail_rule_of(&server, &account(), &refs).unwrap();
        assert_eq!(nail.triggers, [RuleTrigger::Arrival]);
        assert_eq!(
            nail.actions,
            [
                RuleAction::MarkRead { read: true },
                RuleAction::Tag { tag: "boss".into() },
                reading(),
                RuleAction::Stop
            ]
        );
        // And it places back as the same inbox rule.
        let plan = plan_of(&nail);
        assert_eq!(plan.placement, RulePlacement::Server, "{:?}", plan.reasons);
        let back = plan.inbox_rule.unwrap();
        assert_eq!(back.conditions, server.conditions);
        assert_eq!(back.exceptions, server.exceptions);
        assert_eq!(back.actions, server.actions);

        let mut odd = server.clone();
        odd.conditions.importance = Some(nail_api::exchange::rules::Importance::High);
        assert_eq!(
            nail_rule_of(&odd, &account(), &refs).unwrap_err().id,
            texts::SERVER_RULE_UNSUPPORTED
        );
        let mut odd = server.clone();
        odd.actions.move_to_folder = Some("f-unknown".into());
        assert!(nail_rule_of(&odd, &account(), &refs).is_err());
        let mut odd = server;
        odd.has_unsupported = true;
        assert!(nail_rule_of(&odd, &account(), &refs).is_err());
    }
}
