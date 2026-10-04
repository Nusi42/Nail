//! Server-side inbox rules of Exchange accounts (M3c, ADR 0072).
//!
//! Exchange runs inbox rules on the server (Graph `messageRule`, EWS
//! `Get/UpdateInboxRules`). For Exchange accounts they replace Sieve:
//!
//! * Nail rules (`rules.kdl`) whose conditions and actions Exchange can
//!   express are **placed** as inbox rules ([`ExchangeRuleStatus`] with
//!   [`crate::sieve::RulePlacement::Server`]); the rest run in Nail. The
//!   mapping layer (`nail-exchange`) converts a Nail rule into an
//!   [`ExchangeInboxRule`] and says why one is not expressible.
//! * The user's own server rules (made in Outlook) are listed, can be
//!   switched on / off and deleted; Nail does not edit their conditions
//!   (a rule with parts Nail does not model is
//!   [`ExchangeInboxRule::has_unsupported`] and read-only).
//!
//! [`ExchangeRuleConditions`] / [`ExchangeRuleActions`] are the subset of
//! Graph's `messageRulePredicates` / `messageRuleActions` both engines
//! read and write (EWS `RulePredicates` / `RuleActions` carry the same
//! fields). Every list matches when **any** entry matches; all set
//! conditions must match; any exception keeps the rule from running.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::ids::{AccountId, FolderId, RuleId};
use crate::localized::LocalizedMessage;
use crate::sieve::RulePlacement;

/// Message importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Importance {
    /// Low.
    Low,
    /// Normal.
    Normal,
    /// High.
    High,
}

/// Conditions (or exceptions) of an inbox rule. Empty = matches every
/// message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ExchangeRuleConditions {
    /// The sender is one of these addresses.
    pub from_addresses: Vec<String>,
    /// A recipient (To / Cc) is one of these addresses.
    pub sent_to_addresses: Vec<String>,
    /// The subject contains one of these strings.
    pub subject_contains: Vec<String>,
    /// The body contains one of these strings.
    pub body_contains: Vec<String>,
    /// Subject or body contain one of these strings.
    pub body_or_subject_contains: Vec<String>,
    /// The headers contain one of these strings.
    pub header_contains: Vec<String>,
    /// The sender's address contains one of these strings.
    pub sender_contains: Vec<String>,
    /// A recipient's address contains one of these strings.
    pub recipient_contains: Vec<String>,
    /// The message has one of these categories.
    pub categories: Vec<String>,
    /// Has attachments.
    pub has_attachments: Option<bool>,
    /// Has this importance.
    pub importance: Option<Importance>,
    /// The user is in To.
    pub sent_to_me: Option<bool>,
    /// The user is the only recipient.
    pub sent_only_to_me: Option<bool>,
    /// The user is in Cc.
    pub sent_cc_me: Option<bool>,
    /// The user is in To or Cc.
    pub sent_to_or_cc_me: Option<bool>,
    /// The user is not a recipient.
    pub not_sent_to_me: Option<bool>,
    /// A meeting request.
    pub is_meeting_request: Option<bool>,
    /// Size at least this many kilobytes.
    pub min_size_kb: Option<u32>,
    /// Size at most this many kilobytes.
    pub max_size_kb: Option<u32>,
}

impl ExchangeRuleConditions {
    /// Whether no condition is set.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// What an inbox rule does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent boolean actions of an Exchange inbox rule"
)]
pub struct ExchangeRuleActions {
    /// Move to this folder (a folder of the account). At the engine seam
    /// (`ExchangeSettingsEngine`) this carries the folder's **remote id**
    /// (Graph / EWS folder id) wrapped in a `FolderId`; the core translates
    /// to and from its local folder ids (`exchange_folders.remote_id`).
    pub move_to_folder: Option<FolderId>,
    /// Copy to this folder (remote id at the engine seam, like
    /// `move_to_folder`).
    pub copy_to_folder: Option<FolderId>,
    /// Move to Deleted Items.
    pub delete: bool,
    /// Delete permanently.
    pub permanent_delete: bool,
    /// Mark as read.
    pub mark_as_read: bool,
    /// Set the importance.
    pub mark_importance: Option<Importance>,
    /// Add these categories (Nail tags).
    pub assign_categories: Vec<String>,
    /// Forward to these addresses.
    pub forward_to: Vec<String>,
    /// Forward as an attachment to these addresses.
    pub forward_as_attachment_to: Vec<String>,
    /// Redirect to these addresses.
    pub redirect_to: Vec<String>,
    /// Do not run later rules.
    pub stop_processing_rules: bool,
}

