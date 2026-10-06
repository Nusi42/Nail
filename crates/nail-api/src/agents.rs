//! Writes by agents and plug-ins: action kinds, confirmations and
//! "always allow" policies (M4a, ADR 0079; design.md, "MCP server" →
//! "Permissions and safety", "Plug-ins" → "Permissions and sandbox"; plan
//! section 9, "Scope decisions for M4").
//!
//! Every write an MCP client or a plug-in makes is one **action** of an
//! [`ActionKind`]. It goes through the operation queue and the audit log
//! like the user's own writes. Whether the user is asked first depends on
//! the actor's [`ActionPolicy`] for that kind:
//!
//! * destructive kinds ([`ActionKind::is_destructive`]: moving to Trash,
//!   permanent delete, deleting calendar / contact / task items) **always**
//!   ask; no policy relaxes them, and permanent delete is never
//!   auto-confirmed by anything;
//! * outgoing kinds (sending mail, mail to attendees) and rule changes ask
//!   by default; the user may switch them to "always allow" per MCP client
//!   or plug-in;
//! * the other kinds (flags / tags / moves, drafts, calendar, contact and
//!   task writes that mail nobody) run without asking by default; the user
//!   may make them ask.
//!
//! A write that needs the user's decision becomes a pending
//! [`Confirmation`] ([`crate::Event::ConfirmationRequested`]): attributed to
//! its actor, with a localized summary, the affected items and a before /
//! after list. The user approves or denies it in Nail's confirmation center
//! ([`crate::Request::DecideConfirmation`], optionally "always allow" for
//! that actor and kind when [`ActionKind::can_always_allow`]). The caller
//! waits for the decision up to [`CONFIRMATION_TIMEOUT_SECS`]; then the
//! request expires and nothing changes. MCP clients that support
//! elicitation also get the question there ([`ConfirmationSource`]); an
//! elicitation answer may deny anything but approve only kinds that could
//! be always-allowed — destructive actions are approved in Nail only.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::ids::{AccountId, ConfirmationId, McpClientId, PluginId};
use crate::localized::LocalizedMessage;
use crate::mcp::McpScope;
use crate::plugins::PluginPermission;

/// How long a pending confirmation waits for the user by default
/// (seconds); then it expires and the action is not run.
pub const CONFIRMATION_TIMEOUT_SECS: u64 = 300;
/// Longest wait a caller may ask for (seconds).
pub const MAX_CONFIRMATION_TIMEOUT_SECS: u64 = 3600;
/// Most pending confirmations one actor may have at a time; more are
/// refused with `rate_limited`.
pub const MAX_PENDING_PER_ACTOR: u32 = 20;
/// Most items a confirmation lists ([`Confirmation::targets`]); the rest
/// is counted in [`Confirmation::more_targets`].
pub const MAX_CONFIRMATION_TARGETS: usize = 50;
/// Most before / after lines of a confirmation.
pub const MAX_CONFIRMATION_CHANGES: usize = 50;
/// Longest text of a target label or a before / after value (characters;
/// longer ones are cut with `…`).
pub const MAX_CONFIRMATION_TEXT: usize = 500;
/// Decided confirmations kept for the history (the oldest go first).
pub const CONFIRMATION_HISTORY: u32 = 500;

/// What kind of write an action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    /// Flags, tags, read state, moves, archiving, snoozing (MCP
    /// `update_messages`; plug-in `mail.organize`).
    Organize,
    /// Moving messages to Trash (MCP `delete_messages`). Destructive.
    Trash,
    /// Deleting messages for good (MCP `delete_messages` with
    /// `permanent`). Destructive; never auto-confirmed.
    PermanentDelete,
    /// Creating or changing drafts, editing the open draft.
    Draft,
    /// Sending a message (MCP `send_message`, forwarding, a plug-in's
    /// `nail.mail.send`). Outgoing.
    Send,
    /// Calendar writes that mail nobody.
    Calendar,
    /// Calendar writes that mail attendees (invitations, updates,
    /// cancellations) and answers to invitations. Outgoing.
    Invite,
    /// Creating or changing contacts.
    Contact,
    /// Creating or changing tasks.
    Task,
    /// Deleting events, contacts or tasks. Destructive.
    PimDelete,
    /// Creating or changing filter rules (MCP `create_rule`).
    Rule,
}

