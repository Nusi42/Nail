//! Rules: `create_rule` and `run_rules` (writes) and `test_rule` (a dry
//! run, read-only).
//!
//! Rules see every account, so all three need a grant for all accounts (as
//! `nail://rules` does).

use nail_api::agents::{ActionKind, ActionTargetKind};
use nail_api::mcp::McpScope;
use nail_api::mcp::tools::{
    CreateRuleArgs, MAX_RUN_RULES, RunRulesArgs, TestRuleArgs, rule_action_kind, run_rules_kind,
};
use nail_api::{
    AccountId, ApiError, FolderId, LocalizedMessage, MessageId, Request, Response, Rule, RuleId,
};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};

use super::plan::{Action, Plan, accounts, change, count, invalid, not_found, target, unexpected};
use super::{Ctx, audit};
use crate::access::{Access, Denial};
use crate::content::untrusted;
use crate::data::McpData;
use crate::server::{Failure, Shared, summary_json};

/// Longest rule text (characters).
const MAX_RULE_TEXT: usize = 64 * 1024;
/// Default and largest `test_rule` limit.
const TEST_DEFAULT: u32 = 50;
const TEST_MAX: u32 = 500;

impl<D: McpData> Shared<D> {
    /// Parses one rule's text form; errors are tool errors naming the
    /// first problem.
    async fn parse_rule(&self, text: &str) -> Result<Rule, Failure> {
        if text.trim().is_empty() || text.chars().count() > MAX_RULE_TEXT {
            return Err(invalid(format!(
                "the rule text has 1 to {MAX_RULE_TEXT} characters"
            )));
        }
        let response = self
            .data
            .query(Request::RuleFromText {
                text: text.to_owned(),
                draft: false,
            })
            .await?;
        match response {
            Response::RuleParsed {
                rule: Some(rule), ..
            } => Ok(*rule),
            Response::RuleParsed { errors, .. } => {
                let first = errors.into_iter().next();
                let message = first.as_ref().map_or_else(
                    || "the rule text is invalid".to_owned(),
                    |e| format!("line {}, column {}: {}", e.line, e.column, e.message),
                );
                let mut err = ApiError::invalid_request(message);
                if let Some(localized) = first.and_then(|e| e.localized) {
                    err = err.with_localized(localized);
                }
                Err(Failure::Api(err))
            }
            other => Err(unexpected(&other)),
        }
    }

    /// `test_rule`: which messages a rule would match and what it would
    /// do. Changes nothing; matches outside the grant are left out.
    pub(crate) async fn test_rule(
        &self,
        access: &Access,
        a: TestRuleArgs,
    ) -> Result<CallToolResult, Failure> {
        access.require(McpScope::Read)?;
        access.require(McpScope::Rules)?;
        if !access.all_accounts() {
            return Err(Denial::AllAccounts.into());
        }
        let folder_id = a.folder_id.map(FolderId::new);
        if let Some(folder) = &folder_id {
            self.require_folder(access, folder).await?;
        }
        let rule = self.parse_rule(&a.text).await?;
        let response = self
            .data
            .query(Request::TestRule {
                rule: Box::new(rule),
                message_ids: Vec::new(),
                folder_id,
                limit: a.limit.unwrap_or(TEST_DEFAULT).clamp(1, TEST_MAX),
            })
            .await?;
        let Response::RuleTested { scanned, matches } = response else {
            return Err(unexpected(&response));
        };
        let mut shown = Vec::new();
        for m in matches {
            if !access.allows_message(&m.message) {
                continue;
            }
            if !access.has(McpScope::Encrypted) && self.data.is_encrypted(&m.message.id).await? {
                continue;
            }
            shown.push(json!({
                "message": summary_json(&m.message),
                "actions": m.actions,
                "explanation": m.explanation,
            }));
        }
        Ok(CallToolResult::structured(untrusted(json!({
            "scanned": scanned,
            "matches": shown,
        }))))
    }
}

