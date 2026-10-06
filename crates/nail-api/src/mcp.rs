//! MCP clients, grants and the audit log (design.md, "MCP server";
//! ADR 0022).
//!
//! Every agent connecting to Nail's MCP server is a registered client with
//! a bearer token. The core stores only a hash of the token; the token
//! itself is shown once, in [`crate::Response::McpClientCreated`]. A new
//! client has **no access** until the user grants scopes ([`McpGrant`]).
//! Every call is written to the audit log ([`McpAuditEntry`]).
//!
//! M4a (ADR 0079): the write tools and prompts ([`tools`], [`prompts`]),
//! per-client rate limits ([`McpRateLimits`]); confirmations and "always
//! allow" policies are shared with plug-ins ([`crate::agents`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, FolderId, McpClientId};
use crate::localized::LocalizedMessage;

pub mod prompts;
pub mod tools;

/// What a client may do. Reading message content needs `read`; raw
/// source, attachment bytes, encrypted content and the UI context each
/// need their own opt-in scope on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum McpScope {
    /// Accounts, folders, messages, threads, search, rules (text form).
    Read,
    /// Original RFC 5322 source (`nail://messages/{id}/raw`).
    RawSource,
    /// Attachment bytes.
    Attachments,
    /// Decrypted content of encrypted messages.
    Encrypted,
    /// What the user has on screen (`nail://ui/context`, `get_ui_context`).
    Context,
    /// Flag, tag, move, archive, delete to Trash.
    Organize,
    /// Permanent delete.
    PermanentDelete,
    /// Create and edit drafts (and, with `context`, the open draft).
    Draft,
    /// Send drafts (always confirmed by the user).
    Send,
    /// Calendar writes.
    Calendar,
    /// Contact and task writes.
    Pim,
    /// Create and test rules.
    Rules,
}

impl McpScope {
    /// All scopes, in display order.
    pub const ALL: [McpScope; 12] = [
        Self::Read,
        Self::RawSource,
        Self::Attachments,
        Self::Encrypted,
        Self::Context,
        Self::Organize,
        Self::PermanentDelete,
        Self::Draft,
        Self::Send,
        Self::Calendar,
        Self::Pim,
        Self::Rules,
    ];
}

/// A client's permissions. The default grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpGrant {
    /// Granted scopes.
    pub scopes: Vec<McpScope>,
    /// Access to every account, including ones added later.
    pub all_accounts: bool,
    /// Accounts (real or virtual) the client may see when `all_accounts` is
    /// false.
    pub accounts: Vec<AccountId>,
    /// Folders hidden from the client even in granted accounts.
    pub excluded_folders: Vec<FolderId>,
    /// Skip the confirmation prompt for draft writes (never relaxes
    /// sending or permanent delete).
    ///
    /// Superseded in M4a by the client's action policies
    /// ([`crate::agents::ActionPolicy`]; drafts run without asking by
    /// default); kept for the wire format and ignored (ADR 0079).
    pub skip_draft_confirmation: bool,
}

impl McpGrant {
    /// Whether nothing is granted (the state of a new client).
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty() || (!self.all_accounts && self.accounts.is_empty())
    }

    /// Whether `scope` is granted.
    pub fn has(&self, scope: McpScope) -> bool {
        self.scopes.contains(&scope)
    }

    /// Whether the grant reaches a PIM source (M3a, ADR 0058) that belongs
    /// to `account` (`None` for the local source, subscriptions and
    /// standalone DAV servers): a granted account's sources, or every
    /// source with `all_accounts`. `self.accounts` must already be
    /// resolved to real accounts (virtual accounts to their members).
    pub fn allows_pim_source(&self, account: Option<&AccountId>) -> bool {
        self.all_accounts || account.is_some_and(|a| self.accounts.contains(a))
    }
}

/// A registered MCP client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpClient {
    /// The client.
    pub id: McpClientId,
    /// Name given by the user (also `nail mcp --client <name>`).
    pub name: String,
    /// Creation time (RFC 3339).
    pub created_at: String,
    /// Last call (RFC 3339), `null` = never used.
    pub last_used_at: Option<String>,
    /// The last characters of the token, to tell tokens apart.
    pub token_hint: String,
    /// Permissions.
    pub grant: McpGrant,
    /// When the client was revoked (RFC 3339); a revoked client keeps its
    /// audit entries but can no longer connect.
    pub revoked_at: Option<String>,
    /// Its rate limits (M4a).
    #[serde(default)]
    pub rate_limits: McpRateLimits,
}