impl ActionKind {
    /// Every kind, in display order.
    pub const ALL: [ActionKind; 11] = [
        Self::Organize,
        Self::Trash,
        Self::PermanentDelete,
        Self::Draft,
        Self::Send,
        Self::Calendar,
        Self::Invite,
        Self::Contact,
        Self::Task,
        Self::PimDelete,
        Self::Rule,
    ];

    /// The wire name (`permanent-delete`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Organize => "organize",
            Self::Trash => "trash",
            Self::PermanentDelete => "permanent-delete",
            Self::Draft => "draft",
            Self::Send => "send",
            Self::Calendar => "calendar",
            Self::Invite => "invite",
            Self::Contact => "contact",
            Self::Task => "task",
            Self::PimDelete => "pim-delete",
            Self::Rule => "rule",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Whether the action removes data (Trash counts: the user decides,
    /// not an agent). Destructive kinds always ask.
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Trash | Self::PermanentDelete | Self::PimDelete)
    }

    /// Whether the action sends mail to someone.
    pub fn is_outgoing(self) -> bool {
        matches!(self, Self::Send | Self::Invite)
    }

    /// Whether the user may set "always allow" for this kind (every kind
    /// that is not destructive).
    pub fn can_always_allow(self) -> bool {
        !self.is_destructive()
    }

    /// The policy until the user changes it: destructive, outgoing and
    /// rule changes ask; everything else is allowed.
    pub fn default_mode(self) -> ActionPolicyMode {
        if self.is_destructive() || self.is_outgoing() || self == Self::Rule {
            ActionPolicyMode::Ask
        } else {
            ActionPolicyMode::Allow
        }
    }

    /// The MCP scopes a client needs for an action of this kind (all of
    /// them; the account / folder checks come on top).
    pub fn mcp_scopes(self) -> &'static [McpScope] {
        match self {
            Self::Organize | Self::Trash => &[McpScope::Organize],
            Self::PermanentDelete => &[McpScope::Organize, McpScope::PermanentDelete],
            Self::Draft => &[McpScope::Draft],
            Self::Send => &[McpScope::Send],
            Self::Calendar => &[McpScope::Calendar],
            Self::Invite => &[McpScope::Calendar, McpScope::Send],
            Self::Contact | Self::Task => &[McpScope::Pim],
            // Deleting events needs `calendar`, contacts / tasks `pim`; the
            // tool checks the item's kind.
            Self::PimDelete => &[],
            Self::Rule => &[McpScope::Rules],
        }
    }

    /// The plug-in permissions an action of this kind needs; empty when
    /// it depends on the item (PIM deletes: `calendar.write`,
    /// `contacts.write` or `tasks.write`) or plug-ins cannot do it (rules).
    pub fn plugin_permissions(self) -> Vec<PluginPermission> {
        match self {
            Self::Organize | Self::Trash | Self::PermanentDelete => {
                vec![PluginPermission::MailOrganize]
            }
            Self::Draft | Self::Send => vec![PluginPermission::MailSend],
            Self::Calendar => vec![PluginPermission::CalendarWrite],
            Self::Invite => vec![PluginPermission::CalendarWrite, PluginPermission::MailSend],
            Self::Contact => vec![PluginPermission::ContactsWrite],
            Self::Task => vec![PluginPermission::TasksWrite],
            Self::PimDelete | Self::Rule => Vec::new(),
        }
    }

    /// Fluent id of the kind's name (`action-kind-<kind>`).
    pub fn message_id(self) -> String {
        format!("action-kind-{}", self.as_str())
    }
}

impl std::fmt::Display for ActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an actor's actions of one kind ask first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ActionPolicyMode {
    /// A confirmation the user approves or denies.
    Ask,
    /// Runs without asking ("always allow"); only for kinds that
    /// [`ActionKind::can_always_allow`].
    Allow,
}

impl ActionPolicyMode {
    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ask" => Some(Self::Ask),
            "allow" => Some(Self::Allow),
            _ => None,
        }
    }
}

/// Who acts: an MCP client or a plug-in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionActor {
    /// A registered MCP client.
    McpClient {
        /// The client.
        client_id: McpClientId,
    },
    /// An installed plug-in.
    Plugin {
        /// The plug-in.
        plugin_id: PluginId,
    },
}

impl ActionActor {
    /// An MCP client.
    pub fn mcp_client(id: impl Into<McpClientId>) -> Self {
        Self::McpClient {
            client_id: id.into(),
        }
    }

