//! Requests from remote-UI sessions (ADR 0041): the remote UI speaks the
//! same protocol as the desktop, but every request is checked first.
//!
//! 1. [`nail_remote_auth::RemoteAuth::authorize`]: the scope (or `admin`
//!    plus a fresh re-authentication) the request needs, desktop-only
//!    requests refused, every denial audited.
//! 2. The grant's account list and folder exclusions (resolved like an
//!    MCP client's, [`nail_mcp::access::Access`]): messages, folders,
//!    accounts, identities, drafts, outbox entries and operations a
//!    request names must be granted ([`Service::check_remote_request`], an
//!    exhaustive match: a new request cannot slip through); answers are
//!    narrowed to what is granted.
//! 3. The `encrypted` gate: decrypted content of end-to-end encrypted mail
//!    only with that scope (like MCP, [`Service::message_is_encrypted`]).
//!    It covers every way to content: `GetMessage`, drafts replying to,
//!    forwarding or redirecting an encrypted message (their quote or copy
//!    is its plain text), and searches while the index holds decrypted
//!    text.
//! 4. Requests spanning every account (the rules) need a grant for all
//!    accounts without excluded folders, like MCP's rules resource.
//! 5. Credential and pairing requests run with the session as the actor.
//!
//! `GetMessage` also gets its `render_url` on the sandbox origin. Events
//! pushed to a remote socket go through [`Service::remote_event`].

use nail_api::mcp::McpScope;
use nail_api::remote_content::RemoteContentScope;
use nail_api::{
    AccountId, ApiError, Draft, DraftId, ErrorCode, Event, FolderId, IdentityId, MessageId,
    MessageSummary, MoveTarget, OpId, OutboxId, Request, Response, SearchQuery, UiContext,
};
use nail_mcp::access::Access;
use nail_remote_auth::RemoteSessionCtx;
use nail_remote_auth::access::{Access as Needs, required_access};

use crate::error::Result;
use crate::service::Service;

fn forbidden(message: String) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

/// Hits a restricted search looks at, at most (like MCP's).
const RESTRICTED_SCAN: u32 = 5000;
/// Hits fetched per page of a restricted search.
const SCAN_PAGE: u32 = 200;

/// Whether the grant covers everything: every account, no excluded folder.
fn full(access: &Access) -> bool {
    access.all_accounts() && !access.excludes_folders()
}

/// The message a draft quotes, forwards or redirects, if any.
fn draft_original(draft: &Draft) -> Option<MessageId> {
    draft
        .context
        .as_ref()
        .map(|c| c.message_id.clone())
        .or_else(|| match &draft.automatic {
            Some(nail_api::AutomaticMail::Redirect { message_id }) => Some(message_id.clone()),
            _ => None,
        })
}

fn message_allowed(access: &Access, message: &MessageSummary) -> bool {
    access.allows_account(&message.account_id) && access.allows_folder(&message.folder_id)
}

impl Service {
    /// Answers `request` from the remote session `ctx` (see the module
    /// docs). Never fails: errors become [`Response::Error`].
    pub async fn handle_remote(&self, ctx: &RemoteSessionCtx, request: Request) -> Response {
        match self.dispatch_remote(ctx, request).await {
            Ok(response) => response,
            Err(err) => ApiError::from(err).into(),
        }
    }

    fn remote_access(&self, ctx: &RemoteSessionCtx) -> Access {
        Access::resolve(
            &ctx.grant().mail,
            &self.inner.accounts.config().virtual_accounts,
        )
    }

    async fn dispatch_remote(&self, ctx: &RemoteSessionCtx, request: Request) -> Result<Response> {
        self.remote_auth().authorize(ctx, &request)?;
        let access = self.remote_access(ctx);
        self.check_remote_request(&access, &request).await?;
        let response = match request {
            request @ (Request::SetRemotePassword { .. }
            | Request::StartTotpEnrollment { .. }
            | Request::ConfirmTotpEnrollment { .. }
            | Request::DisableTotp { .. }
            | Request::CreatePairingCode { .. }) => {
                self.remote_auth().handle_admin(request, Some(ctx)).await?
            }
            Request::GetMessage { id } => {
                let mut message = self.message_body(id.clone()).await?;
                message.render_url = self.remote_render_url(&id, &ctx.session.id);
                Response::Message {
                    message: Box::new(message),
                }
            }
            Request::Search { query } => self.remote_search(&access, query).await?,
            // Collected addresses are not per account: a narrowed grant
            // would learn other accounts' correspondents.
            Request::SuggestAddresses { .. } if !full(&access) => Response::AddressSuggestions {
                addresses: Vec::new(),
            },
            other => self.handle(other).await,
        };
        self.narrow_remote_response(&access, response).await
    }

    // -----------------------------------------------------------------
    // Checks
    // -----------------------------------------------------------------

