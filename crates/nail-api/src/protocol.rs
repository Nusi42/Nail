//! The transport-agnostic RPC protocol.
//!
//! A client sends [`Request`]s and receives exactly one [`Response`] per
//! request; the core additionally pushes [`Event`]s. The same JSON shapes are
//! used over every transport:
//!
//! * **Tauri** (desktop): the `nail_call` command takes a [`Request`] and
//!   returns a [`Response`]; events arrive on the `nail://event` channel as
//!   [`Event`]s.
//! * **WebSocket** (`nail serve`, `/api/ws`): the client sends [`RpcRequest`]
//!   frames; the server sends [`ServerMessage`] frames, which are either a
//!   response correlated by `id` or an event.
//!
//! See `docs/adr/0002-rpc-protocol.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::accounts::BodyPolicy;
use crate::accounts::{
    AccountConfig, AccountStatus, AccountTestResult, AccountsConfig, DiscoveredAccount, Identity,
    SecretInput, SecretValue, VirtualAccountConfig,
};
use crate::agents::{
    ActionActor, ActionAttribution, ActionKind, ActionPolicy, ActionPolicyMode, Confirmation,
    ConfirmationDecision, ConfirmationFilter,
};
use crate::compose::{
    ComposerMode, Draft, DraftAttachment, DraftContext, OutboxEntry, OutboxState,
};
use crate::config::layout::ResolvedLayout;
use crate::config::theme::ResolvedTheme;
use crate::context::UiContext;
use crate::customize::{
    ConfigBackup, ConfigFile, ConfigIssue, ConfigMergePreview, LayoutPatch, MergeChoice, MergeTake,
};
use crate::desktop::{
    DefaultAppsStatus, DesktopSettings, FirstRunState, InstallationInfo, UpdateSettings,
    UpdateStatus,
};
use crate::domain::{Account, Address, Folder, MessageBody, MessageSummary, ThreadSummary};
use crate::error::{ApiError, ErrorCode};
use crate::exchange::rules::ExchangeRulesStatus;
use crate::exchange::{
    AutomaticReplies, AutomaticRepliesSettings, ExchangeArea, ExchangeCategory,
    ExchangeCategorySettings, ExchangeMailbox, ExchangeMailboxSettings, ExchangeStatus,
    InferenceClassification, RoomAvailability, RoomList,
};
use crate::gmail::{GmailLabel, GmailLabelSettings};
use crate::ids::{
    AccountId, AddressBookId, CalendarId, ConfigBackupId, ConfirmationId, ContactGroupId,
    ContactId, DraftId, EventId, ExchangeCategoryId, FolderId, GmailLabelId, IdentityId, KeyId,
    McpClientId, MessageId, OAuthFlowId, OpId, OutboxId, PairingId, PasskeyId, PimSourceId,
    PluginDialogId, PluginId, ReminderId, RemoteDeviceId, RemoteSessionId, RemoteUserId, RuleId,
    RuleRunId, SavedSearchId, SendVetoId, TaskId, ThreadId,
};
use crate::launch::LaunchRequest;
use crate::localized::LocalizedMessage;
use crate::mcp::{McpAuditEntry, McpClient, McpGrant, McpRateLimits, McpServerInfo, McpSettings};
use crate::ops::{FlagChange, MoveTarget, OpStatus};
use crate::pim::calendar::FreeSlot;
use crate::pim::contact::DuplicateGroup;
use crate::pim::import::{PimImportPreview, PimImportResult, PimImportSource};
use crate::pim::source::{PimOpStatus, PimSourceTestResult};
use crate::pim::{
    AddressBook, AddressBookSettings, Calendar, CalendarComponent, CalendarEvent, CalendarSettings,
    ConflictChoice, Contact, ContactFields, ContactGroup, ContactMatch, ContactSummary, EditScope,
    EventFields, EventInstance, PimItemKind, PimItemRef, PimSearchHit, PimSource, PimSourceConfig,
    PimTime, Recurrence, Reminder, Task, TaskFields,
};
use crate::plugins::runtime::PluginCommandSurface;
use crate::plugins::{
    ComposerEdit, PluginActionTarget, PluginActivity, PluginBadge, PluginBanner, PluginDialog,
    PluginGrant, PluginInfo, PluginInstallPreview, PluginLogEntry, PluginNotification,
    PluginPackageSource, PluginPreview, PluginSendVeto, PluginSettingDef, PluginStopReason,
    PluginUiNode, PluginUiTarget,
};
use crate::remote::{
    PairingCode, RemoteAlert, RemoteAuditEntry, RemoteDevice, RemoteGrant, RemoteSession,
    RemoteSessionFilter, RemoteUiSettings, RemoteUiStatus, RemoteUser, RemoteUserUpdate,
    SessionEndReason, TotpEnrollment,
};
use crate::remote_content::{RemoteContentAllow, RemoteContentScope};
use crate::rules::{Rule, RuleMatch, RuleRun};
use crate::scheduling::{
    CancelScope, FreeBusyResult, Invitation, InvitationResponse, InvitationTarget,
    SchedulingResult, SchedulingSettings,
};
use crate::search::{SavedSearch, SearchError, SearchHit, SearchQuery, ServerSearch};
use crate::security::{
    CryptoKey, KeyAlgorithm, KeyEncoding, KeyFilter, KeyImportResult, KeyLookupCandidate,
    KeyLookupFailure, KeyLookupSource, RecipientKeysReport, SecretProtection, SecuritySettings,
    SendCrypto, TrustDecision, VaultStatus, WindowsCertStore, WindowsCertificate,
};
use crate::sieve::{RuleSieveStatus, SieveAccountStatus, VacationSettings};
use crate::sync::FolderSyncPolicy;
use crate::triage::{
    JunkSettings, JunkStatus, MailNotification, NotificationSettings, SnoozedMessage,
};
// M4c (ADR 0094).
use crate::backup::{BackupContents, BackupManifest, RestorePlan};
use crate::desktop::PathPick;
use crate::diagnostics::{DiagnosticsBundle, DiagnosticsManifest, DiagnosticsOptions};
use crate::ids::{JobId, PinPromptId, PrintJobId};
use crate::palette::RecentActions;
use crate::portability::{
    ExportFormat, ExportOptions, ExportSource, FoundImportSource, ImportOptions, ImportPreview,
    ImportSource, JobProgress, JobReport, JobStatus,
};
use crate::print::{PdfFile, PrintJob, PrintOptions, PrintTarget};
use crate::recovery::RecoveryKeyStatus;
use crate::security::CryptoTechnology;
// M5a (ADR 0102).
use crate::appearance::{Appearance, ThemePreferences};
use crate::smartcard::{
    PinPrompt, PinPromptCloseReason, SmartcardKeyRef, SmartcardSettings, SmartcardToken,
};