    /// A plug-in.
    pub fn plugin(id: impl Into<PluginId>) -> Self {
        Self::Plugin {
            plugin_id: id.into(),
        }
    }

    /// The store's `actor_kind` (`mcp-client`, `plugin`).
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::McpClient { .. } => "mcp-client",
            Self::Plugin { .. } => "plugin",
        }
    }

    /// The client or plug-in id.
    pub fn id_str(&self) -> &str {
        match self {
            Self::McpClient { client_id } => client_id.as_str(),
            Self::Plugin { plugin_id } => plugin_id.as_str(),
        }
    }

    /// Rebuilds an actor from [`Self::kind_str`] and [`Self::id_str`].
    pub fn from_parts(kind: &str, id: &str) -> Option<Self> {
        match kind {
            "mcp-client" => Some(Self::mcp_client(id)),
            "plugin" => Some(Self::plugin(id)),
            _ => None,
        }
    }
}

/// An MCP client a plug-in acts for (a plug-in tool called over MCP,
/// `mcp.expose`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpClientRef {
    /// The client.
    pub client_id: McpClientId,
    /// Its name at the time.
    pub name: String,
}

/// Who asked for an action, as the confirmation center shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActionAttribution {
    /// The actor whose policy applies.
    pub actor: ActionActor,
    /// The client's or plug-in's name at the time (plug-in names already
    /// resolved for the core's locale).
    pub name: String,
    /// For a plug-in serving an MCP tool call: the client that called it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub on_behalf_of: Option<McpClientRef>,
}

/// An actor's policy for one kind ([`crate::Response::ActionPolicies`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActionPolicy {
    /// The kind.
    pub kind: ActionKind,
    /// What applies now.
    pub mode: ActionPolicyMode,
    /// Whether the user changed it (else it is
    /// [`ActionKind::default_mode`]).
    pub customized: bool,
    /// Whether the user may switch it to `allow`
    /// ([`ActionKind::can_always_allow`]).
    pub can_allow: bool,
}

impl ActionPolicy {
    /// The policy of `kind` when the user stored `stored` (or nothing).
    pub fn effective(kind: ActionKind, stored: Option<ActionPolicyMode>) -> Self {
        // A stored `allow` of a destructive kind is never honoured (the
        // store refuses it too).
        let stored = stored.filter(|m| *m == ActionPolicyMode::Ask || kind.can_always_allow());
        Self {
            kind,
            mode: stored.unwrap_or_else(|| kind.default_mode()),
            customized: stored.is_some(),
            can_allow: kind.can_always_allow(),
        }
    }

    /// The full table of an actor: one policy per kind, in
    /// [`ActionKind::ALL`] order, from the stored modes.
    pub fn table(stored: &[(ActionKind, ActionPolicyMode)]) -> Vec<Self> {
        ActionKind::ALL
            .into_iter()
            .map(|kind| {
                let mode = stored.iter().find(|(k, _)| *k == kind).map(|(_, m)| *m);
                Self::effective(kind, mode)
            })
            .collect()
    }

    /// Whether an action of this policy needs a confirmation.
    pub fn asks(&self) -> bool {
        self.mode == ActionPolicyMode::Ask
    }
}

/// Why a policy cannot be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ActionPolicyIssue {
    /// `allow` for a destructive kind (Trash, permanent delete, PIM
    /// deletes always ask).
    Destructive,
}

impl ActionPolicyIssue {
    /// All issues.
    pub const ALL: [ActionPolicyIssue; 1] = [Self::Destructive];

    /// Fluent id (`action-policy-issue-<issue>`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Destructive => "action-policy-issue-destructive",
        }
    }
}

/// Checks a policy the user wants to set.
pub fn check_policy(kind: ActionKind, mode: ActionPolicyMode) -> Result<(), ActionPolicyIssue> {
    if mode == ActionPolicyMode::Allow && !kind.can_always_allow() {
        Err(ActionPolicyIssue::Destructive)
    } else {
        Ok(())
    }
}

/// The state of a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmationState {
    /// Waiting for the user.
    Pending,
    /// Approved; the action ran (or is running).
    Approved,
    /// Denied; nothing changed.
    Denied,
    /// Nobody decided in time; nothing changed.
    Expired,
    /// The caller went away or the action became impossible (the item is
    /// gone, the grant was revoked, the plug-in stopped); nothing changed.
    Cancelled,
    /// Approved, but running the action failed
    /// ([`Confirmation::error`]).
    Failed,
}