    /// Fails unless every account, folder, message, identity, draft,
    /// outbox entry and operation `request` names is within the grant (and
    /// content of encrypted mail it would reveal is covered by the
    /// `encrypted` scope). Exhaustive on purpose: no default arm.
    async fn check_remote_request(&self, access: &Access, request: &Request) -> Result<()> {
        match request {
            // Nothing named, or the answer is narrowed afterwards.
            Request::Ping
            | Request::GetConfig
            | Request::ListAccounts
            | Request::ListIdentities { account_id: None }
            | Request::ListVirtualAccounts
            | Request::ListAccountStatuses
            | Request::GetThread { .. }
            | Request::CheckQuery { .. }
            | Request::ListDrafts
            | Request::ListOutbox
            | Request::UploadAttachment { .. }
            | Request::SuggestAddresses { .. }
            | Request::RuleToText { .. }
            | Request::RuleFromText { .. }
            | Request::GetUiContext
            | Request::ListSavedSearches
            | Request::SaveSavedSearch { .. }
            | Request::DeleteSavedSearch { .. }
            | Request::ListKeys { .. }
            | Request::GetKey { .. }
            | Request::ExportPublicKey { .. }
            | Request::LookupKeys { .. }
            | Request::UnlockKey { .. }
            | Request::GetSecuritySettings
            | Request::GetVaultStatus
            | Request::ListRemoteContentAllows
            | Request::GetNotificationSettings
            | Request::TestNotification
            | Request::ListSnoozed { account_id: None } => Ok(()),

            // Administration (`admin` and a fresh login, checked by
            // `authorize`), self-service on the session's own user, or
            // desktop-only (refused by `authorize`).
            Request::DiscoverAccount { .. }
            | Request::TestAccount { .. }
            | Request::StartOAuth { .. }
            | Request::CancelOAuth { .. }
            | Request::GetAccountsConfig
            | Request::AddAccount { .. }
            | Request::UpdateAccount { .. }
            | Request::RemoveAccount { .. }
            | Request::AddVirtualAccount { .. }
            | Request::UpdateVirtualAccount { .. }
            | Request::RemoveVirtualAccount { .. }
            | Request::ListMcpClients
            | Request::CreateMcpClient { .. }
            | Request::UpdateMcpGrants { .. }
            | Request::RevokeMcpClient { .. }
            | Request::ListMcpAudit { .. }
            | Request::GetMcpServerInfo
            | Request::ReportUiContext { .. }
            | Request::UpdateLayout { .. }
            | Request::PreviewConfigMerge { .. }
            | Request::ResolveConfigMerge { .. }
            | Request::ApplyConfigMerge { .. }
            | Request::ListConfigBackups { .. }
            | Request::RevertConfig { .. }
            | Request::OpenConfigFile { .. }
            | Request::ImportKeys { .. }
            | Request::ImportPkcs12 { .. }
            | Request::GenerateKey { .. }
            | Request::DeleteKey { .. }
            | Request::SetKeyTrust { .. }
            | Request::ListWindowsCertificates { .. }
            | Request::ImportWindowsCertificate { .. }
            | Request::SendAutocryptSetup { .. }
            | Request::ImportAutocryptSetup { .. }
            | Request::UpdateSecuritySettings { .. }
            | Request::SuggestAuthservId { .. }
            | Request::SetVaultEncryption { .. }
            | Request::GetRemoteUiSettings
            | Request::UpdateRemoteUiSettings { .. }
            | Request::RegenerateRemoteCertificate
            | Request::ListRemoteUsers
            | Request::CreateRemoteUser { .. }
            | Request::UpdateRemoteUser { .. }
            | Request::DeleteRemoteUser { .. }
            | Request::SetRemotePassword { .. }
            | Request::StartTotpEnrollment { .. }
            | Request::ConfirmTotpEnrollment { .. }
            | Request::DisableTotp { .. }
            | Request::UnlockRemoteUser { .. }
            | Request::RenamePasskey { .. }
            | Request::DeletePasskey { .. }
            | Request::ListRemoteDevices { .. }
            | Request::UpdateRemoteDevice { .. }
            | Request::RevokeRemoteDevice { .. }
            | Request::ListRemoteSessions { .. }
            | Request::RevokeRemoteSession { .. }
            | Request::RevokeAllRemoteSessions { .. }
            | Request::CreatePairingCode { .. }
            | Request::ListPairingCodes
            | Request::CancelPairingCode { .. }
            | Request::ListRemoteAudit { .. }
            | Request::TakeLaunchRequests
            | Request::ImportMessageFile { .. }
            | Request::SyncSieve { .. }
            | Request::UpdateNotificationSettings { .. }
            | Request::SetFolderSyncPolicy { .. }
            | Request::GetMcpSettings
            | Request::UpdateMcpSettings { .. } => {
                // Defence in depth: this arm relies on `authorize`; if a
                // request here were ever made a plain scope request, it
                // would need its own target check instead.
                match required_access(request).access {
                    Needs::Admin | Needs::AdminOrOwn(_) | Needs::DesktopOnly => Ok(()),
                    _ => Err(forbidden(format!(
                        "{} is not available remotely",
                        crate::service::request_name(request)
                    ))
                    .into()),
                }
            }

            // Accounts and folders.
            Request::ListFolders { account_id }
            | Request::ListIdentities {
                account_id: Some(account_id),
            }
            | Request::SyncNow {
                account_id: Some(account_id),
            } => check_account(access, account_id),
            Request::SyncNow { account_id: None } => {
                if access.all_accounts() {
                    Ok(())
                } else {
                    Err(
                        forbidden("syncing every account needs a grant for all accounts".into())
                            .into(),
                    )
                }
            }
            Request::ChooseIdentity {
                account_id,
                reply_to,
                ..
            } => {
                if let Some(account) = account_id {
                    check_account(access, account)?;
                }
                if let Some(message) = reply_to {
                    self.check_messages(access, std::slice::from_ref(message))
                        .await?;
                }
                Ok(())
            }
            Request::ListMessages { folder_id, .. } | Request::ListThreads { folder_id, .. } => {
                self.check_folder(access, folder_id).await
            }
            Request::Search { query } => {
                if let Some(id) = &query.account_id {
                    check_account(access, id)?;
                }
                if let Some(id) = &query.folder_id {
                    self.check_folder(access, id).await?;
                }
                Ok(())
            }

            // Messages.
            Request::GetMessage { id } => self.check_readable(access, id).await,
            Request::UpdateFlags { message_ids, .. }
            | Request::DeleteMessages { message_ids, .. } => {
                self.check_messages(access, message_ids).await
            }
            Request::MoveMessages {
                message_ids,
                target,
            } => {
                if let MoveTarget::FolderId(id) = target {
                    self.check_folder(access, id).await?;
                }
                self.check_messages(access, message_ids).await
            }
            Request::AllowRemoteContent { message_id, .. } => {
                self.check_messages(access, std::slice::from_ref(message_id))
                    .await
            }
            Request::RemoveRemoteContentAllow { scope, value } => {
                if !matches!(
                    scope,
                    RemoteContentScope::Message | RemoteContentScope::Once
                ) {
                    return Ok(());
                }
                match self.find_message(&MessageId::new(value.as_str())).await? {
                    Some(message) if message_allowed(access, &message) => Ok(()),
                    // A stale entry of a deleted message: its account is
                    // unknown.
                    None if full(access) => Ok(()),
                    _ => Err(forbidden(format!("message {value} is not granted")).into()),
                }
            }

            // Drafts, the composer and sending: the identity's account, and
            // the message a draft quotes, forwards or redirects must be
            // readable (encrypted gate included).
            Request::SaveDraft { draft } => {
                self.check_identity(access, &draft.identity_id)?;
                if let Some(original) = draft_original(draft) {
                    self.check_readable(access, &original).await?;
                }
                if let Some(id) = &draft.id
                    && let Some(stored) = self.find_draft(id).await?
                    && !self.draft_visible(access, &stored).await?
                {
                    return Err(forbidden(format!("draft {id} is not granted")).into());
                }
                Ok(())
            }
            Request::GetDraft { draft_id }
            | Request::GetDraftAttachment { draft_id, .. }
            | Request::DeleteDraft { draft_id }
            | Request::SendDraft { draft_id, .. } => self.check_draft(access, draft_id).await,
            Request::CancelSend { outbox_id } => self.check_outbox(access, outbox_id).await,
            Request::RetryOp { op_id } => self.check_op(access, op_id).await,
            Request::NewDraft {
                identity_id,
                account_id,
                context,
                ..
            } => {
                if let Some(identity) = identity_id {
                    self.check_identity(access, identity)?;
                }
                if let Some(account) = account_id {
                    check_account(access, account)?;
                }
                if let Some(context) = context {
                    self.check_readable(access, &context.message_id).await?;
                }
                Ok(())
            }
            Request::RenderPreview { identity_id, .. } => match identity_id {
                Some(identity) => self.check_identity(access, identity),
                None => Ok(()),
            },
            Request::ConvertDraft { draft, .. } => {
                self.check_identity(access, &draft.identity_id)?;
                if let Some(original) = draft_original(draft) {
                    self.check_readable(access, &original).await?;
                }
                Ok(())
            }
            Request::CheckRecipientKeys { identity_id, .. } => {
                self.check_identity(access, identity_id)
            }

            // Rules span every account (like MCP's rules resource); a dry
            // run or a run on named messages or one folder is checked
            // against those.
            Request::ListRules
            | Request::SaveRule { .. }
            | Request::DeleteRule { .. }
            | Request::ReorderRules { .. }
            | Request::ListRuleRuns { .. }
            | Request::UndoRuleRun { .. } => check_full(access),
            Request::TestRule { folder_id, .. } => match folder_id {
                Some(folder) => self.check_folder(access, folder).await,
                None => check_full(access),
            },
            Request::RunRule {
                message_ids,
                folder_id,
                ..
            } => {
                if message_ids.is_empty() && folder_id.is_none() {
                    return check_full(access);
                }
                if let Some(folder) = folder_id {
                    self.check_folder(access, folder).await?;
                }
                self.check_messages(access, message_ids).await
            }

            // M2c: rule placements and scripts reveal every rule.
            Request::GetSieveStatus { .. } | Request::PreviewSieveScript { .. } => {
                check_full(access)
            }
            Request::GetVacation { account_id }
            | Request::UpdateVacation { account_id, .. }
            | Request::GetJunkStatus { account_id }
            | Request::UpdateJunkSettings { account_id, .. }
            | Request::ResetJunkClassifier { account_id }
            | Request::ListSnoozed {
                account_id: Some(account_id),
            } => check_account(access, account_id),
            Request::MarkJunk { message_ids, .. }
            | Request::UnsnoozeMessages { message_ids }
            | Request::FetchMessageBodies { message_ids } => {
                self.check_messages(access, message_ids).await
            }
            Request::SnoozeMessages {
                message_ids,
                wake_folder_id,
                ..
            } => {
                if let Some(folder) = wake_folder_id {
                    self.check_folder(access, folder).await?;
                }
                self.check_messages(access, message_ids).await
            }
            Request::GetFolderSyncPolicy { folder_id } => {
                self.check_folder(access, folder_id).await
            }
            Request::RuleFromMessage { message_id } => {
                self.check_readable(access, message_id).await
            }

            // M2d plug-ins: a plug-in reads mail with its own grants, so
            // using one needs a grant for every account; the list is
            // narrowed to nothing instead. Packages and folders on the
            // desktop's disk are never read for a remote session.
            Request::ListPlugins
            | Request::EnablePlugin { .. }
            | Request::DisablePlugin { .. }
            | Request::UninstallPlugin { .. }
            | Request::UpdatePluginGrants { .. }
            | Request::ListPluginLogs { .. }
            | Request::ClearPluginLogs { .. }
            | Request::GetPluginSettings { .. }
            | Request::UpdatePluginSettings { .. }
            | Request::PreviewPluginPlacements { .. } => Ok(()),
            Request::GetPlugin { .. }
            | Request::RunPluginCommand { .. }
            | Request::RespondPluginDialog { .. }
            | Request::PluginUiAction { .. }
            | Request::GetPluginPanel { .. } => check_full(access),
            Request::PreviewPluginInstall { source } | Request::InstallPlugin { source, .. } => {
                match source {
                    nail_api::plugins::PluginPackageSource::Url { .. } => Ok(()),
                    _ => Err(
                        forbidden("remote sessions install plug-ins from URLs only".into()).into(),
                    ),
                }
            }
            Request::LoadDevPlugin { .. } => {
                Err(forbidden("dev plug-ins are loaded on the desktop only".into()).into())
            }

            // M3a PIM (ADR 0058): until WP75 narrows answers per source,
            // PIM data needs a grant for every account; files on the
            // desktop's disk are never read for a remote session.
            Request::EventFromMessage { message_id } | Request::TaskFromMessage { message_id } => {
                self.check_readable(access, message_id).await
            }
            Request::PreviewPimImport { source } | Request::ImportPim { source, .. } => {
                match source {
                    nail_api::pim::PimImportSource::Data { .. } => check_full_pim(access),
                    nail_api::pim::PimImportSource::File { .. } => Err(forbidden(
                        "remote sessions import uploaded data only, not desktop files".into(),
                    )
                    .into()),
                }
            }
            Request::ListPimSources
            | Request::AddPimSource { .. }
            | Request::UpdatePimSource { .. }
            | Request::RemovePimSource { .. }
            | Request::TestPimSource { .. }
            | Request::SyncPimSources { .. }
            | Request::ListCalendars { .. }
            | Request::CreateCalendar { .. }
            | Request::UpdateCalendar { .. }
            | Request::DeleteCalendar { .. }
            | Request::ListEventInstances { .. }
            | Request::GetEvent { .. }
            | Request::CreateEvent { .. }
            | Request::UpdateEvent { .. }
            | Request::DeleteEvent { .. }
            | Request::MoveEvent { .. }
            | Request::FindFreeTime { .. }
            | Request::ListAddressBooks { .. }
            | Request::CreateAddressBook { .. }
            | Request::UpdateAddressBook { .. }
            | Request::DeleteAddressBook { .. }
            | Request::ListContacts { .. }
            | Request::GetContact { .. }
            | Request::CreateContact { .. }
            | Request::UpdateContact { .. }
            | Request::DeleteContacts { .. }
            | Request::MoveContact { .. }
            | Request::SearchContacts { .. }
            | Request::FindDuplicateContacts { .. }
            | Request::MergeContacts { .. }
            | Request::UnlinkContact { .. }
            | Request::GetContactPhoto { .. }
            | Request::ListContactGroups { .. }
            | Request::CreateContactGroup { .. }
            | Request::UpdateContactGroup { .. }
            | Request::DeleteContactGroup { .. }
            | Request::ListTasks { .. }
            | Request::GetTask { .. }
            | Request::CreateTask { .. }
            | Request::UpdateTask { .. }
            | Request::CompleteTask { .. }
            | Request::MoveTask { .. }
            | Request::DeleteTask { .. }
            | Request::SearchPim { .. }
            | Request::ResolvePimConflict { .. }
            | Request::ListReminders { .. }
            | Request::SnoozeReminders { .. }
            | Request::DismissReminders { .. } => check_full_pim(access),

            // M3b scheduling (ADR 0065): invitations touch the calendars
            // (full grant, like PIM, which also covers every identity's
            // account) and the message they came in (granted, readable).
            // Whatever can mail an iTIP message from the user's identities
            // also needs `send`, like `SendDraft`.
            Request::GetInvitation { message_id } => {
                check_full_pim(access)?;
                self.check_readable(access, message_id).await
            }
            Request::ApplyInvitation { message_id, .. } => {
                check_full_pim(access)?;
                self.check_readable(access, message_id).await?;
                // Applying a REFRESH mails the current version.
                if self.is_itip_refresh(message_id).await {
                    check_send(access)?;
                }
                Ok(())
            }
            Request::RespondToCounter { message_id, .. } => {
                check_full_pim(access)?;
                check_send(access)?;
                self.check_readable(access, message_id).await
            }
            Request::RespondToInvitation { target, send, .. } => {
                check_full_pim(access)?;
                if *send {
                    check_send(access)?;
                }
                self.check_invitation_target(access, target).await
            }
            Request::ProposeNewTime { target, .. }
            | Request::RefreshInvitation { target }
            | Request::ForwardInvitation { target, .. } => {
                check_full_pim(access)?;
                check_send(access)?;
                self.check_invitation_target(access, target).await
            }
            Request::SendInvitations { .. } | Request::CancelMeeting { .. } => {
                check_full_pim(access)?;
                check_send(access)
            }
            Request::QueryFreeBusy { .. }
            | Request::GetSchedulingSettings
            | Request::SetSchedulingSettings { .. } => check_full_pim(access),
            // M3b Gmail labels: the account must be granted.
            Request::ListGmailLabels { account_id }
            | Request::CreateGmailLabel { account_id, .. }
            | Request::UpdateGmailLabel { account_id, .. }
            | Request::DeleteGmailLabel { account_id, .. } => check_account(access, account_id),
            // M3c Exchange: the account must be granted; Focused Inbox
            // moves need their messages.
            Request::GetExchangeStatus { account_id }
            | Request::ListExchangeMailboxes { account_id }
            | Request::AddExchangeMailbox { account_id, .. }
            | Request::UpdateExchangeMailbox { account_id, .. }
            | Request::RemoveExchangeMailbox { account_id, .. }
            | Request::ListExchangeCategories { account_id }
            | Request::CreateExchangeCategory { account_id, .. }
            | Request::UpdateExchangeCategory { account_id, .. }
            | Request::DeleteExchangeCategory { account_id, .. }
            | Request::GetAutomaticReplies { account_id }
            | Request::SetAutomaticReplies { account_id, .. }
            | Request::GetExchangeRules { account_id }
            | Request::SyncExchangeRules { account_id }
            | Request::SetExchangeInboxRuleEnabled { account_id, .. }
            | Request::DeleteExchangeInboxRule { account_id, .. }
            | Request::ListRoomLists { account_id, .. }
            | Request::FindRooms { account_id, .. } => check_account(access, account_id),
            Request::ClassifyFocused { message_ids, .. } => {
                self.check_messages(access, message_ids).await
            }
        }
    }