/// `create_rule`: a new rule from its text form (never replaces one).
pub(crate) async fn create_rule<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: CreateRuleArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Rules)?;
    if !ctx.access.all_accounts() {
        return Err(Denial::AllAccounts.into());
    }
    let mut rule = ctx.shared.parse_rule(&a.text).await?;
    // An empty id makes the core create a new rule.
    rule.id = RuleId::new(String::new());
    rule.enabled = a.enabled;
    rule.source = None;
    let name = rule.name.clone();
    Ok(Plan {
        kind: ActionKind::Rule,
        summary: LocalizedMessage::new("confirmation-summary-rule")
            .arg("actor", ctx.actor())
            .arg("name", name.as_str()),
        targets: vec![target(ActionTargetKind::Rule, "new", &name)],
        changes: vec![
            change("action-field-rule", None, Some(a.text.trim().to_owned())),
            change("mcp-tool-field-enabled", None, Some(ctx.yes_no(a.enabled))),
        ],
        accounts: Vec::new(),
        audit: vec![
            audit(
                "rule",
                None,
                Some(format!("{} chars", a.text.chars().count())),
            ),
            audit("enabled", None, Some(a.enabled.to_string())),
            audit(
                "actions",
                None,
                Some(
                    rule.actions
                        .iter()
                        .filter_map(|action| {
                            serde_json::to_value(action).ok().and_then(|v| {
                                v.get("type")
                                    .or_else(|| v.get("kind"))
                                    .and_then(Value::as_str)
                                    .map(str::to_owned)
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ),
        ],
        sends: 0,
        action: Action::Requests {
            requests: vec![Request::SaveRule {
                rule: Box::new(rule),
                position: None,
            }],
        },
    })
}

/// The saved rules `wanted` names (by id, else by a name only one rule
/// has), in file order, each once.
fn chosen_rules(saved: Vec<Rule>, wanted: &[String]) -> Result<Vec<Rule>, Failure> {
    if wanted.is_empty() || wanted.len() > MAX_RUN_RULES {
        return Err(invalid(format!("name 1 to {MAX_RUN_RULES} rules")));
    }
    let mut ids: Vec<RuleId> = Vec::new();
    for name in wanted {
        let id = if let Some(rule) = saved.iter().find(|r| r.id.as_str() == name) {
            rule.id.clone()
        } else {
            let mut named = saved.iter().filter(|r| &r.name == name);
            match (named.next(), named.next()) {
                (Some(rule), None) => rule.id.clone(),
                (Some(_), Some(_)) => {
                    return Err(invalid(format!(
                        "several rules are named `{name}`: name it by its id"
                    )));
                }
                (None, _) => return Err(not_found(format!("rule {name}"))),
            }
        };
        ids.push(id);
    }
    Ok(saved.into_iter().filter(|r| ids.contains(&r.id)).collect())
}

/// Refuses a `run_rules` target whose size or folders the grant cannot
/// check. Rules that send run only on named messages, so every send counts
/// against the agent's `sends_per_hour` (a folder's or the rules' own
/// messages are not known in advance). Without a target the rules run on
/// their own folders (else every Inbox), which a grant with excluded
/// folders cannot vouch for, so such a grant must name the target.
fn check_run_target(access: &Access, a: &RunRulesArgs, send_actions: usize) -> Result<(), Failure> {
    if send_actions > 0 && a.message_ids.is_empty() {
        return Err(invalid(
            "rules that forward, redirect or reply run only on named `message_ids`",
        ));
    }
    if a.message_ids.is_empty() && a.folder_id.is_none() && access.excludes_folders() {
        return Err(invalid(
            "the grant excludes folders: name `message_ids` or a `folder_id`",
        ));
    }
    Ok(())
}

/// `run_rules`: runs saved rules now on messages, a folder or each rule's
/// own messages (R1, ADR 0219). The rules' strongest action decides the
/// policy, and every kind of action they take needs its scopes.
pub(crate) async fn run_rules<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: RunRulesArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Rules)?;
    ctx.access.require(McpScope::Organize)?;
    if !ctx.access.all_accounts() {
        return Err(Denial::AllAccounts.into());
    }
    let response = ctx.shared.data.query(Request::ListRules).await?;
    let Response::Rules { rules: saved } = response else {
        return Err(unexpected(&response));
    };
    let rules = chosen_rules(saved, &a.rules)?;
    for action in rules.iter().flat_map(|r| &r.actions) {
        for scope in rule_action_kind(action).mcp_scopes() {
            ctx.access.require(*scope)?;
        }
    }
    let kind = run_rules_kind(rules.iter().flat_map(|r| &r.actions));
    let mut targets: Vec<_> = rules
        .iter()
        .map(|r| target(ActionTargetKind::Rule, r.id.as_str(), &r.name))
        .collect();
    let mut audit_lines = vec![audit(
        "rules",
        None,
        Some(
            rules
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
    )];
    let send_actions = rules
        .iter()
        .flat_map(|r| &r.actions)
        .filter(|action| rule_action_kind(action) == ActionKind::Send)
        .count();
    check_run_target(ctx.access, &a, send_actions)?;
    let mut account_ids: Vec<AccountId> = Vec::new();
    let mut message_ids: Vec<MessageId> = Vec::new();
    let mut folder_id = None;
    let mut sends = 0;
    if !a.message_ids.is_empty() {
        let messages = ctx.target_messages(&a.message_ids).await?;
        targets.extend(
            messages
                .iter()
                .map(|m| target(ActionTargetKind::Message, m.id.as_str(), &m.subject)),
        );
        audit_lines.push(audit("messages", None, count(messages.len())));
        account_ids = accounts(messages.iter().map(|m| &m.account_id));
        sends = u32::try_from(send_actions.saturating_mul(messages.len())).unwrap_or(u32::MAX);
        message_ids = messages.into_iter().map(|m| m.id).collect();
    } else if let Some(id) = a.folder_id {
        let folder = ctx.writable_folder(&FolderId::new(id)).await?;
        targets.push(target(
            ActionTargetKind::Folder,
            folder.id.as_str(),
            &folder.name,
        ));
        audit_lines.push(audit("folder", None, Some(folder.id.as_str().to_owned())));
        account_ids = vec![folder.account_id.clone()];
        folder_id = Some(folder.id);
    }
    let first = rules.first().map(|r| r.name.clone()).unwrap_or_default();
    Ok(Plan {
        kind,
        summary: LocalizedMessage::new("confirmation-summary-run-rules")
            .arg("actor", ctx.actor())
            .arg("count", rules.len())
            .arg("name", first.as_str()),
        targets,
        changes: vec![change(
            "action-field-rule",
            None,
            Some(
                rules
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        )],
        accounts: account_ids,
        audit: audit_lines,
        sends,
        action: Action::Requests {
            requests: vec![Request::RunRules {
                rule_ids: rules.into_iter().map(|r| r.id).collect(),
                message_ids,
                folder_id,
            }],
        },
    })
}