impl ConfirmationState {
    /// All states.
    pub const ALL: [ConfirmationState; 6] = [
        Self::Pending,
        Self::Approved,
        Self::Denied,
        Self::Expired,
        Self::Cancelled,
        Self::Failed,
    ];

    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    /// Whether the decision is made (anything but `pending`).
    pub fn is_final(self) -> bool {
        self != Self::Pending
    }

    /// Fluent id of the state's name (`confirmation-state-<state>`).
    pub fn message_id(self) -> String {
        format!("confirmation-state-{}", self.as_str())
    }
}

/// Where the decision came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmationSource {
    /// Nail's confirmation center (desktop or remote UI).
    Nail,
    /// The MCP client's elicitation form (only denials, and approvals of
    /// kinds that [`ActionKind::can_always_allow`]).
    Elicitation,
    /// The core: expiry or cancellation.
    System,
}

impl ConfirmationSource {
    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nail => "nail",
            Self::Elicitation => "elicitation",
            Self::System => "system",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "nail" => Some(Self::Nail),
            "elicitation" => Some(Self::Elicitation),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    /// Whether a decision from here may approve an action of `kind`.
    pub fn may_approve(self, kind: ActionKind) -> bool {
        match self {
            Self::Nail => true,
            Self::Elicitation => kind.can_always_allow(),
            Self::System => false,
        }
    }
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmationDecision {
    /// Run it.
    Approve,
    /// Do not run it.
    Deny,
}

/// What kind of item an action touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ActionTargetKind {
    /// A message.
    Message,
    /// A draft.
    Draft,
    /// A folder.
    Folder,
    /// A calendar event.
    Event,
    /// A contact.
    Contact,
    /// A task.
    Task,
    /// A filter rule.
    Rule,
    /// A recipient address.
    Recipient,
}

/// One item an action touches, as listed in the confirmation.
///
/// `label` is mailbox content (a subject, a name, an address): the UI shows
/// it as plain text, never as markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActionTarget {
    /// What it is.
    pub kind: ActionTargetKind,
    /// Its id (message id, draft id, …; the address for recipients).
    pub id: String,
    /// What to show (subject, title, name; at most
    /// [`MAX_CONFIRMATION_TEXT`] characters).
    pub label: String,
}

/// One before / after line of a confirmation ("Folder: Inbox → Archive").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActionChange {
    /// What changes (a Fluent id, `action-field-*`, with arguments).
    pub field: LocalizedMessage,
    /// The value now; `null` when the item is new.
    pub before: Option<String>,
    /// The value after the action; `null` when it is removed.
    pub after: Option<String>,
}

/// A write waiting for (or decided by) the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Confirmation {
    /// The confirmation.
    pub id: ConfirmationId,
    /// Who asked.
    pub requested_by: ActionAttribution,
    /// What kind of write.
    pub kind: ActionKind,
    /// The MCP tool (`send_message`) or plug-in API (`nail.mail.send`)
    /// that asked.
    pub operation: String,
    /// One sentence for the user (`confirmation-summary-*` with
    /// arguments).
    pub summary: LocalizedMessage,
    /// The items it touches (at most [`MAX_CONFIRMATION_TARGETS`]).
    pub targets: Vec<ActionTarget>,
    /// How many more items it touches beyond `targets`.
    #[serde(default)]
    pub more_targets: u32,
    /// Before / after (at most [`MAX_CONFIRMATION_CHANGES`]).
    pub changes: Vec<ActionChange>,
    /// The accounts concerned (remote sessions see a confirmation only
    /// with a grant for all of them).
    pub account_ids: Vec<AccountId>,
    /// When it was asked (RFC 3339).
    pub created_at: String,
    /// When it expires while pending (RFC 3339).
    pub expires_at: String,
    /// Its state.
    pub state: ConfirmationState,
    /// When it was decided (RFC 3339).
    pub decided_at: Option<String>,
    /// Where the decision came from.
    pub decided_via: Option<ConfirmationSource>,
    /// Whether the UI may offer "always allow" with the approval
    /// ([`ActionKind::can_always_allow`]).
    pub can_always_allow: bool,
    /// Why running it failed (state `failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<LocalizedMessage>,
}