    async fn check_messages(&self, access: &Access, ids: &[MessageId]) -> Result<()> {
        let ids = ids.to_vec();
        let summaries = self
            .inner
            .store
            .call(move |s| {
                ids.iter()
                    .map(|id| s.get_message(id).map(|m| m.summary))
                    .collect::<nail_store::Result<Vec<_>>>()
            })
            .await?;
        for summary in &summaries {
            if !message_allowed(access, summary) {
                return Err(forbidden(format!("message {} is not granted", summary.id)).into());
            }
        }
        Ok(())
    }

    /// The message is granted, and if it is end-to-end encrypted the
    /// grant has the `encrypted` scope.
    async fn check_readable(&self, access: &Access, id: &MessageId) -> Result<()> {
        self.check_messages(access, std::slice::from_ref(id))
            .await?;
        if !access.has(McpScope::Encrypted) && self.encrypted(id).await? {
            return Err(forbidden(format!(
                "message {id} is encrypted; reading it remotely needs the encrypted scope"
            ))
            .into());
        }
        Ok(())
    }

    /// The invitation message a scheduling request names is readable.
    async fn check_invitation_target(
        &self,
        access: &Access,
        target: &nail_api::scheduling::InvitationTarget,
    ) -> Result<()> {
        match target {
            nail_api::scheduling::InvitationTarget::Message { message_id } => {
                self.check_readable(access, message_id).await
            }
            nail_api::scheduling::InvitationTarget::Event { .. } => Ok(()),
        }
    }

