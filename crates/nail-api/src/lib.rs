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
//! * M3a (ADR 0058): [`pim`] — calendars, events (recurrence, overrides,
//!   attendees, alarms, time zones, conferencing links, attachments),
//!   contacts (vCard 3 / 4, groups, photos, merging), tasks (VTODO),
//!   sources (local, ICS subscriptions, CalDAV, CardDAV; configured in
//!   `accounts.kdl`), reminders, `.ics` / `.vcf` import and the read-only
//!   MCP payloads.
//! * M3b (ADR 0065): [`scheduling`] — iTIP / iMIP invitations (methods,
//!   attendee states, the invitation card on [`MessageBody::invitation`],
//!   answering, counter-proposals, organizer updates / cancellations,
//!   forwarding, refresh), scheduling settings and free/busy; [`gmail`] —
//!   Gmail accounts through the Gmail API (labels ↔ folders and tags,
//!   thread ids, categories, label management). [`engine`] gains the
//!   provider-neutral [`engine::MailApiEngine`], [`engine::AdaptivePoll`]
//!   and [`engine::FreeBusyProvider`].
//! * M3c (ADR 0072): [`exchange`] — Microsoft Exchange accounts (Graph for
//!   Exchange Online, EWS for on-premises; Windows integrated sign-in),
//!   shared mailboxes and delegate access, categories ↔ tags with colours,
//!   Focused Inbox, server-side inbox rules, automatic replies, rooms and
//!   room lists, Microsoft To Do lists, Exchange sync state; the engine
//!   seams in [`exchange::engine`] and the provider-neutral PIM item shapes
//!   in [`exchange::pim`].
//! * M4a (ADR 0079): [`agents`] — writes by MCP clients and plug-ins:
//!   action kinds, confirmations (pending actions with attribution,
//!   summary and before / after, approve / deny), "always allow" policies
//!   per client / plug-in for non-destructive kinds; [`mcp`] gains the write
//!   tools, prompts and per-client rate limits ([`mcp::tools`],
//!   [`mcp::prompts`], [`mcp::McpRateLimits`]); [`plugins`] gains the write
//!   and hook permissions, mail hooks ([`plugins::hooks`]), composer /
//!   attachment / link / MCP extensions ([`plugins::extensions`]) and
//!   background jobs ([`plugins::jobs`]).
//! * M4b (ADR 0087): [`desktop`] — Windows integration of the desktop app:
//!   installation facts (installer, scope, architecture, update policy),
//!   updater settings and state (channels, check, download, restart to
//!   install, policy-disabled), tray / startup settings, the `nail plugin
//!   dev` endpoint, default-app status, first run, and the
//!   [`desktop::DesktopHost`] trait the shell implements; [`mapi`] — Simple
//!   MAPI requests from other programs (recipients, subject, body, files,
//!   UI flag), their result codes and the DLL ↔ app pipe frames.
//! * M4c (ADR 0094): [`portability`] — import (mbox, EML folders, Maildir,
//!   Thunderbird profiles, ICS, vCard) and export jobs with progress and
//!   per-item reports; [`backup`] — encrypted backup archives, verification
//!   and dry-run restores; [`recovery`] — the vault recovery key;
//!   [`smartcard`] — PKCS#11 tokens and PIN prompts; [`palette`] — recent
//!   command-palette actions; [`print`] — print jobs and PDF files;
//!   [`diagnostics`] — the user-made diagnostics bundle; send-as /
//!   on-behalf identities ([`accounts::SendAs`]) and upload progress
//!   ([`engine::OutgoingTransport::send_with_progress`]).
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

#[macro_use]
mod labels;

pub mod accounts;
pub mod agents;
pub mod backup;
pub mod compose;
pub mod config;
pub mod context;
pub mod customize;
pub mod desktop;
pub mod diagnostics;
pub mod domain;
pub mod engine;
pub mod error;
pub mod exchange;
pub mod gmail;
pub mod ids;
pub mod launch;
pub mod localized;
pub mod mapi;
pub mod mcp;
pub mod menus;
pub mod ops;
pub mod palette;
pub mod pim;
pub mod plugins;
pub mod portability;
pub mod print;
pub mod protocol;
pub mod recovery;
pub mod remote;
pub mod remote_content;
pub mod rules;
pub mod scheduling;
pub mod search;
pub mod security;
pub mod sieve;
pub mod smartcard;
pub mod sync;
pub mod triage;

use std::path::{Path, PathBuf};