impl ExchangeRuleActions {
    /// Whether the rule does nothing.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// An inbox rule on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeInboxRule {
    /// The server's id (empty for a rule not created yet).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Order (lower runs first).
    pub sequence: u32,
    /// Whether it runs.
    pub enabled: bool,
    /// Conditions.
    #[serde(default)]
    pub conditions: ExchangeRuleConditions,
    /// Exceptions.
    #[serde(default)]
    pub exceptions: ExchangeRuleConditions,
    /// Actions.
    #[serde(default)]
    pub actions: ExchangeRuleActions,
    /// The server rule has conditions or actions Nail does not model:
    /// Nail only switches it on / off or deletes it.
    #[serde(default)]
    pub has_unsupported: bool,
    /// The server marked it broken (`isReadOnly` / `hasError`).
    #[serde(default)]
    pub in_error: bool,
    /// The Nail rule it was placed from; `null` = the user's own server
    /// rule.
    #[serde(default)]
    pub nail_rule_id: Option<RuleId>,
}

impl ExchangeInboxRule {
    /// Whether Nail may change the rule's conditions and actions (its own
    /// placed rules only).
    pub fn editable(&self) -> bool {
        self.nail_rule_id.is_some() && !self.has_unsupported
    }
}

/// Where one Nail rule runs for an Exchange account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeRuleStatus {
    /// The Nail rule.
    pub rule_id: RuleId,
    /// Server (placed), client, or not expressible.
    pub placement: RulePlacement,
    /// The server rule it was placed as.
    #[serde(default)]
    pub server_rule_id: Option<String>,
    /// Why it runs in Nail (`exchange-rule-reason-*` with the condition's
    /// position); empty for `server`.
    #[serde(default)]
    pub reasons: Vec<LocalizedMessage>,
}

/// State of an account's server rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeRulesState {
    /// Placing is switched off (`server-rules=#false`): every Nail rule
    /// runs in Nail; server rules are still listed.
    Disabled,
    /// Changes wait to be written.
    Pending,
    /// Writing now.
    Syncing,
    /// The server has the current rules.
    InSync,
    /// The last write failed; retried on the next change or
    /// [`crate::Request::SyncExchangeRules`].
    Failed,
}

impl ExchangeRulesState {
    /// Every state.
    pub const ALL: [ExchangeRulesState; 5] = [
        Self::Disabled,
        Self::Pending,
        Self::Syncing,
        Self::InSync,
        Self::Failed,
    ];

    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Pending => "pending",
            Self::Syncing => "syncing",
            Self::InSync => "in-sync",
            Self::Failed => "failed",
        }
    }

    /// The state of a name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

/// The rules of an Exchange account ([`crate::Request::GetExchangeRules`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeRulesStatus {
    /// The account.
    pub account_id: AccountId,
    /// State.
    pub state: ExchangeRulesState,
    /// Where each Nail rule that applies to the account runs, in
    /// `rules.kdl` order.
    #[serde(default)]
    pub rules: Vec<ExchangeRuleStatus>,
    /// Every inbox rule on the server (Nail's and the user's), by
    /// sequence.
    #[serde(default)]
    pub server_rules: Vec<ExchangeInboxRule>,
    /// When the server rules were last read (RFC 3339 UTC).
    #[serde(default)]
    pub synced_at: Option<String>,
    /// Error class of the last failure.
    #[serde(default)]
    pub error: Option<ErrorCode>,
    /// The user-facing text of the failure (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_parts_and_editability() {
        assert!(ExchangeRuleConditions::default().is_empty());
        assert!(ExchangeRuleActions::default().is_empty());
        let mut rule: ExchangeInboxRule = serde_json::from_value(json!({
            "id": "AQAAAJ5", "name": "Newsletters", "sequence": 2, "enabled": true,
            "conditions": {"sender_contains": ["@substack.com"]},
            "actions": {"assign_categories": ["newsletter"], "move_to_folder": "f-reading"}
        }))
        .unwrap();
        assert!(!rule.conditions.is_empty());
        assert_eq!(rule.actions.move_to_folder, Some("f-reading".into()));
        assert!(!rule.editable(), "the user's own rule");
        rule.nail_rule_id = Some("newsletters".into());
        assert!(rule.editable());
        rule.has_unsupported = true;
        assert!(!rule.editable());
    }

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_value(Importance::High).unwrap(),
            json!("high")
        );
        for state in ExchangeRulesState::ALL {
            assert_eq!(ExchangeRulesState::parse(state.as_str()), Some(state));
            assert_eq!(serde_json::to_value(state).unwrap(), json!(state.as_str()));
        }
        let status: ExchangeRulesStatus = serde_json::from_value(json!({
            "account_id": "work", "state": "in-sync",
            "rules": [{"rule_id": "r1", "placement": "server", "server_rule_id": "AQ"}]
        }))
        .unwrap();
        assert_eq!(status.rules[0].placement, RulePlacement::Server);
        assert!(status.server_rules.is_empty());
    }
}
