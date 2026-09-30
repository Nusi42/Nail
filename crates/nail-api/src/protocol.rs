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
    SecretInput, VirtualAccountConfig,
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
use crate::domain::{Account, Address, Folder, MessageBody, MessageSummary, ThreadSummary};
use crate::error::{ApiError, ErrorCode};
use crate::ids::{
    AccountId, ConfigBackupId, DraftId, FolderId, IdentityId, KeyId, McpClientId, MessageId,
    OAuthFlowId, OpId, OutboxId, PairingId, PasskeyId, PluginDialogId, PluginId, RemoteDeviceId,
    RemoteSessionId, RemoteUserId, RuleId, RuleRunId, SavedSearchId, ThreadId,
};
use crate::launch::LaunchRequest;
use crate::localized::LocalizedMessage;
use crate::mcp::{McpAuditEntry, McpClient, McpGrant, McpServerInfo, McpSettings};
use crate::ops::{FlagChange, MoveTarget, OpStatus};
use crate::plugins::runtime::PluginCommandSurface;
use crate::plugins::{
    PluginDialog, PluginGrant, PluginInfo, PluginInstallPreview, PluginLogEntry,
    PluginNotification, PluginPackageSource, PluginSettingDef, PluginStopReason, PluginUiNode,
    PluginUiTarget,
};
use crate::remote::{
    PairingCode, RemoteAlert, RemoteAuditEntry, RemoteDevice, RemoteGrant, RemoteSession,
    RemoteSessionFilter, RemoteUiSettings, RemoteUiStatus, RemoteUser, RemoteUserUpdate,
    SessionEndReason, TotpEnrollment,
};
use crate::remote_content::{RemoteContentAllow, RemoteContentScope};
use crate::rules::{Rule, RuleMatch, RuleRun};
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
                email: "ada@example.org".into()
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
        assert_eq!(
            crypto.technology,
            Some(crate::security::CryptoTechnology::OpenPgp)
        );

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
}
