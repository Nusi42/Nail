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
