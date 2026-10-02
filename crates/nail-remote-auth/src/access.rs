//! Authorization of remote sessions: which scope (or `admin`) every
//! [`Request`] needs ([`required_access`], an exhaustive match, so a new
//! request cannot be forgotten), and the check itself
//! ([`RemoteAuth::authorize`]) with its audit entries.
//!
//! Scopes are the MCP scopes ([`McpScope`]); the grant's account list and
//! folder exclusions are enforced by the core when it answers (like the
//! MCP adapter does), see [`account_allowed`] / [`folder_allowed`].

use nail_api::remote::{RemoteAuditKind, RemoteAuditOutcome};
use nail_api::{
    AccountId, ApiError, ErrorCode, FolderId, McpScope, RemoteDeviceId, RemoteSessionId,
    RemoteUserId, Request,
};

use crate::service::RemoteAuth;
use crate::session::RemoteSessionCtx;

/// What a remote session needs for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Any valid session (no mail data involved).
    Session,
    /// The scope.
    Scope(McpScope),
    /// The scope and a fresh (re-)authentication.
    Sensitive(McpScope),
    /// `admin` and a fresh (re-)authentication.
    Admin,
    /// The session's own user, device or session (self-service, no
    /// re-authentication), else `admin` and a fresh (re-)authentication.
    AdminOrOwn(Own),
    /// Never from a remote session (it acts on the desktop itself).
    DesktopOnly,
}

/// The target of a self-service request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Own {
    /// Lists filtered to this user (`None` = everyone's: admin only).
    User(Option<RemoteUserId>),
    /// This device.
    Device(RemoteDeviceId),
    /// This session.
    Session(RemoteSessionId),
}

/// A request's name (for the audit log) and what it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAccess {
    /// The request type (`ListMessages`, …).
    pub name: &'static str,
    /// What it needs.
    pub access: Access,
}

impl RequestAccess {
    /// Whether allowed requests of this kind are audited (everything that
    /// writes or administers; plain reads are not).
    pub fn audited(&self) -> bool {
        !matches!(
            self.access,
            Access::Session | Access::Scope(McpScope::Read) | Access::Scope(McpScope::Context)
        ) && !self.name.starts_with("List")
            && !self.name.starts_with("Get")
    }
}

