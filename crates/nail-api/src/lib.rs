//! The Nail API contract.
//!
//! Everything a client (the React UI over Tauri or WebSocket, and later MCP
//! and plug-ins) exchanges with the core is defined here, once, in Rust:
//!
//! * [`ids`] and [`domain`]: accounts, folders, message summaries and bodies,
//!   thread summaries.
//! * [`accounts`]: account configuration (`accounts.kdl`), identities,
//!   virtual accounts, account setup and status.
//! * [`ops`]: message writes and operation-queue status.
//! * [`search`]: search queries, hits and query errors.
//! * [`compose`]: drafts (plain text, HTML, Markdown, attachments,
//!   reply / forward context) and the outbox.
//! * [`rules`]: filter rules (triggers, nested conditions, actions, runs).
//! * [`mcp`]: MCP clients, grants and the audit log.
//! * [`context`]: the UI context reported to the core (for MCP).
//! * [`customize`]: configuration errors, layout write-back, the merge
//!   dialog and backups.
//! * [`security`]: end-to-end encryption (message security status, keys
//!   and certificates, recipient key status, sign / encrypt options),
//!   sender authentication, reader warnings, encryption at rest and the
//!   security settings (M2a, ADR 0029).
//! * [`remote`]: the remote UI — settings and status, users and
//!   credentials, paired devices, sessions, pairing codes, grants, the
//!   audit log and the browser login protocol (M2b, ADR 0036).
//! * [`remote_content`]: remote content in mail — allow-lists per
//!   message / sender / domain and the proxy URL form (M2b, ADR 0036).
//! * M2c (ADR 0043): [`localized`] (user-facing messages built by the core
//!   as Fluent id + arguments; every diagnostic in the contract has a
//!   `localized` sibling), [`sieve`] (ManageSieve settings, rule placement
//!   server / client / not expressible, vacation), [`triage`] (junk
//!   classifier, snooze, notification settings and requests), [`sync`]
//!   (per-folder sync policy, on-demand bodies), [`menus`] (context menus
//!   resolved from the layout).
//! * M2d (ADR 0050): [`plugins`] — the plug-in preview: `manifest.json`,
//!   permissions and grants, the declarative UI component set, dialogs,
//!   notifications, the read-only mail view, installed plug-ins, logs and
//!   settings. Its plug-in-facing types are also exported for the SDK
//!   (`@nail/plugin-api`) by [`export_plugin_typescript`].
//! * [`config`]: the resolved theme and layout models.
//! * [`protocol`]: [`Request`], [`Response`], [`Event`] and the wire envelopes.
//! * [`error`]: [`ApiError`] and [`ErrorCode`].
//! * [`engine`]: Rust-only traits between the core and the protocol /
//!   platform crates ([`engine::SecretStore`], [`engine::OutgoingTransport`],
//!   [`engine::AccessTokenProvider`]); not part of the wire protocol.
//!
//! TypeScript definitions are generated from these types with `ts-rs` into
//! `ui/src/api/generated/` by [`export_typescript`]. The generated files are
//! checked in; the `bindings` integration test fails when they are stale.
//! Regenerate with `npm run gen:api` (or
//! `NAIL_UPDATE_BINDINGS=1 cargo test -p nail-api --test bindings`).

#![warn(missing_docs)]

pub mod accounts;
pub mod compose;
pub mod config;
pub mod context;
pub mod customize;
pub mod domain;
pub mod engine;
pub mod error;
pub mod ids;
pub mod launch;
pub mod localized;
pub mod mcp;
pub mod menus;
pub mod ops;
pub mod plugins;
pub mod protocol;
pub mod remote;
pub mod remote_content;
pub mod rules;
pub mod search;
pub mod security;
pub mod sieve;
pub mod sync;
pub mod triage;

use std::path::{Path, PathBuf};

