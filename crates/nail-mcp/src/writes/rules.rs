//! Rules: `create_rule` (a write) and `test_rule` (a dry run, read-only).
//!
//! Rules see every account, so both need a grant for all accounts (as
//! `nail://rules` does).

use nail_api::agents::{ActionKind, ActionTargetKind};
use nail_api::mcp::McpScope;
use nail_api::mcp::tools::{CreateRuleArgs, TestRuleArgs};
use nail_api::{ApiError, FolderId, LocalizedMessage, Request, Response, Rule, RuleId};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};

use super::plan::{Action, Plan, change, invalid, target, unexpected};
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