/// What a remote session needs for `request`. Exhaustive on purpose.
pub fn required_access(request: &Request) -> RequestAccess {
    use Access::{Admin, AdminOrOwn, DesktopOnly, Scope, Sensitive, Session};
    use McpScope as S;
    let (name, access) = match request {
        // --- Basics ---
        Request::Ping => ("Ping", Session),
        Request::GetConfig => ("GetConfig", Session),
        Request::ListAccounts => ("ListAccounts", Scope(S::Read)),
        Request::ListFolders { .. } => ("ListFolders", Scope(S::Read)),
        Request::ListMessages { .. } => ("ListMessages", Scope(S::Read)),
        Request::GetMessage { .. } => ("GetMessage", Scope(S::Read)),
        Request::SyncNow { .. } => ("SyncNow", Scope(S::Read)),

        // --- Account setup: administration; OAuth runs on the desktop ---
        Request::DiscoverAccount { .. } => ("DiscoverAccount", Admin),
        Request::TestAccount { .. } => ("TestAccount", Admin),
        Request::StartOAuth { .. } => ("StartOAuth", DesktopOnly),
        Request::CancelOAuth { .. } => ("CancelOAuth", DesktopOnly),
        Request::GetAccountsConfig => ("GetAccountsConfig", Admin),
        Request::AddAccount { .. } => ("AddAccount", Admin),
        Request::UpdateAccount { .. } => ("UpdateAccount", Admin),
        Request::RemoveAccount { .. } => ("RemoveAccount", Admin),
        Request::ListIdentities { .. } => ("ListIdentities", Scope(S::Read)),
        Request::ChooseIdentity { .. } => ("ChooseIdentity", Scope(S::Read)),
        Request::ListVirtualAccounts => ("ListVirtualAccounts", Scope(S::Read)),
        Request::AddVirtualAccount { .. } => ("AddVirtualAccount", Admin),
        Request::UpdateVirtualAccount { .. } => ("UpdateVirtualAccount", Admin),
        Request::RemoveVirtualAccount { .. } => ("RemoveVirtualAccount", Admin),
        Request::ListAccountStatuses => ("ListAccountStatuses", Scope(S::Read)),

        // --- Message writes ---
        Request::UpdateFlags { .. } => ("UpdateFlags", Scope(S::Organize)),
        Request::MoveMessages { .. } => ("MoveMessages", Scope(S::Organize)),
        Request::DeleteMessages { permanent, .. } => (
            "DeleteMessages",
            Scope(if *permanent {
                S::PermanentDelete
            } else {
                S::Organize
            }),
        ),

        // --- Threads and search ---
        Request::ListThreads { .. } => ("ListThreads", Scope(S::Read)),
        Request::GetThread { .. } => ("GetThread", Scope(S::Read)),
        Request::Search { .. } => ("Search", Scope(S::Read)),
        Request::CheckQuery { .. } => ("CheckQuery", Session),

        // --- Drafts and sending ---
        Request::SaveDraft { .. } => ("SaveDraft", Scope(S::Draft)),
        Request::GetDraft { .. } => ("GetDraft", Scope(S::Draft)),
        Request::GetDraftAttachment { .. } => ("GetDraftAttachment", Scope(S::Draft)),
        Request::ListDrafts => ("ListDrafts", Scope(S::Draft)),
        Request::DeleteDraft { .. } => ("DeleteDraft", Scope(S::Draft)),
        Request::SendDraft { .. } => ("SendDraft", Scope(S::Send)),
        Request::CancelSend { .. } => ("CancelSend", Scope(S::Send)),
        Request::ListOutbox => ("ListOutbox", Scope(S::Read)),
        Request::RetryOp { .. } => ("RetryOp", Scope(S::Organize)),

        // --- Composer ---
        Request::NewDraft { .. } => ("NewDraft", Scope(S::Draft)),
        Request::RenderPreview { .. } => ("RenderPreview", Scope(S::Draft)),
        Request::ConvertDraft { .. } => ("ConvertDraft", Scope(S::Draft)),
        Request::UploadAttachment { .. } => ("UploadAttachment", Scope(S::Draft)),
        Request::SuggestAddresses { .. } => ("SuggestAddresses", Scope(S::Read)),

        // --- Rules ---
        Request::ListRules => ("ListRules", Scope(S::Read)),
        Request::SaveRule { .. } => ("SaveRule", Scope(S::Rules)),
        Request::DeleteRule { .. } => ("DeleteRule", Scope(S::Rules)),
        Request::ReorderRules { .. } => ("ReorderRules", Scope(S::Rules)),
        Request::TestRule { .. } => ("TestRule", Scope(S::Rules)),
        Request::RunRule { .. } => ("RunRule", Scope(S::Rules)),
        Request::ListRuleRuns { .. } => ("ListRuleRuns", Scope(S::Read)),
        Request::UndoRuleRun { .. } => ("UndoRuleRun", Scope(S::Rules)),
        Request::RuleToText { .. } => ("RuleToText", Scope(S::Rules)),
        Request::RuleFromText { .. } => ("RuleFromText", Scope(S::Rules)),

        // --- MCP clients ---
        Request::ListMcpClients => ("ListMcpClients", Admin),
        Request::CreateMcpClient { .. } => ("CreateMcpClient", Admin),
        Request::UpdateMcpGrants { .. } => ("UpdateMcpGrants", Admin),
        Request::RevokeMcpClient { .. } => ("RevokeMcpClient", Admin),
        Request::ListMcpAudit { .. } => ("ListMcpAudit", Admin),
        Request::GetMcpServerInfo => ("GetMcpServerInfo", Admin),

        // --- UI context: the desktop reports it; agents read it ---
        Request::ReportUiContext { .. } => ("ReportUiContext", DesktopOnly),
        Request::GetUiContext => ("GetUiContext", Scope(S::Context)),

        // --- Customization: the desktop's files ---
        Request::UpdateLayout { .. } => ("UpdateLayout", Admin),
        Request::PreviewConfigMerge { .. } => ("PreviewConfigMerge", Admin),
        Request::ResolveConfigMerge { .. } => ("ResolveConfigMerge", Admin),
        Request::ApplyConfigMerge { .. } => ("ApplyConfigMerge", Admin),
        Request::ListConfigBackups { .. } => ("ListConfigBackups", Admin),
        Request::RevertConfig { .. } => ("RevertConfig", Admin),
        Request::OpenConfigFile { .. } => ("OpenConfigFile", DesktopOnly),
        Request::ListSavedSearches => ("ListSavedSearches", Scope(S::Read)),
        Request::SaveSavedSearch { .. } => ("SaveSavedSearch", Scope(S::Organize)),
        Request::DeleteSavedSearch { .. } => ("DeleteSavedSearch", Scope(S::Organize)),

        // --- Keys and certificates ---
        Request::ListKeys { .. } => ("ListKeys", Scope(S::Read)),
        Request::GetKey { .. } => ("GetKey", Scope(S::Read)),
        Request::ImportKeys { .. } => ("ImportKeys", Admin),
        Request::ImportPkcs12 { .. } => ("ImportPkcs12", Admin),
        Request::ExportPublicKey { .. } => ("ExportPublicKey", Scope(S::Read)),
        Request::GenerateKey { .. } => ("GenerateKey", Admin),
        Request::DeleteKey { .. } => ("DeleteKey", Admin),
        Request::SetKeyTrust { .. } => ("SetKeyTrust", Admin),
        Request::UnlockKey { .. } => ("UnlockKey", Sensitive(S::Encrypted)),
        Request::LookupKeys { .. } => ("LookupKeys", Scope(S::Read)),
        Request::ListWindowsCertificates { .. } => ("ListWindowsCertificates", Admin),
        Request::ImportWindowsCertificate { .. } => ("ImportWindowsCertificate", Admin),

        // --- Composing and Autocrypt ---
        Request::CheckRecipientKeys { .. } => ("CheckRecipientKeys", Scope(S::Draft)),
        // The setup message carries the secret key (encrypted).
        Request::SendAutocryptSetup { .. } => ("SendAutocryptSetup", Admin),
        Request::ImportAutocryptSetup { .. } => ("ImportAutocryptSetup", Admin),

        // --- Security settings and encryption at rest ---
        Request::GetSecuritySettings => ("GetSecuritySettings", Scope(S::Read)),
        Request::UpdateSecuritySettings { .. } => ("UpdateSecuritySettings", Admin),
        Request::SuggestAuthservId { .. } => ("SuggestAuthservId", Admin),
        Request::GetVaultStatus => ("GetVaultStatus", Scope(S::Read)),
        Request::SetVaultEncryption { .. } => ("SetVaultEncryption", Admin),

        // --- Remote UI administration (self-service where noted) ---
        Request::GetRemoteUiSettings => ("GetRemoteUiSettings", Admin),
        Request::UpdateRemoteUiSettings { .. } => ("UpdateRemoteUiSettings", Admin),
        Request::RegenerateRemoteCertificate => ("RegenerateRemoteCertificate", Admin),
        Request::ListRemoteUsers => ("ListRemoteUsers", Admin),
        Request::CreateRemoteUser { .. } => ("CreateRemoteUser", Admin),
        Request::UpdateRemoteUser { .. } => ("UpdateRemoteUser", Admin),
        Request::DeleteRemoteUser { .. } => ("DeleteRemoteUser", Admin),
        Request::SetRemotePassword { .. } => ("SetRemotePassword", Admin),
        Request::StartTotpEnrollment { .. } => ("StartTotpEnrollment", Admin),
        Request::ConfirmTotpEnrollment { .. } => ("ConfirmTotpEnrollment", Admin),
        Request::DisableTotp { .. } => ("DisableTotp", Admin),
        Request::UnlockRemoteUser { .. } => ("UnlockRemoteUser", Admin),
        Request::RenamePasskey { .. } => ("RenamePasskey", Admin),
        Request::DeletePasskey { .. } => ("DeletePasskey", Admin),
        Request::ListRemoteDevices { user_id } => {
            ("ListRemoteDevices", AdminOrOwn(Own::User(user_id.clone())))
        }
        Request::UpdateRemoteDevice { .. } => ("UpdateRemoteDevice", Admin),
        Request::RevokeRemoteDevice { device_id } => (
            "RevokeRemoteDevice",
            AdminOrOwn(Own::Device(device_id.clone())),
        ),
        Request::ListRemoteSessions { filter } => (
            "ListRemoteSessions",
            AdminOrOwn(Own::User(filter.user_id.clone())),
        ),
        Request::RevokeRemoteSession { session_id } => (
            "RevokeRemoteSession",
            AdminOrOwn(Own::Session(session_id.clone())),
        ),
        Request::RevokeAllRemoteSessions { .. } => ("RevokeAllRemoteSessions", Admin),
        Request::CreatePairingCode { .. } => ("CreatePairingCode", Admin),
        Request::ListPairingCodes => ("ListPairingCodes", Admin),
        Request::CancelPairingCode { .. } => ("CancelPairingCode", Admin),
        Request::ListRemoteAudit { .. } => ("ListRemoteAudit", Admin),

        // --- Remote content (a reader choice) ---
        Request::AllowRemoteContent { .. } => ("AllowRemoteContent", Scope(S::Read)),
        Request::ListRemoteContentAllows => ("ListRemoteContentAllows", Scope(S::Read)),
        Request::RemoveRemoteContentAllow { .. } => ("RemoveRemoteContentAllow", Scope(S::Read)),

        // --- Launch requests: they read local files of the desktop ---
        Request::TakeLaunchRequests => ("TakeLaunchRequests", DesktopOnly),
        Request::ImportMessageFile { .. } => ("ImportMessageFile", DesktopOnly),

        // --- Server-side rules and vacation (M2c): rule placements and the
        // script reveal every rule, like `ListRules` ---
        Request::GetSieveStatus { .. } => ("GetSieveStatus", Scope(S::Read)),
        Request::SyncSieve { .. } => ("SyncSieve", Admin),
        Request::PreviewSieveScript { .. } => ("PreviewSieveScript", Scope(S::Rules)),
        Request::GetVacation { .. } => ("GetVacation", Scope(S::Read)),
        Request::UpdateVacation { .. } => ("UpdateVacation", Admin),

        // --- Junk and snooze: organizing mail; settings are administration ---
        Request::MarkJunk { .. } => ("MarkJunk", Scope(S::Organize)),
        Request::GetJunkStatus { .. } => ("GetJunkStatus", Scope(S::Read)),
        Request::UpdateJunkSettings { .. } => ("UpdateJunkSettings", Admin),
        Request::ResetJunkClassifier { .. } => ("ResetJunkClassifier", Admin),
        Request::SnoozeMessages { .. } => ("SnoozeMessages", Scope(S::Organize)),
        Request::UnsnoozeMessages { .. } => ("UnsnoozeMessages", Scope(S::Organize)),
        Request::ListSnoozed { .. } => ("ListSnoozed", Scope(S::Read)),

        // --- Notifications ---
        Request::GetNotificationSettings => ("GetNotificationSettings", Scope(S::Read)),
        Request::UpdateNotificationSettings { .. } => ("UpdateNotificationSettings", Admin),
        Request::TestNotification => ("TestNotification", Session),

        // --- Sync policy (account settings) and bodies (reading) ---
        Request::GetFolderSyncPolicy { .. } => ("GetFolderSyncPolicy", Scope(S::Read)),
        Request::SetFolderSyncPolicy { .. } => ("SetFolderSyncPolicy", Admin),
        Request::FetchMessageBodies { .. } => ("FetchMessageBodies", Scope(S::Read)),

        // --- MCP settings ---
        Request::GetMcpSettings => ("GetMcpSettings", Admin),
        Request::UpdateMcpSettings { .. } => ("UpdateMcpSettings", Admin),

        // --- Rules from messages ---
        Request::RuleFromMessage { .. } => ("RuleFromMessage", Scope(S::Rules)),

        // --- Plug-ins (M2d, ADR 0050). Plug-ins read mail with their own
        // grants, so using them needs a grant for every account (the core
        // checks `full`); administering them is `admin`. Loading a dev
        // folder reads the desktop's files. ---
        Request::ListPlugins => ("ListPlugins", Scope(S::Read)),
        Request::GetPlugin { .. } => ("GetPlugin", Scope(S::Read)),
        Request::PreviewPluginInstall { .. } => ("PreviewPluginInstall", Admin),
        Request::InstallPlugin { .. } => ("InstallPlugin", Admin),
        Request::LoadDevPlugin { .. } => ("LoadDevPlugin", DesktopOnly),
        Request::EnablePlugin { .. } => ("EnablePlugin", Admin),
        Request::DisablePlugin { .. } => ("DisablePlugin", Admin),
        Request::UninstallPlugin { .. } => ("UninstallPlugin", Admin),
        Request::UpdatePluginGrants { .. } => ("UpdatePluginGrants", Admin),
        Request::ListPluginLogs { .. } => ("ListPluginLogs", Admin),
        Request::ClearPluginLogs { .. } => ("ClearPluginLogs", Admin),
        Request::GetPluginSettings { .. } => ("GetPluginSettings", Admin),
        Request::UpdatePluginSettings { .. } => ("UpdatePluginSettings", Admin),
        Request::PreviewPluginPlacements { .. } => ("PreviewPluginPlacements", Admin),
        Request::RunPluginCommand { .. } => ("RunPluginCommand", Scope(S::Read)),
        Request::RespondPluginDialog { .. } => ("RespondPluginDialog", Scope(S::Read)),
        Request::PluginUiAction { .. } => ("PluginUiAction", Scope(S::Read)),
        Request::GetPluginPanel { .. } => ("GetPluginPanel", Scope(S::Read)),

        // --- Calendars, contacts and tasks (M3a, ADR 0058): reads are
        // `read`, event / calendar / reminder writes `calendar`, contact
        // and task writes `pim` (an import of events also needs `calendar`,
        // checked by the core), sources are administration. ---
        Request::ListPimSources => ("ListPimSources", Scope(S::Read)),
        Request::AddPimSource { .. } => ("AddPimSource", Admin),
        Request::UpdatePimSource { .. } => ("UpdatePimSource", Admin),
        Request::RemovePimSource { .. } => ("RemovePimSource", Admin),
        Request::TestPimSource { .. } => ("TestPimSource", Admin),
        Request::SyncPimSources { .. } => ("SyncPimSources", Scope(S::Read)),
        Request::ListCalendars { .. } => ("ListCalendars", Scope(S::Read)),
        Request::CreateCalendar { .. } => ("CreateCalendar", Scope(S::Calendar)),
        Request::UpdateCalendar { .. } => ("UpdateCalendar", Scope(S::Calendar)),
        Request::DeleteCalendar { .. } => ("DeleteCalendar", Sensitive(S::Calendar)),
        Request::ListEventInstances { .. } => ("ListEventInstances", Scope(S::Read)),
        Request::GetEvent { .. } => ("GetEvent", Scope(S::Read)),
        Request::CreateEvent { .. } => ("CreateEvent", Scope(S::Calendar)),
        Request::UpdateEvent { .. } => ("UpdateEvent", Scope(S::Calendar)),
        Request::DeleteEvent { .. } => ("DeleteEvent", Scope(S::Calendar)),
        Request::MoveEvent { .. } => ("MoveEvent", Scope(S::Calendar)),
        Request::EventFromMessage { .. } => ("EventFromMessage", Scope(S::Read)),
        Request::FindFreeTime { .. } => ("FindFreeTime", Scope(S::Read)),
        Request::ListAddressBooks { .. } => ("ListAddressBooks", Scope(S::Read)),
        Request::CreateAddressBook { .. } => ("CreateAddressBook", Scope(S::Pim)),
        Request::UpdateAddressBook { .. } => ("UpdateAddressBook", Scope(S::Pim)),
        Request::DeleteAddressBook { .. } => ("DeleteAddressBook", Sensitive(S::Pim)),
        Request::ListContacts { .. } => ("ListContacts", Scope(S::Read)),
        Request::GetContact { .. } => ("GetContact", Scope(S::Read)),
        Request::CreateContact { .. } => ("CreateContact", Scope(S::Pim)),
        Request::UpdateContact { .. } => ("UpdateContact", Scope(S::Pim)),
        Request::DeleteContacts { .. } => ("DeleteContacts", Scope(S::Pim)),
        Request::MoveContact { .. } => ("MoveContact", Scope(S::Pim)),
        Request::SearchContacts { .. } => ("SearchContacts", Scope(S::Read)),
        Request::FindDuplicateContacts { .. } => ("FindDuplicateContacts", Scope(S::Read)),
        Request::MergeContacts { .. } => ("MergeContacts", Scope(S::Pim)),
        Request::UnlinkContact { .. } => ("UnlinkContact", Scope(S::Pim)),
        Request::GetContactPhoto { .. } => ("GetContactPhoto", Scope(S::Read)),
        Request::ListContactGroups { .. } => ("ListContactGroups", Scope(S::Read)),
        Request::CreateContactGroup { .. } => ("CreateContactGroup", Scope(S::Pim)),
        Request::UpdateContactGroup { .. } => ("UpdateContactGroup", Scope(S::Pim)),
        Request::DeleteContactGroup { .. } => ("DeleteContactGroup", Scope(S::Pim)),
        Request::ListTasks { .. } => ("ListTasks", Scope(S::Read)),
        Request::GetTask { .. } => ("GetTask", Scope(S::Read)),
        Request::CreateTask { .. } => ("CreateTask", Scope(S::Pim)),
        Request::UpdateTask { .. } => ("UpdateTask", Scope(S::Pim)),
        Request::CompleteTask { .. } => ("CompleteTask", Scope(S::Pim)),
        Request::MoveTask { .. } => ("MoveTask", Scope(S::Pim)),
        Request::DeleteTask { .. } => ("DeleteTask", Scope(S::Pim)),
        Request::TaskFromMessage { .. } => ("TaskFromMessage", Scope(S::Read)),
        Request::SearchPim { .. } => ("SearchPim", Scope(S::Read)),
        Request::PreviewPimImport { .. } => ("PreviewPimImport", Scope(S::Read)),
        Request::ImportPim { .. } => ("ImportPim", Scope(S::Pim)),
        Request::ResolvePimConflict { .. } => ("ResolvePimConflict", Scope(S::Pim)),
        Request::ListReminders { .. } => ("ListReminders", Scope(S::Read)),
        Request::SnoozeReminders { .. } => ("SnoozeReminders", Scope(S::Calendar)),
        Request::DismissReminders { .. } => ("DismissReminders", Scope(S::Calendar)),

        // --- Invitations and scheduling (M3b, ADR 0065): cards and
        // free/busy are reads; answering and organizing write the
        // calendar (`calendar`; the core's `check_remote_request` also
        // requires `send` whenever the request can mail an iTIP message:
        // answers with `send`, proposals, refreshes, forwards, organizer
        // requests, counters, applying a REFRESH); settings are
        // administration. ---
        Request::GetInvitation { .. } => ("GetInvitation", Scope(S::Read)),
        Request::RespondToInvitation { .. } => ("RespondToInvitation", Scope(S::Calendar)),
        Request::ProposeNewTime { .. } => ("ProposeNewTime", Scope(S::Calendar)),
        Request::RefreshInvitation { .. } => ("RefreshInvitation", Scope(S::Calendar)),
        Request::ForwardInvitation { .. } => ("ForwardInvitation", Scope(S::Calendar)),
        Request::ApplyInvitation { .. } => ("ApplyInvitation", Scope(S::Calendar)),
        Request::SendInvitations { .. } => ("SendInvitations", Scope(S::Calendar)),
        Request::CancelMeeting { .. } => ("CancelMeeting", Scope(S::Calendar)),
        Request::RespondToCounter { .. } => ("RespondToCounter", Scope(S::Calendar)),
        Request::QueryFreeBusy { .. } => ("QueryFreeBusy", Scope(S::Read)),
        Request::GetSchedulingSettings => ("GetSchedulingSettings", Scope(S::Read)),
        Request::SetSchedulingSettings { .. } => ("SetSchedulingSettings", Admin),

        // --- Gmail labels (M3b): like tags (`organize`). ---
        Request::ListGmailLabels { .. } => ("ListGmailLabels", Scope(S::Read)),
        Request::CreateGmailLabel { .. } => ("CreateGmailLabel", Scope(S::Organize)),
        Request::UpdateGmailLabel { .. } => ("UpdateGmailLabel", Scope(S::Organize)),
        Request::DeleteGmailLabel { .. } => ("DeleteGmailLabel", Scope(S::Organize)),

        // --- Exchange (M3c, ADR 0072): status, lists and rooms are reads;
        // other mailboxes are account configuration (`admin`); categories
        // and Focused Inbox like tags (`organize`); server rules like
        // Sieve and automatic replies like the vacation reply (`admin`;
        // the core also needs the account). ---
        Request::GetExchangeStatus { .. } => ("GetExchangeStatus", Scope(S::Read)),
        Request::ListExchangeMailboxes { .. } => ("ListExchangeMailboxes", Scope(S::Read)),
        Request::AddExchangeMailbox { .. } => ("AddExchangeMailbox", Admin),
        Request::UpdateExchangeMailbox { .. } => ("UpdateExchangeMailbox", Admin),
        Request::RemoveExchangeMailbox { .. } => ("RemoveExchangeMailbox", Admin),
        Request::ListExchangeCategories { .. } => ("ListExchangeCategories", Scope(S::Read)),
        Request::CreateExchangeCategory { .. } => ("CreateExchangeCategory", Scope(S::Organize)),
        Request::UpdateExchangeCategory { .. } => ("UpdateExchangeCategory", Scope(S::Organize)),
        Request::DeleteExchangeCategory { .. } => ("DeleteExchangeCategory", Scope(S::Organize)),
        Request::ClassifyFocused { .. } => ("ClassifyFocused", Scope(S::Organize)),
        Request::GetAutomaticReplies { .. } => ("GetAutomaticReplies", Scope(S::Read)),
        Request::SetAutomaticReplies { .. } => ("SetAutomaticReplies", Admin),
        Request::GetExchangeRules { .. } => ("GetExchangeRules", Scope(S::Read)),
        Request::SyncExchangeRules { .. } => ("SyncExchangeRules", Admin),
        Request::SetExchangeInboxRuleEnabled { .. } => ("SetExchangeInboxRuleEnabled", Admin),
        Request::DeleteExchangeInboxRule { .. } => ("DeleteExchangeInboxRule", Admin),
        Request::ListRoomLists { .. } => ("ListRoomLists", Scope(S::Read)),
        Request::FindRooms { .. } => ("FindRooms", Scope(S::Read)),

        // --- Confirmations and policies (M4a, ADR 0079): deciding for an
        // agent or plug-in, and relaxing its policies, is administration
        // (like MCP client grants); the core also needs every account a
        // confirmation names. Rate limits like grants. ---
        Request::ListConfirmations { .. } => ("ListConfirmations", Admin),
        Request::DecideConfirmation { .. } => ("DecideConfirmation", Admin),
        Request::ListActionPolicies { .. } => ("ListActionPolicies", Admin),
        Request::SetActionPolicy { .. } => ("SetActionPolicy", Admin),
        Request::UpdateMcpRateLimits { .. } => ("UpdateMcpRateLimits", Admin),
        // --- Plug-in extensions (M4a): activity and manual job runs are
        // plug-in administration; annotations and actions follow the
        // plug-in rules of M2d (a grant for every account), previews need
        // attachment bytes, "Send anyway" sends. ---
        Request::GetPluginActivity { .. } => ("GetPluginActivity", Admin),
        Request::RunPluginJob { .. } => ("RunPluginJob", Admin),
        Request::GetMessagePluginAnnotations { .. } => {
            ("GetMessagePluginAnnotations", Scope(S::Read))
        }
        Request::RenderAttachmentPreview { .. } => {
            ("RenderAttachmentPreview", Scope(S::Attachments))
        }
        Request::RunPluginAction { .. } => ("RunPluginAction", Scope(S::Read)),
        Request::OverrideSendVeto { .. } => ("OverrideSendVeto", Scope(S::Send)),

        // --- M4b desktop (ADR 0087): installation, updater, tray, default
        // apps and first run belong to the desktop that runs them ---
        Request::GetInstallation => ("GetInstallation", DesktopOnly),
        Request::GetUpdateStatus => ("GetUpdateStatus", DesktopOnly),
        Request::SetUpdateSettings { .. } => ("SetUpdateSettings", DesktopOnly),
        Request::CheckForUpdates => ("CheckForUpdates", DesktopOnly),
        Request::DownloadUpdate => ("DownloadUpdate", DesktopOnly),
        Request::InstallUpdate => ("InstallUpdate", DesktopOnly),
        Request::GetDesktopSettings => ("GetDesktopSettings", DesktopOnly),
        Request::SetDesktopSettings { .. } => ("SetDesktopSettings", DesktopOnly),
        Request::GetDefaultApps => ("GetDefaultApps", DesktopOnly),
        Request::OpenDefaultAppsSettings => ("OpenDefaultAppsSettings", DesktopOnly),
        Request::GetFirstRun => ("GetFirstRun", DesktopOnly),
        Request::CompleteFirstRun => ("CompleteFirstRun", DesktopOnly),
        // --- M4c (ADR 0094): paths on the desktop, the vault's recovery
        // key, smartcards and diagnostics belong to the desktop; the
        // palette's recent actions hold no mail data; print jobs only
        // name what the session may read anyway (checked by the core) ---
        Request::FindImportSources => ("FindImportSources", DesktopOnly),
        Request::PreviewImport { .. } => ("PreviewImport", DesktopOnly),
        Request::StartImport { .. } => ("StartImport", DesktopOnly),
        Request::StartExport { .. } => ("StartExport", DesktopOnly),
        Request::CreateBackup { .. } => ("CreateBackup", DesktopOnly),
        Request::InspectBackup { .. } => ("InspectBackup", DesktopOnly),
        Request::VerifyBackup { .. } => ("VerifyBackup", DesktopOnly),
        Request::PreviewRestore { .. } => ("PreviewRestore", DesktopOnly),
        Request::StartRestore { .. } => ("StartRestore", DesktopOnly),
        Request::ListPortabilityJobs => ("ListPortabilityJobs", DesktopOnly),
        Request::GetPortabilityJob { .. } => ("GetPortabilityJob", DesktopOnly),
        Request::CancelPortabilityJob { .. } => ("CancelPortabilityJob", DesktopOnly),
        Request::CreateRecoveryKey { .. } => ("CreateRecoveryKey", DesktopOnly),
        Request::VerifyRecoveryKey { .. } => ("VerifyRecoveryKey", DesktopOnly),
        Request::RestoreVaultKey { .. } => ("RestoreVaultKey", DesktopOnly),
        Request::RemoveRecoveryKey => ("RemoveRecoveryKey", DesktopOnly),
        Request::GetSmartcardSettings => ("GetSmartcardSettings", DesktopOnly),
        Request::SetSmartcardSettings { .. } => ("SetSmartcardSettings", DesktopOnly),
        Request::ListSmartcards => ("ListSmartcards", DesktopOnly),
        Request::UseSmartcardKey { .. } => ("UseSmartcardKey", DesktopOnly),
        Request::AnswerPinPrompt { .. } => ("AnswerPinPrompt", DesktopOnly),
        Request::SavePrintPdf { .. } => ("SavePrintPdf", DesktopOnly),
        Request::PreviewDiagnostics { .. } => ("PreviewDiagnostics", DesktopOnly),
        Request::CreateDiagnosticsBundle { .. } => ("CreateDiagnosticsBundle", DesktopOnly),
        Request::PickPath { .. } => ("PickPath", DesktopOnly),
        Request::GetRecentActions => ("GetRecentActions", Session),
        Request::RecordActionUsed { .. } => ("RecordActionUsed", Session),
        Request::ClearRecentActions => ("ClearRecentActions", Session),
        Request::CreatePrintJob { .. } => ("CreatePrintJob", Scope(S::Read)),
        Request::GetPrintJob { .. } => ("GetPrintJob", Scope(S::Read)),
        Request::PrintViewReady { .. } => ("PrintViewReady", Scope(S::Read)),
    };
    RequestAccess { name, access }
}