pub use accounts::{
    AccountConfig, AccountState, AccountStatus, AccountsConfig, AuthConfig, Identity,
    IncomingServer, OutgoingServer, SecretRef, ServerEndpoint, TlsMode, VirtualAccountConfig,
};
pub use compose::{
    AutomaticMail, Draft, DraftAttachment, DraftContext, DraftContextMode, LOOP_HEADER,
    OutboxEntry, OutboxState, SignatureChoice,
};
pub use config::SourceLocation;
pub use config::layout::{
    ActionDef, Breakpoint, KeyBinding, LayoutNode, MenuItem, Orientation, ResolvedLayout,
    ToolbarItem,
};
pub use config::theme::{
    AncestorSelector, HtmlDarkMode, Icon, MessageDisplay, PartCondition, PartRule, RawCss,
    ResolvedTheme, ThemeMode, TokenSet,
};
pub use context::UiContext;
pub use customize::{ConfigFile, ConfigIssue, LayoutNodeRef, LayoutPatch};
pub use domain::{
    Account, AccountKind, Address, AttachmentInfo, Flags, Folder, FolderRole, MessageBody,
    MessageHeaders, MessageLocation, MessageSummary, ThreadSummary,
};
pub use error::{ApiError, ErrorCode};
pub use ids::{
    AccountId, ConfigBackupId, DraftId, FolderId, IdentityId, KeyId, McpClientId, MessageId,
    OAuthFlowId, OpId, OutboxId, PairingId, PasskeyId, PluginDialogId, PluginId, RemoteDeviceId,
    RemoteSessionId, RemoteUserId, RuleId, RuleRunId, SavedSearchId, ThreadId,
};
pub use launch::{LaunchRequest, MailtoLink};
pub use localized::{LocalizedMessage, MessageArg};
pub use mcp::{McpClient, McpGrant, McpScope, McpSettings};
pub use menus::{ResolvedMenu, ResolvedMenuEntry};
pub use ops::{FlagChange, MoveTarget, OpState, OpStatus};
pub use plugins::{PluginGrant, PluginInfo, PluginManifest, PluginPermission, PluginUiNode};
pub use protocol::{
    Event, PROTOCOL_VERSION, Request, Response, RpcRequest, RpcResponse, ServerMessage,
};
pub use remote::{
    RemoteAuditEntry, RemoteDevice, RemoteGrant, RemoteSession, RemoteUiSettings, RemoteUiStatus,
    RemoteUser,
};
pub use remote_content::{RemoteContentAllow, RemoteContentInfo, RemoteContentScope};
pub use rules::{Condition, Rule, RuleAction, RuleRun, RuleTrigger};
pub use search::{SavedSearch, SearchError, SearchErrorKind, SearchHit, SearchQuery};
pub use security::{
    AuthResults, CryptoKey, CryptoTechnology, IdentityCrypto, MessageWarning, SecurityStatus,
    SendCrypto, TrustLevel, VaultStatus,
};
pub use sieve::{
    ManageSieveConfig, RulePlacement, RuleSieveStatus, SieveAccountStatus, SieveState,
    VacationSettings,
};
pub use sync::{BodyState, FolderSyncPolicy};
pub use triage::{
    JunkSettings, JunkStatus, JunkVerdict, MailNotification, NotificationSettings, SnoozedMessage,
};

/// Name of the barrel file written by [`export_typescript`].
pub const TS_INDEX_FILE: &str = "index.ts";