    async fn encrypted(&self, id: &MessageId) -> Result<bool> {
        let id = id.clone();
        self.blocking(move |s| s.message_is_encrypted(&id)).await
    }

    async fn check_folder(&self, access: &Access, id: &FolderId) -> Result<()> {
        if self.folder_allowed(access, id).await? {
            Ok(())
        } else {
            Err(forbidden(format!("folder {id} is not granted")).into())
        }
    }

    /// Whether the folder is not excluded and belongs to a granted
    /// account (a virtual folder: its virtual account).
    async fn folder_allowed(&self, access: &Access, id: &FolderId) -> Result<bool> {
        if !access.allows_folder(id) {
            return Ok(false);
        }
        if access.all_accounts() {
            return Ok(true);
        }
        if let Some(account) = crate::virtuals::virtual_account_of(id) {
            return Ok(access.allows_account(&account));
        }
        let lookup = id.clone();
        let folder = self
            .inner
            .store
            .call(move |s| match s.get_folder(&lookup) {
                Ok(folder) => Ok(Some(folder)),
                Err(nail_store::Error::NotFound { .. }) => Ok(None),
                Err(err) => Err(err),
            })
            .await?;
        Ok(folder.is_some_and(|f| access.allows_account(&f.account_id)))
    }

    /// The account of an identity, if the identity exists.
    fn identity_account(&self, id: &IdentityId) -> Option<AccountId> {
        self.inner
            .accounts
            .config()
            .identity(id)
            .map(|i| i.account_id.clone())
    }

