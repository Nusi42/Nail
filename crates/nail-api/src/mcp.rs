//! MCP clients, grants and the audit log (design.md, "MCP server";
//! ADR 0022).
//!
//! Every agent connecting to Nail's MCP server is a registered client with
//! a bearer token. The core stores only a hash of the token; the token
//! itself is shown once, in [`crate::Response::McpClientCreated`]. A new
//! client has **no access** until the user grants scopes ([`McpGrant`]).
//! Every call is written to the audit log ([`McpAuditEntry`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, FolderId, McpClientId};

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
        assert_eq!(
            serde_json::to_value(McpScope::PermanentDelete).unwrap(),
            json!("permanent-delete")
        );
        let unique: std::collections::BTreeSet<_> = McpScope::ALL.iter().collect();
        assert_eq!(unique.len(), McpScope::ALL.len());
    }
}