/// Which confirmations [`crate::Request::ListConfirmations`] returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmationFilter {
    /// Only pending ones, oldest first.
    #[default]
    Pending,
    /// Only decided ones (the history), newest first.
    Decided,
    /// Everything, newest first.
    All,
}

/// A confirmation as its requester submits it (Rust only: what `nail-mcp`
/// and `nail-plugins` hand to the core; the core assigns id, times and
/// state and keeps `payload` sealed until the decision).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConfirmation {
    /// Who asks.
    pub requested_by: ActionAttribution,
    /// What kind of write.
    pub kind: ActionKind,
    /// The tool or API name.
    pub operation: String,
    /// The summary sentence.
    pub summary: LocalizedMessage,
    /// The items (cut to [`MAX_CONFIRMATION_TARGETS`] by
    /// [`Self::normalize`]).
    pub targets: Vec<ActionTarget>,
    /// Before / after.
    pub changes: Vec<ActionChange>,
    /// The accounts concerned.
    pub account_ids: Vec<AccountId>,
    /// What to run once approved, in the requester's own format.
    pub payload: Value,
    /// The caller's idempotency key, if any (MCP).
    pub idempotency_key: Option<String>,
    /// How long to wait (seconds; capped at
    /// [`MAX_CONFIRMATION_TIMEOUT_SECS`]).
    pub timeout_secs: u64,
}

impl NewConfirmation {
    /// Applies the limits: targets beyond [`MAX_CONFIRMATION_TARGETS`] are
    /// counted, changes cut to [`MAX_CONFIRMATION_CHANGES`], texts to
    /// [`MAX_CONFIRMATION_TEXT`] characters, the timeout to 1 s ..=
    /// [`MAX_CONFIRMATION_TIMEOUT_SECS`]. Returns the number of targets
    /// left out.
    pub fn normalize(&mut self) -> u32 {
        let more = self.targets.len().saturating_sub(MAX_CONFIRMATION_TARGETS);
        self.targets.truncate(MAX_CONFIRMATION_TARGETS);
        self.changes.truncate(MAX_CONFIRMATION_CHANGES);
        for target in &mut self.targets {
            target.label = cut_text(&target.label);
        }
        for change in &mut self.changes {
            change.before = change.before.as_deref().map(cut_text);
            change.after = change.after.as_deref().map(cut_text);
        }
        self.timeout_secs = self.timeout_secs.clamp(1, MAX_CONFIRMATION_TIMEOUT_SECS);
        u32::try_from(more).unwrap_or(u32::MAX)
    }
}

/// Cuts `text` to [`MAX_CONFIRMATION_TEXT`] characters (with `…`).
pub fn cut_text(text: &str) -> String {
    if text.chars().count() <= MAX_CONFIRMATION_TEXT {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(MAX_CONFIRMATION_TEXT - 1).collect();
        out.push('…');
        out
    }
}

/// Fluent ids of the before / after field labels the contract names
/// (`action-field-*`, in `core-agents.ftl`).
pub const ACTION_FIELD_IDS: &[&str] = &[
    "action-field-folder",
    "action-field-flagged",
    "action-field-read",
    "action-field-tags",
    "action-field-snoozed-until",
    "action-field-to",
    "action-field-cc",
    "action-field-bcc",
    "action-field-subject",
    "action-field-body",
    "action-field-attachments",
    "action-field-send-at",
    "action-field-title",
    "action-field-start",
    "action-field-end",
    "action-field-location",
    "action-field-attendees",
    "action-field-response",
    "action-field-name",
    "action-field-emails",
    "action-field-due",
    "action-field-completed",
    "action-field-rule",
];