/// How often a client may call (M4a; design.md: "Rate limits per
/// client"). `null` = no limit of that kind. Calls over a limit fail with
/// `rate_limited` (a tool error naming the limit and when to retry) and are
/// audited as denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpRateLimits {
    /// Calls of any kind (resources, tools, prompts) per minute.
    pub requests_per_minute: Option<u32>,
    /// Write tool calls per minute (each counts once, whether it runs, asks
    /// or is refused).
    pub writes_per_minute: Option<u32>,
    /// Messages sent per hour (`send_message`, mail to attendees).
    pub sends_per_hour: Option<u32>,
}

impl McpRateLimits {
    /// Largest value of any limit.
    pub const MAX: u32 = 10_000;
    /// Default calls per minute.
    pub const DEFAULT_REQUESTS_PER_MINUTE: u32 = 120;
    /// Default write calls per minute.
    pub const DEFAULT_WRITES_PER_MINUTE: u32 = 20;
    /// Default messages per hour.
    pub const DEFAULT_SENDS_PER_HOUR: u32 = 10;

    /// No limits at all.
    pub fn unlimited() -> Self {
        Self {
            requests_per_minute: None,
            writes_per_minute: None,
            sends_per_hour: None,
        }
    }

    /// Problems with the limits (empty = valid).
    pub fn issues(&self) -> Vec<McpRateLimitIssue> {
        let values = [
            self.requests_per_minute,
            self.writes_per_minute,
            self.sends_per_hour,
        ];
        let mut out = Vec::new();
        if values.contains(&Some(0)) {
            out.push(McpRateLimitIssue::Zero);
        }
        if values.iter().flatten().any(|v| *v > Self::MAX) {
            out.push(McpRateLimitIssue::TooHigh);
        }
        out
    }
}

impl Default for McpRateLimits {
    fn default() -> Self {
        Self {
            requests_per_minute: Some(Self::DEFAULT_REQUESTS_PER_MINUTE),
            writes_per_minute: Some(Self::DEFAULT_WRITES_PER_MINUTE),
            sends_per_hour: Some(Self::DEFAULT_SENDS_PER_HOUR),
        }
    }
}

/// A problem with [`McpRateLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum McpRateLimitIssue {
    /// A limit of 0 (switch the client's scopes off instead).
    Zero,
    /// A limit above [`McpRateLimits::MAX`].
    TooHigh,
}

impl McpRateLimitIssue {
    /// All issues.
    pub const ALL: [McpRateLimitIssue; 2] = [Self::Zero, Self::TooHigh];

    /// Fluent message id (`mcp-rate-limit-issue-<issue>`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Zero => "mcp-rate-limit-issue-zero",
            Self::TooHigh => "mcp-rate-limit-issue-too-high",
        }
    }
}

/// Kind of an audited MCP call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum McpCallKind {
    /// `resources/read`.
    Resource,
    /// `resources/subscribe`.
    Subscribe,
    /// `tools/call`.
    Tool,
    /// `prompts/get`.
    Prompt,
    /// Connection attempt (logged when refused).
    Connect,
}

/// How an audited call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum McpOutcome {
    /// Served.
    Allowed,
    /// Refused: scope, account or folder not granted, or a bad token.
    Denied,
    /// The user declined the confirmation prompt.
    Declined,
    /// Failed with an error.
    Error,
}

/// One entry of the MCP audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpAuditEntry {
    /// Monotonic id (newest is highest); page with
    /// [`crate::Request::ListMcpAudit`]'s `before`.
    pub id: u64,
    /// The client (`null` for a connection with an unknown token).
    pub client_id: Option<McpClientId>,
    /// The client's name at the time of the call.
    pub client_name: String,
    /// Time of the call (RFC 3339).
    pub at: String,
    /// Kind of call.
    pub kind: McpCallKind,
    /// Tool, prompt or resource URI.
    pub name: String,
    /// Outcome.
    pub outcome: McpOutcome,
    /// Diagnostic detail (English), e.g. the missing scope; else `null`.
    pub detail: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// MCP server settings (M2c), kept in the store.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpSettings {
    /// A fixed loopback port, `null` = any free port (written to
    /// `mcp-endpoint.json`). `nail serve --mcp-port` overrides it for one
    /// run. Takes effect when the server (re)starts.
    pub port: Option<u16>,
}