    /// A draft belongs to its identity's account.
    fn check_identity(&self, access: &Access, id: &IdentityId) -> Result<()> {
        match self.identity_account(id) {
            Some(account) if access.allows_account(&account) => Ok(()),
            // Unknown identities are refused by the handler itself.
            None => Ok(()),
            Some(account) => Err(forbidden(format!(
                "identity {id} belongs to account {account}, which is not granted"
            ))
            .into()),
        }
    }

    async fn find_message(&self, id: &MessageId) -> Result<Option<MessageSummary>> {
        let id = id.clone();
        Ok(self
            .inner
            .store
            .call(move |s| match s.get_message(&id) {
                Ok(message) => Ok(Some(message.summary)),
                Err(nail_store::Error::NotFound { .. }) => Ok(None),
                Err(err) => Err(err),
            })
            .await?)
    }

    async fn find_draft(&self, id: &DraftId) -> Result<Option<Draft>> {
        let id = id.clone();
        Ok(self
            .inner
            .store
            .call(move |s| match s.get_draft(&id) {
                Ok(draft) => Ok(Some(draft)),
                Err(nail_store::Error::NotFound { .. }) => Ok(None),
                Err(err) => Err(err),
            })
            .await?)
    }

    async fn check_draft(&self, access: &Access, id: &DraftId) -> Result<()> {
        match self.find_draft(id).await? {
            Some(draft) if !self.draft_visible(access, &draft).await? => {
                Err(forbidden(format!("draft {id} is not granted")).into())
            }
            // Unknown drafts: the handler answers `not_found`.
            _ => Ok(()),
        }
    }

    /// Whether the grant covers a draft: its identity's account is granted
    /// and the message it quotes, forwards or redirects is readable (with
    /// the `encrypted` scope if that one is encrypted). A draft of a
    /// removed identity, or whose original is gone (its account and
    /// encryption unknown), needs a grant for everything.
    async fn draft_visible(&self, access: &Access, draft: &Draft) -> Result<bool> {
        match self.identity_account(&draft.identity_id) {
            Some(account) if access.allows_account(&account) => {}
            None if access.all_accounts() => {}
            _ => return Ok(false),
        }
        let Some(original) = draft_original(draft) else {
            return Ok(true);
        };
        match self.find_message(&original).await? {
            None => Ok(full(access) && access.has(McpScope::Encrypted)),
            Some(message) if !message_allowed(access, &message) => Ok(false),
            Some(_) => Ok(access.has(McpScope::Encrypted) || !self.encrypted(&original).await?),
        }
    }