/// Whether a grant reaches `account` (the core applies this to every
/// answer, like the MCP adapter).
pub fn account_allowed(grant: &nail_api::remote::RemoteGrant, account: &AccountId) -> bool {
    grant.mail.all_accounts || grant.mail.accounts.contains(account)
}

/// Whether a grant hides `folder`.
pub fn folder_allowed(grant: &nail_api::remote::RemoteGrant, folder: &FolderId) -> bool {
    !grant.mail.excluded_folders.contains(folder)
}

/// Why a remote request is refused.
fn denial(code: ErrorCode, message: String) -> ApiError {
    ApiError::new(code, message)
}

impl RemoteAuth {
    /// Checks that the session `ctx` may send `request` now: the scope (or
    /// `admin`) of its grant, a fresh re-authentication for admin and
    /// sensitive requests, ownership for self-service. Denials answer
    /// `forbidden` or `reauth_required` and are always audited; allowed
    /// writes are audited too.
    pub fn authorize(&self, ctx: &RemoteSessionCtx, request: &Request) -> Result<(), ApiError> {
        let now = self.now();
        let required = required_access(request);
        let grant = ctx.grant();
        let fresh = ctx.is_fresh(now);
        let needs_admin = || -> Result<(), ApiError> {
            if !grant.admin {
                return Err(denial(
                    ErrorCode::Forbidden,
                    format!("{} needs the admin grant", required.name),
                ));
            }
            if !fresh {
                return Err(denial(
                    ErrorCode::ReauthRequired,
                    format!("{} needs a fresh re-authentication", required.name),
                ));
            }
            Ok(())
        };
        let needs_scope = |scope: McpScope| -> Result<(), ApiError> {
            if grant.mail.has(scope) && !grant.mail.is_empty() {
                Ok(())
            } else {
                let scope = serde_json::to_value(scope)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                Err(denial(
                    ErrorCode::Forbidden,
                    format!("{} needs the {scope} scope", required.name),
                ))
            }
        };
        let result = match &required.access {
            Access::Session => Ok(()),
            Access::Scope(scope) => needs_scope(*scope),
            Access::Sensitive(scope) => needs_scope(*scope).and_then(|()| {
                if fresh {
                    Ok(())
                } else {
                    Err(denial(
                        ErrorCode::ReauthRequired,
                        format!("{} needs a fresh re-authentication", required.name),
                    ))
                }
            }),
            Access::Admin => needs_admin(),
            Access::AdminOrOwn(own) => {
                if self.owns(ctx, own) {
                    Ok(())
                } else if grant.admin {
                    needs_admin()
                } else {
                    Err(denial(
                        ErrorCode::Forbidden,
                        format!(
                            "{} is limited to your own devices and sessions",
                            required.name
                        ),
                    ))
                }
            }
            Access::DesktopOnly => Err(denial(
                ErrorCode::Forbidden,
                format!("{} is only available on the desktop", required.name),
            )),
        };
        let session = &ctx.session;
        match &result {
            Err(error) => self.audit(
                RemoteAuditKind::Request,
                RemoteAuditOutcome::Denied,
                &ctx.client,
                |r| {
                    r.user_id = Some(session.user_id.clone());
                    r.user_name = Some(session.user_name.clone());
                    r.device_id = Some(session.device_id.clone());
                    r.session_id = Some(session.id.clone());
                    r.name = Some(required.name.to_owned());
                    r.error = Some(error.code);
                    r.detail = Some(error.message.clone());
                },
            ),
            Ok(()) if required.audited() => self.audit(
                RemoteAuditKind::Request,
                RemoteAuditOutcome::Success,
                &ctx.client,
                |r| {
                    r.user_id = Some(session.user_id.clone());
                    r.user_name = Some(session.user_name.clone());
                    r.device_id = Some(session.device_id.clone());
                    r.session_id = Some(session.id.clone());
                    r.name = Some(required.name.to_owned());
                },
            ),
            Ok(()) => {}
        }
        result
    }