/// Fluent ids of the summary sentences the contract names
/// (`confirmation-summary-*` in `core-agents.ftl`; arguments `$actor` and
/// `$count`, plus `$subject` / `$title` / `$name` / `$tool` where the
/// catalog says so).
pub const CONFIRMATION_SUMMARY_IDS: &[&str] = &[
    "confirmation-summary-organize",
    "confirmation-summary-trash",
    "confirmation-summary-permanent-delete",
    "confirmation-summary-draft",
    "confirmation-summary-send",
    "confirmation-summary-calendar",
    "confirmation-summary-invite",
    "confirmation-summary-respond",
    "confirmation-summary-contact",
    "confirmation-summary-task",
    "confirmation-summary-pim-delete",
    "confirmation-summary-rule",
    "confirmation-summary-plugin-tool",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_and_defaults() {
        for kind in ActionKind::ALL {
            assert_eq!(ActionKind::parse(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
            assert!(crate::localized::is_message_id(&kind.message_id()));
            // Destructive kinds always ask and can never be relaxed.
            if kind.is_destructive() {
                assert_eq!(kind.default_mode(), ActionPolicyMode::Ask);
                assert!(!kind.can_always_allow());
            }
        }
        assert!(ActionKind::PermanentDelete.is_destructive());
        assert!(ActionKind::Trash.is_destructive());
        assert!(!ActionKind::Send.is_destructive() && ActionKind::Send.is_outgoing());
        assert_eq!(ActionKind::Send.default_mode(), ActionPolicyMode::Ask);
        assert_eq!(ActionKind::Invite.default_mode(), ActionPolicyMode::Ask);
        assert_eq!(ActionKind::Rule.default_mode(), ActionPolicyMode::Ask);
        // Drafts, flags / tags, PIM writes run without asking by default.
        for kind in [
            ActionKind::Organize,
            ActionKind::Draft,
            ActionKind::Calendar,
            ActionKind::Contact,
            ActionKind::Task,
        ] {
            assert_eq!(kind.default_mode(), ActionPolicyMode::Allow, "{kind}");
        }
        assert_eq!(
            ActionKind::Invite.mcp_scopes(),
            [McpScope::Calendar, McpScope::Send]
        );
        assert!(
            ActionKind::PermanentDelete
                .mcp_scopes()
                .contains(&McpScope::PermanentDelete)
        );
        assert_eq!(
            ActionKind::Task.plugin_permissions(),
            [PluginPermission::TasksWrite]
        );
        assert_eq!(ActionKind::Rule.plugin_permissions().len(), 0);
        assert_eq!(ActionKind::parse("everything"), None);
    }

    #[test]
    fn policies() {
        assert_eq!(
            check_policy(ActionKind::PermanentDelete, ActionPolicyMode::Allow),
            Err(ActionPolicyIssue::Destructive)
        );
        assert_eq!(
            check_policy(ActionKind::PermanentDelete, ActionPolicyMode::Ask),
            Ok(())
        );
        assert_eq!(
            check_policy(ActionKind::Send, ActionPolicyMode::Allow),
            Ok(())
        );
        let table = ActionPolicy::table(&[
            (ActionKind::Send, ActionPolicyMode::Allow),
            (ActionKind::Draft, ActionPolicyMode::Ask),
            // Never honoured, even if it reached the store.
            (ActionKind::Trash, ActionPolicyMode::Allow),
        ]);
        assert_eq!(table.len(), ActionKind::ALL.len());
        let send = table.iter().find(|p| p.kind == ActionKind::Send).unwrap();
        assert!(!send.asks() && send.customized && send.can_allow);
        let draft = table.iter().find(|p| p.kind == ActionKind::Draft).unwrap();
        assert!(draft.asks() && draft.customized);
        let trash = table.iter().find(|p| p.kind == ActionKind::Trash).unwrap();
        assert!(trash.asks() && !trash.customized && !trash.can_allow);
        let organize = table
            .iter()
            .find(|p| p.kind == ActionKind::Organize)
            .unwrap();
        assert!(!organize.asks() && !organize.customized);
        for mode in [ActionPolicyMode::Ask, ActionPolicyMode::Allow] {
            assert_eq!(ActionPolicyMode::parse(mode.as_str()), Some(mode));
        }
        assert!(crate::localized::is_message_id(
            ActionPolicyIssue::Destructive.message_id()
        ));
    }

    #[test]
    fn actors_round_trip() {
        for actor in [
            ActionActor::mcp_client("c1"),
            ActionActor::plugin("follow-up"),
        ] {
            assert_eq!(
                ActionActor::from_parts(actor.kind_str(), actor.id_str()),
                Some(actor.clone())
            );
            let value = serde_json::to_value(&actor).unwrap();
            assert_eq!(value["kind"], actor.kind_str());
            assert_eq!(serde_json::from_value::<ActionActor>(value).unwrap(), actor);
        }
        assert_eq!(
            serde_json::to_value(ActionActor::plugin("x")).unwrap(),
            json!({"kind": "plugin", "plugin_id": "x"})
        );
        assert_eq!(ActionActor::from_parts("robot", "x"), None);
    }

    #[test]
    fn states_and_sources() {
        for state in ConfirmationState::ALL {
            assert_eq!(ConfirmationState::parse(state.as_str()), Some(state));
            assert!(crate::localized::is_message_id(&state.message_id()));
        }
        assert!(!ConfirmationState::Pending.is_final());
        assert!(ConfirmationState::Expired.is_final());
        for source in [
            ConfirmationSource::Nail,
            ConfirmationSource::Elicitation,
            ConfirmationSource::System,
        ] {
            assert_eq!(ConfirmationSource::parse(source.as_str()), Some(source));
        }
        // Elicitation never approves destructive actions; the system never
        // approves anything.
        assert!(ConfirmationSource::Nail.may_approve(ActionKind::PermanentDelete));
        assert!(!ConfirmationSource::Elicitation.may_approve(ActionKind::PermanentDelete));
        assert!(!ConfirmationSource::Elicitation.may_approve(ActionKind::Trash));
        assert!(ConfirmationSource::Elicitation.may_approve(ActionKind::Send));
        assert!(!ConfirmationSource::System.may_approve(ActionKind::Draft));
        assert_eq!(ConfirmationFilter::default(), ConfirmationFilter::Pending);
    }

    #[test]
    fn new_confirmations_are_bounded() {
        let target = |i: usize| ActionTarget {
            kind: ActionTargetKind::Message,
            id: format!("m{i}"),
            label: "x".repeat(MAX_CONFIRMATION_TEXT + 10),
        };
        let mut new = NewConfirmation {
            requested_by: ActionAttribution {
                actor: ActionActor::mcp_client("c1"),
                name: "Claude".into(),
                on_behalf_of: None,
            },
            kind: ActionKind::Trash,
            operation: "delete_messages".into(),
            summary: LocalizedMessage::new("confirmation-summary-trash").arg("count", 60),
            targets: (0..60).map(target).collect(),
            changes: vec![
                ActionChange {
                    field: LocalizedMessage::new("action-field-folder"),
                    before: Some("Inbox".into()),
                    after: Some("Trash".into()),
                };
                60
            ],
            account_ids: vec!["work".into()],
            payload: json!({"message_ids": ["m0"]}),
            idempotency_key: None,
            timeout_secs: 999_999,
        };
        assert_eq!(new.normalize(), 10);
        assert_eq!(new.targets.len(), MAX_CONFIRMATION_TARGETS);
        assert_eq!(new.changes.len(), MAX_CONFIRMATION_CHANGES);
        assert_eq!(new.targets[0].label.chars().count(), MAX_CONFIRMATION_TEXT);
        assert!(new.targets[0].label.ends_with('…'));
        assert_eq!(new.timeout_secs, MAX_CONFIRMATION_TIMEOUT_SECS);
        assert_eq!(cut_text("short"), "short");
        for id in ACTION_FIELD_IDS.iter().chain(CONFIRMATION_SUMMARY_IDS) {
            assert!(crate::localized::is_message_id(id), "{id}");
        }
    }

    #[test]
    fn confirmations_serialize_snake_case() {
        let c = Confirmation {
            id: "1".into(),
            requested_by: ActionAttribution {
                actor: ActionActor::plugin("crm"),
                name: "CRM".into(),
                on_behalf_of: Some(McpClientRef {
                    client_id: "c1".into(),
                    name: "Claude".into(),
                }),
            },
            kind: ActionKind::Send,
            operation: "nail.mail.send".into(),
            summary: LocalizedMessage::new("confirmation-summary-send").arg("count", 1),
            targets: vec![],
            more_targets: 0,
            changes: vec![],
            account_ids: vec!["work".into()],
            created_at: "2026-10-02T10:00:00Z".into(),
            expires_at: "2026-10-02T10:05:00Z".into(),
            state: ConfirmationState::Pending,
            decided_at: None,
            decided_via: None,
            can_always_allow: true,
            error: None,
        };
        let value = serde_json::to_value(&c).unwrap();
        assert_eq!(value["requested_by"]["actor"]["kind"], "plugin");
        assert_eq!(value["requested_by"]["on_behalf_of"]["name"], "Claude");
        assert_eq!(value["state"], "pending");
        assert!(value.get("error").is_none());
        assert_eq!(serde_json::from_value::<Confirmation>(value).unwrap(), c);
    }
}