    async fn check_outbox(&self, access: &Access, id: &OutboxId) -> Result<()> {
        let lookup = id.clone();
        let entry = self
            .inner
            .store
            .call(move |s| match s.get_outbox_entry(&lookup) {
                Ok(entry) => Ok(Some(entry)),
                Err(nail_store::Error::NotFound { .. }) => Ok(None),
                Err(err) => Err(err),
            })
            .await?;
        match entry {
            Some(entry) if !access.allows_account(&entry.account_id) => {
                Err(forbidden(format!("outbox entry {id} is not granted")).into())
            }
            _ => Ok(()),
        }
    }

    async fn check_op(&self, access: &Access, id: &OpId) -> Result<()> {
        if access.all_accounts() {
            return Ok(());
        }
        let Some(op) = crate::ids::parse_op_id(id) else {
            // The handler answers `not_found`.
            return Ok(());
        };
        let queued = self
            .inner
            .store
            .call(move |s| match s.get_op(op) {
                Ok(op) => Ok(Some(op)),
                Err(nail_store::Error::NotFound { .. }) => Ok(None),
                Err(err) => Err(err),
            })
            .await?;
        match queued {
            Some(op) if !access.allows_account(&op.account_id) => {
                Err(forbidden(format!("operation {id} is not granted")).into())
            }
            _ => Ok(()),
        }
    }

    // -----------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------

    /// Whether the search index may hold decrypted text of encrypted mail
    /// (an account opted in to decrypted indexing).
    async fn index_holds_decrypted(&self) -> Result<bool> {
        Ok(!crate::security::security_settings(self)
            .await?
            .index_decrypted
            .is_empty())
    }

    /// `Search` for a remote session. With a narrowed grant, or while the
    /// index holds decrypted text and the grant lacks `encrypted`, hits the
    /// grant does not cover are dropped *before* paging (like MCP's
    /// restricted search, ADR 0025): nothing in the answer tells whether
    /// hidden mail matched. `total` then counts only the covered hits
    /// found (at least one more than the page when there are more).
    async fn remote_search(&self, access: &Access, query: SearchQuery) -> Result<Response> {
        let hide_encrypted =
            !access.has(McpScope::Encrypted) && self.index_holds_decrypted().await?;
        if full(access) && !hide_encrypted {
            return self.search(query).await;
        }
        let offset = query.offset as usize;
        let limit = query.limit.min(SCAN_PAGE) as usize;
        let wanted = offset + limit + 1;
        let mut visible = Vec::new();
        let mut scanned: u32 = 0;
        // The server search runs once, on the first page, only on folders
        // the grant covers (no login to a server outside the grant).
        let mut server = None;
        let mut server_hits = Vec::new();
        if offset == 0 {
            let grant = access.clone();
            let only =
                crate::search::ServerSearchMode::Only(Box::new(move |f: &nail_api::Folder| {
                    grant.allows_account(&f.account_id) && grant.allows_folder(&f.id)
                }));
            let probe = SearchQuery {
                offset: 0,
                limit: limit as u32,
                ..query.clone()
            };
            if let Response::SearchResults {
                hits, server: info, ..
            } = self.search_with(probe, only).await?
            {
                server = info;
                server_hits = hits.into_iter().filter(|h| h.from_server).collect();
            }
        }
        while scanned < RESTRICTED_SCAN && visible.len() < wanted {
            let page = SearchQuery {
                offset: scanned,
                limit: SCAN_PAGE,
                ..query.clone()
            };
            let (total, hits) = match self
                .search_with(page, crate::search::ServerSearchMode::Off)
                .await?
            {
                Response::SearchResults { total, hits, .. } => (total, hits),
                other => return Ok(other),
            };
            let returned = u32::try_from(hits.len()).unwrap_or(u32::MAX);
            for hit in hits {
                if !message_allowed(access, &hit.message) {
                    continue;
                }
                if hide_encrypted && self.encrypted(&hit.message.id).await? {
                    continue;
                }
                visible.push(hit);
            }
            scanned = scanned.saturating_add(returned);
            if returned == 0 || scanned >= total {
                break;
            }
        }
        let mut hidden_server = 0u32;
        server_hits.retain(|h| {
            let keep = message_allowed(access, &h.message);
            hidden_server += u32::from(!keep);
            keep
        });
        let mut server = server.map(|mut s| {
            s.hits = s.hits.saturating_sub(hidden_server);
            s
        });
        if hide_encrypted {
            let mut kept = Vec::with_capacity(server_hits.len());
            for hit in server_hits {
                if self.encrypted(&hit.message.id).await? {
                    if let Some(s) = server.as_mut() {
                        s.hits = s.hits.saturating_sub(1);
                    }
                } else {
                    kept.push(hit);
                }
            }
            server_hits = kept;
        }
        let total = u32::try_from(visible.len() + server_hits.len()).unwrap_or(u32::MAX);
        let end = visible.len().min(offset + limit);
        let mut hits: Vec<nail_api::SearchHit> = if offset < end {
            visible.drain(offset..end).collect()
        } else {
            Vec::new()
        };
        hits.extend(server_hits);
        Ok(Response::SearchResults {
            offset: query.offset,
            total,
            hits,
            server,
        })
    }

    // -----------------------------------------------------------------
    // Answers
    // -----------------------------------------------------------------