pub use accounts::{
    AccountConfig, AccountState, AccountStatus, AccountsConfig, AuthConfig, Identity,
    IncomingServer, OutgoingServer, SecretRef, ServerEndpoint, TlsMode, VirtualAccountConfig,
};
pub use agents::{
    ActionActor, ActionAttribution, ActionKind, ActionPolicy, ActionPolicyMode, Confirmation,
    ConfirmationState,
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
pub use desktop::{
    DefaultAppsStatus, DesktopHost, DesktopSettings, FirstRunState, InstallationInfo,
    UpdateSettings, UpdateState, UpdateStatus,
};
pub use domain::{
    Account, AccountKind, Address, AttachmentInfo, Flags, Folder, FolderRole, MessageBody,
    MessageHeaders, MessageLocation, MessageSummary, ThreadSummary,
};
pub use error::{ApiError, ErrorCode};
pub use exchange::{
    AutomaticReplies, AutomaticRepliesSettings, ExchangeCategory, ExchangeEngine, ExchangeMailbox,
    ExchangeMessageInfo, ExchangeOptions, ExchangeStatus, InferenceClassification, Room, RoomList,
};
pub use gmail::{GmailCategory, GmailLabel, GmailLabelSettings, GmailMessageInfo, GmailOptions};
pub use ids::{
    AccountId, AddressBookId, CalendarId, ConfigBackupId, ConfirmationId, ContactGroupId,
    ContactId, DraftId, EventId, ExchangeCategoryId, FolderId, GmailLabelId, IdentityId, KeyId,
    McpClientId, MessageId, OAuthFlowId, OpId, OutboxId, PairingId, PasskeyId, PimSourceId,
    PluginDialogId, PluginId, ReminderId, RemoteDeviceId, RemoteSessionId, RemoteUserId, RuleId,
    RuleRunId, SavedSearchId, SendVetoId, TaskId, ThreadId,
};
pub use ids::{JobId, PinPromptId, PrintJobId};
pub use launch::{LaunchRequest, MailtoLink};
pub use localized::{LocalizedMessage, MessageArg};
pub use mapi::{MapiMessage, MapiResult};
pub use mcp::{McpClient, McpGrant, McpRateLimits, McpScope, McpSettings};
pub use menus::{ResolvedMenu, ResolvedMenuEntry};
pub use ops::{FlagChange, MoveTarget, OpState, OpStatus};
pub use pim::{
    Calendar, CalendarEvent, Contact, PimSource, PimSourceConfig, PimTime, Reminder, Task,
};
pub use plugins::{PluginGrant, PluginInfo, PluginManifest, PluginPermission, PluginUiNode};
pub use portability::{ImportSource, JobReport, JobStatus};
pub use print::{PrintJob, PrintTarget};
pub use protocol::{
    Event, PROTOCOL_VERSION, Request, Response, RpcRequest, RpcResponse, ServerMessage,
};
pub use remote::{
    RemoteAuditEntry, RemoteDevice, RemoteGrant, RemoteSession, RemoteUiSettings, RemoteUiStatus,
    RemoteUser,
};
pub use remote_content::{RemoteContentAllow, RemoteContentInfo, RemoteContentScope};
pub use rules::{Condition, Rule, RuleAction, RuleRun, RuleTrigger};
pub use scheduling::{
    FreeBusyResult, Invitation, InvitationResponse, InvitationTarget, ItipMethod, SchedulingResult,
    SchedulingSettings,
};
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
    // M3a: PIM types the UI names but that are reachable only through
    // optional fields, or not at all (validation issues, MCP payloads,
    // contact entry kinds).
    pim::PimIssue::export_all(&cfg)?;
    pim::source::PimSourceIssue::export_all(&cfg)?;
    pim::source::PimSourceKind::export_all(&cfg)?;
    pim::source::PimSourceState::export_all(&cfg)?;
    pim::PimLink::export_all(&cfg)?;
    pim::contact::ContactPhoto::export_all(&cfg)?;
    pim::import::PimFormat::export_all(&cfg)?;
    pim::mcp::McpListEventsArgs::export_all(&cfg)?;
    pim::mcp::McpFindFreeTimeArgs::export_all(&cfg)?;
    pim::mcp::McpSearchContactsArgs::export_all(&cfg)?;
    pim::mcp::McpListTasksArgs::export_all(&cfg)?;
    pim::mcp::McpCalendarsResource::export_all(&cfg)?;
    pim::mcp::McpEventsResource::export_all(&cfg)?;
    pim::mcp::McpFreeTimeResult::export_all(&cfg)?;
    pim::mcp::McpContactsResource::export_all(&cfg)?;
    pim::mcp::McpContactResource::export_all(&cfg)?;
    pim::mcp::McpContactSearchResult::export_all(&cfg)?;
    pim::mcp::McpTasksResource::export_all(&cfg)?;
    // M3b: scheduling and Gmail types the UI names but that are reachable
    // only through optional fields, or not at all (issues, actions, the
    // folder mapping, label kinds).
    scheduling::ItipIssue::export_all(&cfg)?;
    scheduling::FreeBusyIssue::export_all(&cfg)?;
    scheduling::InvitationAction::export_all(&cfg)?;
    scheduling::InvitationState::export_all(&cfg)?;
    scheduling::InvitationRole::export_all(&cfg)?;
    scheduling::BusyKind::export_all(&cfg)?;
    Invitation::export_all(&cfg)?;
    gmail::GmailLabelIssue::export_all(&cfg)?;
    gmail::GmailFolder::export_all(&cfg)?;
    gmail::GmailLabelKind::export_all(&cfg)?;
    GmailCategory::export_all(&cfg)?;
    GmailMessageInfo::export_all(&cfg)?;
    GmailOptions::export_all(&cfg)?;
    // M3c: Exchange types the UI names but that are reachable only through
    // optional fields, or not at all (issues, warnings, options, colours,
    // roles, the auth package).
    exchange::ExchangeConfigIssue::export_all(&cfg)?;
    exchange::ExchangeWarning::export_all(&cfg)?;
    exchange::ExchangeMailboxIssue::export_all(&cfg)?;
    exchange::ExchangeCategoryIssue::export_all(&cfg)?;
    exchange::oof::AutomaticRepliesIssue::export_all(&cfg)?;
    exchange::ExchangeWellKnownFolder::export_all(&cfg)?;
    exchange::ExchangeCollectionInfo::export_all(&cfg)?;
    exchange::CategoryColor::export_all(&cfg)?;
    ExchangeMessageInfo::export_all(&cfg)?;
    ExchangeOptions::export_all(&cfg)?;
    accounts::SspiPackage::export_all(&cfg)?;
    // M4a: agent / plug-in types the UI names but that are reachable only
    // through optional fields, or not at all (issues, kinds, hook and
    // extension shapes the plug-in manager shows).
    agents::ActionPolicyIssue::export_all(&cfg)?;
    agents::ConfirmationDecision::export_all(&cfg)?;
    agents::ActionTargetKind::export_all(&cfg)?;
    mcp::McpRateLimitIssue::export_all(&cfg)?;
    plugins::PluginHookKind::export_all(&cfg)?;
    plugins::PluginHookOutcome::export_all(&cfg)?;
    plugins::PluginEventName::export_all(&cfg)?;
    plugins::PluginJobOutcome::export_all(&cfg)?;
    plugins::jobs::CronIssue::export_all(&cfg)?;
    plugins::ComposerEdit::export_all(&cfg)?;
    plugins::PluginPreview::export_all(&cfg)?;
    // M4b: desktop types the UI names but that are reachable only through
    // optional fields or tagged states, or not at all.
    desktop::UpdateFailure::export_all(&cfg)?;
    desktop::UpdateDisabledReason::export_all(&cfg)?;
    desktop::UiEndpointFile::export_all(&cfg)?;
    MapiResult::export_all(&cfg)?;
    // M4c: portability, backup, recovery, smartcard, palette, print and
    // diagnostics types the UI names but that are reachable only through
    // optional fields, or not at all (issues, kinds, availability).
    portability::PortabilityIssueCode::export_all(&cfg)?;
    portability::IssueSeverity::export_all(&cfg)?;
    portability::JobPhase::export_all(&cfg)?;
    backup::BackupItem::export_all(&cfg)?;
    backup::BackupIssue::export_all(&cfg)?;
    backup::RestoreEffect::export_all(&cfg)?;
    recovery::RecoveryKeyIssue::export_all(&cfg)?;
    smartcard::SmartcardSettingsIssue::export_all(&cfg)?;
    smartcard::SmartcardError::export_all(&cfg)?;
    smartcard::PinPurpose::export_all(&cfg)?;
    palette::ActionAvailability::export_all(&cfg)?;
    print::PrintTargetIssue::export_all(&cfg)?;
    print::AgendaLayout::export_all(&cfg)?;
    diagnostics::DiagnosticsIssue::export_all(&cfg)?;
    diagnostics::Redaction::export_all(&cfg)?;
    accounts::SendAs::export_all(&cfg)?;

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
    plugins::PluginCalendarEvent::export_all(&cfg)?;
    PluginPermission::export_all(&cfg)?;
    plugins::PluginLogLevel::export_all(&cfg)?;
    // M4a: hooks, extensions, jobs, composer edits.
    plugins::PluginArrivalResult::export_all(&cfg)?;
    plugins::PluginBeforeSendResult::export_all(&cfg)?;
    plugins::PluginOpenResult::export_all(&cfg)?;
    plugins::PluginOutgoingMessage::export_all(&cfg)?;
    plugins::PluginActionContext::export_all(&cfg)?;
    plugins::PluginMarkdownBlockInput::export_all(&cfg)?;
    plugins::PluginMarkdownBlockResult::export_all(&cfg)?;
    plugins::PluginPreview::export_all(&cfg)?;
    plugins::PluginJobTrigger::export_all(&cfg)?;
    plugins::ComposerEdit::export_all(&cfg)?;
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