impl McpSettings {
    /// Lowest port accepted for [`Self::port`] (no privileged ports).
    pub const MIN_PORT: u16 = 1024;

    /// Problems with the settings (empty = valid).
    pub fn issues(&self) -> Vec<McpSettingsIssue> {
        match self.port {
            Some(port) if port < Self::MIN_PORT => vec![McpSettingsIssue::Port],
            _ => Vec::new(),
        }
    }
}

/// A problem with [`McpSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum McpSettingsIssue {
    /// The port is below 1024.
    Port,
}

impl McpSettingsIssue {
    /// All issues.
    pub const ALL: [McpSettingsIssue; 1] = [Self::Port];

    /// Fluent message id, e.g. `mcp-settings-issue-port`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Port => "mcp-settings-issue-port",
        }
    }
}

/// Where the MCP server listens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpServerInfo {
    /// Whether the server is running.
    pub running: bool,
    /// Streamable HTTP endpoint (`http://127.0.0.1:<port>/mcp`), `null`
    /// when not running.
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_grants_are_empty() {
        let grant: McpGrant = serde_json::from_value(json!({})).unwrap();
        assert!(grant.is_empty());
        let read_only_nowhere = McpGrant {
            scopes: vec![McpScope::Read],
            ..McpGrant::default()
        };
        assert!(read_only_nowhere.is_empty());
        let granted = McpGrant {
            all_accounts: true,
            ..read_only_nowhere
        };
        assert!(!granted.is_empty() && granted.has(McpScope::Read));
        assert!(!granted.has(McpScope::RawSource));
        assert!(granted.allows_pim_source(None));
        let work = McpGrant {
            scopes: vec![McpScope::Read],
            accounts: vec!["work".into()],
            ..McpGrant::default()
        };
        assert!(work.allows_pim_source(Some(&"work".into())));
        assert!(!work.allows_pim_source(Some(&"home".into())));
        assert!(!work.allows_pim_source(None));
        assert_eq!(
            serde_json::to_value(McpScope::PermanentDelete).unwrap(),
            json!("permanent-delete")
        );
        let unique: std::collections::BTreeSet<_> = McpScope::ALL.iter().collect();
        assert_eq!(unique.len(), McpScope::ALL.len());
    }

    #[test]
    fn rate_limits() {
        let defaults: McpRateLimits = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, McpRateLimits::default());
        assert_eq!(defaults.writes_per_minute, Some(20));
        assert_eq!(defaults.issues().len(), 0);
        assert_eq!(McpRateLimits::unlimited().issues().len(), 0);
        let off: McpRateLimits =
            serde_json::from_value(json!({"requests_per_minute": null})).unwrap();
        assert_eq!(off.requests_per_minute, None);
        assert_eq!(off.sends_per_hour, Some(10));
        let bad = McpRateLimits {
            requests_per_minute: Some(0),
            writes_per_minute: Some(McpRateLimits::MAX + 1),
            sends_per_hour: None,
        };
        assert_eq!(
            bad.issues(),
            [McpRateLimitIssue::Zero, McpRateLimitIssue::TooHigh]
        );
        for issue in McpRateLimitIssue::ALL {
            assert!(crate::localized::is_message_id(issue.message_id()));
        }
        // Clients stored before M4a get the defaults.
        let client: McpClient = serde_json::from_value(json!({
            "id": "c1", "name": "x", "created_at": "2026-10-02T00:00:00Z",
            "last_used_at": null, "token_hint": "abcd", "grant": {}, "revoked_at": null
        }))
        .unwrap();
        assert_eq!(client.rate_limits, McpRateLimits::default());
    }

    #[test]
    fn mcp_settings_validate_the_port() {
        let s: McpSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s.port, None);
        assert_eq!(s.issues().len(), 0);
        assert_eq!(McpSettings { port: Some(4799) }.issues().len(), 0);
        assert_eq!(
            McpSettings { port: Some(80) }.issues(),
            [McpSettingsIssue::Port]
        );
        assert_eq!(
            McpSettingsIssue::Port.message_id(),
            "mcp-settings-issue-port"
        );
    }
}