    /// Drops what the grant does not cover from answers.
    async fn narrow_remote_response(
        &self,
        access: &Access,
        response: Response,
    ) -> Result<Response> {
        let keep = |m: &MessageSummary| message_allowed(access, m);
        Ok(match response {
            Response::Accounts { mut accounts } => {
                accounts.retain(|a| access.allows_account(&a.id));
                Response::Accounts { accounts }
            }
            Response::Folders { mut folders } => {
                folders.retain(|f| {
                    access.allows_account(&f.account_id) && access.allows_folder(&f.id)
                });
                Response::Folders { folders }
            }
            Response::Messages {
                folder_id,
                offset,
                total,
                mut messages,
            } => {
                messages.retain(keep);
                Response::Messages {
                    folder_id,
                    offset,
                    total,
                    messages,
                }
            }
            Response::Thread {
                thread,
                mut messages,
            } => {
                messages.retain(keep);
                if messages.is_empty() {
                    return Err(forbidden(format!("thread {} is not granted", thread.id)).into());
                }
                Response::Thread { thread, messages }
            }
            Response::SearchResults {
                offset,
                total,
                mut hits,
                server,
            } => {
                hits.retain(|h| keep(&h.message));
                let server = server.map(|mut s| {
                    s.accounts.retain(|a| access.allows_account(a));
                    s.failed.retain(|a| access.allows_account(a));
                    s
                });
                Response::SearchResults {
                    offset,
                    total,
                    hits,
                    server,
                }
            }
            Response::Snoozed { mut messages } => {
                messages.retain(|m| keep(&m.message));
                Response::Snoozed { messages }
            }
            Response::NotificationSettings { mut settings } => {
                let mut kept = Vec::with_capacity(settings.overrides.len());
                for o in settings.overrides {
                    let visible = match &o.scope {
                        nail_api::triage::NotificationScope::Account { account_id } => {
                            access.allows_account(account_id)
                        }
                        nail_api::triage::NotificationScope::Folder { folder_id } => self
                            .folder_allowed(access, folder_id)
                            .await
                            .unwrap_or(false),
                    };
                    if visible {
                        kept.push(o);
                    }
                }
                settings.overrides = kept;
                Response::NotificationSettings { settings }
            }
            Response::AccountStatuses { mut statuses } => {
                statuses.retain(|s| access.allows_account(&s.account_id));
                Response::AccountStatuses { statuses }
            }
            Response::Identities { mut identities } => {
                identities.retain(|i| access.allows_account(&i.account_id));
                Response::Identities { identities }
            }
            Response::IdentityChosen { identity_id } => Response::IdentityChosen {
                identity_id: identity_id.filter(|id| {
                    self.identity_account(id)
                        .is_some_and(|a| access.allows_account(&a))
                }),
            },
            Response::VirtualAccounts {
                mut virtual_accounts,
            } => {
                virtual_accounts.retain(|v| access.allows_account(&v.id));
                Response::VirtualAccounts { virtual_accounts }
            }
            Response::Draft { draft } => {
                if !self.draft_visible(access, &draft).await? {
                    return Err(forbidden("the draft is not granted".into()).into());
                }
                Response::Draft { draft }
            }
            Response::Drafts { drafts } => {
                let mut visible = Vec::with_capacity(drafts.len());
                for draft in drafts {
                    if self.draft_visible(access, &draft).await? {
                        visible.push(draft);
                    }
                }
                Response::Drafts { drafts: visible }
            }
            Response::Outbox { mut entries } => {
                entries.retain(|e| access.allows_account(&e.account_id));
                Response::Outbox { entries }
            }
            Response::RuleTested {
                scanned,
                mut matches,
            } => {
                matches.retain(|m| keep(&m.message));
                Response::RuleTested { scanned, matches }
            }
            Response::RemoteContentAllows { entries } => {
                let mut visible = Vec::with_capacity(entries.len());
                for entry in entries {
                    if entry.scope == RemoteContentScope::Message {
                        let id = MessageId::new(entry.value.as_str());
                        let allowed = match self.find_message(&id).await? {
                            Some(message) => message_allowed(access, &message),
                            None => full(access),
                        };
                        if !allowed {
                            continue;
                        }
                    }
                    visible.push(entry);
                }
                Response::RemoteContentAllows { entries: visible }
            }
            Response::UiContext { context } => Response::UiContext {
                context: self.narrow_ui_context(access, context).await?,
            },
            // Plug-ins need a grant for every account (M2d).
            Response::Plugins { .. } if !full(access) => Response::Plugins { plugins: vec![] },
            other => other,
        })
    }

    /// The UI context as far as the grant covers it (like MCP's
    /// `get_ui_context`): what is open elsewhere is left out, and the
    /// selected text only goes with a readable message.
    async fn narrow_ui_context(&self, access: &Access, context: UiContext) -> Result<UiContext> {
        let mut narrowed = UiContext {
            account_id: context.account_id.filter(|a| access.allows_account(a)),
            ..UiContext::default()
        };
        if let Some(folder) = context.folder_id
            && self.folder_allowed(access, &folder).await?
        {
            narrowed.folder_id = Some(folder);
        }
        if let Some(message) = context.message_id
            && self.check_readable(access, &message).await.is_ok()
        {
            narrowed.message_id = Some(message);
            narrowed.selected_text = context.selected_text;
        }
        if let Some(thread) = context.thread_id {
            let visible = match self
                .handle(Request::GetThread {
                    thread_id: thread.clone(),
                })
                .await
            {
                Response::Thread { messages, .. } => {
                    messages.iter().any(|m| message_allowed(access, m))
                }
                _ => false,
            };
            if visible {
                narrowed.thread_id = Some(thread);
            }
        }
        for id in context.selected_message_ids {
            if let Some(message) = self.find_message(&id).await?
                && message_allowed(access, &message)
            {
                narrowed.selected_message_ids.push(id);
            }
        }
        if let Some(draft_id) = context.draft_id
            && let Some(draft) = self.find_draft(&draft_id).await?
            && self.draft_visible(access, &draft).await?
        {
            narrowed.draft_id = Some(draft_id);
            narrowed.draft_cursor = context.draft_cursor;
        }
        Ok(narrowed)
    }

    // -----------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------