/// Version of this protocol. Bumped on incompatible changes; returned by
/// [`Response::Pong`] so clients can detect a mismatched core.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from a client to the core.
///
/// Every request may also be answered with [`Response::Error`]. Requests
/// the running core does not implement yet are answered with the error code
/// `unsupported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Request {
    /// Liveness and version check. Answered with [`Response::Pong`].
    Ping,
    /// The resolved theme, layout and locale. Answered with [`Response::Config`].
    GetConfig,
    /// All accounts. Answered with [`Response::Accounts`].
    ListAccounts,
    /// All folders of an account, parents before children. Answered with
    /// [`Response::Folders`].
    ListFolders {
        /// The account.
        account_id: AccountId,
    },
    /// A page of a folder's messages, newest first. Answered with
    /// [`Response::Messages`].
    ListMessages {
        /// The folder.
        folder_id: FolderId,
        /// Index of the first message of the page (0 = newest).
        offset: u32,
        /// Maximum number of messages to return.
        limit: u32,
    },
    /// One parsed message. Answered with [`Response::Message`].
    GetMessage {
        /// The message.
        id: MessageId,
    },
    /// Start synchronizing one account, or all accounts when `account_id` is
    /// `null`. Answered with [`Response::SyncStarted`] immediately; progress
    /// arrives as [`Event::SyncProgress`] and [`Event::MailChanged`].
    SyncNow {
        /// The account, or `null` for all accounts.
        account_id: Option<AccountId>,
    },

    // --- Account setup and configuration (M1a) ---
    /// Look up server settings for an address (autoconfig, ISPDB, SRV,
    /// Autodiscover, guessing). Answered with [`Response::Discovered`]; an
    /// empty list means nothing was found and the user enters settings by
    /// hand.
    DiscoverAccount {
        /// The address to set up.
        email: String,
        /// Hosts outside the address's domain the user trusts with the
        /// login (Exchange Autodiscover; M3c,
        /// [`crate::exchange::ExchangeDiscoveryPrompt::ConfirmRedirect`]).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        trusted_hosts: Vec<String>,
        /// Let Exchange Autodiscover sign in as the signed-in Windows user
        /// (servers of the address's domain or trusted hosts only; M3c,
        /// [`crate::exchange::ExchangeDiscoveryPrompt::NeedsLogin`]).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        windows_sign_in: bool,
    },
    /// Connect and log in with a configuration without saving anything.
    /// Answered with [`Response::AccountTested`], which reports failures
    /// per server (a failed login is not a [`Response::Error`]).
    TestAccount {
        /// The configuration to test.
        config: Box<AccountConfig>,
        /// New secrets for it (not stored).
        secrets: Vec<SecretInput>,
    },
    /// Start an OAuth2 sign-in (authorization code + PKCE with a loopback
    /// redirect). Answered with [`Response::OAuthStarted`] at once; the
    /// client opens `authorize_url` unless the core already opened a
    /// browser. The outcome arrives as [`Event::OAuthFinished`]; the tokens
    /// stay in the core until a request references the flow in a
    /// [`crate::accounts::SecretValue::OAuth2`], or the flow expires after
    /// 10 minutes.
    StartOAuth {
        /// Provider id (`google`, `microsoft` or one from `accounts.kdl`).
        provider: String,
        /// Address to pre-fill on the provider's sign-in page.
        login_hint: Option<String>,
        /// Ask for the scopes of this protocol only (M3c: Microsoft issues
        /// tokens per resource, so an Exchange account signs in for
        /// `graph` or `ews`); `null` = the provider's mail scopes.
        #[serde(default)]
        protocol: Option<crate::accounts::MailProtocol>,
    },
    /// Abort a running OAuth2 sign-in. Answered with [`Response::Done`]; the
    /// flow finishes with `cancelled`.
    CancelOAuth {
        /// The flow.
        flow_id: OAuthFlowId,
    },
    /// The account configuration (`accounts.kdl`). Answered with
    /// [`Response::AccountsConfig`].
    GetAccountsConfig,
    /// Add an account: store the secrets in the keychain, write
    /// `accounts.kdl`, start syncing. Answered with
    /// [`Response::AccountSaved`]; `conflict` when the id is taken.
    AddAccount {
        /// The new account; an empty `id` lets the core derive one.
        config: Box<AccountConfig>,
        /// Secrets for its servers.
        secrets: Vec<SecretInput>,
    },
    /// Replace an account's configuration (matched by `config.id`).
    /// Answered with [`Response::AccountSaved`].
    UpdateAccount {
        /// The new configuration.
        config: Box<AccountConfig>,
        /// Changed secrets only; the others stay as they are.
        secrets: Vec<SecretInput>,
    },
    /// Remove an account: its entry in `accounts.kdl` (and its membership in
    /// virtual accounts), its keychain secrets and its local mail. Nothing
    /// on the server changes. Answered with [`Response::Done`].
    RemoveAccount {
        /// The account.
        account_id: AccountId,
    },
    /// Sending identities. Answered with [`Response::Identities`].
    ListIdentities {
        /// A real account (its identities), a virtual account (its members'
        /// identities) or `null` for all.
        account_id: Option<AccountId>,
    },
    /// The identity to send from, for a new message, a reply or a forward.
    /// Answered with [`Response::IdentityChosen`]. The first of: the
    /// virtual account's identity rules (the first rule matching any
    /// recipient), the identity the replied-to message was addressed to,
    /// the account's default identity (a virtual account's
    /// `default-identity`, else its first).
    ChooseIdentity {
        /// The active account (real or virtual), `null` for all.
        account_id: Option<AccountId>,
        /// Recipient addresses entered so far (To, Cc, Bcc).
        #[serde(default)]
        recipients: Vec<String>,
        /// The message replied to or forwarded, if any.
        #[serde(default)]
        reply_to: Option<MessageId>,
    },
    /// User-defined virtual accounts. Answered with
    /// [`Response::VirtualAccounts`].
    ListVirtualAccounts,
    /// Add a virtual account. Answered with
    /// [`Response::VirtualAccountSaved`]; `conflict` when the id is taken.
    AddVirtualAccount {
        /// The new virtual account; an empty `id` lets the core derive one.
        config: Box<VirtualAccountConfig>,
    },
    /// Replace a virtual account (matched by `config.id`). Answered with
    /// [`Response::VirtualAccountSaved`].
    UpdateVirtualAccount {
        /// The new configuration.
        config: Box<VirtualAccountConfig>,
    },
    /// Remove a virtual account (its members are not touched). Answered with
    /// [`Response::Done`].
    RemoveVirtualAccount {
        /// The virtual account.
        account_id: AccountId,
    },
    /// Current connection state of every real account. Answered with
    /// [`Response::AccountStatuses`]; changes arrive as
    /// [`Event::AccountStatus`].
    ListAccountStatuses,

    // --- Message writes (through the operation queue) ---
    /// Set or clear flags and tags. Answered with [`Response::Queued`].
    UpdateFlags {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// The change.
        change: FlagChange,
    },
    /// Move messages. Answered with [`Response::Queued`].
    MoveMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// Where to.
        target: MoveTarget,
    },
    /// Delete messages. Answered with [`Response::Queued`].
    DeleteMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// `false`: move to the account's Trash (messages already in Trash
        /// are deleted for good). `true`: delete for good at once.
        permanent: bool,
    },

    // --- Threads and search ---
    /// A page of a folder's threads, newest activity first. Answered with
    /// [`Response::Threads`].
    ListThreads {
        /// The folder (real or virtual).
        folder_id: FolderId,
        /// Index of the first thread of the page.
        offset: u32,
        /// Maximum number of threads to return.
        limit: u32,
    },
    /// All messages of a thread across folders (and, in virtual accounts,
    /// accounts). Answered with [`Response::Thread`].
    GetThread {
        /// The thread.
        thread_id: ThreadId,
    },
    /// Run a query. Answered with [`Response::SearchResults`], or with
    /// [`Response::InvalidQuery`] when the query does not parse.
    Search {
        /// The query.
        query: SearchQuery,
    },
    /// Parse a query without running it (live feedback in the search bar).
    /// Answered with [`Response::QueryChecked`].
    CheckQuery {
        /// The query text.
        text: String,
    },

    // --- Drafts and sending ---
    /// Create or update a draft (stored locally; the core also keeps a copy
    /// in the server's Drafts folder). Answered with
    /// [`Response::DraftSaved`].
    SaveDraft {
        /// The draft; `id: null` creates a new one.
        draft: Draft,
    },
    /// One draft. Answered with [`Response::Draft`].
    GetDraft {
        /// The draft.
        draft_id: DraftId,
    },
    /// The content of one attachment of a saved draft (e.g. the preview of
    /// an inline image when a draft is reopened). Only blobs the draft lists
    /// in `attachments` are served. Answered with
    /// [`Response::AttachmentData`].
    GetDraftAttachment {
        /// The draft.
        draft_id: DraftId,
        /// The attachment's blob hash ([`crate::DraftAttachment::blob`]).
        blob: String,
    },
    /// Saved drafts that are not in the outbox, most recently changed
    /// first (to continue one after a restart). Answered with
    /// [`Response::Drafts`].
    ListDrafts,
    /// Discard a draft. Answered with [`Response::Done`].
    DeleteDraft {
        /// The draft.
        draft_id: DraftId,
    },
    /// Put a saved draft into the outbox. Answered with
    /// [`Response::SendQueued`]; progress arrives as [`Event::SendStatus`].
    SendDraft {
        /// The draft (save it first).
        draft_id: DraftId,
        /// Undo-send delay in seconds; `null` = the configured default
        /// ([`AccountsConfig::undo_send_secs`]), `0` = send at once.
        delay_secs: Option<u32>,
        /// Send later: when to send (RFC 3339); overrides `delay_secs`.
        /// `null` = now plus the delay.
        send_at: Option<String>,
        /// Sign / encrypt (M2a); absent = the draft's `crypto`, else the
        /// identity's defaults. The message is signed / encrypted when it
        /// is sent, so the choice is checked here: `no_key` when a needed
        /// key is missing, `locked` when the signing key needs its
        /// passphrase. It never falls back to sending unprotected.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        crypto: Option<SendCrypto>,
        /// Post-M4a (ADR 0086): send only the draft at this revision (the
        /// one a confirmation showed; `nail_store::draft_revision`). The
        /// check and the outbox entry are one store transaction; a draft
        /// edited since answers `conflict` (`mcp-tool-error-draft-changed`)
        /// and nothing is queued. Absent = whatever the draft holds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        expected_revision: Option<String>,
    },
    /// Stop a scheduled message and turn it back into a draft. Answered with
    /// [`Response::SendCancelled`]; `conflict` when it is already being sent.
    CancelSend {
        /// The outbox entry.
        outbox_id: OutboxId,
    },
    /// Messages in the outbox (scheduled, sending, failed). Answered with
    /// [`Response::Outbox`].
    ListOutbox,
    /// Put a failed operation back into the queue and replay it. Answered
    /// with [`Response::Queued`] (the same op id); `conflict` when the
    /// operation has not failed.
    RetryOp {
        /// The operation (from [`Event::OpStatus`]).
        op_id: OpId,
    },

    // --- Composer (M1b WP21) ---
    /// A new draft, not yet saved: identity (given or chosen as by
    /// [`Request::ChooseIdentity`]), mode (given or the identity's default),
    /// signature and, for a reply or forward, recipients, subject and the
    /// quoted original in that mode (forwards also carry the original
    /// attachments). Answered with [`Response::Draft`] (`id: null`).
    NewDraft {
        /// The identity; `null` = choose one.
        #[serde(default)]
        identity_id: Option<IdentityId>,
        /// The active account (real or virtual), for choosing the identity.
        #[serde(default)]
        account_id: Option<AccountId>,
        /// The editing mode; `null` = the identity's default.
        #[serde(default)]
        mode: Option<ComposerMode>,
        /// Reply or forward; `null` for a new message.
        #[serde(default)]
        context: Option<DraftContext>,
    },
    /// Render Markdown to the HTML that would be sent (sanitized, with the
    /// identity's mail stylesheet inlined), for the preview. Answered with
    /// [`Response::PreviewRendered`].
    RenderPreview {
        /// Markdown source.
        markdown: String,
        /// Whose mail stylesheet to apply; `null` = none.
        #[serde(default)]
        identity_id: Option<IdentityId>,
    },
    /// Convert a draft's body to another mode (Markdown → HTML is
    /// lossless; HTML → Markdown / plain text is best effort). Answered with
    /// [`Response::DraftConverted`]; nothing is saved.
    ConvertDraft {
        /// The draft as edited.
        draft: Box<Draft>,
        /// The target mode.
        mode: ComposerMode,
    },
    /// Store a file for a draft in the blob store. Answered with
    /// [`Response::AttachmentUploaded`]; add the attachment to the draft and
    /// save it. `too_large` above the size limit.
    UploadAttachment {
        /// File name.
        filename: String,
        /// MIME type (`application/octet-stream` when unknown).
        mime: String,
        /// The content, base64 (standard alphabet, padded).
        data: String,
        /// `Content-ID` for an inline image; `null` for an attachment.
        #[serde(default)]
        content_id: Option<String>,
    },
    /// Addresses for autocomplete, from the addresses collected from sent
    /// and received mail, best first. Answered with
    /// [`Response::AddressSuggestions`].
    SuggestAddresses {
        /// What the user typed (matches name or address, case-insensitive).
        prefix: String,
        /// Maximum number of suggestions.
        limit: u32,
    },

    // --- Rules (M1b WP22) ---
    /// All rules in `rules.kdl` order. Answered with [`Response::Rules`].
    ListRules,
    /// Create or replace a rule (matched by `rule.id`) in `rules.kdl`.
    /// Answered with [`Response::RuleSaved`].
    SaveRule {
        /// The rule; an empty `id` creates a new rule.
        rule: Box<Rule>,
        /// Position for a new rule (0 = first); `null` = last. Ignored for
        /// existing rules.
        #[serde(default)]
        position: Option<u32>,
    },
    /// Remove a rule. Answered with [`Response::Done`].
    DeleteRule {
        /// The rule.
        rule_id: RuleId,
    },
    /// Reorder rules; `rule_ids` must list every rule once. Answered with
    /// [`Response::Done`].
    ReorderRules {
        /// All rule ids in the new order.
        rule_ids: Vec<RuleId>,
    },
    /// Dry run: which messages would match and what would happen. Nothing
    /// changes. Answered with [`Response::RuleTested`].
    TestRule {
        /// The rule (need not be saved).
        rule: Box<Rule>,
        /// Test in this folder; `null` = the rule's scope.
        #[serde(default)]
        folder_id: Option<FolderId>,
        /// Maximum number of matches to return.
        limit: u32,
    },
    /// Run a saved rule now. Answered with [`Response::RuleRan`]; progress
    /// arrives as [`Event::RuleRunFinished`].
    RunRule {
        /// The rule.
        rule_id: RuleId,
        /// Run on these messages; empty = on `folder_id` or the rule's scope.
        #[serde(default)]
        message_ids: Vec<MessageId>,
        /// Run on this folder; `null` = the rule's scope.
        #[serde(default)]
        folder_id: Option<FolderId>,
    },
    /// Recent rule runs, newest first. Answered with [`Response::RuleRuns`].
    ListRuleRuns {
        /// Runs of this rule; `null` = of all rules.
        #[serde(default)]
        rule_id: Option<RuleId>,
        /// Maximum number of runs.
        limit: u32,
    },
    /// Undo everything a run did, as one unit (the inverse operations go
    /// through the queue). Answered with [`Response::Queued`]; `conflict`
    /// when the run is not undoable.
    UndoRuleRun {
        /// The run.
        run_id: RuleRunId,
    },
    /// The text form (`rules.kdl` syntax) of a rule, for the editor's text
    /// view. Answered with [`Response::RuleText`].
    RuleToText {
        /// The rule.
        rule: Box<Rule>,
    },
    /// Parse the text form of one rule. Answered with
    /// [`Response::RuleParsed`].
    RuleFromText {
        /// One `rule "<name>" { … }` node.
        text: String,
    },

    // --- MCP clients (M1b WP23) ---
    /// Registered MCP clients (revoked ones included). Answered with
    /// [`Response::McpClients`].
    ListMcpClients,
    /// Register a client and issue its bearer token. Answered with
    /// [`Response::McpClientCreated`], the only time the token is shown.
    CreateMcpClient {
        /// Display name (unique among active clients; `conflict` otherwise).
        name: String,
        /// Initial permissions (usually the empty default).
        #[serde(default)]
        grant: McpGrant,
    },
    /// Replace a client's permissions. Answered with
    /// [`Response::McpClientSaved`].
    UpdateMcpGrants {
        /// The client.
        client_id: McpClientId,
        /// The new permissions.
        grant: McpGrant,
    },
    /// Revoke a client: its token stops working at once, its audit entries
    /// stay. Answered with [`Response::Done`].
    RevokeMcpClient {
        /// The client.
        client_id: McpClientId,
    },
    /// A page of the MCP audit log, newest first. Answered with
    /// [`Response::McpAudit`].
    ListMcpAudit {
        /// Entries of this client; `null` = of all clients.
        #[serde(default)]
        client_id: Option<McpClientId>,
        /// Only entries with a smaller id (paging); `null` = the newest.
        #[serde(default)]
        before: Option<u64>,
        /// Maximum number of entries.
        limit: u32,
    },
    /// Whether the MCP server runs and where. Answered with
    /// [`Response::McpServer`].
    GetMcpServerInfo,

    // --- UI context (M1b; read by MCP clients) ---
    /// Report what the UI shows now (see [`crate::context`]). Answered with
    /// [`Response::Done`].
    ReportUiContext {
        /// The current context (replaces the previous report).
        context: UiContext,
    },
    /// The last reported UI context. Answered with [`Response::UiContext`].
    GetUiContext,

    // --- Customization (M1b WP24) ---
    /// Write UI edits into `layout.kdl`, keeping comments and formatting.
    /// Answered with [`Response::Done`]; the new layout follows as
    /// [`Event::ConfigChanged`]. `config` when `layout.kdl` is invalid.
    UpdateLayout {
        /// The edits, applied in order as one write.
        patches: Vec<LayoutPatch>,
    },
    /// Compare an incoming theme or layout with the user's file. Answered
    /// with [`Response::ConfigMergePreview`].
    PreviewConfigMerge {
        /// `theme` or `layout`.
        file: ConfigFile,
        /// The incoming file's text.
        incoming: String,
    },
    /// Resolve a merge without writing it, for the live preview. Answered
    /// with [`Response::ConfigMergeResolved`].
    ResolveConfigMerge {
        /// `theme` or `layout`.
        file: ConfigFile,
        /// The incoming file's text.
        incoming: String,
        /// Choices per item; items without one use `default`.
        choices: Vec<MergeChoice>,
        /// The side for items without a choice.
        #[serde(default)]
        default: MergeTake,
    },
    /// Write the merge result after backing up the current file. Answered
    /// with [`Response::ConfigMergeApplied`].
    ApplyConfigMerge {
        /// `theme` or `layout`.
        file: ConfigFile,
        /// The incoming file's text.
        incoming: String,
        /// Choices per item; items without one use `default`.
        choices: Vec<MergeChoice>,
        /// The side for items without a choice.
        #[serde(default)]
        default: MergeTake,
    },
    /// Backups of a configuration file, newest first. Answered with
    /// [`Response::ConfigBackups`].
    ListConfigBackups {
        /// The file.
        file: ConfigFile,
    },
    /// Restore a backup (the current file is backed up first). Answered
    /// with [`Response::Done`].
    RevertConfig {
        /// The file.
        file: ConfigFile,
        /// The backup; `null` = the newest.
        #[serde(default)]
        backup_id: Option<ConfigBackupId>,
    },
    /// Open a configuration file in the system editor at a position (the
    /// inspector's "open file at line"). Answered with [`Response::Done`];
    /// `unsupported` in web mode.
    OpenConfigFile {
        /// The file.
        file: ConfigFile,
        /// 1-based line.
        line: u32,
        /// 1-based column.
        column: u32,
    },
    /// Saved searches in display order. Answered with
    /// [`Response::SavedSearches`].
    ListSavedSearches,
    /// Create (`id: null`) or replace a saved search. Answered with
    /// [`Response::SavedSearchSaved`]; `invalid_request` when the query does
    /// not parse.
    SaveSavedSearch {
        /// The saved search.
        search: SavedSearch,
    },
    /// Remove a saved search. Answered with [`Response::Done`].
    DeleteSavedSearch {
        /// The saved search.
        search_id: SavedSearchId,
    },

    // --- Keys and certificates (M2a WP30, ADR 0029) ---
    /// OpenPGP keys and S/MIME certificates in the keyring, own keys first,
    /// then by primary address. Answered with [`Response::Keys`].
    ListKeys {
        /// Filters; the default lists everything.
        #[serde(default)]
        filter: KeyFilter,
    },
    /// One key. Answered with [`Response::Key`].
    GetKey {
        /// The key.
        key_id: KeyId,
    },
    /// Import OpenPGP keys (armored or binary; public or secret, several
    /// at once) or certificates (PEM or DER). Existing keys are merged.
    /// Unprotected secret keys are protected with the keychain; protected
    /// ones keep their passphrase. Answered with [`Response::KeysImported`];
    /// `crypto` when nothing in `data` is key material.
    ImportKeys {
        /// The key data.
        data: String,
        /// How `data` is encoded.
        #[serde(default)]
        encoding: KeyEncoding,
    },
    /// Import an S/MIME certificate with its private key from a PKCS#12
    /// file (`.p12` / `.pfx`). Answered with [`Response::KeysImported`];
    /// `auth_failed` for a wrong passphrase.
    ImportPkcs12 {
        /// The file, base64.
        data: String,
        /// The file's passphrase (never stored).
        passphrase: String,
        /// How to protect the private key in Nail.
        #[serde(default)]
        protection: SecretProtection,
    },
    /// The public part of a key (OpenPGP transferable public key, or the
    /// certificate). Answered with [`Response::KeyExported`].
    ExportPublicKey {
        /// The key.
        key_id: KeyId,
        /// `text` (armored / PEM) or `base64` (binary / DER).
        #[serde(default)]
        encoding: KeyEncoding,
    },
    /// Generate an OpenPGP key for an identity (user id = its name and
    /// address). Answered with [`Response::KeyGenerated`].
    GenerateKey {
        /// The identity.
        identity_id: IdentityId,
        /// Algorithm (default Ed25519 + X25519).
        #[serde(default)]
        algorithm: KeyAlgorithm,
        /// Expiry in days from now; `null` = does not expire.
        #[serde(default)]
        expires_days: Option<u32>,
        /// How to protect the secret key.
        #[serde(default)]
        protection: SecretProtection,
    },
    /// Remove a key. Answered with [`Response::Done`]; `conflict` when it
    /// has a secret part and `delete_secret` is `false`.
    DeleteKey {
        /// The key.
        key_id: KeyId,
        /// Also delete the secret part (cannot be undone).
        #[serde(default)]
        delete_secret: bool,
    },
    /// Store or clear the user's trust decision. Answered with
    /// [`Response::KeySaved`].
    SetKeyTrust {
        /// The key.
        key_id: KeyId,
        /// The decision.
        decision: TrustDecision,
    },
    /// Enter the passphrase of a secret key. Answered with
    /// [`Response::Done`]; `auth_failed` for a wrong passphrase.
    UnlockKey {
        /// The key.
        key_id: KeyId,
        /// The passphrase.
        passphrase: String,
        /// Keep it unlocked for this many minutes; `null` = until Nail
        /// quits.
        #[serde(default)]
        remember_minutes: Option<u32>,
    },
    /// Look up an address's OpenPGP keys online (Web Key Directory,
    /// keys.openpgp.org). Nothing is imported. Answered with
    /// [`Response::KeyLookup`]; `unsupported` when online lookup is off
    /// ([`SecuritySettings::key_lookup`]).
    LookupKeys {
        /// The address.
        email: String,
        /// Where to look; empty = [`SecuritySettings::lookup_sources`].
        #[serde(default)]
        sources: Vec<KeyLookupSource>,
    },
    /// Certificates in a Windows certificate store. Answered with
    /// [`Response::WindowsCertificates`]; `unsupported` on other systems.
    ListWindowsCertificates {
        /// The store.
        store: WindowsCertStore,
    },
    /// Use a certificate from the Windows store (its private key stays in
    /// the store). Answered with [`Response::KeysImported`].
    ImportWindowsCertificate {
        /// The store.
        store: WindowsCertStore,
        /// SHA-1 thumbprint.
        thumbprint: String,
    },

    // --- Composing and Autocrypt (M2a) ---
    /// Key status of each recipient while composing, and what to suggest
    /// for the sign / encrypt toggles. Answered with
    /// [`Response::RecipientKeys`].
    CheckRecipientKeys {
        /// The sending identity.
        identity_id: IdentityId,
        /// Recipient addresses (To, Cc, Bcc).
        recipients: Vec<String>,
        /// Also look up recipients without a key online (subject to
        /// [`SecuritySettings::key_lookup`]); found keys are imported.
        #[serde(default)]
        lookup: bool,
    },
    /// Send an Autocrypt setup message (the identity's secret key,
    /// encrypted with a new passcode) to the identity's own address.
    /// Answered with [`Response::AutocryptSetupSent`].
    SendAutocryptSetup {
        /// The identity.
        identity_id: IdentityId,
    },
    /// Import the key from an Autocrypt setup message. Answered with
    /// [`Response::KeysImported`]; `auth_failed` for a wrong passcode.
    ImportAutocryptSetup {
        /// The setup message.
        message_id: MessageId,
        /// The passcode shown on the other device.
        passcode: String,
    },

    // --- Security settings and encryption at rest (M2a) ---
    /// Global security settings. Answered with
    /// [`Response::SecuritySettings`].
    GetSecuritySettings,
    /// Replace the global security settings. Answered with
    /// [`Response::SecuritySettings`] (as saved).
    UpdateSecuritySettings {
        /// The new settings.
        settings: SecuritySettings,
    },
    /// Suggest the `authserv-id` of an account's receiving server, learned
    /// from its recent inbox mail (the id on top of most of it, belonging
    /// to the server that received it). Nail never trusts a suggestion by
    /// itself: only the configured `AccountConfig::authserv_ids` count, so
    /// the user confirms it by saving it there. Answered with
    /// [`Response::AuthservIdSuggestion`].
    SuggestAuthservId {
        /// The account.
        account_id: AccountId,
    },
    /// State of encryption at rest. Answered with [`Response::VaultStatus`].
    GetVaultStatus,
    /// Turn encryption at rest on or off. The migration runs in the
    /// background (crash-safe, resumed on start); progress arrives as
    /// [`Event::VaultProgress`]. Answered with [`Response::VaultStatus`];
    /// `conflict` while another migration runs.
    SetVaultEncryption {
        /// `true` = encrypt, `false` = decrypt.
        enabled: bool,
    },

    // --- Remote UI (M2b WP40, ADR 0036) ---
    // Administration: from the desktop, or from a remote session whose
    // grant has `admin` and that re-authenticated recently
    // (`reauth_required` otherwise).
    /// Remote-UI settings and status. Answered with [`Response::RemoteUi`].
    GetRemoteUiSettings,
    /// Replace the remote-UI settings; the remote UI restarts with them.
    /// Answered with [`Response::RemoteUi`]; `invalid_request` for invalid
    /// settings, `forbidden` for internet mode or ACME while internet mode
    /// is locked ([`crate::remote::RemoteUiSettings::issues`]).
    UpdateRemoteUiSettings {
        /// The new settings.
        settings: RemoteUiSettings,
    },
    /// Make a new self-signed certificate (paired browsers must accept the
    /// new fingerprint). Answered with [`Response::RemoteUi`].
    RegenerateRemoteCertificate,
    /// Remote-UI users. Answered with [`Response::RemoteUsers`].
    ListRemoteUsers,
    /// Add a user (without credentials: set a password or pair a device
    /// to register a passkey). Answered with [`Response::RemoteUserSaved`];
    /// `conflict` when the name is taken.
    CreateRemoteUser {
        /// Login name (unique, case-insensitive).
        name: String,
        /// Display name.
        #[serde(default)]
        display_name: Option<String>,
        /// The most the user's devices and sessions may be granted.
        #[serde(default)]
        grant: RemoteGrant,
    },
    /// Change a user. Answered with [`Response::RemoteUserSaved`].
    UpdateRemoteUser {
        /// The user.
        user_id: RemoteUserId,
        /// The changes.
        update: RemoteUserUpdate,
    },
    /// Remove a user with their passkeys, devices and sessions (audit
    /// entries stay). Answered with [`Response::Done`].
    DeleteRemoteUser {
        /// The user.
        user_id: RemoteUserId,
    },
    /// Set (or, with `null`, remove) a user's password; stored as an
    /// Argon2id hash. Ends the user's other sessions. Answered with
    /// [`Response::RemoteUserSaved`]; `invalid_request` for a password
    /// the policy rejects.
    SetRemotePassword {
        /// The user.
        user_id: RemoteUserId,
        /// The new password, `null` = none.
        password: Option<String>,
    },
    /// Start enrolling TOTP: a new secret (kept by the core until
    /// confirmed). Answered with [`Response::TotpEnrollment`].
    StartTotpEnrollment {
        /// The user.
        user_id: RemoteUserId,
    },
    /// Confirm TOTP with a code from the authenticator app; from then on
    /// password logins need a code. Answered with
    /// [`Response::RemoteUserSaved`]; `auth_failed` for a wrong code.
    ConfirmTotpEnrollment {
        /// The user.
        user_id: RemoteUserId,
        /// A current code.
        code: String,
    },
    /// Remove TOTP. Answered with [`Response::RemoteUserSaved`].
    DisableTotp {
        /// The user.
        user_id: RemoteUserId,
    },
    /// Lift a lockout after failed logins. Answered with
    /// [`Response::RemoteUserSaved`].
    UnlockRemoteUser {
        /// The user.
        user_id: RemoteUserId,
    },
    /// Rename a passkey. Answered with [`Response::RemoteUserSaved`].
    RenamePasskey {
        /// The passkey.
        passkey_id: PasskeyId,
        /// New name.
        name: String,
    },
    /// Remove a passkey. Answered with [`Response::RemoteUserSaved`].
    DeletePasskey {
        /// The passkey.
        passkey_id: PasskeyId,
    },
    /// Paired devices (revoked ones included). Answered with
    /// [`Response::RemoteDevices`].
    ListRemoteDevices {
        /// Only this user's; `null` = all.
        #[serde(default)]
        user_id: Option<RemoteUserId>,
    },
    /// Rename a device or change its grant (within its user's; its
    /// sessions are narrowed at once). Answered with
    /// [`Response::RemoteDeviceSaved`].
    UpdateRemoteDevice {
        /// The device.
        device_id: RemoteDeviceId,
        /// New name; `null` = unchanged.
        #[serde(default)]
        name: Option<String>,
        /// New grant; `null` = unchanged.
        #[serde(default)]
        grant: Option<RemoteGrant>,
    },
    /// Revoke a device: its sessions end, it must pair again. Answered
    /// with [`Response::Done`].
    RevokeRemoteDevice {
        /// The device.
        device_id: RemoteDeviceId,
    },
    /// Remote sessions, newest first. Answered with
    /// [`Response::RemoteSessions`].
    ListRemoteSessions {
        /// Which ones; the default lists valid sessions of everyone.
        #[serde(default)]
        filter: RemoteSessionFilter,
    },
    /// End a session (its browser is logged out at once). Answered with
    /// [`Response::Done`].
    RevokeRemoteSession {
        /// The session.
        session_id: RemoteSessionId,
    },
    /// End every session (of one user, or of everyone). Answered with
    /// [`Response::Done`].
    RevokeAllRemoteSessions {
        /// Only this user's; `null` = everyone's.
        #[serde(default)]
        user_id: Option<RemoteUserId>,
    },
    /// Create a one-time pairing code (this approves the device that
    /// enters it). Answered with [`Response::PairingCodeCreated`], the only
    /// time the code is shown.
    CreatePairingCode {
        /// The user the device will belong to.
        user_id: RemoteUserId,
        /// Name for the device; `null` = what the browser sends.
        #[serde(default)]
        device_name: Option<String>,
        /// Grant of the device; `null` = the user's grant.
        #[serde(default)]
        grant: Option<RemoteGrant>,
        /// Validity in minutes; `null` = 10 (the core caps it at 60).
        #[serde(default)]
        ttl_minutes: Option<u32>,
    },
    /// Recent pairing codes (pending first). Answered with
    /// [`Response::PairingCodes`].
    ListPairingCodes,
    /// Cancel a pending pairing code. Answered with [`Response::Done`].
    CancelPairingCode {
        /// The code.
        pairing_id: PairingId,
    },
    /// A page of the remote-UI audit log, newest first. Answered with
    /// [`Response::RemoteAudit`].
    ListRemoteAudit {
        /// Entries of this user; `null` = all.
        #[serde(default)]
        user_id: Option<RemoteUserId>,
        /// Only entries with a smaller id (paging); `null` = the newest.
        #[serde(default)]
        before: Option<u64>,
        /// Maximum number of entries.
        limit: u32,
    },

    // --- Remote content (M2b) ---
    /// Load a message's remote content through the proxy from now on, for
    /// this message, its sender or its sender's domain. Answered with
    /// [`Response::RemoteContentAllowed`]; `invalid_request` when the
    /// message has no usable sender for `sender` / `domain`. Refetch the
    /// message ([`Event::RemoteContentPolicyChanged`] follows).
    AllowRemoteContent {
        /// The message.
        message_id: MessageId,
        /// How far the allowance reaches.
        scope: RemoteContentScope,
    },
    /// The remote-content allow-lists. Answered with
    /// [`Response::RemoteContentAllows`].
    ListRemoteContentAllows,
    /// Remove an allow-list entry. Answered with [`Response::Done`]
    /// (also when there was none).
    RemoveRemoteContentAllow {
        /// Its scope.
        scope: RemoteContentScope,
        /// Its value (as listed).
        value: String,
    },

    // --- Launch requests (M2b, desktop only; ADR 0041) ---
    /// The launch requests (`mailto:` links, opened files) waiting for the
    /// UI; each is handed out once. Answered with
    /// [`Response::LaunchRequests`].
    TakeLaunchRequests,
    /// Import a `.eml` file into the local "Opened files" account
    /// ([`crate::launch::OPENED_FILES_ACCOUNT_ID`]) to show it. Answered
    /// with [`Response::MessageImported`]; `not_found` / `invalid_request`
    /// for a missing, oversized or unreadable file.
    ImportMessageFile {
        /// Absolute path of the file.
        path: String,
    },

    // --- Server-side rules and vacation (M2c WP50, ADR 0043) ---
    /// Where each rule runs (server / client / not expressible) and the
    /// state of each IMAP account's Sieve script. Answered with
    /// [`Response::SieveStatus`].
    GetSieveStatus {
        /// One account; `null` = every account with a `sieve` node.
        #[serde(default)]
        account_id: Option<AccountId>,
    },
    /// Upload the account's script now (e.g. after a failure). Answered
    /// with [`Response::SieveStatus`] for the account (usually
    /// `uploading`); the outcome arrives as [`Event::SieveStatusChanged`].
    /// `unsupported` when the account has no ManageSieve settings.
    SyncSieve {
        /// The account.
        account_id: AccountId,
    },
    /// The script Nail uploads for an account, for display. Answered with
    /// [`Response::SieveScript`].
    PreviewSieveScript {
        /// The account.
        account_id: AccountId,
    },
    /// The account's vacation reply. Answered with [`Response::Vacation`].
    GetVacation {
        /// The account.
        account_id: AccountId,
    },
    /// Save the vacation reply; the script is re-uploaded. Answered with
    /// [`Response::Vacation`]; `invalid_request` for invalid settings
    /// ([`crate::sieve::VacationSettings::issues`]), `unsupported` for an
    /// account without ManageSieve settings.
    UpdateVacation {
        /// The account.
        account_id: AccountId,
        /// The new settings.
        settings: VacationSettings,
    },

    // --- Junk (M2c) ---
    /// "Mark as junk" / "Not junk": trains the account's classifier, sets
    /// `$Junk` / `$NotJunk` and moves the messages to Junk (or from Junk
    /// back to the Inbox), through the operation queue. Answered with
    /// [`Response::Queued`].
    MarkJunk {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// `true` = junk, `false` = not junk.
        junk: bool,
    },
    /// The classifier of an account. Answered with
    /// [`Response::JunkStatus`].
    GetJunkStatus {
        /// The account.
        account_id: AccountId,
    },
    /// Change an account's junk settings. Answered with
    /// [`Response::JunkStatus`]; `invalid_request` for invalid settings.
    UpdateJunkSettings {
        /// The account.
        account_id: AccountId,
        /// The new settings.
        settings: JunkSettings,
    },
    /// Forget everything the account's classifier learned. Answered with
    /// [`Response::JunkStatus`].
    ResetJunkClassifier {
        /// The account.
        account_id: AccountId,
    },

    // --- Snooze (M2c) ---
    /// Snooze messages until a time: keyword + move to the hidden Snoozed
    /// folder through the operation queue; they come back at `until`.
    /// Answered with [`Response::Queued`]; `invalid_request` for a time
    /// in the past.
    SnoozeMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// When they come back (RFC 3339).
        until: String,
        /// Where they come back to; `null` = the folder each is in now.
        #[serde(default)]
        wake_folder_id: Option<FolderId>,
    },
    /// Bring snoozed messages back now. Answered with
    /// [`Response::Queued`].
    UnsnoozeMessages {
        /// The messages.
        message_ids: Vec<MessageId>,
    },
    /// Snoozed messages, soonest first. Answered with
    /// [`Response::Snoozed`].
    ListSnoozed {
        /// One account (real or virtual); `null` = all.
        #[serde(default)]
        account_id: Option<AccountId>,
    },

    // --- Notifications (M2c) ---
    /// Answered with [`Response::NotificationSettings`].
    GetNotificationSettings,
    /// Answered with [`Response::NotificationSettings`];
    /// `invalid_request` for invalid settings.
    UpdateNotificationSettings {
        /// The new settings.
        settings: NotificationSettings,
    },
    /// Push a test [`Event::NotificationRequested`] (ignores quiet hours).
    /// Answered with [`Response::Done`].
    TestNotification,

    // --- Sync policy and bodies (M2c) ---
    /// The resolved sync policy of a folder. Answered with
    /// [`Response::FolderSyncPolicy`].
    GetFolderSyncPolicy {
        /// The folder (of a real account).
        folder_id: FolderId,
    },
    /// Write the rule for exactly this folder into the account's `sync`
    /// block in `accounts.kdl`; `null` fields are left out of the rule, all
    /// `null` removes it. Answered with [`Response::FolderSyncPolicy`];
    /// followed by [`Event::AccountsChanged`].
    SetFolderSyncPolicy {
        /// The folder (of a real account).
        folder_id: FolderId,
        /// What to download.
        #[serde(default)]
        bodies: Option<BodyPolicy>,
        /// Whether to synchronize it.
        #[serde(default)]
        enabled: Option<bool>,
        /// Whether to watch it with IDLE.
        #[serde(default)]
        idle: Option<bool>,
    },
    /// Download the bodies of messages that have none locally (e.g. before
    /// going offline). Answered with [`Response::BodiesFetched`] when done.
    FetchMessageBodies {
        /// The messages.
        message_ids: Vec<MessageId>,
    },

    // --- MCP settings (M2c) ---
    /// Answered with [`Response::McpSettings`].
    GetMcpSettings,
    /// Save the MCP settings; the server restarts on the new port.
    /// Answered with [`Response::McpSettings`]; `invalid_request` for a
    /// port below 1024, `conflict` when the port is taken.
    UpdateMcpSettings {
        /// The new settings.
        settings: McpSettings,
    },

    // --- Rules from messages (M2c) ---
    /// A new (unsaved) rule pre-filled from a message, using all its
    /// headers (`List-Id`, sender, …). Answered with
    /// [`Response::RuleSuggested`].
    RuleFromMessage {
        /// The message.
        message_id: MessageId,
    },

    // --- Plug-ins: administration (M2d WP60, ADR 0050) ---
    /// Every installed plug-in (dev plug-ins included). Answered with
    /// [`Response::Plugins`].
    ListPlugins,
    /// One plug-in. Answered with [`Response::Plugin`]; `not_found`.
    GetPlugin {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Read a package (a `.nailplugin` file or URL) without installing it,
    /// for the install dialog (manifest, permissions, issues, hash).
    /// Answered with [`Response::PluginInstallPreview`]; `invalid_request`
    /// for a `dev` source, `too_large`, `network`.
    PreviewPluginInstall {
        /// The package.
        source: PluginPackageSource,
    },
    /// Install (or update) a package with the permissions the user
    /// granted. Answered with [`Response::Plugin`], followed by
    /// [`Event::PluginsChanged`]; `conflict` when the package's hash is not
    /// `package_sha256` (it changed since the preview), `invalid_request`
    /// for manifest issues, grants of undeclared permissions or a `dev`
    /// source.
    InstallPlugin {
        /// The package.
        source: PluginPackageSource,
        /// SHA-256 (hex) from [`Request::PreviewPluginInstall`].
        package_sha256: String,
        /// The granted permissions.
        grants: Vec<PluginGrant>,
        /// Whether to enable (start) it right away.
        enable: bool,
    },
    /// Load an unpacked plug-in folder for development (`nail plugin dev`):
    /// installed as a `dev` plug-in, enabled, reloaded when its files
    /// change. Desktop only. Answered with [`Response::Plugin`];
    /// `invalid_request` with the manifest's issues.
    LoadDevPlugin {
        /// Absolute path of the folder holding `manifest.json`.
        folder: String,
        /// The granted permissions (the CLI grants what the manifest
        /// declares).
        grants: Vec<PluginGrant>,
    },
    /// Enable and start a plug-in (also restarts a crashed one). Answered
    /// with [`Response::Plugin`], followed by [`Event::PluginsChanged`].
    EnablePlugin {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Stop and disable a plug-in. Answered with [`Response::Plugin`],
    /// followed by [`Event::PluginsChanged`].
    DisablePlugin {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Stop and remove a plug-in with its grants, settings, storage and
    /// logs. Items it placed in `layout.kdl` stay (the UI ignores actions
    /// and panels of missing plug-ins; the user removes them). Answered
    /// with [`Response::Done`], followed by [`Event::PluginsChanged`].
    UninstallPlugin {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Replace a plug-in's grants (revoking = leaving one out). Answered
    /// with [`Response::Plugin`], followed by [`Event::PluginsChanged`];
    /// `invalid_request` for undeclared permissions or account lists on
    /// permissions that take none.
    UpdatePluginGrants {
        /// The plug-in.
        plugin_id: PluginId,
        /// The complete new grants.
        grants: Vec<PluginGrant>,
    },
    /// Log lines, oldest first. Answered with [`Response::PluginLogs`].
    ListPluginLogs {
        /// One plug-in; `null` = all.
        #[serde(default)]
        plugin_id: Option<PluginId>,
        /// Only lines after this sequence number (for tailing).
        #[serde(default)]
        after_seq: Option<u64>,
        /// At most this many lines (the newest ones).
        limit: u32,
    },
    /// Delete a plug-in's log. Answered with [`Response::Done`].
    ClearPluginLogs {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// A plug-in's settings schema and values. Answered with
    /// [`Response::PluginSettings`].
    GetPluginSettings {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Change a plug-in's settings (missing keys keep their value).
    /// Answered with [`Response::PluginSettings`]; `invalid_request` for
    /// unknown keys or invalid values.
    UpdatePluginSettings {
        /// The plug-in.
        plugin_id: PluginId,
        /// Setting key → value.
        values: BTreeMap<String, Value>,
    },
    /// The user's `layout.kdl` with the plug-in's suggested placements
    /// added, compared with the current file, for the merge dialog; apply
    /// with [`Request::ApplyConfigMerge`] (`file: layout`, `incoming`).
    /// Answered with [`Response::PluginPlacements`].
    PreviewPluginPlacements {
        /// The plug-in.
        plugin_id: PluginId,
    },

    // --- Plug-ins: running (M2d) ---
    /// Run a plug-in command (`plugin.<id>.<command>`) with the UI's
    /// context. Answered with [`Response::Done`] once the plug-in took it;
    /// what it does arrives as events (dialogs, notifications, log lines).
    /// `not_found` for an unknown command, `unavailable` when the plug-in
    /// is disabled or crashed.
    RunPluginCommand {
        /// The action id.
        command: String,
        /// Where it was invoked.
        surface: PluginCommandSurface,
        /// The UI's context (selection, open message, folder, draft).
        context: UiContext,
        /// The UI's random id (see [`crate::plugins::runtime`]).
        #[serde(default)]
        client_id: Option<String>,
    },
    /// Close a plug-in dialog with a button (or dismiss it with `null`)
    /// and its values. Answered with [`Response::Done`], followed by
    /// [`Event::PluginDialogClosed`]; `not_found` when it is no longer
    /// open.
    RespondPluginDialog {
        /// The dialog.
        dialog_id: PluginDialogId,
        /// The button id; `null` = dismissed.
        button: Option<String>,
        /// Values of the input nodes by id.
        values: BTreeMap<String, Value>,
    },
    /// A button inside a dialog body or panel was pressed. Answered with
    /// [`Response::Done`].
    PluginUiAction {
        /// Where.
        target: PluginUiTarget,
        /// The button's id.
        component_id: String,
        /// Current values of the input nodes of that dialog or panel.
        values: BTreeMap<String, Value>,
        /// The UI's random id.
        #[serde(default)]
        client_id: Option<String>,
    },
    /// The content of a plug-in panel (`plugin-panel "<id>.<panel>"`) for
    /// the UI's context. Answered with [`Response::PluginPanel`]; later
    /// changes arrive as [`Event::PluginPanelChanged`].
    GetPluginPanel {
        /// `<plugin>.<panel>`.
        panel: String,
        /// The UI's context.
        context: UiContext,
        /// The UI's random id.
        #[serde(default)]
        client_id: Option<String>,
    },

    // --- PIM sources (M3a WP70, ADR 0058) ---
    /// Every source (the local one first), with its status. Answered with
    /// [`Response::PimSources`].
    ListPimSources,
    /// Add a subscription or DAV source to `accounts.kdl` and start
    /// syncing it. Answered with [`Response::PimSourceSaved`], followed by
    /// [`Event::PimSourcesChanged`] and [`Event::AccountsChanged`];
    /// `invalid_request` with a `pim-source-issue-*` id, `conflict` for a
    /// taken id.
    AddPimSource {
        /// The source; an empty `id` lets the core derive one.
        config: Box<PimSourceConfig>,
        /// Its secret (password or finished OAuth flow), stored in the
        /// keychain under the source's reference.
        #[serde(default)]
        secret: Option<SecretValue>,
    },
    /// Replace a source's configuration (matched by `config.id`).
    /// Answered with [`Response::PimSourceSaved`].
    UpdatePimSource {
        /// The new configuration.
        config: Box<PimSourceConfig>,
        /// A changed secret, `null` = keep.
        #[serde(default)]
        secret: Option<SecretValue>,
    },
    /// Remove a source from `accounts.kdl` with all its local data and
    /// queued writes (nothing changes on the server). Answered with
    /// [`Response::Done`]; `invalid_request` for the local source.
    RemovePimSource {
        /// The source.
        source_id: PimSourceId,
    },
    /// Try a configuration without saving it: discovery (RFC 6764 for DAV
    /// sources without URL), login, the collections found. Answered with
    /// [`Response::PimSourceTested`].
    TestPimSource {
        /// The configuration.
        config: Box<PimSourceConfig>,
        /// A secret to try.
        #[serde(default)]
        secret: Option<SecretValue>,
    },
    /// Sync now. Answered with [`Response::Done`] at once; progress arrives
    /// as [`Event::PimSourcesChanged`].
    SyncPimSources {
        /// One source; `null` = all.
        #[serde(default)]
        source_id: Option<PimSourceId>,
    },

    // --- Calendars and task lists (M3a) ---
    /// Answered with [`Response::Calendars`] (by `order`, then name).
    ListCalendars {
        /// One source; `null` = all.
        #[serde(default)]
        source_id: Option<PimSourceId>,
    },
    /// Create a calendar or task list (local, or `MKCALENDAR` on a CalDAV
    /// source). Answered with [`Response::CalendarSaved`];
    /// `invalid_request` for ICS sources and CardDAV sources.
    CreateCalendar {
        /// The source.
        source_id: PimSourceId,
        /// Its settings.
        settings: CalendarSettings,
        /// What it holds (at least one).
        components: Vec<CalendarComponent>,
    },
    /// Change a calendar's settings. Answered with
    /// [`Response::CalendarSaved`] (works for read-only calendars too:
    /// colour, visibility and reminders are local).
    UpdateCalendar {
        /// The calendar.
        calendar_id: CalendarId,
        /// The new settings.
        settings: CalendarSettings,
    },
    /// Delete a calendar with its events and tasks (on the server too).
    /// Answered with [`Response::PimDeleted`]; `forbidden` for
    /// subscriptions (remove the source instead).
    DeleteCalendar {
        /// The calendar.
        calendar_id: CalendarId,
    },

    // --- Events (M3a) ---
    /// The occurrences overlapping `[from, to)`, by start, from the instance
    /// cache (cancelled occurrences and hidden calendars left out).
    /// Answered with [`Response::EventInstances`]; `invalid_request` for a
    /// range over 400 days.
    ListEventInstances {
        /// Range start (RFC 3339).
        from: String,
        /// Range end (RFC 3339).
        to: String,
        /// Calendars; `null` = every visible one.
        #[serde(default)]
        calendar_ids: Option<Vec<CalendarId>>,
        /// IANA zone that all-day and floating events are placed in (the
        /// UI's zone).
        tz: String,
    },
    /// One event (the whole series). Answered with [`Response::PimEvent`].
    GetEvent {
        /// The event.
        event_id: EventId,
    },
    /// Create an event. Answered with [`Response::EventSaved`];
    /// `invalid_request` with a `pim-issue-*` id, `forbidden` for a
    /// read-only calendar.
    CreateEvent {
        /// Its calendar.
        calendar_id: CalendarId,
        /// The fields.
        fields: Box<EventFields>,
        /// Recurrence, `null` = single.
        #[serde(default)]
        recurrence: Option<Recurrence>,
    },
    /// Change an event, one occurrence, or an occurrence and the following
    /// ones (see [`EditScope`]). Answered with [`Response::EventSaved`]
    /// (`split` holds the new series of a `this-and-following` edit);
    /// `invalid_request` (`pim-issue-invalid-scope`) for `this` /
    /// `this-and-following` without `recurrence_id`.
    UpdateEvent {
        /// The event (series).
        event_id: EventId,
        /// Which occurrences.
        #[serde(default)]
        scope: EditScope,
        /// The occurrence (its recurrence id from [`EventInstance`]) for
        /// `this` / `this-and-following`.
        #[serde(default)]
        recurrence_id: Option<PimTime>,
        /// The new fields (of the series, the occurrence, or the new
        /// series).
        fields: Box<EventFields>,
        /// The new recurrence (scope `all` and `this-and-following`);
        /// ignored for `this`.
        #[serde(default)]
        recurrence: Option<Recurrence>,
    },
    /// Delete an event, one occurrence (an exception) or an occurrence and
    /// the following ones (the series ends before it). Answered with
    /// [`Response::PimDeleted`].
    DeleteEvent {
        /// The event (series).
        event_id: EventId,
        /// Which occurrences.
        #[serde(default)]
        scope: EditScope,
        /// The occurrence for `this` / `this-and-following`.
        #[serde(default)]
        recurrence_id: Option<PimTime>,
    },
    /// Move an event (with all occurrences) to another calendar. Answered
    /// with [`Response::EventSaved`].
    MoveEvent {
        /// The event.
        event_id: EventId,
        /// The target calendar.
        calendar_id: CalendarId,
    },
    /// "Create event from email": unsaved fields pre-filled from a message
    /// (subject as title, sender and recipients as attendees, a link to the
    /// message, an attached `.ics` when there is one). Save with
    /// [`Request::CreateEvent`]. Answered with [`Response::EventSuggested`].
    EventFromMessage {
        /// The message.
        message_id: MessageId,
    },
    /// Free stretches of at least `duration_minutes` in `[from, to)`,
    /// counting busy (opaque, not cancelled) occurrences of the calendars.
    /// Answered with [`Response::FreeTime`].
    FindFreeTime {
        /// Range start (RFC 3339).
        from: String,
        /// Range end (RFC 3339).
        to: String,
        /// Shortest slot, minutes.
        duration_minutes: u32,
        /// Calendars that count; `null` = every visible one.
        #[serde(default)]
        calendar_ids: Option<Vec<CalendarId>>,
        /// IANA zone for all-day and floating events.
        tz: String,
    },

    // --- Address books, contacts and groups (M3a) ---
    /// Answered with [`Response::AddressBooks`].
    ListAddressBooks {
        /// One source; `null` = all.
        #[serde(default)]
        source_id: Option<PimSourceId>,
    },
    /// Create an address book (local, or extended `MKCOL` on CardDAV).
    /// Answered with [`Response::AddressBookSaved`].
    CreateAddressBook {
        /// The source.
        source_id: PimSourceId,
        /// Its settings.
        settings: AddressBookSettings,
    },
    /// Change an address book's settings. Answered with
    /// [`Response::AddressBookSaved`].
    UpdateAddressBook {
        /// The address book.
        address_book_id: AddressBookId,
        /// The new settings.
        settings: AddressBookSettings,
    },
    /// Delete an address book with its contacts (on the server too).
    /// Answered with [`Response::PimDeleted`]; `forbidden` for the
    /// Collected book.
    DeleteAddressBook {
        /// The address book.
        address_book_id: AddressBookId,
    },
    /// A page of contacts by sort name (linked contacts once, under the
    /// first of them). Answered with [`Response::Contacts`].
    ListContacts {
        /// Address books; `null` = every visible one.
        #[serde(default)]
        address_book_ids: Option<Vec<AddressBookId>>,
        /// Only members of this group.
        #[serde(default)]
        group_id: Option<ContactGroupId>,
        /// Index of the first contact.
        offset: u32,
        /// At most this many.
        limit: u32,
    },
    /// One contact. Answered with [`Response::PimContact`].
    GetContact {
        /// The contact.
        contact_id: ContactId,
    },
    /// Create a contact. Answered with [`Response::ContactSaved`];
    /// `invalid_request` with a `pim-issue-*` id.
    CreateContact {
        /// Its address book.
        address_book_id: AddressBookId,
        /// The fields.
        fields: Box<ContactFields>,
    },
    /// Change a contact. Answered with [`Response::ContactSaved`].
    UpdateContact {
        /// The contact.
        contact_id: ContactId,
        /// The new fields.
        fields: Box<ContactFields>,
    },
    /// Delete contacts. Answered with [`Response::PimDeleted`].
    DeleteContacts {
        /// The contacts.
        contact_ids: Vec<ContactId>,
    },
    /// Move a contact to another address book. Answered with
    /// [`Response::ContactSaved`].
    MoveContact {
        /// The contact.
        contact_id: ContactId,
        /// The target address book.
        address_book_id: AddressBookId,
    },
    /// Contacts whose name, email, phone or organization contains `query`
    /// (case-insensitive), best first. Answered with
    /// [`Response::ContactMatches`].
    SearchContacts {
        /// What to look for.
        query: String,
        /// Address books; `null` = all (hidden ones too).
        #[serde(default)]
        address_book_ids: Option<Vec<AddressBookId>>,
        /// At most this many.
        limit: u32,
    },
    /// Contacts that look like the same person (shared email or phone,
    /// same name), across books. Answered with
    /// [`Response::ContactDuplicates`].
    FindDuplicateContacts {
        /// Address books; `null` = all.
        #[serde(default)]
        address_book_ids: Option<Vec<AddressBookId>>,
    },
    /// Merge contacts into `primary_id` with the merged `fields` (chosen in
    /// the merge dialog): contacts in the primary's address book are folded
    /// into it and deleted, contacts in other books are linked to it and
    /// keep their cards. Answered with [`Response::ContactSaved`].
    MergeContacts {
        /// The contact that remains.
        primary_id: ContactId,
        /// The others (at least one).
        contact_ids: Vec<ContactId>,
        /// The merged fields for the primary.
        fields: Box<ContactFields>,
    },
    /// Undo a link from a merge: the contact shows on its own again.
    /// Answered with [`Response::Done`].
    UnlinkContact {
        /// The contact.
        contact_id: ContactId,
    },
    /// A contact's stored photo. Answered with [`Response::ContactPhoto`];
    /// `not_found` when it has none.
    GetContactPhoto {
        /// The contact.
        contact_id: ContactId,
    },
    /// Answered with [`Response::ContactGroups`] (by name).
    ListContactGroups {
        /// Address books; `null` = all.
        #[serde(default)]
        address_book_ids: Option<Vec<AddressBookId>>,
    },
    /// Create a group. Answered with [`Response::ContactGroupSaved`].
    CreateContactGroup {
        /// Its address book (members must be in it).
        address_book_id: AddressBookId,
        /// Name.
        name: String,
        /// Member contacts.
        member_ids: Vec<ContactId>,
        /// Members that are only addresses.
        #[serde(default)]
        member_emails: Vec<String>,
    },
    /// Rename a group or change its members. Answered with
    /// [`Response::ContactGroupSaved`].
    UpdateContactGroup {
        /// The group.
        group_id: ContactGroupId,
        /// Name.
        name: String,
        /// Member contacts.
        member_ids: Vec<ContactId>,
        /// Members that are only addresses.
        #[serde(default)]
        member_emails: Vec<String>,
    },
    /// Delete a group (its members stay). Answered with
    /// [`Response::PimDeleted`].
    DeleteContactGroup {
        /// The group.
        group_id: ContactGroupId,
    },

    // --- Tasks (M3a) ---
    /// Tasks of the lists, parents before their subtasks, by due date /
    /// manual order. Answered with [`Response::Tasks`].
    ListTasks {
        /// Task lists; `null` = every visible one.
        #[serde(default)]
        calendar_ids: Option<Vec<CalendarId>>,
        /// Include completed and cancelled tasks.
        #[serde(default)]
        include_completed: bool,
    },
    /// One task. Answered with [`Response::PimTask`].
    GetTask {
        /// The task.
        task_id: TaskId,
    },
    /// Create a task. Answered with [`Response::TaskSaved`];
    /// `invalid_request` with a `pim-issue-*` id (`pim-issue-task-cycle`).
    CreateTask {
        /// Its task list.
        calendar_id: CalendarId,
        /// The fields.
        fields: Box<TaskFields>,
        /// Recurrence, `null` = single.
        #[serde(default)]
        recurrence: Option<Recurrence>,
    },
    /// Change a task. Answered with [`Response::TaskSaved`].
    UpdateTask {
        /// The task.
        task_id: TaskId,
        /// The new fields.
        fields: Box<TaskFields>,
        /// The new recurrence.
        #[serde(default)]
        recurrence: Option<Recurrence>,
    },
    /// Mark a task done (recurring: the current occurrence, the task moves
    /// to the next) or open again. Answered with [`Response::TaskSaved`].
    CompleteTask {
        /// The task.
        task_id: TaskId,
        /// Done or open.
        completed: bool,
    },
    /// Move a task (with its subtasks) to another list. Answered with
    /// [`Response::TaskSaved`].
    MoveTask {
        /// The task.
        task_id: TaskId,
        /// The target list.
        calendar_id: CalendarId,
    },
    /// Delete a task. Answered with [`Response::PimDeleted`].
    DeleteTask {
        /// The task.
        task_id: TaskId,
        /// Delete its subtasks too (else they move up a level).
        #[serde(default)]
        with_subtasks: bool,
    },
    /// "Create task from email": unsaved fields pre-filled from a message
    /// (subject as title, a link to the message). Save with
    /// [`Request::CreateTask`]. Answered with [`Response::TaskSuggested`].
    TaskFromMessage {
        /// The message.
        message_id: MessageId,
    },

    // --- PIM search, import, conflicts (M3a) ---
    /// Full-text search over events, tasks and contacts. Answered with
    /// [`Response::PimSearchResults`].
    SearchPim {
        /// Free text.
        query: String,
        /// What to search; empty = everything.
        #[serde(default)]
        kinds: Vec<PimItemKind>,
        /// At most this many hits.
        limit: u32,
    },
    /// Read `.ics` / `.vcf` data and say what importing it would do.
    /// Answered with [`Response::PimImportPreview`]; `invalid_request` when
    /// it is neither, `too_large`.
    PreviewPimImport {
        /// The data.
        source: PimImportSource,
    },
    /// Import `.ics` data into a calendar (events) / task list (tasks) and
    /// `.vcf` data into an address book. Answered with
    /// [`Response::PimImported`].
    ImportPim {
        /// The data.
        source: PimImportSource,
        /// Target for events and tasks (a calendar with the right
        /// components); `null` = the default calendar.
        #[serde(default)]
        calendar_id: Option<CalendarId>,
        /// Target for contacts; `null` = the default address book.
        #[serde(default)]
        address_book_id: Option<AddressBookId>,
    },
    /// Resolve a sync conflict ([`crate::pim::PimItemSync::Conflict`]).
    /// Answered with [`Response::Done`].
    ResolvePimConflict {
        /// The item.
        item: PimItemRef,
        /// Which version wins.
        keep: ConflictChoice,
    },

    // --- Reminders (M3a) ---
    /// Reminders that fired or are snoozed, plus pending ones up to
    /// `until`. Answered with [`Response::Reminders`].
    ListReminders {
        /// Also pending reminders firing before this instant (RFC 3339).
        #[serde(default)]
        until: Option<String>,
    },
    /// Snooze reminders until a time. Answered with [`Response::Done`],
    /// followed by [`Event::RemindersChanged`].
    SnoozeReminders {
        /// The reminders.
        reminder_ids: Vec<ReminderId>,
        /// When they come back (RFC 3339, in the future).
        until: String,
    },
    /// Dismiss reminders (acknowledged on the server where supported).
    /// Answered with [`Response::Done`], followed by
    /// [`Event::RemindersChanged`].
    DismissReminders {
        /// The reminders.
        reminder_ids: Vec<ReminderId>,
    },

    // --- Invitations and scheduling (M3b WP80, ADR 0065) ---
    /// The invitation card of a message (also on
    /// [`MessageBody::invitation`]). Answered with
    /// [`Response::Invitation`]; `invitation` is `null` for a message
    /// without an iTIP part.
    GetInvitation {
        /// The message.
        message_id: MessageId,
    },
    /// Accept, tentatively accept or decline: sets the user's
    /// `PARTSTAT` in the calendar copy (adding the event when it is new)
    /// and sends a `REPLY` to the organizer (iMIP, or nothing extra when
    /// the server schedules). Answered with [`Response::Scheduled`];
    /// `invalid_request` with an `itip-issue-*` id when the invitation is
    /// unusable or the user is not invited.
    RespondToInvitation {
        /// The invitation message, or the event in a calendar.
        target: InvitationTarget,
        /// The answer.
        response: InvitationResponse,
        /// A comment for the organizer (`COMMENT`).
        #[serde(default)]
        comment: Option<String>,
        /// Send the reply (`false` = only update the calendar, "don't send
        /// a response").
        #[serde(default = "default_true")]
        send: bool,
        /// Where a new event goes; `null` = the scheduling settings'
        /// calendar, else the default calendar.
        #[serde(default)]
        calendar_id: Option<CalendarId>,
    },
    /// Propose another time (`COUNTER` to the organizer). The user's copy
    /// stays as it is. Answered with [`Response::Scheduled`].
    ProposeNewTime {
        /// The invitation message, or the event in a calendar.
        target: InvitationTarget,
        /// Proposed start.
        start: PimTime,
        /// Proposed end.
        end: PimTime,
        /// A comment.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Ask the organizer for the current version (`REFRESH`). Answered with
    /// [`Response::Scheduled`].
    RefreshInvitation {
        /// The invitation message, or the event in a calendar.
        target: InvitationTarget,
    },
    /// Forward an invitation to other people (RFC 5546 §2.1.5: the
    /// original `REQUEST`, unchanged, as iMIP). Answered with
    /// [`Response::Scheduled`].
    ForwardInvitation {
        /// The invitation message, or the event in a calendar.
        target: InvitationTarget,
        /// Recipients.
        to: Vec<Address>,
        /// A note in the mail's text part.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Process a received iTIP message without answering: add a `PUBLISH`
    /// / `REQUEST` / `ADD`, apply an update or `CANCEL`, record a `REPLY`'s
    /// answer, answer a `REFRESH` with the current version, or note a
    /// `DECLINECOUNTER`. Answered with [`Response::Scheduled`];
    /// `conflict` when the message is outdated.
    ApplyInvitation {
        /// The message.
        message_id: MessageId,
        /// Where a new event goes; `null` = as for
        /// [`Request::RespondToInvitation`].
        #[serde(default)]
        calendar_id: Option<CalendarId>,
    },
    /// Organizer: send the event to its attendees (`REQUEST`, new or
    /// updated; `SEQUENCE` goes up when the change is significant). Save
    /// the event first with [`Request::CreateEvent`] /
    /// [`Request::UpdateEvent`]. Answered with [`Response::Scheduled`];
    /// `invalid_request` when the user is not the organizer.
    SendInvitations {
        /// The event.
        event_id: EventId,
        /// One occurrence (an override); `null` = the series.
        #[serde(default)]
        recurrence_id: Option<PimTime>,
        /// Only these attendees (new ones); empty = everyone.
        #[serde(default)]
        attendees: Vec<String>,
        /// A note in the mail's text part.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Organizer: cancel the meeting, one occurrence, or some attendees
    /// (`CANCEL`). Answered with [`Response::Scheduled`].
    CancelMeeting {
        /// The event.
        event_id: EventId,
        /// What is cancelled.
        #[serde(default)]
        scope: CancelScope,
        /// The occurrence, for [`CancelScope::Occurrence`].
        #[serde(default)]
        recurrence_id: Option<PimTime>,
        /// The attendees, for [`CancelScope::Attendees`].
        #[serde(default)]
        attendees: Vec<String>,
        /// A note in the mail's text part.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Organizer: accept a `COUNTER` (the event moves to the proposal and
    /// everyone gets the update) or decline it (`DECLINECOUNTER` to the
    /// proposer). Answered with [`Response::Scheduled`].
    RespondToCounter {
        /// The `COUNTER` message.
        message_id: MessageId,
        /// Accept the proposal.
        accept: bool,
        /// A note.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Busy periods of attendees in a range. Answered with
    /// [`Response::FreeBusy`]; `invalid_request` with a `freebusy-issue-*`
    /// id.
    QueryFreeBusy {
        /// Attendee addresses (at most
        /// [`crate::scheduling::MAX_FREE_BUSY_ATTENDEES`]).
        attendees: Vec<String>,
        /// Range start (RFC 3339 UTC).
        from: String,
        /// Range end (RFC 3339 UTC; at most
        /// [`crate::scheduling::MAX_FREE_BUSY_DAYS`] after `from`).
        to: String,
        /// The organizer identity asking (picks the CalDAV source / account
        /// to ask); `null` = the default identity.
        #[serde(default)]
        identity_id: Option<IdentityId>,
        /// Leave this event out of the user's own busy time (editing it).
        #[serde(default)]
        ignore_event_id: Option<EventId>,
    },
    /// Answered with [`Response::SchedulingSettings`].
    GetSchedulingSettings,
    /// Replace the scheduling settings. Answered with
    /// [`Response::SchedulingSettings`], followed by
    /// [`Event::SchedulingSettingsChanged`].
    SetSchedulingSettings {
        /// The settings.
        settings: SchedulingSettings,
    },

    // --- Gmail labels (M3b) ---
    /// Labels of a Gmail account (system and user labels, with counts).
    /// Answered with [`Response::GmailLabels`]; `invalid_request` for
    /// accounts of other kinds.
    ListGmailLabels {
        /// The Gmail account.
        account_id: AccountId,
    },
    /// Create a user label (queued; it has a `local-…` id until the
    /// server assigned one). Answered with [`Response::GmailLabelSaved`],
    /// followed by [`Event::GmailLabelsChanged`]; `invalid_request` with a
    /// `gmail-label-issue-*` id.
    CreateGmailLabel {
        /// The Gmail account.
        account_id: AccountId,
        /// The settings.
        settings: GmailLabelSettings,
    },
    /// Rename / recolour a user label (renaming renames its tag on every
    /// message). Answered with [`Response::GmailLabelSaved`];
    /// `invalid_request` for system labels.
    UpdateGmailLabel {
        /// The Gmail account.
        account_id: AccountId,
        /// The label.
        label_id: GmailLabelId,
        /// The new settings.
        settings: GmailLabelSettings,
    },
    /// Delete a user label (its tag goes from every message; the messages
    /// stay). Answered with [`Response::Done`]; `invalid_request` for
    /// system labels.
    DeleteGmailLabel {
        /// The Gmail account.
        account_id: AccountId,
        /// The label.
        label_id: GmailLabelId,
    },

    // --- Exchange accounts (M3c WP90, ADR 0072) ---
    // Every request below answers `invalid_request` for accounts that are
    // not Exchange accounts.
    /// The Exchange-specific state of an account (engine, server version,
    /// push, throttling, warnings). Answered with
    /// [`Response::ExchangeStatus`].
    GetExchangeStatus {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// The shared and delegate mailboxes of an account. Answered with
    /// [`Response::ExchangeMailboxes`].
    ListExchangeMailboxes {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// Open another mailbox under the account (its folder tree, and its
    /// calendar / contacts / tasks as chosen); the engine checks access
    /// on the next sync. Answered with [`Response::ExchangeMailboxSaved`]
    /// (state `pending`), followed by [`Event::ExchangeChanged`];
    /// `invalid_request` with an `exchange-mailbox-issue-*` id.
    AddExchangeMailbox {
        /// The Exchange account.
        account_id: AccountId,
        /// The mailbox and what to show.
        settings: ExchangeMailboxSettings,
    },
    /// Change what of a mailbox is shown (`settings.address` names it).
    /// Answered with [`Response::ExchangeMailboxSaved`].
    UpdateExchangeMailbox {
        /// The Exchange account.
        account_id: AccountId,
        /// The new settings.
        settings: ExchangeMailboxSettings,
    },
    /// Stop showing a mailbox (its local folders and collections go; an
    /// automapped one is hidden, not deleted). Answered with
    /// [`Response::Done`].
    RemoveExchangeMailbox {
        /// The Exchange account.
        account_id: AccountId,
        /// The mailbox's address.
        address: String,
    },
    /// The account's master category list (with message counts of their
    /// tags). Answered with [`Response::ExchangeCategories`].
    ListExchangeCategories {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// Add a category (queued; it has a `local-…` id until the server
    /// assigned one). Answered with [`Response::ExchangeCategorySaved`],
    /// followed by [`Event::ExchangeChanged`]; `invalid_request` with an
    /// `exchange-category-issue-*` id.
    CreateExchangeCategory {
        /// The Exchange account.
        account_id: AccountId,
        /// Name and colour.
        settings: ExchangeCategorySettings,
    },
    /// Rename / recolour a category (renaming renames its tag on every
    /// message, through the mail queue). Answered with
    /// [`Response::ExchangeCategorySaved`].
    UpdateExchangeCategory {
        /// The Exchange account.
        account_id: AccountId,
        /// The category.
        category_id: ExchangeCategoryId,
        /// The new settings.
        settings: ExchangeCategorySettings,
    },
    /// Remove a category from the master list (its tag stays on messages,
    /// like Outlook). Answered with [`Response::Queued`].
    DeleteExchangeCategory {
        /// The Exchange account.
        account_id: AccountId,
        /// The category.
        category_id: ExchangeCategoryId,
    },
    /// Move inbox messages between Focused and Other (applied locally,
    /// queued for the server); `always` also adds an override for their
    /// senders. Answered with [`Response::Queued`]; `invalid_request` for
    /// messages of other accounts.
    ClassifyFocused {
        /// The messages.
        message_ids: Vec<MessageId>,
        /// Where to.
        classification: InferenceClassification,
        /// Also for future mail from the same senders.
        #[serde(default)]
        always: bool,
    },
    /// The automatic replies (out of office). Answered with
    /// [`Response::AutomaticReplies`] (no op ids).
    GetAutomaticReplies {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// Replace the automatic replies (queued). Answered with
    /// [`Response::AutomaticReplies`] (`sync` = `pending`), followed by
    /// [`Event::ExchangeChanged`]; `invalid_request` with an
    /// `exchange-oof-issue-*` id.
    SetAutomaticReplies {
        /// The Exchange account.
        account_id: AccountId,
        /// The new settings.
        settings: AutomaticRepliesSettings,
    },
    /// Where each Nail rule runs for the account and every inbox rule on
    /// the server. Answered with [`Response::ExchangeRules`].
    GetExchangeRules {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// Write the placed rules now and re-read the server's (e.g. after a
    /// failure). Answered with [`Response::ExchangeRules`] (usually
    /// `syncing`); the outcome arrives as [`Event::ExchangeChanged`].
    SyncExchangeRules {
        /// The Exchange account.
        account_id: AccountId,
    },
    /// Switch an inbox rule on or off (queued). Answered with
    /// [`Response::Queued`].
    SetExchangeInboxRuleEnabled {
        /// The Exchange account.
        account_id: AccountId,
        /// The server rule.
        server_rule_id: String,
        /// On or off.
        enabled: bool,
    },
    /// Delete an inbox rule (queued; a rule Nail placed is placed again on
    /// the next sync unless its Nail rule changes). Answered with
    /// [`Response::Queued`].
    DeleteExchangeInboxRule {
        /// The Exchange account.
        account_id: AccountId,
        /// The server rule.
        server_rule_id: String,
    },
    /// The organization's room lists (cached; `refresh` asks the server).
    /// Answered with [`Response::RoomLists`]; `forbidden` with
    /// `exchange-warning-rooms-need-consent` when the tenant did not grant
    /// the permission.
    ListRoomLists {
        /// The Exchange account.
        account_id: AccountId,
        /// Ask the server instead of the cache.
        #[serde(default)]
        refresh: bool,
    },
    /// Rooms (of a room list, or all) with their availability in a range
    /// (free/busy through the account). Answered with
    /// [`Response::Rooms`]; `invalid_request` with a `freebusy-issue-*`
    /// id for a bad range.
    FindRooms {
        /// The Exchange account.
        account_id: AccountId,
        /// The room list's address; `null` = every room.
        #[serde(default)]
        room_list: Option<String>,
        /// Range start (RFC 3339 UTC).
        from: String,
        /// Range end (RFC 3339 UTC).
        to: String,
        /// Only rooms with at least this many seats.
        #[serde(default)]
        min_capacity: Option<u32>,
        /// Ask the server for the room list instead of the cache.
        #[serde(default)]
        refresh: bool,
    },

    // --- Confirmations and action policies (M4a WP100, ADR 0079) ---
    /// Confirmations of writes by MCP clients and plug-ins. Answered with
    /// [`Response::Confirmations`].
    ListConfirmations {
        /// Pending (oldest first), decided (newest first) or all.
        #[serde(default)]
        filter: ConfirmationFilter,
        /// Only this client's / plug-in's; `null` = everyone's.
        #[serde(default)]
        actor: Option<ActionActor>,
        /// Only confirmations with a smaller id (paging the history);
        /// `null` = from the start.
        #[serde(default)]
        before: Option<ConfirmationId>,
        /// At most this many.
        limit: u32,
    },
    /// Approve or deny a pending confirmation; with `always_allow`, an
    /// approval also sets the actor's policy for the kind to `allow`.
    /// Answered with [`Response::ConfirmationDecided`], followed by
    /// [`Event::ConfirmationResolved`] (and [`Event::ActionPoliciesChanged`]);
    /// `not_found`, `conflict` when it is no longer pending,
    /// `invalid_request` with `action-policy-issue-destructive` for
    /// `always_allow` on a destructive kind.
    DecideConfirmation {
        /// The confirmation.
        confirmation_id: ConfirmationId,
        /// The answer.
        decision: ConfirmationDecision,
        /// Also allow this actor's actions of this kind from now on.
        #[serde(default)]
        always_allow: bool,
    },
    /// An actor's policy for every kind. Answered with
    /// [`Response::ActionPolicies`]; `not_found` for an unknown client or
    /// plug-in.
    ListActionPolicies {
        /// The MCP client or plug-in.
        actor: ActionActor,
    },
    /// Set (or, with `null`, reset to the default) an actor's policy for
    /// one kind. Answered with [`Response::ActionPolicies`], followed by
    /// [`Event::ActionPoliciesChanged`]; `invalid_request` with
    /// `action-policy-issue-destructive` for `allow` on a destructive kind.
    SetActionPolicy {
        /// The MCP client or plug-in.
        actor: ActionActor,
        /// The kind.
        kind: ActionKind,
        /// The new mode; `null` = the default.
        mode: Option<ActionPolicyMode>,
    },

    // --- MCP rate limits (M4a) ---
    /// Replace a client's rate limits. Answered with
    /// [`Response::McpClientSaved`]; `invalid_request` with an
    /// `mcp-rate-limit-issue-*` id.
    UpdateMcpRateLimits {
        /// The client.
        client_id: McpClientId,
        /// The new limits.
        limits: McpRateLimits,
    },

    // --- Plug-ins: hooks, extensions, jobs (M4a) ---
    /// A plug-in's jobs, hooks and CPU use. Answered with
    /// [`Response::PluginActivity`].
    GetPluginActivity {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// Run a plug-in's job now (trigger `manual`). Answered with
    /// [`Response::Done`] once the job started; `not_found` for an unknown
    /// job, `unavailable` when the plug-in is not running, `conflict`
    /// while the job runs.
    RunPluginJob {
        /// The plug-in.
        plugin_id: PluginId,
        /// The job's name.
        job: String,
    },
    /// The banners and badges the plug-ins' open hooks show with a message
    /// (the reader asks when it opens one; hooks that fail or time out
    /// contribute nothing). Answered with
    /// [`Response::MessagePluginAnnotations`].
    GetMessagePluginAnnotations {
        /// The message.
        message_id: MessageId,
        /// The UI's random id (see [`crate::plugins::runtime`]).
        #[serde(default)]
        client_id: Option<String>,
    },
    /// Render an attachment with a plug-in's preview renderer. Answered
    /// with [`Response::AttachmentPreview`]; `not_found`, `too_large` over
    /// the renderer's size, `unavailable` when the plug-in is not running,
    /// `invalid_request` when it does not render this type.
    RenderAttachmentPreview {
        /// The message.
        message_id: MessageId,
        /// The attachment's index.
        index: u32,
        /// `<plugin>.<renderer>`.
        renderer: String,
        /// The UI's random id.
        #[serde(default)]
        client_id: Option<String>,
    },
    /// Run a composer, attachment or link action of a plug-in. Answered
    /// with [`Response::Done`] once the plug-in took it; what it does
    /// arrives as events (composer edits, dialogs, notifications).
    /// `not_found` for an unknown action, `unavailable` when the plug-in
    /// is not running.
    RunPluginAction {
        /// `<plugin>.<action>`.
        action: String,
        /// Where it was invoked.
        target: PluginActionTarget,
        /// The UI's context.
        #[serde(default)]
        context: UiContext,
        /// The UI's random id.
        #[serde(default)]
        client_id: Option<String>,
    },
    /// "Send anyway": send a draft a before-send hook vetoed, with the
    /// options of the original [`Request::SendDraft`] (the other hooks'
    /// headers and content still apply). Answered with
    /// [`Response::SendQueued`]; `not_found` when the veto expired (send
    /// again).
    OverrideSendVeto {
        /// The veto ([`Response::SendVetoed`]).
        veto_id: SendVetoId,
    },

    // --- Desktop: installation, updates, tray, default apps, first run
    // (M4b WP110, ADR 0087; desktop only: `forbidden` remotely,
    // `unsupported` without the desktop shell unless noted) ---
    /// Facts about this installation (version, installer, scope,
    /// architecture, update policy, MAPI component). Answered with
    /// [`Response::Installation`].
    GetInstallation,
    /// The updater's settings and state. Answered with
    /// [`Response::UpdateStatus`].
    GetUpdateStatus,
    /// Store the update settings. Answered with [`Response::UpdateStatus`],
    /// followed by [`Event::UpdateStatusChanged`]; `forbidden`
    /// (`update-channel-locked`) for another channel than the one policy
    /// forces.
    SetUpdateSettings {
        /// The new settings.
        settings: UpdateSettings,
    },
    /// Check for an update now. Answered with [`Response::UpdateStatus`]
    /// (state `checking`); progress arrives as
    /// [`Event::UpdateStatusChanged`]. `forbidden` when updates are
    /// disabled, `conflict` while a check, download or install runs.
    CheckForUpdates,
    /// Download the available update (when `auto_download` is off).
    /// Answered with [`Response::UpdateStatus`]; `conflict` without an
    /// available update, `forbidden` when updates are disabled.
    DownloadUpdate,
    /// "Restart to install": quit, run the downloaded update's installer
    /// and restart. Answered with [`Response::Done`] just before Nail
    /// quits; `conflict` unless an update is ready, `forbidden` when
    /// updates are disabled.
    InstallUpdate,
    /// The tray / startup / dev-endpoint settings. Answered with
    /// [`Response::DesktopSettings`].
    GetDesktopSettings,
    /// Store and apply them (Run key, dev endpoint). Answered with
    /// [`Response::DesktopSettings`], followed by
    /// [`Event::DesktopSettingsChanged`].
    SetDesktopSettings {
        /// The new settings.
        settings: DesktopSettings,
    },
    /// Default-app registration and status. Answered with
    /// [`Response::DefaultApps`].
    GetDefaultApps,
    /// Open Settings → Default apps on Nail's page. Answered with
    /// [`Response::Done`].
    OpenDefaultAppsSettings,
    /// Whether the first-run screen was completed. Served from the store
    /// in every mode (also `nail serve`). Answered with
    /// [`Response::FirstRun`].
    GetFirstRun,
    /// Mark the first-run screen completed (finished or skipped). Answered
    /// with [`Response::FirstRun`].
    CompleteFirstRun,

    // --- Import, export, backup, restore (M4c WP120, ADR 0094; every
    // request naming a path is desktop-only: `forbidden` remotely) ---
    /// Import sources found on this computer (Thunderbird profiles).
    /// Answered with [`Response::ImportSources`].
    FindImportSources,
    /// Read a source and say what importing it would bring in; writes
    /// nothing. Answered with [`Response::ImportPreview`]; `not_found` for a
    /// missing path, `invalid_request` when it is not that format.
    PreviewImport {
        /// The source.
        source: ImportSource,
    },
    /// Start importing. Answered with [`Response::PortabilityJob`] (state
    /// `running`); progress as [`Event::PortabilityJobProgress`], the end as
    /// [`Event::PortabilityJobFinished`].
    StartImport {
        /// The source.
        source: ImportSource,
        /// How.
        #[serde(default)]
        options: ImportOptions,
    },
    /// Start exporting. Answered with [`Response::PortabilityJob`];
    /// `invalid_request` when the format cannot write the source,
    /// `conflict` when the target exists and `overwrite` is off.
    StartExport {
        /// What.
        source: ExportSource,
        /// The format.
        format: ExportFormat,
        /// The target file (mbox, ICS, vCard with one source) or folder.
        path: String,
        /// How.
        #[serde(default)]
        options: ExportOptions,
    },
    /// Start writing a backup archive. Answered with
    /// [`Response::PortabilityJob`]; `invalid_request` with a
    /// `backup-issue-*` text for bad contents or a short passphrase.
    CreateBackup {
        /// The archive to write (`.nailbackup`).
        path: String,
        /// What goes in.
        #[serde(default)]
        contents: BackupContents,
        /// The passphrase (never stored or logged).
        passphrase: String,
        /// Post-M4c: replace an existing file (atomically: written under a
        /// temporary name, then renamed over it). Only set when the user
        /// confirmed replacing it in the Save dialog; otherwise an existing
        /// file is `conflict` (`portability-error-target-exists`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        overwrite: bool,
    },
    /// Read an archive's manifest. Answered with
    /// [`Response::BackupManifest`]; `auth_failed`
    /// (`backup-wrong-passphrase`) for a wrong passphrase.
    InspectBackup {
        /// The archive.
        path: String,
        /// Its passphrase.
        passphrase: String,
    },
    /// Read an archive completely and check every entry. Answered with
    /// [`Response::PortabilityJob`] (kind `verify-backup`).
    VerifyBackup {
        /// The archive.
        path: String,
        /// Its passphrase.
        passphrase: String,
    },
    /// The dry run of a restore: what restoring would do; writes nothing.
    /// Answered with [`Response::RestorePlan`].
    PreviewRestore {
        /// The archive.
        path: String,
        /// Its passphrase.
        passphrase: String,
        /// The items to restore; `null` = everything in the archive.
        #[serde(default)]
        contents: Option<BackupContents>,
    },
    /// Restore from an archive. Answered with
    /// [`Response::PortabilityJob`] (kind `restore`).
    StartRestore {
        /// The archive.
        path: String,
        /// Its passphrase.
        passphrase: String,
        /// The items to restore; `null` = everything in the archive.
        #[serde(default)]
        contents: Option<BackupContents>,
    },
    /// Running and recently finished jobs. Answered with
    /// [`Response::PortabilityJobs`].
    ListPortabilityJobs,
    /// One job. Answered with [`Response::PortabilityJob`]; `not_found`
    /// when unknown or forgotten.
    GetPortabilityJob {
        /// The job.
        job_id: JobId,
    },
    /// Cancel a running job (what was written stays; the report says
    /// what). Answered with [`Response::PortabilityJob`].
    CancelPortabilityJob {
        /// The job.
        job_id: JobId,
    },

    // --- Vault recovery key (M4c; desktop only) ---
    /// Create a recovery key for the data key (replacing an older one).
    /// Answered with [`Response::RecoveryKeyCreated`] — the only time the
    /// key is shown; `conflict` (`vault-not-encrypted`) unless the vault is
    /// encrypted.
    CreateRecoveryKey {
        /// Also write a recovery kit text file here.
        #[serde(default)]
        save_to: Option<String>,
        /// Post-M4c: replace an existing kit file (atomically: written under a
        /// temporary name, then renamed over it). Only set when the user
        /// confirmed replacing it in the Save dialog; otherwise an existing
        /// file is `conflict` (`portability-error-target-exists`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        overwrite: bool,
    },
    /// Check a typed recovery key without changing anything. Answered with
    /// [`Response::RecoveryKeyVerified`].
    VerifyRecoveryKey {
        /// The key as typed.
        key: String,
    },
    /// Put the data key back into the keychain with the recovery key (after
    /// the keychain entry was lost). Answered with [`Response::VaultStatus`];
    /// `auth_failed` (`recovery-key-wrong`) for a wrong key,
    /// `invalid_request` with a `recovery-key-issue-*` text for a mistyped
    /// one.
    RestoreVaultKey {
        /// The key as typed.
        key: String,
    },
    /// Remove the recovery key. Answered with [`Response::VaultStatus`].
    RemoveRecoveryKey,

    // --- Smartcards (M4c; desktop only) ---
    /// The PKCS#11 modules and PIN cache. Answered with
    /// [`Response::SmartcardSettings`].
    GetSmartcardSettings,
    /// Store them. Answered with [`Response::SmartcardSettings`];
    /// `invalid_request` with a `smartcard-settings-issue-*` text.
    SetSmartcardSettings {
        /// The new settings.
        settings: SmartcardSettings,
    },
    /// The tokens of the enabled modules with their keys and certificates.
    /// Answered with [`Response::Smartcards`].
    ListSmartcards,
    /// Add a key on a token to the keyring (OpenPGP or S/MIME; the private
    /// key stays on the token). Answered with [`Response::Key`].
    UseSmartcardKey {
        /// The key.
        key: SmartcardKeyRef,
        /// OpenPGP or S/MIME.
        technology: CryptoTechnology,
    },
    /// Answer a [`Event::PinRequested`] prompt. Answered with
    /// [`Response::Done`]; `not_found` when the prompt is gone.
    AnswerPinPrompt {
        /// The prompt.
        prompt_id: PinPromptId,
        /// The PIN; `null` = cancel.
        #[serde(default)]
        pin: Option<String>,
    },

    // --- Command palette (M4c) ---
    /// The recently used palette actions. Answered with
    /// [`Response::RecentActions`].
    GetRecentActions,
    /// Record that an action ran from the palette. Answered with
    /// [`Response::RecentActions`]; `invalid_request` for a malformed id.
    RecordActionUsed {
        /// The action id.
        action_id: String,
    },
    /// Forget the recent actions. Answered with
    /// [`Response::RecentActions`].
    ClearRecentActions,

    // --- Printing (M4c) ---
    /// Record what to print. Answered with [`Response::PrintJob`];
    /// `invalid_request` with a `print-target-issue-*` text.
    CreatePrintJob {
        /// What.
        target: PrintTarget,
        /// How.
        #[serde(default)]
        options: PrintOptions,
    },
    /// Read a print job (the print view). Answered with
    /// [`Response::PrintJob`]; `not_found` when expired.
    GetPrintJob {
        /// The job.
        job_id: PrintJobId,
    },
    /// The print view is laid out (the desktop shell waits for this before
    /// writing a PDF). Answered with [`Response::Done`].
    PrintViewReady {
        /// The job.
        job_id: PrintJobId,
    },
    /// Write a print job as PDF (desktop only). Answered with
    /// [`Response::PdfSaved`]; `unsupported` without the desktop shell.
    SavePrintPdf {
        /// The job.
        job_id: PrintJobId,
        /// The file to write.
        path: String,
        /// Post-M4c: replace an existing file (atomically: written under a
        /// temporary name, then renamed over it). Only set when the user
        /// confirmed replacing it in the Save dialog; otherwise an existing
        /// file is `conflict` (`portability-error-target-exists`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        overwrite: bool,
    },

    // --- Diagnostics (M4c; desktop only) ---
    /// Exactly what a diagnostics bundle with these options would hold.
    /// Answered with [`Response::DiagnosticsPreview`].
    PreviewDiagnostics {
        /// What to include.
        #[serde(default)]
        options: DiagnosticsOptions,
    },
    /// Write the bundle (the same entries the preview showed). Answered
    /// with [`Response::DiagnosticsBundle`].
    CreateDiagnosticsBundle {
        /// What to include.
        #[serde(default)]
        options: DiagnosticsOptions,
        /// The zip to write.
        path: String,
        /// Post-M4c: replace an existing file (atomically: written under a
        /// temporary name, then renamed over it). Only set when the user
        /// confirmed replacing it in the Save dialog; otherwise an existing
        /// file is `conflict` (`portability-error-target-exists`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        overwrite: bool,
    },

    // --- File dialogs (M4c; desktop only) ---
    /// Show the system's file / folder dialog. Answered with
    /// [`Response::PathPicked`]; `unsupported` without the desktop shell
    /// (the UI then asks for a typed path).
    PickPath {
        /// What to ask for.
        pick: PathPick,
    },

    // --- Appearance (M5a, ADR 0102) ---
    /// The themes (built-in and user), the selection, the active theme and
    /// the OS light / dark state. Answered with [`Response::Themes`].
    ListThemes,
    /// Use one theme, whatever Windows does: writes `extends="<id>"` into
    /// the user's `theme.kdl` and switches Follow Windows off. Answered
    /// with [`Response::Themes`]; `invalid_request` for an unknown or
    /// unusable theme (`theme-problem-*`), `config` when `theme.kdl` is
    /// invalid. Sends [`Event::ThemeChanged`] and [`Event::ConfigChanged`].
    SetTheme {
        /// A usable theme id from [`Response::Themes`].
        theme_id: String,
    },
    /// Follow Windows and its light / dark pair, density and the font-size
    /// / radius / accent overrides: writes the `appearance` node and the
    /// override tokens of the user's `theme.kdl` (format preserving).
    /// Answered with [`Response::Themes`]; `invalid_request` for unusable
    /// pair themes or invalid overrides (`theme-overrides-issue-*`).
    SetThemePreferences {
        /// The new preferences (all of them).
        preferences: ThemePreferences,
    },
}

fn default_true() -> bool {
    true
}

/// The core's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Response {
    /// Answer to [`Request::Ping`].
    Pong {
        /// Version of the core (`CARGO_PKG_VERSION` of `nail-core`).
        version: String,
        /// [`PROTOCOL_VERSION`] spoken by the core.
        protocol: u32,
    },
    /// Answer to [`Request::GetConfig`].
    Config {
        /// The resolved theme.
        theme: Box<ResolvedTheme>,
        /// The resolved layout.
        layout: Box<ResolvedLayout>,
        /// UI language override from the settings as a BCP 47 tag (e.g.
        /// `"de"`), or `null` to follow the OS / browser language.
        locale: Option<String>,
    },
    /// Answer to [`Request::ListAccounts`].
    Accounts {
        /// All accounts, in display order.
        accounts: Vec<Account>,
    },
    /// Answer to [`Request::ListFolders`].
    Folders {
        /// The folders, parents before children.
        folders: Vec<Folder>,
    },
    /// Answer to [`Request::ListMessages`].
    Messages {
        /// The folder the page belongs to.
        folder_id: FolderId,
        /// Offset of the first message in `messages`.
        offset: u32,
        /// Total number of messages in the folder.
        total: u32,
        /// The page, newest first.
        messages: Vec<MessageSummary>,
    },
    /// Answer to [`Request::GetMessage`].
    Message {
        /// The parsed message.
        message: Box<MessageBody>,
    },
    /// Answer to [`Request::SyncNow`]: synchronization was started.
    SyncStarted,
    /// Success without data (e.g. [`Request::RemoveAccount`],
    /// [`Request::CancelOAuth`], [`Request::DeleteDraft`]).
    Done,
    /// Answer to [`Request::DiscoverAccount`].
    Discovered {
        /// The address that was looked up.
        email: String,
        /// Proposals, best first.
        candidates: Vec<DiscoveredAccount>,
        /// What Exchange Autodiscover added (a question for the user,
        /// automapped mailboxes); `null` when it found nothing (M3c).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        exchange: Option<Box<crate::exchange::ExchangeDiscoveryInfo>>,
    },
    /// Answer to [`Request::TestAccount`].
    AccountTested {
        /// Per-server results.
        result: Box<AccountTestResult>,
    },
    /// Answer to [`Request::StartOAuth`].
    OAuthStarted {
        /// The flow, for [`Event::OAuthFinished`] and
        /// [`crate::accounts::SecretValue::OAuth2`].
        flow_id: OAuthFlowId,
        /// The provider's sign-in page.
        authorize_url: String,
        /// Whether the core already opened it in the system browser.
        opened: bool,
    },
    /// Answer to [`Request::GetAccountsConfig`].
    AccountsConfig {
        /// The configuration.
        config: AccountsConfig,
    },
    /// Answer to [`Request::AddAccount`] and [`Request::UpdateAccount`].
    AccountSaved {
        /// The configuration as saved (with the assigned id).
        config: Box<AccountConfig>,
    },
    /// Answer to [`Request::ListIdentities`].
    Identities {
        /// The identities in account order, each account's default first.
        identities: Vec<Identity>,
    },
    /// Answer to [`Request::ChooseIdentity`].
    IdentityChosen {
        /// The identity; `null` when there is none (no accounts).
        identity_id: Option<IdentityId>,
    },
    /// Answer to [`Request::ListVirtualAccounts`].
    VirtualAccounts {
        /// User-defined virtual accounts in file order.
        virtual_accounts: Vec<VirtualAccountConfig>,
    },
    /// Answer to [`Request::AddVirtualAccount`] and
    /// [`Request::UpdateVirtualAccount`].
    VirtualAccountSaved {
        /// The configuration as saved (with the assigned id).
        config: Box<VirtualAccountConfig>,
    },
    /// Answer to [`Request::ListAccountStatuses`].
    AccountStatuses {
        /// One entry per real account.
        statuses: Vec<AccountStatus>,
    },
    /// Answer to message writes: applied locally and queued for the server.
    Queued {
        /// The queued operations (the core may split a request per account
        /// or message), for matching [`Event::OpStatus`].
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::ListThreads`].
    Threads {
        /// The folder the page belongs to.
        folder_id: FolderId,
        /// Offset of the first thread in `threads`.
        offset: u32,
        /// Total number of threads in the folder.
        total: u32,
        /// The page.
        threads: Vec<ThreadSummary>,
    },
    /// Answer to [`Request::GetThread`].
    Thread {
        /// The thread.
        thread: Box<ThreadSummary>,
        /// Its messages, oldest first.
        messages: Vec<MessageSummary>,
    },
    /// Answer to [`Request::Search`].
    SearchResults {
        /// Offset of the first hit in `hits`.
        offset: u32,
        /// Total number of hits.
        total: u32,
        /// The page of hits.
        hits: Vec<SearchHit>,
        /// The server-side part (M2c), absent when no server was asked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        server: Option<ServerSearch>,
    },
    /// Answer to [`Request::Search`] when the query does not parse.
    InvalidQuery {
        /// What is wrong and where.
        error: SearchError,
    },
    /// Answer to [`Request::CheckQuery`].
    QueryChecked {
        /// `null` when the query is valid.
        error: Option<SearchError>,
    },
    /// Answer to [`Request::SaveDraft`].
    DraftSaved {
        /// The draft's id (new or unchanged).
        draft_id: DraftId,
    },
    /// Answer to [`Request::GetDraft`].
    Draft {
        /// The draft.
        draft: Box<Draft>,
    },
    /// Answer to [`Request::GetDraftAttachment`].
    AttachmentData {
        /// MIME type (from the draft's attachment entry).
        mime: String,
        /// The content, base64 (standard alphabet, padded).
        data: String,
    },
    /// Answer to [`Request::ListDrafts`].
    Drafts {
        /// The drafts, each with its id.
        drafts: Vec<Draft>,
    },
    /// Answer to [`Request::SendDraft`].
    SendQueued {
        /// The new outbox entry.
        entry: Box<OutboxEntry>,
    },
    /// Answer to [`Request::CancelSend`].
    SendCancelled {
        /// The restored draft.
        draft_id: DraftId,
    },
    /// Answer to [`Request::ListOutbox`].
    Outbox {
        /// Entries, earliest send time first.
        entries: Vec<OutboxEntry>,
    },
    /// Answer to [`Request::RenderPreview`].
    PreviewRendered {
        /// The HTML as it would be sent.
        html: String,
    },
    /// Answer to [`Request::ConvertDraft`].
    DraftConverted {
        /// The draft in the new mode.
        draft: Box<Draft>,
        /// Whether formatting was dropped (the UI warns before switching).
        lossy: bool,
    },
    /// Answer to [`Request::UploadAttachment`].
    AttachmentUploaded {
        /// The attachment to add to the draft.
        attachment: DraftAttachment,
    },
    /// Answer to [`Request::SuggestAddresses`].
    AddressSuggestions {
        /// Best first.
        addresses: Vec<Address>,
    },
    /// Answer to [`Request::ListRules`].
    Rules {
        /// The rules in file order.
        rules: Vec<Rule>,
    },
    /// Answer to [`Request::SaveRule`].
    RuleSaved {
        /// The rule as saved (with its id and source location).
        rule: Box<Rule>,
    },
    /// Answer to [`Request::TestRule`].
    RuleTested {
        /// Messages examined.
        scanned: u32,
        /// Matches, up to the limit, newest first.
        matches: Vec<RuleMatch>,
    },
    /// Answer to [`Request::RunRule`].
    RuleRan {
        /// The run (usually still `running`).
        run: Box<RuleRun>,
    },
    /// Answer to [`Request::ListRuleRuns`].
    RuleRuns {
        /// Newest first.
        runs: Vec<RuleRun>,
    },
    /// Answer to [`Request::RuleToText`].
    RuleText {
        /// The rule in `rules.kdl` syntax.
        text: String,
    },
    /// Answer to [`Request::RuleFromText`].
    RuleParsed {
        /// The rule, `null` when the text has errors.
        rule: Option<Box<Rule>>,
        /// Every error found (positions within `text`).
        errors: Vec<ConfigIssue>,
    },
    /// Answer to [`Request::ListMcpClients`].
    McpClients {
        /// Clients, oldest first.
        clients: Vec<McpClient>,
    },
    /// Answer to [`Request::CreateMcpClient`].
    McpClientCreated {
        /// The new client.
        client: Box<McpClient>,
        /// The bearer token. Shown once; the core keeps only its hash.
        token: String,
    },
    /// Answer to [`Request::UpdateMcpGrants`].
    McpClientSaved {
        /// The client as saved.
        client: Box<McpClient>,
    },
    /// Answer to [`Request::ListMcpAudit`].
    McpAudit {
        /// Newest first.
        entries: Vec<McpAuditEntry>,
    },
    /// Answer to [`Request::GetMcpServerInfo`].
    McpServer {
        /// State and endpoint.
        info: McpServerInfo,
    },
    /// Answer to [`Request::GetUiContext`].
    UiContext {
        /// The last reported context (empty when none was reported).
        context: UiContext,
    },
    /// Answer to [`Request::PreviewConfigMerge`].
    ConfigMergePreview {
        /// The comparison.
        preview: Box<ConfigMergePreview>,
    },
    /// Answer to [`Request::ResolveConfigMerge`].
    ConfigMergeResolved {
        /// The resolved theme (for a theme merge), else `null`.
        theme: Option<Box<ResolvedTheme>>,
        /// The resolved layout (for a layout merge), else `null`.
        layout: Option<Box<ResolvedLayout>>,
    },
    /// Answer to [`Request::ApplyConfigMerge`].
    ConfigMergeApplied {
        /// The backup of the previous file (for revert).
        backup: ConfigBackup,
    },
    /// Answer to [`Request::ListConfigBackups`].
    ConfigBackups {
        /// Newest first.
        backups: Vec<ConfigBackup>,
    },
    /// Answer to [`Request::ListSavedSearches`].
    SavedSearches {
        /// In display order.
        searches: Vec<SavedSearch>,
    },
    /// Answer to [`Request::SaveSavedSearch`].
    SavedSearchSaved {
        /// The saved search (with its id).
        search: SavedSearch,
    },
    /// Answer to [`Request::ListKeys`].
    Keys {
        /// Own keys first, then by primary address.
        keys: Vec<CryptoKey>,
    },
    /// Answer to [`Request::GetKey`].
    Key {
        /// The key.
        key: Box<CryptoKey>,
    },
    /// Answer to [`Request::ImportKeys`], [`Request::ImportPkcs12`],
    /// [`Request::ImportWindowsCertificate`] and
    /// [`Request::ImportAutocryptSetup`].
    KeysImported {
        /// What was imported.
        result: Box<KeyImportResult>,
    },
    /// Answer to [`Request::ExportPublicKey`].
    KeyExported {
        /// Suggested file name, e.g. `ada@example.org.asc`.
        filename: String,
        /// MIME type (`application/pgp-keys`, `application/pkix-cert`).
        mime: String,
        /// How `data` is encoded.
        encoding: KeyEncoding,
        /// The key.
        data: String,
    },
    /// Answer to [`Request::GenerateKey`].
    KeyGenerated {
        /// The new key.
        key: Box<CryptoKey>,
    },
    /// Answer to [`Request::SetKeyTrust`].
    KeySaved {
        /// The key with its new trust.
        key: Box<CryptoKey>,
    },
    /// Answer to [`Request::LookupKeys`].
    KeyLookup {
        /// The address looked up.
        email: String,
        /// Keys found, best first (none = not found).
        candidates: Vec<KeyLookupCandidate>,
        /// Sources that failed.
        failures: Vec<KeyLookupFailure>,
    },
    /// Answer to [`Request::ListWindowsCertificates`].
    WindowsCertificates {
        /// The certificates.
        certificates: Vec<WindowsCertificate>,
    },
    /// Answer to [`Request::CheckRecipientKeys`].
    RecipientKeys {
        /// The report.
        report: Box<RecipientKeysReport>,
    },
    /// Answer to [`Request::SendAutocryptSetup`].
    AutocryptSetupSent {
        /// The outbox entry of the setup message.
        outbox_id: OutboxId,
        /// The passcode to enter on the other device. Shown once.
        passcode: String,
    },
    /// Answer to [`Request::GetSecuritySettings`] and
    /// [`Request::UpdateSecuritySettings`].
    SecuritySettings {
        /// The settings.
        settings: SecuritySettings,
    },
    /// Answer to [`Request::SuggestAuthservId`].
    AuthservIdSuggestion {
        /// The account.
        account_id: AccountId,
        /// The suggested id; `None` when the recent mail shows none.
        authserv_id: Option<String>,
    },
    /// Answer to [`Request::GetVaultStatus`] and
    /// [`Request::SetVaultEncryption`].
    VaultStatus {
        /// The state.
        status: VaultStatus,
    },
    /// Answer to [`Request::GetRemoteUiSettings`],
    /// [`Request::UpdateRemoteUiSettings`] and
    /// [`Request::RegenerateRemoteCertificate`].
    RemoteUi {
        /// The settings (as saved).
        settings: RemoteUiSettings,
        /// The current state.
        status: RemoteUiStatus,
    },
    /// Answer to [`Request::ListRemoteUsers`].
    RemoteUsers {
        /// Users by name.
        users: Vec<RemoteUser>,
    },
    /// Answer to requests that change a user or their credentials.
    RemoteUserSaved {
        /// The user as saved.
        user: Box<RemoteUser>,
    },
    /// Answer to [`Request::StartTotpEnrollment`].
    TotpEnrollment {
        /// The secret to scan. Shown once.
        enrollment: TotpEnrollment,
    },
    /// Answer to [`Request::ListRemoteDevices`].
    RemoteDevices {
        /// Devices, most recently seen first.
        devices: Vec<RemoteDevice>,
    },
    /// Answer to [`Request::UpdateRemoteDevice`].
    RemoteDeviceSaved {
        /// The device as saved.
        device: Box<RemoteDevice>,
    },
    /// Answer to [`Request::ListRemoteSessions`].
    RemoteSessions {
        /// Newest first.
        sessions: Vec<RemoteSession>,
    },
    /// Answer to [`Request::CreatePairingCode`].
    PairingCodeCreated {
        /// The code's record.
        pairing: PairingCode,
        /// The code to enter in the browser (e.g. `K7QM-2WXR`). Shown once;
        /// the core keeps only its hash.
        code: String,
        /// Address to open in the browser, `null` when the remote UI is
        /// not running.
        url: Option<String>,
        /// Fingerprint of the served certificate, for the browser to
        /// compare; `null` without TLS.
        certificate_fingerprint: Option<String>,
    },
    /// Answer to [`Request::ListPairingCodes`].
    PairingCodes {
        /// Pending first, then newest first.
        codes: Vec<PairingCode>,
    },
    /// Answer to [`Request::ListRemoteAudit`].
    RemoteAudit {
        /// Newest first.
        entries: Vec<RemoteAuditEntry>,
    },
    /// Answer to [`Request::AllowRemoteContent`].
    RemoteContentAllowed {
        /// The stored entry.
        entry: RemoteContentAllow,
    },
    /// Answer to [`Request::ListRemoteContentAllows`].
    RemoteContentAllows {
        /// Entries by scope (message, sender, domain), newest first.
        entries: Vec<RemoteContentAllow>,
    },
    /// Answer to [`Request::TakeLaunchRequests`].
    LaunchRequests {
        /// Oldest first.
        requests: Vec<LaunchRequest>,
    },
    /// Answer to [`Request::ImportMessageFile`].
    MessageImported {
        /// The imported message.
        message: Box<MessageSummary>,
    },
    /// Answer to [`Request::GetSieveStatus`] and [`Request::SyncSieve`].
    SieveStatus {
        /// One entry per account asked for.
        accounts: Vec<SieveAccountStatus>,
    },
    /// Answer to [`Request::PreviewSieveScript`].
    SieveScript {
        /// The account.
        account_id: AccountId,
        /// The script text (RFC 5228).
        script: String,
        /// Where each rule runs.
        rules: Vec<RuleSieveStatus>,
    },
    /// Answer to [`Request::GetVacation`] and [`Request::UpdateVacation`].
    Vacation {
        /// The account.
        account_id: AccountId,
        /// Its vacation reply.
        settings: VacationSettings,
    },
    /// Answer to the junk requests.
    JunkStatus {
        /// The account's classifier.
        status: JunkStatus,
    },
    /// Answer to [`Request::ListSnoozed`].
    Snoozed {
        /// Soonest first.
        messages: Vec<SnoozedMessage>,
    },
    /// Answer to the notification-settings requests.
    NotificationSettings {
        /// The settings.
        settings: NotificationSettings,
    },
    /// Answer to [`Request::GetFolderSyncPolicy`] and
    /// [`Request::SetFolderSyncPolicy`].
    FolderSyncPolicy {
        /// The folder's policy.
        policy: FolderSyncPolicy,
    },
    /// Answer to [`Request::FetchMessageBodies`].
    BodiesFetched {
        /// Messages whose body is now stored.
        fetched: Vec<MessageId>,
        /// Messages that could not be fetched (offline, gone).
        failed: Vec<MessageId>,
    },
    /// Answer to the MCP-settings requests.
    McpSettings {
        /// The settings.
        settings: McpSettings,
        /// Where the server runs now.
        info: McpServerInfo,
    },
    /// Answer to [`Request::RuleFromMessage`].
    RuleSuggested {
        /// The unsaved rule (empty id).
        rule: Box<Rule>,
    },
    /// Answer to [`Request::ListPlugins`].
    Plugins {
        /// Installed plug-ins, by name.
        plugins: Vec<PluginInfo>,
    },
    /// Answer to the requests that change or read one plug-in.
    Plugin {
        /// The plug-in.
        plugin: Box<PluginInfo>,
    },
    /// Answer to [`Request::PreviewPluginInstall`].
    PluginInstallPreview {
        /// What installing would do.
        preview: Box<PluginInstallPreview>,
    },
    /// Answer to [`Request::ListPluginLogs`].
    PluginLogs {
        /// Log lines, oldest first.
        entries: Vec<PluginLogEntry>,
    },
    /// Answer to the plug-in settings requests.
    PluginSettings {
        /// The plug-in.
        plugin_id: PluginId,
        /// The schema (titles resolved).
        schema: Vec<PluginSettingDef>,
        /// Every setting's value (defaults filled in).
        values: BTreeMap<String, Value>,
    },
    /// Answer to [`Request::PreviewPluginPlacements`].
    PluginPlacements {
        /// The plug-in.
        plugin_id: PluginId,
        /// The user's `layout.kdl` with the placements added.
        incoming: String,
        /// The comparison for the merge dialog.
        preview: Box<ConfigMergePreview>,
    },
    /// Answer to [`Request::GetPluginPanel`].
    PluginPanel {
        /// `<plugin>.<panel>`.
        panel: String,
        /// Its content (empty while the plug-in has not rendered it).
        tree: Vec<PluginUiNode>,
    },
    /// Answer to [`Request::ListPimSources`].
    PimSources {
        /// The sources, the local one first, then in file order.
        sources: Vec<PimSource>,
    },
    /// Answer to [`Request::AddPimSource`] and [`Request::UpdatePimSource`].
    PimSourceSaved {
        /// The saved source.
        source: Box<PimSource>,
    },
    /// Answer to [`Request::TestPimSource`].
    PimSourceTested {
        /// The result.
        result: Box<PimSourceTestResult>,
    },
    /// Answer to [`Request::ListCalendars`].
    Calendars {
        /// The calendars and task lists.
        calendars: Vec<Calendar>,
    },
    /// Answer to [`Request::CreateCalendar`] and [`Request::UpdateCalendar`].
    CalendarSaved {
        /// The calendar.
        calendar: Box<Calendar>,
        /// The queued operations (empty for local-only changes).
        op_ids: Vec<OpId>,
    },
    /// Answer to the PIM delete requests.
    PimDeleted {
        /// The queued operations (empty for local items).
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::ListEventInstances`].
    EventInstances {
        /// Range start as asked.
        from: String,
        /// Range end as asked.
        to: String,
        /// The occurrences, by start.
        instances: Vec<EventInstance>,
    },
    /// Answer to [`Request::GetEvent`].
    PimEvent {
        /// The event.
        event: Box<CalendarEvent>,
    },
    /// Answer to [`Request::CreateEvent`], [`Request::UpdateEvent`] and
    /// [`Request::MoveEvent`].
    EventSaved {
        /// The event (series) as saved.
        event: Box<CalendarEvent>,
        /// The new series a `this-and-following` edit split off.
        split: Option<Box<CalendarEvent>>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::EventFromMessage`].
    EventSuggested {
        /// The pre-filled fields (unsaved).
        fields: Box<EventFields>,
    },
    /// Answer to [`Request::FindFreeTime`].
    FreeTime {
        /// Free slots, by start.
        slots: Vec<FreeSlot>,
    },
    /// Answer to [`Request::ListAddressBooks`].
    AddressBooks {
        /// The address books.
        address_books: Vec<AddressBook>,
    },
    /// Answer to [`Request::CreateAddressBook`] and
    /// [`Request::UpdateAddressBook`].
    AddressBookSaved {
        /// The address book.
        address_book: Box<AddressBook>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::ListContacts`].
    Contacts {
        /// Contacts matching the filter in total.
        total: u32,
        /// Index of the first contact of the page.
        offset: u32,
        /// The page.
        contacts: Vec<ContactSummary>,
    },
    /// Answer to [`Request::GetContact`].
    PimContact {
        /// The contact.
        contact: Box<Contact>,
    },
    /// Answer to [`Request::CreateContact`], [`Request::UpdateContact`],
    /// [`Request::MoveContact`] and [`Request::MergeContacts`].
    ContactSaved {
        /// The contact.
        contact: Box<Contact>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::SearchContacts`].
    ContactMatches {
        /// Best first.
        matches: Vec<ContactMatch>,
    },
    /// Answer to [`Request::FindDuplicateContacts`].
    ContactDuplicates {
        /// Groups of likely duplicates.
        groups: Vec<DuplicateGroup>,
    },
    /// Answer to [`Request::GetContactPhoto`].
    ContactPhoto {
        /// The contact.
        contact_id: ContactId,
        /// MIME type.
        mime_type: String,
        /// The image (base64).
        data: String,
    },
    /// Answer to [`Request::ListContactGroups`].
    ContactGroups {
        /// The groups.
        groups: Vec<ContactGroup>,
    },
    /// Answer to [`Request::CreateContactGroup`] and
    /// [`Request::UpdateContactGroup`].
    ContactGroupSaved {
        /// The group.
        group: Box<ContactGroup>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::ListTasks`].
    Tasks {
        /// The tasks.
        tasks: Vec<Task>,
    },
    /// Answer to [`Request::GetTask`].
    PimTask {
        /// The task.
        task: Box<Task>,
    },
    /// Answer to [`Request::CreateTask`], [`Request::UpdateTask`],
    /// [`Request::CompleteTask`] and [`Request::MoveTask`].
    TaskSaved {
        /// The task.
        task: Box<Task>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::TaskFromMessage`].
    TaskSuggested {
        /// The pre-filled fields (unsaved).
        fields: Box<TaskFields>,
    },
    /// Answer to [`Request::SearchPim`].
    PimSearchResults {
        /// Best first.
        hits: Vec<PimSearchHit>,
    },
    /// Answer to [`Request::PreviewPimImport`].
    PimImportPreview {
        /// What importing would do.
        preview: Box<PimImportPreview>,
    },
    /// Answer to [`Request::ImportPim`].
    PimImported {
        /// What it did.
        result: Box<PimImportResult>,
    },
    /// Answer to [`Request::GetInvitation`].
    Invitation {
        /// The card; `null` = the message has no iTIP part.
        invitation: Option<Box<Invitation>>,
    },
    /// Answer to the scheduling requests ([`Request::RespondToInvitation`],
    /// [`Request::ProposeNewTime`], [`Request::RefreshInvitation`],
    /// [`Request::ForwardInvitation`], [`Request::ApplyInvitation`],
    /// [`Request::SendInvitations`], [`Request::CancelMeeting`],
    /// [`Request::RespondToCounter`]).
    Scheduled {
        /// What happened.
        result: Box<SchedulingResult>,
    },
    /// Answer to [`Request::QueryFreeBusy`].
    FreeBusy {
        /// Busy periods per attendee.
        result: Box<FreeBusyResult>,
    },
    /// Answer to [`Request::GetSchedulingSettings`] and
    /// [`Request::SetSchedulingSettings`].
    SchedulingSettings {
        /// The settings.
        settings: SchedulingSettings,
    },
    /// Answer to [`Request::ListGmailLabels`].
    GmailLabels {
        /// The account.
        account_id: AccountId,
        /// System labels first (in [`crate::gmail::GmailFolder`] order,
        /// then the categories), then user labels by name.
        labels: Vec<GmailLabel>,
    },
    /// Answer to [`Request::CreateGmailLabel`] and
    /// [`Request::UpdateGmailLabel`].
    GmailLabelSaved {
        /// The label.
        label: Box<GmailLabel>,
        /// The queued label operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::GetExchangeStatus`].
    ExchangeStatus {
        /// The state.
        status: Box<ExchangeStatus>,
    },
    /// Answer to [`Request::ListExchangeMailboxes`].
    ExchangeMailboxes {
        /// The account.
        account_id: AccountId,
        /// The mailboxes, by name (hidden automapped ones left out).
        mailboxes: Vec<ExchangeMailbox>,
    },
    /// Answer to [`Request::AddExchangeMailbox`] and
    /// [`Request::UpdateExchangeMailbox`].
    ExchangeMailboxSaved {
        /// The mailbox.
        mailbox: Box<ExchangeMailbox>,
    },
    /// Answer to [`Request::ListExchangeCategories`].
    ExchangeCategories {
        /// The account.
        account_id: AccountId,
        /// The master list, by name.
        categories: Vec<ExchangeCategory>,
    },
    /// Answer to [`Request::CreateExchangeCategory`] and
    /// [`Request::UpdateExchangeCategory`].
    ExchangeCategorySaved {
        /// The category.
        category: Box<ExchangeCategory>,
        /// The queued operations.
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::GetAutomaticReplies`] and
    /// [`Request::SetAutomaticReplies`].
    AutomaticReplies {
        /// The replies.
        replies: Box<AutomaticReplies>,
        /// The queued operations (empty for `Get…`).
        op_ids: Vec<OpId>,
    },
    /// Answer to [`Request::GetExchangeRules`] and
    /// [`Request::SyncExchangeRules`].
    ExchangeRules {
        /// The rules.
        status: Box<ExchangeRulesStatus>,
    },
    /// Answer to [`Request::ListRoomLists`].
    RoomLists {
        /// The account.
        account_id: AccountId,
        /// The room lists, by name.
        lists: Vec<RoomList>,
    },
    /// Answer to [`Request::FindRooms`].
    Rooms {
        /// The account.
        account_id: AccountId,
        /// Range start.
        from: String,
        /// Range end.
        to: String,
        /// The rooms, free ones first, then by name.
        rooms: Vec<RoomAvailability>,
    },
    /// Answer to [`Request::ListReminders`].
    Reminders {
        /// Fired and snoozed reminders (and pending ones up to `until`),
        /// by fire time.
        reminders: Vec<Reminder>,
    },
    /// Answer to [`Request::ListConfirmations`].
    Confirmations {
        /// The confirmations.
        confirmations: Vec<Confirmation>,
    },
    /// Answer to [`Request::DecideConfirmation`].
    ConfirmationDecided {
        /// The confirmation in its new state.
        confirmation: Box<Confirmation>,
    },
    /// Answer to [`Request::ListActionPolicies`] and
    /// [`Request::SetActionPolicy`].
    ActionPolicies {
        /// The actor.
        actor: ActionActor,
        /// One policy per kind, in [`ActionKind::ALL`] order.
        policies: Vec<ActionPolicy>,
    },
    /// Answer to [`Request::GetPluginActivity`].
    PluginActivity {
        /// The activity.
        activity: Box<PluginActivity>,
    },
    /// Answer to [`Request::GetMessagePluginAnnotations`].
    MessagePluginAnnotations {
        /// The message.
        message_id: MessageId,
        /// Banners, by plug-in name.
        banners: Vec<PluginBanner>,
        /// Badges, by plug-in name.
        badges: Vec<PluginBadge>,
    },
    /// Answer to [`Request::RenderAttachmentPreview`].
    AttachmentPreview {
        /// The message.
        message_id: MessageId,
        /// The attachment's index.
        index: u32,
        /// The renderer.
        renderer: String,
        /// What to show.
        preview: Box<PluginPreview>,
    },
    /// Answer to [`Request::SendDraft`] when a before-send hook vetoed:
    /// nothing was queued; the UI names each plug-in with its reason and
    /// offers "Send anyway" ([`Request::OverrideSendVeto`]).
    SendVetoed {
        /// The draft (unchanged).
        draft_id: DraftId,
        /// Pass to [`Request::OverrideSendVeto`] (valid for 10 minutes).
        veto_id: SendVetoId,
        /// The vetoes.
        vetoes: Vec<PluginSendVeto>,
    },
    /// Answer to [`Request::GetInstallation`].
    Installation {
        /// The facts.
        info: Box<InstallationInfo>,
    },
    /// Answer to [`Request::GetUpdateStatus`], [`Request::SetUpdateSettings`],
    /// [`Request::CheckForUpdates`] and [`Request::DownloadUpdate`].
    UpdateStatus {
        /// Settings and state.
        status: Box<UpdateStatus>,
    },
    /// Answer to [`Request::GetDesktopSettings`] and
    /// [`Request::SetDesktopSettings`].
    DesktopSettings {
        /// The settings (`start_with_windows` as the Run key says).
        settings: DesktopSettings,
        /// The running `nail plugin dev` endpoint, when on.
        dev_endpoint_url: Option<String>,
        /// "Start with Windows" is on for every user by the machine-wide Run
        /// key (the MSI's `AUTOSTART=1`): only an administrator can turn it
        /// off, so the UI shows the switch as managed.
        #[serde(default)]
        start_with_windows_managed: bool,
    },
    /// Answer to [`Request::GetDefaultApps`].
    DefaultApps {
        /// Registration and status.
        status: Box<DefaultAppsStatus>,
        /// The Settings page [`Request::OpenDefaultAppsSettings`] opens.
        settings_uri: String,
    },
    /// Answer to [`Request::GetFirstRun`] and [`Request::CompleteFirstRun`].
    FirstRun {
        /// The state.
        state: FirstRunState,
    },
    /// Answer to [`Request::FindImportSources`].
    ImportSources {
        /// The sources found (may be empty).
        sources: Vec<FoundImportSource>,
    },
    /// Answer to [`Request::PreviewImport`].
    ImportPreview {
        /// What the import would bring in.
        preview: Box<ImportPreview>,
    },
    /// Answer to the requests that start, read or cancel a job.
    PortabilityJob {
        /// The job.
        job: Box<JobStatus>,
    },
    /// Answer to [`Request::ListPortabilityJobs`].
    PortabilityJobs {
        /// Running first, then finished ones, newest first.
        jobs: Vec<JobStatus>,
    },
    /// Answer to [`Request::InspectBackup`].
    BackupManifest {
        /// The archive's manifest.
        manifest: Box<BackupManifest>,
    },
    /// Answer to [`Request::PreviewRestore`].
    RestorePlan {
        /// What restoring would do.
        plan: Box<RestorePlan>,
    },
    /// Answer to [`Request::CreateRecoveryKey`].
    RecoveryKeyCreated {
        /// The key in its text form — shown once, never stored.
        key: String,
        /// Where the recovery kit was written, if asked.
        saved_to: Option<String>,
        /// The new status.
        status: RecoveryKeyStatus,
    },
    /// Answer to [`Request::VerifyRecoveryKey`].
    RecoveryKeyVerified {
        /// Whether the key opens the vault.
        valid: bool,
        /// The status (with `verified_at` updated when valid).
        status: RecoveryKeyStatus,
    },
    /// Answer to [`Request::GetSmartcardSettings`] and
    /// [`Request::SetSmartcardSettings`].
    SmartcardSettings {
        /// The settings.
        settings: SmartcardSettings,
    },
    /// Answer to [`Request::ListSmartcards`].
    Smartcards {
        /// The tokens found.
        tokens: Vec<SmartcardToken>,
        /// Modules that could not be used, with why (`smartcard-error-*`).
        problems: Vec<SmartcardModuleProblem>,
    },
    /// Answer to the recent-action requests.
    RecentActions {
        /// Most recent first.
        recent: RecentActions,
    },
    /// Answer to [`Request::CreatePrintJob`] and [`Request::GetPrintJob`].
    PrintJob {
        /// The job.
        job: Box<PrintJob>,
    },
    /// Answer to [`Request::SavePrintPdf`].
    PdfSaved {
        /// The written file.
        file: PdfFile,
    },
    /// Answer to [`Request::PreviewDiagnostics`].
    DiagnosticsPreview {
        /// Exactly what the bundle would hold.
        manifest: Box<DiagnosticsManifest>,
    },
    /// Answer to [`Request::CreateDiagnosticsBundle`].
    DiagnosticsBundle {
        /// The written bundle.
        bundle: Box<DiagnosticsBundle>,
    },
    /// Answer to [`Request::PickPath`].
    PathPicked {
        /// The chosen path; `null` = cancelled.
        path: Option<String>,
    },
    /// Answer to [`Request::ListThemes`], [`Request::SetTheme`] and
    /// [`Request::SetThemePreferences`] (M5a).
    Themes {
        /// Themes, selection, active theme, OS state.
        appearance: Box<Appearance>,
    },
    /// The request failed. May answer any request.
    Error {
        /// Machine-readable error class.
        code: ErrorCode,
        /// Diagnostic detail (English, for logs and developer tools). The UI
        /// shows a localized text chosen by `code`.
        message: String,
        /// The user-facing text (Fluent id + arguments) when the core has a more
        /// specific one than the error class; else `null` (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<Box<LocalizedMessage>>,
    },
}

impl From<ApiError> for Response {
    fn from(err: ApiError) -> Self {
        Self::Error {
            code: err.code,
            message: err.message,
            localized: err.localized,
        }
    }
}

impl Response {
    /// Converts an [`Response::Error`] into `Err(ApiError)`, anything else into `Ok`.
    pub fn into_result(self) -> Result<Self, ApiError> {
        match self {
            Self::Error {
                code,
                message,
                localized,
            } => Err(ApiError {
                code,
                message,
                localized,
            }),
            other => Ok(other),
        }
    }
}

/// A notification pushed by the core, not tied to a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Event {
    /// The theme or layout file changed and was valid; replaces the previous
    /// configuration. Also clears any error shown for [`Event::ConfigError`];
    /// errors of files that are still invalid follow right after it again.
    ConfigChanged {
        /// The new resolved theme.
        theme: Box<ResolvedTheme>,
        /// The new resolved layout.
        layout: Box<ResolvedLayout>,
    },
    /// A changed configuration file is invalid; the last valid configuration
    /// stays active.
    ConfigError {
        /// File name, e.g. `"theme.kdl"`.
        file: String,
        /// 1-based line of the error.
        line: u32,
        /// 1-based column of the error.
        column: u32,
        /// Diagnostic message (English, like a compiler message).
        message: String,
        /// The user-facing text (Fluent id + arguments) when the core has a more
        /// specific one than the error class; else `null` (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<LocalizedMessage>,
    },
    /// Messages or counts in these folders changed; clients should refetch
    /// what they display from them.
    MailChanged {
        /// The affected folders.
        folder_ids: Vec<FolderId>,
    },
    /// Progress of a running synchronization.
    SyncProgress {
        /// The account being synchronized.
        account_id: AccountId,
        /// Work units done so far.
        done: u32,
        /// Total work units (0 while unknown). `done == total` means finished.
        total: u32,
    },
    /// Synchronizing an account failed. The account keeps the state of its
    /// last successful synchronization; a later [`Request::SyncNow`] retries.
    SyncFailed {
        /// The account whose synchronization failed.
        account_id: AccountId,
        /// Machine-readable error class (the UI shows a localized text).
        code: ErrorCode,
        /// Diagnostic detail (English, for logs and developer tools).
        message: String,
        /// The user-facing text (Fluent id + arguments) when the core has a more
        /// specific one than the error class; else `null` (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<LocalizedMessage>,
    },
    /// A queued operation changed state.
    OpStatus {
        /// The new status.
        status: OpStatus,
    },
    /// An outbox entry changed state.
    SendStatus {
        /// The entry.
        outbox_id: OutboxId,
        /// Its new state.
        state: OutboxState,
        /// Error class when `failed`, else `null`.
        error: Option<ErrorCode>,
        /// Diagnostic detail (English), else `null`.
        message: Option<String>,
        /// The user-facing text (Fluent id + arguments) when the core has a more
        /// specific one than the error class; else `null` (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<LocalizedMessage>,
    },
    /// New messages arrived from a server (for notifications; lists are
    /// refreshed through [`Event::MailChanged`]).
    NewMail {
        /// The account.
        account_id: AccountId,
        /// The folder.
        folder_id: FolderId,
        /// The new messages.
        message_ids: Vec<MessageId>,
    },
    /// An account's connection state changed.
    AccountStatus {
        /// The new status.
        status: AccountStatus,
    },
    /// The account configuration changed (through a request or an edit of
    /// `accounts.kdl`); clients refetch accounts, identities and virtual
    /// accounts. An invalid `accounts.kdl` is reported as
    /// [`Event::ConfigError`] instead.
    AccountsChanged,
    /// An OAuth2 sign-in finished.
    OAuthFinished {
        /// The flow.
        flow_id: OAuthFlowId,
        /// `null` on success (the core holds the tokens for the flow id);
        /// `cancelled`, `auth_failed`, `network`, … otherwise.
        error: Option<ErrorCode>,
        /// Diagnostic detail (English), else `null`.
        message: Option<String>,
        /// The user-facing text (Fluent id + arguments) when the core has a more
        /// specific one than the error class; else `null` (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<LocalizedMessage>,
    },
    /// Every error of one configuration file (M1b; supersedes
    /// [`Event::ConfigError`], which the core keeps sending for the first
    /// error until WP25 switches the UI over). An empty `errors` list
    /// means the file is valid again. Sent for `theme.kdl`, `layout.kdl`,
    /// `accounts.kdl` and `rules.kdl`.
    ConfigErrors {
        /// File name, e.g. `"layout.kdl"`.
        file: String,
        /// The errors, in file order.
        errors: Vec<ConfigIssue>,
    },
    /// `rules.kdl` changed (by a request or by hand); clients refetch
    /// [`Request::ListRules`]. An invalid file is reported as
    /// [`Event::ConfigErrors`].
    RulesChanged,
    /// A rule run changed state (finished, failed or undone).
    RuleRunFinished {
        /// The run.
        run: Box<RuleRun>,
    },
    /// An MCP client read or edited what is on screen (the UI shows an
    /// indicator).
    McpContextAccess {
        /// The client.
        client_id: McpClientId,
        /// Its name.
        client_name: String,
        /// `true` when it edited the open draft, `false` for a read.
        edit: bool,
    },
    /// MCP clients or their grants changed; clients refetch
    /// [`Request::ListMcpClients`].
    McpClientsChanged,
    /// Saved searches changed; clients refetch
    /// [`Request::ListSavedSearches`].
    SavedSearchesChanged,
    /// The keyring changed (import, generation, deletion, trust, a key
    /// learned from Autocrypt or a lookup); clients refetch keys and
    /// recipient key status, and messages whose security they show.
    KeysChanged,
    /// Encryption at rest changed state or made progress.
    VaultProgress {
        /// The new state.
        status: VaultStatus,
    },
    /// The global security settings changed; clients refetch
    /// [`Request::GetSecuritySettings`].
    SecuritySettingsChanged,
    /// The remote UI started, stopped, failed or changed its certificate.
    RemoteUiStatusChanged {
        /// The new state.
        status: RemoteUiStatus,
    },
    /// Remote users, devices, sessions or pairing codes changed; clients
    /// refetch what they show of them.
    RemoteAccessChanged,
    /// A remote session ended. The browser holding it shows the login
    /// page (its WebSocket is closed right after this event).
    RemoteSessionEnded {
        /// The session.
        session_id: RemoteSessionId,
        /// Why.
        reason: SessionEndReason,
    },
    /// A security alert: a new device, a lockout. Shown as a
    /// notification.
    RemoteSecurityAlert {
        /// The alert.
        alert: RemoteAlert,
    },
    /// The remote-content allow-lists changed; clients refetch the shown
    /// message and the lists.
    RemoteContentPolicyChanged,
    /// A launch request arrived (the app was asked to open a link or a
    /// file); the UI takes it with [`Request::TakeLaunchRequests`].
    LaunchRequested,
    /// An account's Sieve state changed (upload started, finished or
    /// failed; rule placements changed); clients refetch
    /// [`Request::GetSieveStatus`].
    SieveStatusChanged {
        /// The account.
        account_id: AccountId,
    },
    /// An account's classifier was trained, reset or reconfigured.
    JunkStatusChanged {
        /// The account.
        account_id: AccountId,
    },
    /// Messages were snoozed or came back; clients refetch
    /// [`Request::ListSnoozed`].
    SnoozeChanged,
    /// The notification settings changed.
    NotificationSettingsChanged,
    /// Show a notification (the desktop shell shows an OS notification,
    /// the remote UI a Web Notification). Only sent when the policy allows
    /// it (settings, rules, quiet hours).
    NotificationRequested {
        /// What to show.
        notification: MailNotification,
    },
    /// The MCP settings changed (and the server may have moved).
    McpSettingsChanged,
    /// Plug-ins were installed, removed, enabled, disabled, re-granted,
    /// reloaded or changed state; clients refetch [`Request::ListPlugins`]
    /// (toolbars, menus, keys and panels depend on it).
    PluginsChanged,
    /// A plug-in opened a dialog. A UI shows it when `client_id` is its own
    /// or `null`.
    PluginDialogOpened {
        /// The dialog.
        dialog: Box<PluginDialog>,
    },
    /// A plug-in dialog was closed (answered in some UI, closed by the
    /// plug-in, or its plug-in stopped); every UI removes it.
    PluginDialogClosed {
        /// The dialog.
        dialog_id: PluginDialogId,
        /// Its plug-in.
        plugin_id: PluginId,
    },
    /// A plug-in notification (toast; OS notification when `system`).
    PluginNotification {
        /// What to show.
        notification: Box<PluginNotification>,
    },
    /// A plug-in panel's content changed.
    PluginPanelChanged {
        /// `<plugin>.<panel>`.
        panel: String,
        /// The new content.
        tree: Vec<PluginUiNode>,
    },
    /// A new log line (the plug-in manager and `nail plugin dev` tail
    /// them).
    PluginLogLine {
        /// The line.
        entry: Box<PluginLogEntry>,
    },
    /// A plug-in's runtime stopped; crashes (errors, CPU budget, memory
    /// limit) flag it as `crashed` and the UI tells the user.
    PluginStopped {
        /// The plug-in.
        plugin_id: PluginId,
        /// Why.
        reason: PluginStopReason,
        /// Details (English, for logs).
        error: Option<String>,
        /// The user-facing text (ADR 0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        localized: Option<Box<LocalizedMessage>>,
    },
    /// PIM sources were added, removed, reconfigured, or changed sync
    /// state; clients refetch [`Request::ListPimSources`] (M3a).
    PimSourcesChanged,
    /// Calendars or task lists were added, removed or changed; clients
    /// refetch [`Request::ListCalendars`].
    CalendarsChanged,
    /// Address books were added, removed or changed.
    AddressBooksChanged,
    /// Events of these calendars changed (local edits, sync, imports);
    /// clients refetch the instances they show.
    EventsChanged {
        /// The calendars.
        calendar_ids: Vec<CalendarId>,
    },
    /// Tasks of these lists changed.
    TasksChanged {
        /// The task lists.
        calendar_ids: Vec<CalendarId>,
    },
    /// Contacts or groups of these address books changed.
    ContactsChanged {
        /// The address books.
        address_book_ids: Vec<AddressBookId>,
    },
    /// A reminder fired (or a snoozed one came back): the UI shows it with
    /// snooze / dismiss. The desktop shell also gets a
    /// [`Event::NotificationRequested`] of kind `reminder`.
    ReminderDue {
        /// The reminder.
        reminder: Box<Reminder>,
    },
    /// Reminders were snoozed, dismissed or recomputed; clients refetch
    /// [`Request::ListReminders`].
    RemindersChanged,
    /// A queued PIM operation changed state.
    PimOpStatus {
        /// The new status.
        status: PimOpStatus,
    },
    /// Invitation cards of these messages changed (a reply was applied,
    /// the event changed or was answered elsewhere); clients refetch
    /// [`Request::GetInvitation`] / the message (M3b).
    InvitationsChanged {
        /// The messages.
        message_ids: Vec<MessageId>,
    },
    /// The scheduling settings changed.
    SchedulingSettingsChanged,
    /// Labels of a Gmail account were added, changed or removed (locally
    /// or by sync); clients refetch [`Request::ListGmailLabels`] (M3b).
    GmailLabelsChanged {
        /// The account.
        account_id: AccountId,
    },
    /// Exchange state of an account changed (locally or by sync):
    /// clients refetch the requests of the named areas (M3c).
    ExchangeChanged {
        /// The account.
        account_id: AccountId,
        /// What changed.
        areas: Vec<ExchangeArea>,
    },
    /// An MCP client or plug-in asks the user to confirm a write (M4a):
    /// the confirmation center shows it (and the desktop may notify).
    ConfirmationRequested {
        /// The pending confirmation.
        confirmation: Box<Confirmation>,
    },
    /// A confirmation was decided, expired or cancelled.
    ConfirmationResolved {
        /// The confirmation in its final state.
        confirmation: Box<Confirmation>,
    },
    /// An actor's action policies changed; clients refetch
    /// [`Request::ListActionPolicies`].
    ActionPoliciesChanged {
        /// The actor.
        actor: ActionActor,
    },
    /// A plug-in's composer action or MCP's `edit_open_draft` edits the
    /// open draft: the composer showing `draft_id` applies it live.
    ComposerEditRequested {
        /// The draft.
        draft_id: DraftId,
        /// The UI that ran the composer action; `null` = every UI showing
        /// the draft (MCP).
        client_id: Option<String>,
        /// Who edits (shown next to the change).
        by: Box<ActionAttribution>,
        /// The edit.
        edit: ComposerEdit,
    },
    /// A plug-in's jobs, hooks or CPU use changed; the plug-in manager
    /// refetches [`Request::GetPluginActivity`].
    PluginActivityChanged {
        /// The plug-in.
        plugin_id: PluginId,
    },
    /// The updater's settings, state or progress changed (M4b).
    UpdateStatusChanged {
        /// Settings and state.
        status: Box<UpdateStatus>,
    },
    /// The desktop settings changed (or the Run key / dev endpoint did);
    /// clients refetch [`Request::GetDesktopSettings`] (M4b).
    DesktopSettingsChanged,
    /// Default-app registration may have changed; clients refetch
    /// [`Request::GetDefaultApps`] (M4b).
    DefaultAppsChanged,
    /// A portability job made progress (at most a few times per second;
    /// M4c, ADR 0094). Desktop / local UI only.
    PortabilityJobProgress {
        /// The job.
        job_id: JobId,
        /// Where it is.
        progress: JobProgress,
    },
    /// A portability job finished (completed, failed or cancelled).
    PortabilityJobFinished {
        /// What it did.
        report: Box<JobReport>,
    },
    /// A smartcard needs its PIN: show a PIN dialog (desktop only).
    PinRequested {
        /// The prompt.
        prompt: PinPrompt,
    },
    /// A PIN dialog is no longer needed.
    PinPromptClosed {
        /// The prompt.
        prompt_id: PinPromptId,
        /// Why.
        reason: PinPromptCloseReason,
    },
    /// A token was inserted or removed (refresh the smartcard list).
    SmartcardsChanged,
    /// Upload progress of a message being sent (Graph upload sessions for
    /// large messages; M4c).
    SendProgress {
        /// The outbox entry.
        outbox_id: OutboxId,
        /// Bytes uploaded.
        sent_bytes: u64,
        /// Bytes in total.
        total_bytes: u64,
    },
    // --- Appearance (M5a, ADR 0102) ---
    /// The theme list, the selection, the active theme or the OS light /
    /// dark state changed (`theme.kdl` / `themes/` edited, `SetTheme`,
    /// `SetThemePreferences`, Windows switched modes). When the active
    /// theme or the density changed, [`Event::ConfigChanged`] with the new
    /// resolved theme follows.
    ThemeChanged {
        /// The new state.
        appearance: Box<Appearance>,
    },
}

/// A PKCS#11 module that could not be used ([`Response::Smartcards`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SmartcardModuleProblem {
    /// The module path.
    pub module: String,
    /// What went wrong.
    pub error: crate::smartcard::SmartcardError,
    /// Diagnostic detail (English).
    pub detail: String,
}

/// A request frame on a multiplexed transport (WebSocket).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RpcRequest {
    /// Client-chosen correlation id, unique among the client's in-flight
    /// requests. Kept below 2^53 so it is exact in JavaScript.
    pub id: u64,
    /// The request.
    pub request: Request,
}

/// A response frame on a multiplexed transport (WebSocket).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct RpcResponse {
    /// The `id` of the [`RpcRequest`] this answers.
    pub id: u64,
    /// The response.
    pub response: Response,
}

/// Any frame sent by the server on a multiplexed transport (WebSocket).
///
/// JSON: `{"kind":"response","id":1,"response":{…}}` or
/// `{"kind":"event","event":{…}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ServerMessage {
    /// The answer to an [`RpcRequest`].
    Response {
        /// The `id` of the [`RpcRequest`] this answers.
        id: u64,
        /// The response.
        response: Response,
    },
    /// A pushed event.
    Event {
        /// The event.
        event: Event,
    },
}

impl From<RpcResponse> for ServerMessage {
    fn from(r: RpcResponse) -> Self {
        Self::Response {
            id: r.id,
            response: r.response,
        }
    }
}

impl From<Event> for ServerMessage {
    fn from(event: Event) -> Self {
        Self::Event { event }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn requests_use_type_tag() {
        assert_eq!(
            serde_json::to_value(Request::ListAccounts).unwrap(),
            json!({"type": "ListAccounts"})
        );
        assert_eq!(
            serde_json::to_value(Request::ListMessages {
                folder_id: "f".into(),
                offset: 0,
                limit: 50
            })
            .unwrap(),
            json!({"type": "ListMessages", "folder_id": "f", "offset": 0, "limit": 50})
        );
        assert_eq!(
            serde_json::to_value(Request::SyncNow { account_id: None }).unwrap(),
            json!({"type": "SyncNow", "account_id": null})
        );
    }

    #[test]
    fn rpc_request_round_trips() {
        let frame = json!({"id": 7, "request": {"type": "GetMessage", "id": "m1"}});
        let req: RpcRequest = serde_json::from_value(frame.clone()).unwrap();
        assert_eq!(
            req,
            RpcRequest {
                id: 7,
                request: Request::GetMessage { id: "m1".into() }
            }
        );
        assert_eq!(serde_json::to_value(&req).unwrap(), frame);
    }

    #[test]
    fn server_messages_are_kind_tagged() {
        let response: ServerMessage = RpcResponse {
            id: 3,
            response: Response::SyncStarted,
        }
        .into();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"kind": "response", "id": 3, "response": {"type": "SyncStarted"}})
        );
        let event: ServerMessage = Event::MailChanged {
            folder_ids: vec!["f1".into()],
        }
        .into();
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({"kind": "event", "event": {"type": "MailChanged", "folder_ids": ["f1"]}})
        );
        assert_eq!(
            serde_json::from_value::<ServerMessage>(value).unwrap(),
            event
        );
    }

    #[test]
    fn m1a_requests_serialize() {
        assert_eq!(
            serde_json::to_value(Request::DiscoverAccount {
                email: "ada@example.org".into(),
                trusted_hosts: Vec::new(),
                windows_sign_in: false,
            })
            .unwrap(),
            json!({"type": "DiscoverAccount", "email": "ada@example.org"})
        );
        assert_eq!(
            serde_json::to_value(Request::MoveMessages {
                message_ids: vec!["m1".into()],
                target: MoveTarget::Role(crate::FolderRole::Archive),
            })
            .unwrap(),
            json!({"type": "MoveMessages", "message_ids": ["m1"], "target": {"role": "archive"}})
        );
        assert_eq!(
            serde_json::to_value(Request::SendDraft {
                draft_id: "d1".into(),
                delay_secs: None,
                send_at: None,
                crypto: None,
                expected_revision: None,
            })
            .unwrap(),
            json!({"type": "SendDraft", "draft_id": "d1", "delay_secs": null, "send_at": null})
        );
        let req: Request = serde_json::from_value(json!({
            "type": "UpdateFlags", "message_ids": ["m1", "m2"], "change": {"seen": true}
        }))
        .unwrap();
        let Request::UpdateFlags {
            message_ids,
            change,
        } = req
        else {
            panic!("wrong variant");
        };
        assert_eq!(message_ids.len(), 2);
        assert_eq!(change.seen, Some(true));
        assert_eq!(
            serde_json::to_value(Request::GetAccountsConfig).unwrap(),
            json!({"type": "GetAccountsConfig"})
        );
    }

    #[test]
    fn m1a_responses_and_events_serialize() {
        assert_eq!(
            serde_json::to_value(Response::Queued {
                op_ids: vec!["op-1".into()]
            })
            .unwrap(),
            json!({"type": "Queued", "op_ids": ["op-1"]})
        );
        assert_eq!(
            serde_json::to_value(Response::Done).unwrap(),
            json!({"type": "Done"})
        );
        let invalid = Response::InvalidQuery {
            error: SearchError {
                kind: crate::search::SearchErrorKind::UnknownField,
                message: "unknown field `frm`".into(),
                start: 0,
                end: 3,
            },
        };
        assert_eq!(
            serde_json::to_value(&invalid).unwrap(),
            json!({"type": "InvalidQuery", "error": {
                "kind": "unknown-field", "message": "unknown field `frm`", "start": 0, "end": 3
            }})
        );
        let event = Event::OpStatus {
            status: OpStatus {
                op_id: "op-1".into(),
                account_id: "work".into(),
                state: crate::ops::OpState::Done,
                error: None,
                message: None,
                localized: None,
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["status"]["state"], json!("done"));
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), event);
        assert_eq!(
            serde_json::to_value(Event::AccountsChanged).unwrap(),
            json!({"type": "AccountsChanged"})
        );
    }

    #[test]
    fn m1b_requests_serialize() {
        assert_eq!(
            serde_json::to_value(Request::RetryOp {
                op_id: "op-3".into()
            })
            .unwrap(),
            json!({"type": "RetryOp", "op_id": "op-3"})
        );
        let new_draft: Request = serde_json::from_value(json!({
            "type": "NewDraft",
            "context": {"mode": "forward", "message_id": "m1"}
        }))
        .unwrap();
        assert_eq!(
            new_draft,
            Request::NewDraft {
                identity_id: None,
                account_id: None,
                mode: None,
                context: Some(DraftContext {
                    mode: crate::compose::DraftContextMode::Forward,
                    message_id: "m1".into(),
                }),
            }
        );
        let update: Request = serde_json::from_value(json!({
            "type": "UpdateLayout",
            "patches": [{"op": "toolbar-order", "toolbar": "reader",
                         "items": [{"type": "action", "action": "mail.reply"}, {"type": "spacer"}]}]
        }))
        .unwrap();
        let Request::UpdateLayout { patches } = update else {
            panic!("wrong variant");
        };
        assert_eq!(patches.len(), 1);
        let merge: Request = serde_json::from_value(json!({
            "type": "ApplyConfigMerge", "file": "theme", "incoming": "theme \"x\" {}",
            "choices": [{"item_id": "tokens/color.bg", "take": "theirs"}]
        }))
        .unwrap();
        assert!(matches!(
            merge,
            Request::ApplyConfigMerge {
                default: MergeTake::Mine,
                ..
            }
        ));
        assert_eq!(
            serde_json::to_value(Request::ReportUiContext {
                context: UiContext {
                    message_id: Some("m1".into()),
                    ..UiContext::default()
                }
            })
            .unwrap()["context"]["message_id"],
            json!("m1")
        );
        let grants: Request = serde_json::from_value(json!({
            "type": "CreateMcpClient", "name": "claude"
        }))
        .unwrap();
        assert!(matches!(grants, Request::CreateMcpClient { grant, .. } if grant.is_empty()));
    }

    #[test]
    fn m1b_responses_and_events_serialize() {
        let created = Response::McpClientCreated {
            client: Box::new(McpClient {
                id: "mcp-1".into(),
                name: "claude".into(),
                created_at: "2026-09-29T10:00:00Z".into(),
                last_used_at: None,
                token_hint: "x1y2".into(),
                grant: McpGrant::default(),
                revoked_at: None,
                rate_limits: McpRateLimits::default(),
            }),
            token: "secret".into(),
        };
        let value = serde_json::to_value(&created).unwrap();
        assert_eq!(value["type"], json!("McpClientCreated"));
        assert_eq!(value["client"]["grant"]["scopes"], json!([]));
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), created);
        let errors = Event::ConfigErrors {
            file: "layout.kdl".into(),
            errors: vec![
                ConfigIssue {
                    line: 1,
                    column: 2,
                    message: "a".into(),
                    localized: None,
                },
                ConfigIssue {
                    line: 5,
                    column: 1,
                    message: "b".into(),
                    localized: None,
                },
            ],
        };
        let value = serde_json::to_value(&errors).unwrap();
        assert_eq!(value["errors"][1]["line"], json!(5));
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), errors);
        assert_eq!(
            serde_json::to_value(Event::RulesChanged).unwrap(),
            json!({"type": "RulesChanged"})
        );
        assert_eq!(
            serde_json::to_value(Response::ConfigMergeResolved {
                theme: None,
                layout: None
            })
            .unwrap(),
            json!({"type": "ConfigMergeResolved", "theme": null, "layout": null})
        );
    }

    #[test]
    fn m2a_requests_serialize() {
        // M1 clients (no `crypto`) still parse; absent crypto is not sent.
        let send: Request = serde_json::from_value(json!({
            "type": "SendDraft", "draft_id": "d1", "delay_secs": 5, "send_at": null
        }))
        .unwrap();
        assert!(matches!(send, Request::SendDraft { crypto: None, .. }));
        let value = serde_json::to_value(&send).unwrap();
        assert!(value.get("crypto").is_none());
        let send: Request = serde_json::from_value(json!({
            "type": "SendDraft", "draft_id": "d1", "delay_secs": null, "send_at": null,
            "crypto": {"sign": true, "encrypt": true, "technology": "openpgp"}
        }))
        .unwrap();
        let Request::SendDraft {
            crypto: Some(crypto),
            ..
        } = send
        else {
            panic!("wrong variant");
        };
        assert!(crypto.sign && crypto.encrypt);
        assert_eq!(crypto.technology, Some(CryptoTechnology::OpenPgp));

        let list: Request = serde_json::from_value(json!({"type": "ListKeys"})).unwrap();
        assert_eq!(
            list,
            Request::ListKeys {
                filter: KeyFilter::default()
            }
        );
        let generate: Request = serde_json::from_value(json!({
            "type": "GenerateKey", "identity_id": "ada@example.org"
        }))
        .unwrap();
        assert_eq!(
            generate,
            Request::GenerateKey {
                identity_id: "ada@example.org".into(),
                algorithm: KeyAlgorithm::Curve25519,
                expires_days: None,
                protection: SecretProtection::Keychain,
            }
        );
        assert_eq!(
            serde_json::to_value(Request::SetKeyTrust {
                key_id: "key-1".into(),
                decision: TrustDecision::Verified,
            })
            .unwrap(),
            json!({"type": "SetKeyTrust", "key_id": "key-1", "decision": "verified"})
        );
        assert_eq!(
            serde_json::to_value(Request::ListWindowsCertificates {
                store: WindowsCertStore::OtherPeople
            })
            .unwrap(),
            json!({"type": "ListWindowsCertificates", "store": "other-people"})
        );
        let check: Request = serde_json::from_value(json!({
            "type": "CheckRecipientKeys", "identity_id": "ada@example.org",
            "recipients": ["bob@example.org"]
        }))
        .unwrap();
        assert!(matches!(
            check,
            Request::CheckRecipientKeys { lookup: false, .. }
        ));
        assert_eq!(
            serde_json::to_value(Request::SetVaultEncryption { enabled: true }).unwrap(),
            json!({"type": "SetVaultEncryption", "enabled": true})
        );
        assert_eq!(
            serde_json::to_value(Request::SuggestAuthservId {
                account_id: "work".into()
            })
            .unwrap(),
            json!({"type": "SuggestAuthservId", "account_id": "work"})
        );
        assert_eq!(
            serde_json::to_value(Response::AuthservIdSuggestion {
                account_id: "work".into(),
                authserv_id: None
            })
            .unwrap(),
            json!({"type": "AuthservIdSuggestion", "account_id": "work", "authserv_id": null})
        );
    }

    #[test]
    fn m2a_responses_and_events_serialize() {
        let status = Response::VaultStatus {
            status: VaultStatus::plain(),
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["type"], json!("VaultStatus"));
        assert_eq!(value["status"]["state"], json!("plain"));
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), status);
        assert_eq!(
            serde_json::to_value(Event::KeysChanged).unwrap(),
            json!({"type": "KeysChanged"})
        );
        let settings = Response::SecuritySettings {
            settings: SecuritySettings::default(),
        };
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["settings"]["key_lookup"], json!("on-demand"));
        assert_eq!(
            value["settings"]["lookup_sources"],
            json!(["wkd", "keys-openpgp-org"])
        );
        let lookup = Response::KeyLookup {
            email: "bob@example.org".into(),
            candidates: vec![],
            failures: vec![KeyLookupFailure {
                source: KeyLookupSource::Wkd,
                code: ErrorCode::Network,
                message: "offline".into(),
                localized: None,
            }],
        };
        let value = serde_json::to_value(&lookup).unwrap();
        assert_eq!(value["failures"][0]["source"], json!("wkd"));
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), lookup);
    }

    #[test]
    fn m2b_requests_serialize() {
        let create: Request = serde_json::from_value(json!({
            "type": "CreateRemoteUser", "name": "ada"
        }))
        .unwrap();
        assert_eq!(
            create,
            Request::CreateRemoteUser {
                name: "ada".into(),
                display_name: None,
                grant: RemoteGrant::default(),
            }
        );
        let pairing: Request = serde_json::from_value(json!({
            "type": "CreatePairingCode", "user_id": "ruser-1"
        }))
        .unwrap();
        assert!(matches!(
            pairing,
            Request::CreatePairingCode {
                grant: None,
                ttl_minutes: None,
                ..
            }
        ));
        let sessions: Request =
            serde_json::from_value(json!({"type": "ListRemoteSessions"})).unwrap();
        assert_eq!(
            sessions,
            Request::ListRemoteSessions {
                filter: RemoteSessionFilter::default()
            }
        );
        assert_eq!(
            serde_json::to_value(Request::AllowRemoteContent {
                message_id: "m1".into(),
                scope: RemoteContentScope::Sender,
            })
            .unwrap(),
            json!({"type": "AllowRemoteContent", "message_id": "m1", "scope": "sender"})
        );
        let update: Request = serde_json::from_value(json!({
            "type": "UpdateRemoteUiSettings",
            "settings": {"enabled": true, "bind_address": "0.0.0.0"}
        }))
        .unwrap();
        let Request::UpdateRemoteUiSettings { settings } = update else {
            panic!("wrong variant");
        };
        assert!(settings.enabled && !settings.is_loopback());
        assert_eq!(settings.port, crate::remote::DEFAULT_REMOTE_UI_PORT);
        assert_eq!(
            serde_json::to_value(Request::SetRemotePassword {
                user_id: "ruser-1".into(),
                password: None,
            })
            .unwrap(),
            json!({"type": "SetRemotePassword", "user_id": "ruser-1", "password": null})
        );
    }

    #[test]
    fn m2b_responses_and_events_serialize() {
        let ui = Response::RemoteUi {
            settings: RemoteUiSettings::default(),
            status: RemoteUiStatus::stopped(),
        };
        let value = serde_json::to_value(&ui).unwrap();
        assert_eq!(value["status"]["internet_mode_locked"], json!(true));
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), ui);
        let ended = Event::RemoteSessionEnded {
            session_id: "rsess-3".into(),
            reason: SessionEndReason::Revoked,
        };
        let value = serde_json::to_value(&ended).unwrap();
        assert_eq!(
            value,
            json!({"type": "RemoteSessionEnded", "session_id": "rsess-3", "reason": "revoked"})
        );
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), ended);
        let alert = Event::RemoteSecurityAlert {
            alert: RemoteAlert::NewDevice {
                device_id: "rdev-1".into(),
                device_name: "Phone".into(),
                user_name: "ada".into(),
                ip: Some("192.168.1.20".into()),
            },
        };
        assert_eq!(
            serde_json::to_value(&alert).unwrap()["alert"]["kind"],
            json!("new-device")
        );
        assert_eq!(
            serde_json::to_value(Event::RemoteContentPolicyChanged).unwrap(),
            json!({"type": "RemoteContentPolicyChanged"})
        );
    }

    #[test]
    fn errors_convert_both_ways() {
        let response: Response = ApiError::new(ErrorCode::NotFound, "no such message").into();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"type": "Error", "code": "not_found", "message": "no such message"})
        );
        let err = response.into_result().unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(Response::SyncStarted.into_result().is_ok());

        // M2c: the localized text survives both conversions.
        let localized = LocalizedMessage::new("sieve-error-connect").arg("host", "h");
        let response: Response = ApiError::new(ErrorCode::Network, "refused")
            .with_localized(localized.clone())
            .into();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"type": "Error", "code": "network", "message": "refused",
                   "localized": {"id": "sieve-error-connect", "args": {"host": "h"}}})
        );
        assert_eq!(
            response.into_result().unwrap_err().localized,
            Some(Box::new(localized))
        );
    }

    #[test]
    fn m2c_requests_serialize() {
        let status: Request = serde_json::from_value(json!({"type": "GetSieveStatus"})).unwrap();
        assert_eq!(status, Request::GetSieveStatus { account_id: None });
        let snooze: Request = serde_json::from_value(json!({
            "type": "SnoozeMessages", "message_ids": ["m1"], "until": "2026-10-01T08:00:00Z"
        }))
        .unwrap();
        assert!(matches!(
            snooze,
            Request::SnoozeMessages {
                wake_folder_id: None,
                ..
            }
        ));
        let policy: Request = serde_json::from_value(json!({
            "type": "SetFolderSyncPolicy", "folder_id": "f1",
            "bodies": {"mode": "recent", "days": 14}
        }))
        .unwrap();
        assert_eq!(
            policy,
            Request::SetFolderSyncPolicy {
                folder_id: "f1".into(),
                bodies: Some(BodyPolicy::Recent { days: 14 }),
                enabled: None,
                idle: None,
            }
        );
        let vacation: Request = serde_json::from_value(json!({
            "type": "UpdateVacation", "account_id": "work",
            "settings": {"enabled": true, "body": "Away"}
        }))
        .unwrap();
        let Request::UpdateVacation { settings, .. } = vacation else {
            panic!("wrong variant");
        };
        assert!(settings.enabled && settings.days == 7 && settings.issues().is_empty());
        assert_eq!(
            serde_json::to_value(Request::MarkJunk {
                message_ids: vec!["m1".into()],
                junk: true,
            })
            .unwrap(),
            json!({"type": "MarkJunk", "message_ids": ["m1"], "junk": true})
        );
        assert_eq!(
            serde_json::to_value(Request::UpdateMcpSettings {
                settings: McpSettings { port: Some(4799) },
            })
            .unwrap(),
            json!({"type": "UpdateMcpSettings", "settings": {"port": 4799}})
        );
        assert_eq!(
            serde_json::to_value(Request::RuleFromMessage {
                message_id: "m1".into(),
            })
            .unwrap(),
            json!({"type": "RuleFromMessage", "message_id": "m1"})
        );
        for plain in [
            "TestNotification",
            "GetNotificationSettings",
            "GetMcpSettings",
        ] {
            let request: Request = serde_json::from_value(json!({ "type": plain })).unwrap();
            assert_eq!(
                serde_json::to_value(&request).unwrap()["type"],
                json!(plain)
            );
        }
    }

    #[test]
    fn m2c_responses_and_events_serialize() {
        let results = Response::SearchResults {
            offset: 0,
            total: 0,
            hits: vec![],
            server: None,
        };
        assert!(
            serde_json::to_value(&results)
                .unwrap()
                .get("server")
                .is_none()
        );
        let parsed: Response =
            serde_json::from_value(json!({"type": "SearchResults", "offset": 0, "total": 0,
                "hits": [], "server": {"accounts": ["work"], "hits": 2, "failed": []}}))
            .unwrap();
        assert!(matches!(
            parsed,
            Response::SearchResults {
                server: Some(ServerSearch { hits: 2, .. }),
                ..
            }
        ));
        let fetched = Response::BodiesFetched {
            fetched: vec!["m1".into()],
            failed: vec![],
        };
        assert_eq!(
            serde_json::to_value(&fetched).unwrap(),
            json!({"type": "BodiesFetched", "fetched": ["m1"], "failed": []})
        );
        let failed = Event::SyncFailed {
            account_id: "work".into(),
            code: ErrorCode::Network,
            message: "down".into(),
            localized: None,
        };
        assert!(
            serde_json::to_value(&failed)
                .unwrap()
                .get("localized")
                .is_none()
        );
        let parsed: Event = serde_json::from_value(json!({
            "type": "SyncFailed", "account_id": "work", "code": "network", "message": "down"
        }))
        .unwrap();
        assert_eq!(parsed, failed);
        let requested = Event::NotificationRequested {
            notification: MailNotification {
                id: "n1".into(),
                tag: "t".into(),
                kind: crate::triage::NotificationKind::NewMail,
                account_id: None,
                folder_id: None,
                message_ids: vec![],
                title: LocalizedMessage::new("notification-new-mail-title"),
                body: None,
                sound: false,
            },
        };
        let value = serde_json::to_value(&requested).unwrap();
        assert_eq!(value["type"], json!("NotificationRequested"));
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), requested);
        assert_eq!(
            serde_json::to_value(Event::SnoozeChanged).unwrap(),
            json!({"type": "SnoozeChanged"})
        );
    }

    #[test]
    fn m3a_requests_serialize() {
        use crate::pim::{EventFields, PimTime};
        let start = PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin");
        let end = PimTime::zoned("2026-10-01T10:00:00", "Europe/Berlin");
        let update = Request::UpdateEvent {
            event_id: "ev1".into(),
            scope: EditScope::ThisAndFollowing,
            recurrence_id: Some(start.clone()),
            fields: Box::new(EventFields::new("Planning", start.clone(), end)),
            recurrence: None,
        };
        let value = serde_json::to_value(&update).unwrap();
        assert_eq!(value["type"], "UpdateEvent");
        assert_eq!(value["scope"], "this-and-following");
        assert_eq!(value["recurrence_id"]["tz"], "Europe/Berlin");
        assert_eq!(serde_json::from_value::<Request>(value).unwrap(), update);

        // Optional fields may be left out; the scope defaults to `all`.
        let delete: Request =
            serde_json::from_value(json!({"type": "DeleteEvent", "event_id": "ev1"})).unwrap();
        assert_eq!(
            delete,
            Request::DeleteEvent {
                event_id: "ev1".into(),
                scope: EditScope::All,
                recurrence_id: None
            }
        );
        let list: Request = serde_json::from_value(json!({
            "type": "ListEventInstances",
            "from": "2026-10-01T00:00:00Z", "to": "2026-11-01T00:00:00Z", "tz": "Europe/Berlin"
        }))
        .unwrap();
        assert!(matches!(
            list,
            Request::ListEventInstances {
                calendar_ids: None,
                ..
            }
        ));
        assert_eq!(
            serde_json::to_value(Request::ListPimSources).unwrap(),
            json!({"type": "ListPimSources"})
        );
        let import = Request::ImportPim {
            source: PimImportSource::File {
                path: r"C:\cal\team.ics".into(),
            },
            calendar_id: Some("c1".into()),
            address_book_id: None,
        };
        let value = serde_json::to_value(&import).unwrap();
        assert_eq!(value["source"]["type"], "file");
        assert_eq!(serde_json::from_value::<Request>(value).unwrap(), import);
        let snooze = Request::SnoozeReminders {
            reminder_ids: vec!["r1".into()],
            until: "2026-10-01T09:10:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(&snooze).unwrap(),
            json!({"type": "SnoozeReminders", "reminder_ids": ["r1"], "until": "2026-10-01T09:10:00Z"})
        );
        let resolve = Request::ResolvePimConflict {
            item: PimItemRef::Contact {
                contact_id: "k1".into(),
            },
            keep: ConflictChoice::Server,
        };
        assert_eq!(
            serde_json::to_value(&resolve).unwrap(),
            json!({"type": "ResolvePimConflict", "item": {"type": "contact", "contact_id": "k1"}, "keep": "server"})
        );
    }

    #[test]
    fn m3a_responses_and_events_serialize() {
        let deleted = Response::PimDeleted {
            op_ids: vec!["op-1".into()],
        };
        assert_eq!(
            serde_json::to_value(&deleted).unwrap(),
            json!({"type": "PimDeleted", "op_ids": ["op-1"]})
        );
        let changed = Event::EventsChanged {
            calendar_ids: vec!["c1".into()],
        };
        assert_eq!(
            serde_json::to_value(&changed).unwrap(),
            json!({"type": "EventsChanged", "calendar_ids": ["c1"]})
        );
        let status = Event::PimOpStatus {
            status: PimOpStatus {
                op_id: "op-1".into(),
                source_id: "work-dav".into(),
                state: crate::ops::OpState::Failed,
                error: Some(ErrorCode::Conflict),
                message: Some("412 Precondition Failed".into()),
                localized: None,
            },
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["status"]["state"], "failed");
        assert!(value["status"].get("localized").is_none());
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), status);
        assert_eq!(
            serde_json::to_value(Event::RemindersChanged).unwrap(),
            json!({"type": "RemindersChanged"})
        );
        let free = Response::FreeTime {
            slots: vec![FreeSlot {
                start: "2026-10-01T10:00:00Z".into(),
                end: "2026-10-01T11:00:00Z".into(),
            }],
        };
        let value = serde_json::to_value(&free).unwrap();
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), free);
    }

    #[test]
    fn m3c_requests_responses_and_events_serialize() {
        let classify = Request::ClassifyFocused {
            message_ids: vec!["m1".into()],
            classification: InferenceClassification::Other,
            always: true,
        };
        assert_eq!(
            serde_json::to_value(&classify).unwrap(),
            json!({"type": "ClassifyFocused", "message_ids": ["m1"], "classification": "other", "always": true})
        );
        let parsed: Request = serde_json::from_value(json!({
            "type": "FindRooms", "account_id": "work",
            "from": "2026-10-05T08:00:00Z", "to": "2026-10-05T09:00:00Z"
        }))
        .unwrap();
        let Request::FindRooms {
            room_list,
            min_capacity,
            refresh,
            ..
        } = parsed
        else {
            panic!("FindRooms");
        };
        assert_eq!((room_list, min_capacity, refresh), (None, None, false));
        let add = Request::AddExchangeMailbox {
            account_id: "work".into(),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        };
        let value = serde_json::to_value(&add).unwrap();
        assert_eq!(value["settings"]["kind"], json!("shared"));
        assert_eq!(serde_json::from_value::<Request>(value).unwrap(), add);
        let set = Request::SetAutomaticReplies {
            account_id: "work".into(),
            settings: AutomaticRepliesSettings::default(),
        };
        let value = serde_json::to_value(&set).unwrap();
        assert_eq!(value["settings"]["status"], json!("disabled"));
        let changed = Event::ExchangeChanged {
            account_id: "work".into(),
            areas: vec![ExchangeArea::Categories, ExchangeArea::Rules],
        };
        assert_eq!(
            serde_json::to_value(&changed).unwrap(),
            json!({"type": "ExchangeChanged", "account_id": "work", "areas": ["categories", "rules"]})
        );
        let lists = Response::RoomLists {
            account_id: "work".into(),
            lists: vec![RoomList {
                address: "building1@contoso.example".into(),
                name: "Building 1".into(),
            }],
        };
        let value = serde_json::to_value(&lists).unwrap();
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), lists);
    }

    #[test]
    fn m4a_requests_responses_and_events_serialize() {
        let list: Request =
            serde_json::from_value(json!({"type": "ListConfirmations", "limit": 20})).unwrap();
        assert_eq!(
            list,
            Request::ListConfirmations {
                filter: ConfirmationFilter::Pending,
                actor: None,
                before: None,
                limit: 20,
            }
        );
        let decide = Request::DecideConfirmation {
            confirmation_id: "7".into(),
            decision: ConfirmationDecision::Approve,
            always_allow: true,
        };
        assert_eq!(
            serde_json::to_value(&decide).unwrap(),
            json!({"type": "DecideConfirmation", "confirmation_id": "7",
                   "decision": "approve", "always_allow": true})
        );
        let set = Request::SetActionPolicy {
            actor: ActionActor::mcp_client("c1"),
            kind: ActionKind::Send,
            mode: Some(ActionPolicyMode::Allow),
        };
        let value = serde_json::to_value(&set).unwrap();
        assert_eq!(
            value["actor"],
            json!({"kind": "mcp-client", "client_id": "c1"})
        );
        assert_eq!(serde_json::from_value::<Request>(value).unwrap(), set);
        let policies = Response::ActionPolicies {
            actor: ActionActor::plugin("follow-up"),
            policies: ActionPolicy::table(&[]),
        };
        let value = serde_json::to_value(&policies).unwrap();
        assert_eq!(
            value["policies"].as_array().unwrap().len(),
            ActionKind::ALL.len()
        );
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), policies);
        let action: Request = serde_json::from_value(json!({
            "type": "RunPluginAction", "action": "acme-crm.save",
            "target": {"kind": "attachment", "message_id": "m1", "index": 0}
        }))
        .unwrap();
        assert!(matches!(
            action,
            Request::RunPluginAction {
                client_id: None,
                ..
            }
        ));
        let vetoed = Response::SendVetoed {
            draft_id: "d1".into(),
            veto_id: "v1".into(),
            vetoes: vec![PluginSendVeto {
                plugin_id: "attachment-check".into(),
                plugin_name: "Attachment check".into(),
                reason: "You mention an attachment but none is attached.".into(),
            }],
        };
        let value = serde_json::to_value(&vetoed).unwrap();
        assert_eq!(value["vetoes"][0]["plugin_name"], "Attachment check");
        assert_eq!(serde_json::from_value::<Response>(value).unwrap(), vetoed);
        let edit = Event::ComposerEditRequested {
            draft_id: "d1".into(),
            client_id: None,
            by: Box::new(ActionAttribution {
                actor: ActionActor::mcp_client("c1"),
                name: "Claude".into(),
                on_behalf_of: None,
            }),
            edit: ComposerEdit {
                op: crate::plugins::ComposerEditOp::Insert,
                text: "Hello".into(),
                format: crate::plugins::ComposerTextFormat::Markdown,
            },
        };
        let value = serde_json::to_value(&edit).unwrap();
        assert_eq!(
            value["edit"],
            json!({"op": "insert", "text": "Hello", "format": "markdown"})
        );
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), edit);
        let limits: Request = serde_json::from_value(json!({
            "type": "UpdateMcpRateLimits", "client_id": "c1", "limits": {"sends_per_hour": null}
        }))
        .unwrap();
        let Request::UpdateMcpRateLimits { limits, .. } = limits else {
            panic!("wrong request");
        };
        assert_eq!(limits.sends_per_hour, None);
        assert_eq!(limits.requests_per_minute, Some(120));
        let banner_action: Request = serde_json::from_value(json!({
            "type": "PluginUiAction", "component_id": "open-crm", "values": {},
            "target": {"kind": "banner", "message_id": "m1", "banner": "acme-crm.customer"}
        }))
        .unwrap();
        assert!(matches!(
            banner_action,
            Request::PluginUiAction {
                target: PluginUiTarget::Banner { .. },
                ..
            }
        ));
    }

    #[test]
    fn m4c_requests_responses_and_events_serialize() {
        use crate::portability::{ImportFormat, JobPhase};
        let import: Request = serde_json::from_value(json!({
            "type": "StartImport",
            "source": {"format": "maildir", "path": "C:/Mail"}
        }))
        .unwrap();
        let Request::StartImport { source, options } = import else {
            panic!("StartImport expected");
        };
        assert_eq!(source.format, ImportFormat::Maildir);
        assert!(options.skip_duplicates);
        let backup: Request = serde_json::from_value(json!({
            "type": "CreateBackup", "path": "C:/b.nailbackup",
            "passphrase": "correct horse battery"
        }))
        .unwrap();
        assert!(matches!(
            backup,
            Request::CreateBackup { contents, .. } if contents == BackupContents::default()
        ));
        assert_eq!(
            serde_json::to_value(Request::AnswerPinPrompt {
                prompt_id: "p1".into(),
                pin: None
            })
            .unwrap(),
            json!({"type": "AnswerPinPrompt", "prompt_id": "p1", "pin": null})
        );
        assert_eq!(
            serde_json::to_value(Request::RecordActionUsed {
                action_id: "mail.reply".into()
            })
            .unwrap(),
            json!({"type": "RecordActionUsed", "action_id": "mail.reply"})
        );
        let print: Request = serde_json::from_value(json!({
            "type": "CreatePrintJob",
            "target": {"type": "thread", "thread_id": "t1"}
        }))
        .unwrap();
        assert!(
            matches!(print, Request::CreatePrintJob { options, .. } if options == PrintOptions::default())
        );
        assert_eq!(
            serde_json::to_value(Response::PathPicked { path: None }).unwrap(),
            json!({"type": "PathPicked", "path": null})
        );
        assert_eq!(
            serde_json::to_value(Event::PortabilityJobProgress {
                job_id: "j1".into(),
                progress: JobProgress {
                    phase: JobPhase::Writing,
                    done: 3,
                    total: Some(10),
                    bytes_done: 0,
                    bytes_total: None,
                    current: Some("Inbox".into()),
                },
            })
            .unwrap(),
            json!({
                "type": "PortabilityJobProgress", "job_id": "j1",
                "progress": {"phase": "writing", "done": 3, "total": 10, "bytes_done": 0,
                             "bytes_total": null, "current": "Inbox"}
            })
        );
        assert_eq!(
            serde_json::to_value(Event::SendProgress {
                outbox_id: "o1".into(),
                sent_bytes: 4,
                total_bytes: 8
            })
            .unwrap(),
            json!({"type": "SendProgress", "outbox_id": "o1", "sent_bytes": 4, "total_bytes": 8})
        );
    }
}