/// Writes the TypeScript definitions of the whole API into `out_dir`: one
/// file per type (plus `serde_json/JsonValue.ts`) and an `index.ts` that
/// re-exports every type. Returns the written paths relative to `out_dir`,
/// sorted, with `/` separators.
///
/// 64-bit integers are emitted as `number` (ids and sizes stay below 2^53).
pub fn export_typescript(out_dir: &Path) -> Result<Vec<String>, ts_rs::ExportError> {
    use ts_rs::TS;

    let cfg = ts_rs::Config::new()
        .with_out_dir(out_dir)
        .with_large_int("number");

    // Roots: everything reachable from these is exported as a dependency.
    // Types clients may name directly but that are not reachable are listed too.
    RpcRequest::export_all(&cfg)?;
    RpcResponse::export_all(&cfg)?;
    ServerMessage::export_all(&cfg)?;
    Request::export_all(&cfg)?;
    Response::export_all(&cfg)?;
    Event::export_all(&cfg)?;
    ApiError::export_all(&cfg)?;
    SourceLocation::export_all(&cfg)?;
    accounts::MailProtocol::export_all(&cfg)?;
    accounts::SecretKind::export_all(&cfg)?;
    McpScope::export_all(&cfg)?;
    ConfigFile::export_all(&cfg)?;
    // M2a types reachable only through optional fields, or not at all.
    security::AuthVerdict::export_all(&cfg)?;
    SecurityStatus::export_all(&cfg)?;
    AuthResults::export_all(&cfg)?;
    MessageWarning::export_all(&cfg)?;
    IdentityCrypto::export_all(&cfg)?;
    SendCrypto::export_all(&cfg)?;
    // M2b: the login protocol (HTTP, not the RPC) and optional fields.
    remote::AuthRequest::export_all(&cfg)?;
    remote::AuthResponse::export_all(&cfg)?;
    remote::RemoteUiSettingsIssue::export_all(&cfg)?;
    remote::RemoteAccessPreset::export_all(&cfg)?;
    RemoteContentInfo::export_all(&cfg)?;
    // M2c: validation issues (named by the UI), context menus (resolved in
    // the UI from the layout), body states and optional fields.
    sieve::ManageSieveIssue::export_all(&cfg)?;
    sieve::VacationIssue::export_all(&cfg)?;
    triage::JunkIssue::export_all(&cfg)?;
    triage::NotificationIssue::export_all(&cfg)?;
    mcp::McpSettingsIssue::export_all(&cfg)?;
    ResolvedMenu::export_all(&cfg)?;
    BodyState::export_all(&cfg)?;
    ManageSieveConfig::export_all(&cfg)?;
    JunkVerdict::export_all(&cfg)?;
    LocalizedMessage::export_all(&cfg)?;
    // M2d: plug-in types the UI names but that are reachable only through
    // other types (or not at all).
    plugins::PluginManifestIssueKind::export_all(&cfg)?;
    plugins::PluginUiIssue::export_all(&cfg)?;
    plugins::PluginDialogResult::export_all(&cfg)?;
    plugins::PluginCommandContext::export_all(&cfg)?;

    write_index(out_dir)
}

/// Writes the TypeScript definitions that plug-in code sees into
/// `out_dir` (`packages/plugin-api/src/generated/`): the manifest, the
/// component trees, dialogs and their results, notifications, command
/// contexts, the mail view, permissions and log levels, with everything
/// they reference, plus an `index.ts`. Same format as
/// [`export_typescript`].
pub fn export_plugin_typescript(out_dir: &Path) -> Result<Vec<String>, ts_rs::ExportError> {
    use ts_rs::TS;

    let cfg = ts_rs::Config::new()
        .with_out_dir(out_dir)
        .with_large_int("number");
    PluginManifest::export_all(&cfg)?;
    plugins::PluginManifestIssue::export_all(&cfg)?;
    PluginUiNode::export_all(&cfg)?;
    plugins::PluginDialogSpec::export_all(&cfg)?;
    plugins::PluginDialogResult::export_all(&cfg)?;
    plugins::PluginNotificationSpec::export_all(&cfg)?;
    plugins::PluginCommandContext::export_all(&cfg)?;
    plugins::PluginMessage::export_all(&cfg)?;
    plugins::PluginMessageBody::export_all(&cfg)?;
    PluginPermission::export_all(&cfg)?;
    plugins::PluginLogLevel::export_all(&cfg)?;
    write_index(out_dir)
}

/// Writes `index.ts` re-exporting every generated file below `out_dir`;
/// returns all written paths (relative, sorted, `/` separators).
fn write_index(out_dir: &Path) -> Result<Vec<String>, ts_rs::ExportError> {
    let mut files = Vec::new();
    collect_ts_files(out_dir, out_dir, &mut files)?;
    files.retain(|f| f != TS_INDEX_FILE);
    files.sort();

    let mut index = String::from(
        "// This file was generated by nail-api (export_typescript). Do not edit this file manually.\n\n",
    );
    for file in &files {
        let module = file.trim_end_matches(".ts");
        index.push_str(&format!("export type * from \"./{module}\";\n"));
    }
    std::fs::write(out_dir.join(TS_INDEX_FILE), index)?;

    files.push(TS_INDEX_FILE.to_owned());
    files.sort();
    Ok(files)
}

fn collect_ts_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path: PathBuf = entry?.path();
        if path.is_dir() {
            collect_ts_files(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "ts") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>();
            out.push(rel.join("/"));
        }
    }
    Ok(())
}