    /// The event as the remote session `ctx` may see it, `None` when it
    /// must not see it at all: administration events (security alerts
    /// with user names and addresses, the remote-UI status, MCP clients)
    /// only for admin sessions; mail and account events only for granted
    /// accounts and folders; desktop-only events never.
    pub async fn remote_event(&self, ctx: &RemoteSessionCtx, event: Event) -> Option<Event> {
        let admin = ctx.grant().admin;
        let access = self.remote_access(ctx);
        let allowed = match &event {
            Event::ConfigChanged { .. }
            | Event::ConfigError { .. }
            | Event::ConfigErrors { .. }
            | Event::AccountsChanged
            | Event::SavedSearchesChanged
            | Event::KeysChanged
            | Event::SecuritySettingsChanged
            | Event::RemoteContentPolicyChanged
            | Event::VaultProgress { .. }
            | Event::RulesChanged => true,
            Event::MailChanged { folder_ids } => {
                let mut kept = Vec::with_capacity(folder_ids.len());
                for folder in folder_ids {
                    if self.folder_allowed(&access, folder).await.unwrap_or(false) {
                        kept.push(folder.clone());
                    }
                }
                if kept.is_empty() {
                    return None;
                }
                return Some(Event::MailChanged { folder_ids: kept });
            }
            Event::NewMail {
                account_id,
                folder_id,
                ..
            } => {
                access.allows_account(account_id)
                    && self
                        .folder_allowed(&access, folder_id)
                        .await
                        .unwrap_or(false)
            }
            Event::SyncProgress { account_id, .. } | Event::SyncFailed { account_id, .. } => {
                access.allows_account(account_id)
            }
            Event::AccountStatus { status } => access.allows_account(&status.account_id),
            Event::OpStatus { status } => access.allows_account(&status.account_id),
            Event::SendStatus { outbox_id, .. } => {
                let lookup = outbox_id.clone();
                match self
                    .inner
                    .store
                    .call(move |s| s.get_outbox_entry(&lookup))
                    .await
                {
                    Ok(entry) => access.allows_account(&entry.account_id),
                    Err(_) => access.all_accounts(),
                }
            }
            // Rules span every account.
            Event::RuleRunFinished { .. } => full(&access),
            Event::McpContextAccess { .. }
            | Event::McpClientsChanged
            | Event::RemoteUiStatusChanged { .. }
            | Event::RemoteAccessChanged
            | Event::RemoteSecurityAlert { .. } => admin,
            Event::RemoteSessionEnded { session_id, .. } => admin || *session_id == ctx.session.id,
            // Desktop-only flows.
            Event::OAuthFinished { .. } | Event::LaunchRequested => false,
            // M2c.
            Event::SieveStatusChanged { .. } => full(&access),
            Event::JunkStatusChanged { account_id } => access.allows_account(account_id),
            Event::SnoozeChanged | Event::NotificationSettingsChanged => true,
            Event::NotificationRequested { notification }
                if notification.kind == nail_api::triage::NotificationKind::Reminder =>
            {
                full(&access)
            }
            Event::NotificationRequested { notification } => {
                let account_ok = notification
                    .account_id
                    .as_ref()
                    .is_none_or(|a| access.allows_account(a));
                let folder_ok = match &notification.folder_id {
                    Some(folder) => self.folder_allowed(&access, folder).await.unwrap_or(false),
                    // Messages without a folder to check: full grants only.
                    None => notification.message_ids.is_empty() || full(&access),
                };
                let account_named =
                    notification.account_id.is_some() || notification.message_ids.is_empty();
                account_ok && folder_ok && (account_named || full(&access))
            }
            Event::McpSettingsChanged => admin,
            // M2d: plug-ins read with their own grants: full grants only;
            // logs and crashes are administration.
            Event::PluginsChanged
            | Event::PluginDialogOpened { .. }
            | Event::PluginDialogClosed { .. }
            | Event::PluginNotification { .. }
            | Event::PluginPanelChanged { .. } => full(&access),
            Event::PluginLogLine { .. } | Event::PluginStopped { .. } => admin,
            // M3a PIM: full grants only until WP75 narrows per source.
            Event::PimSourcesChanged
            | Event::CalendarsChanged
            | Event::AddressBooksChanged
            | Event::EventsChanged { .. }
            | Event::TasksChanged { .. }
            | Event::ContactsChanged { .. }
            | Event::ReminderDue { .. }
            | Event::RemindersChanged
            | Event::PimOpStatus { .. } => full(&access),
            // M3b: invitation cards and scheduling settings follow PIM
            // (full grants); Gmail labels follow their account.
            Event::InvitationsChanged { .. } | Event::SchedulingSettingsChanged => full(&access),
            Event::GmailLabelsChanged { account_id } => access.allows_account(account_id),
            // M3c: Exchange state follows its account.
            Event::ExchangeChanged { account_id, .. } => access.allows_account(account_id),
        };
        allowed.then_some(event)
    }
}

fn check_account(access: &Access, id: &AccountId) -> Result<()> {
    if access.allows_account(id) {
        Ok(())
    } else {
        Err(forbidden(format!("account {id} is not granted")).into())
    }
}

/// Mail leaving from the user's identities needs the `send` scope.
fn check_send(access: &Access) -> Result<()> {
    if access.has(McpScope::Send) {
        Ok(())
    } else {
        Err(forbidden("sending mail needs the send scope".into()).into())
    }
}

fn check_full_pim(access: &Access) -> Result<()> {
    if full(access) {
        Ok(())
    } else {
        Err(
            forbidden("calendars, contacts and tasks need a grant for all accounts (M3a)".into())
                .into(),
        )
    }
}

fn check_full(access: &Access) -> Result<()> {
    if full(access) {
        Ok(())
    } else {
        Err(forbidden("rules span every account: they need a grant for all accounts".into()).into())
    }
}