    fn owns(&self, ctx: &RemoteSessionCtx, own: &Own) -> bool {
        let user = &ctx.session.user_id;
        match own {
            Own::User(filter) => filter.as_ref() == Some(user),
            Own::Device(device) => self
                .store()
                .get_remote_device(device)
                .is_ok_and(|d| d.user_id == *user),
            Own::Session(session) => self
                .store()
                .get_remote_session(session)
                .is_ok_and(|s| s.user_id == *user),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::MessageId;

    #[test]
    fn permanent_delete_needs_its_own_scope() {
        let delete = |permanent| Request::DeleteMessages {
            message_ids: vec![MessageId::from("m1")],
            permanent,
        };
        assert_eq!(
            required_access(&delete(false)).access,
            Access::Scope(McpScope::Organize)
        );
        assert_eq!(
            required_access(&delete(true)).access,
            Access::Scope(McpScope::PermanentDelete)
        );
    }

    #[test]
    fn names_are_the_wire_type_names() {
        for request in [
            Request::Ping,
            Request::ListAccounts,
            Request::GetRemoteUiSettings,
            Request::ListRemoteContentAllows,
            Request::ListMcpClients,
            Request::GetUiContext,
            Request::GetSieveStatus { account_id: None },
            Request::MarkJunk {
                message_ids: vec![],
                junk: true,
            },
            Request::ListSnoozed { account_id: None },
            Request::GetNotificationSettings,
            Request::TestNotification,
            Request::FetchMessageBodies {
                message_ids: vec![],
            },
            Request::GetMcpSettings,
            Request::RuleFromMessage {
                message_id: "m1".into(),
            },
            Request::ListPlugins,
            Request::LoadDevPlugin {
                folder: "C:/dev/x".into(),
                grants: vec![],
            },
            Request::RunPluginCommand {
                command: "plugin.x.y".into(),
                surface: nail_api::plugins::PluginCommandSurface::Reader,
                context: nail_api::UiContext::default(),
                client_id: None,
            },
            Request::PreviewPluginPlacements {
                plugin_id: "x".into(),
            },
        ] {
            let wire = serde_json::to_value(&request).unwrap();
            assert_eq!(wire["type"], required_access(&request).name);
        }
    }

    #[test]
    fn audited_requests() {
        assert!(!required_access(&Request::ListAccounts).audited());
        assert!(!required_access(&Request::Ping).audited());
        assert!(!required_access(&Request::ListRemoteUsers).audited());
        assert!(
            required_access(&Request::UpdateFlags {
                message_ids: vec![],
                change: nail_api::FlagChange::default(),
            })
            .audited()
        );
        assert!(required_access(&Request::RegenerateRemoteCertificate).audited());
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn plugin_administration_is_admin_and_dev_loading_desktop_only() {
        let id = || nail_api::PluginId::new("x");
        for request in [
            Request::EnablePlugin { plugin_id: id() },
            Request::DisablePlugin { plugin_id: id() },
            Request::UninstallPlugin { plugin_id: id() },
            Request::UpdatePluginGrants {
                plugin_id: id(),
                grants: vec![],
            },
            Request::ListPluginLogs {
                plugin_id: None,
                after_seq: None,
                limit: 10,
            },
            Request::GetPluginSettings { plugin_id: id() },
        ] {
            assert_eq!(
                required_access(&request).access,
                Access::Admin,
                "{request:?}"
            );
        }
        assert_eq!(
            required_access(&Request::LoadDevPlugin {
                folder: "C:/x".into(),
                grants: vec![],
            })
            .access,
            Access::DesktopOnly
        );
        let run = required_access(&Request::RespondPluginDialog {
            dialog_id: "d1".into(),
            button: None,
            values: Default::default(),
        });
        assert_eq!(run.access, Access::Scope(McpScope::Read));
        assert!(!run.audited(), "reads are not audited");
    }
}

#[cfg(test)]
mod pim_tests {
    use super::*;
    use nail_api::pim::{ContactFields, EditScope};

    #[test]
    fn pim_reads_writes_and_sources() {
        let read = required_access(&Request::ListEventInstances {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-08T00:00:00Z".into(),
            calendar_ids: None,
            tz: "UTC".into(),
        });
        assert_eq!(read.access, Access::Scope(McpScope::Read));
        assert!(!read.audited());
        let delete = required_access(&Request::DeleteEvent {
            event_id: "ev1".into(),
            scope: EditScope::All,
            recurrence_id: None,
        });
        assert_eq!(delete.access, Access::Scope(McpScope::Calendar));
        assert!(delete.audited());
        let contact = required_access(&Request::CreateContact {
            address_book_id: "ab1".into(),
            fields: Box::new(ContactFields::named("Ada")),
        });
        assert_eq!(contact.access, Access::Scope(McpScope::Pim));
        assert_eq!(
            required_access(&Request::RemovePimSource {
                source_id: "work-dav".into()
            })
            .access,
            Access::Admin
        );
        assert_eq!(
            required_access(&Request::DeleteCalendar {
                calendar_id: "c1".into()
            })
            .access,
            Access::Sensitive(McpScope::Calendar)
        );
        assert_eq!(
            required_access(&Request::ListPimSources).name,
            "ListPimSources"
        );
    }
}

#[cfg(test)]
mod m3b_tests {
    use super::*;
    use nail_api::gmail::GmailLabelSettings;
    use nail_api::scheduling::{InvitationResponse, InvitationTarget, SchedulingSettings};

    #[test]
    fn scheduling_and_gmail_label_access() {
        let card = required_access(&Request::GetInvitation {
            message_id: "m1".into(),
        });
        assert_eq!(card.access, Access::Scope(McpScope::Read));
        assert!(!card.audited());
        let answer = required_access(&Request::RespondToInvitation {
            target: InvitationTarget::Message {
                message_id: "m1".into(),
            },
            response: InvitationResponse::Accept,
            comment: None,
            send: true,
            calendar_id: None,
        });
        assert_eq!(answer.access, Access::Scope(McpScope::Calendar));
        assert!(answer.audited());
        let free_busy = required_access(&Request::QueryFreeBusy {
            attendees: vec!["bob@example.org".into()],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-06T00:00:00Z".into(),
            identity_id: None,
            ignore_event_id: None,
        });
        assert_eq!(free_busy.access, Access::Scope(McpScope::Read));
        assert_eq!(
            required_access(&Request::SetSchedulingSettings {
                settings: SchedulingSettings::default()
            })
            .access,
            Access::Admin
        );
        let label = required_access(&Request::CreateGmailLabel {
            account_id: "gm".into(),
            settings: GmailLabelSettings::named("Work"),
        });
        assert_eq!(label.access, Access::Scope(McpScope::Organize));
        assert_eq!(label.name, "CreateGmailLabel");
    }
}

#[cfg(test)]
mod m3c_tests {
    use super::*;
    use nail_api::exchange::{
        AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeMailboxSettings,
        InferenceClassification,
    };

    #[test]
    fn exchange_access() {
        let status = required_access(&Request::GetExchangeStatus {
            account_id: "work".into(),
        });
        assert_eq!(status.access, Access::Scope(McpScope::Read));
        assert!(!status.audited());
        let rooms = required_access(&Request::FindRooms {
            account_id: "work".into(),
            room_list: None,
            from: "2026-10-05T08:00:00Z".into(),
            to: "2026-10-05T09:00:00Z".into(),
            min_capacity: None,
            refresh: false,
        });
        assert_eq!(rooms.access, Access::Scope(McpScope::Read));
        let mailbox = required_access(&Request::AddExchangeMailbox {
            account_id: "work".into(),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        });
        assert_eq!(mailbox.access, Access::Admin);
        let category = required_access(&Request::CreateExchangeCategory {
            account_id: "work".into(),
            settings: ExchangeCategorySettings::named("Red"),
        });
        assert_eq!(category.access, Access::Scope(McpScope::Organize));
        assert!(category.audited());
        let focus = required_access(&Request::ClassifyFocused {
            message_ids: vec!["m1".into()],
            classification: InferenceClassification::Other,
            always: false,
        });
        assert_eq!(focus.access, Access::Scope(McpScope::Organize));
        let oof = required_access(&Request::SetAutomaticReplies {
            account_id: "work".into(),
            settings: AutomaticRepliesSettings::default(),
        });
        assert_eq!(
            (oof.name, oof.access),
            ("SetAutomaticReplies", Access::Admin)
        );
        assert_eq!(
            required_access(&Request::DeleteExchangeInboxRule {
                account_id: "work".into(),
                server_rule_id: "r1".into()
            })
            .access,
            Access::Admin
        );
    }
}

#[cfg(test)]
mod m4a_tests {
    use super::*;
    use nail_api::agents::{ActionActor, ActionKind, ConfirmationDecision, ConfirmationFilter};
    use nail_api::plugins::PluginActionTarget;

    #[test]
    fn agent_and_extension_access() {
        let decide = required_access(&Request::DecideConfirmation {
            confirmation_id: "1".into(),
            decision: ConfirmationDecision::Approve,
            always_allow: false,
        });
        assert_eq!(decide.access, Access::Admin);
        assert!(decide.audited());
        let list = required_access(&Request::ListConfirmations {
            filter: ConfirmationFilter::Pending,
            actor: None,
            before: None,
            limit: 10,
        });
        assert_eq!(list.access, Access::Admin);
        let policy = required_access(&Request::SetActionPolicy {
            actor: ActionActor::mcp_client("mcp-1"),
            kind: ActionKind::Send,
            mode: None,
        });
        assert_eq!(
            (policy.name, policy.access),
            ("SetActionPolicy", Access::Admin)
        );
        let preview = required_access(&Request::RenderAttachmentPreview {
            message_id: "m1".into(),
            index: 0,
            renderer: "csv.table".into(),
            client_id: None,
        });
        assert_eq!(preview.access, Access::Scope(McpScope::Attachments));
        let action = required_access(&Request::RunPluginAction {
            action: "crm.save".into(),
            target: PluginActionTarget::Link {
                url: "https://x.example".into(),
                text: None,
                message_id: None,
            },
            context: Default::default(),
            client_id: None,
        });
        assert_eq!(action.access, Access::Scope(McpScope::Read));
        let anyway = required_access(&Request::OverrideSendVeto {
            veto_id: "v1".into(),
        });
        assert_eq!(anyway.access, Access::Scope(McpScope::Send));
        assert!(anyway.audited());
        assert_eq!(
            required_access(&Request::GetPluginActivity {
                plugin_id: "x".into()
            })
            .access,
            Access::Admin
        );
    }

    #[test]
    fn m4b_desktop_requests_are_desktop_only() {
        use nail_api::desktop::{DesktopSettings, UpdateSettings};
        for request in [
            Request::GetInstallation,
            Request::GetUpdateStatus,
            Request::SetUpdateSettings {
                settings: UpdateSettings::default(),
            },
            Request::CheckForUpdates,
            Request::DownloadUpdate,
            Request::InstallUpdate,
            Request::GetDesktopSettings,
            Request::SetDesktopSettings {
                settings: DesktopSettings::default(),
            },
            Request::GetDefaultApps,
            Request::OpenDefaultAppsSettings,
            Request::GetFirstRun,
            Request::CompleteFirstRun,
        ] {
            assert_eq!(
                required_access(&request).access,
                Access::DesktopOnly,
                "{request:?}"
            );
        }
    }

    #[test]
    fn m4c_requests() {
        use nail_api::portability::{ImportFormat, ImportSource};
        use nail_api::print::{PrintOptions, PrintTarget};
        let desktop = [
            Request::FindImportSources,
            Request::StartImport {
                source: ImportSource {
                    format: ImportFormat::Mbox,
                    path: "C:/x.mbox".into(),
                },
                options: Default::default(),
            },
            Request::CreateBackup {
                path: "C:/b.nailbackup".into(),
                contents: Default::default(),
                passphrase: "correct horse battery".into(),
                overwrite: false,
            },
            Request::RestoreVaultKey { key: "k".into() },
            Request::ListSmartcards,
            Request::AnswerPinPrompt {
                prompt_id: "p".into(),
                pin: None,
            },
            Request::SavePrintPdf {
                job_id: "j".into(),
                path: "C:/x.pdf".into(),
                overwrite: true,
            },
            Request::CreateDiagnosticsBundle {
                options: Default::default(),
                path: "C:/d.zip".into(),
                overwrite: false,
            },
        ];
        for request in desktop {
            assert_eq!(
                required_access(&request).access,
                Access::DesktopOnly,
                "{request:?}"
            );
        }
        assert_eq!(
            required_access(&Request::RecordActionUsed {
                action_id: "mail.reply".into()
            })
            .access,
            Access::Session
        );
        assert_eq!(
            required_access(&Request::CreatePrintJob {
                target: PrintTarget::Message {
                    message_id: "m".into()
                },
                options: PrintOptions::default(),
            })
            .access,
            Access::Scope(McpScope::Read)
        );
    }
}
