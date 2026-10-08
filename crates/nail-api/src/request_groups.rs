//! Groups of [`Request`](crate::Request)s as `match` patterns (wave Q2,
//! ADR 0171): the one list of each group, used by the core's dispatch, its
//! remote-session checks and the handlers. `request @ (pim_contact_requests!()
//! | Request::SearchPim { .. })` matches the group and more.

/// The MCP administration requests (`nail_mcp::admin` answers them).
#[macro_export]
macro_rules! mcp_admin_requests {
    () => {
        $crate::Request::ListMcpClients
            | $crate::Request::CreateMcpClient { .. }
            | $crate::Request::UpdateMcpGrants { .. }
            | $crate::Request::RevokeMcpClient { .. }
            | $crate::Request::ListMcpAudit { .. }
            | $crate::Request::GetMcpServerInfo
            | $crate::Request::UpdateMcpRateLimits { .. }
    };
}

/// The configuration merge, backup and file requests (the customize dialog).
#[macro_export]
macro_rules! config_merge_requests {
    () => {
        $crate::Request::PreviewConfigMerge { .. }
            | $crate::Request::ResolveConfigMerge { .. }
            | $crate::Request::ApplyConfigMerge { .. }
            | $crate::Request::ListConfigBackups { .. }
            | $crate::Request::RevertConfig { .. }
            | $crate::Request::OpenConfigFile { .. }
    };
}

/// The remote-login requests `nail-remote-auth` answers itself (password,
/// TOTP, pairing codes); part of [`remote_admin_requests`](crate::remote_admin_requests).
#[macro_export]
macro_rules! remote_auth_requests {
    () => {
        $crate::Request::SetRemotePassword { .. }
            | $crate::Request::StartTotpEnrollment { .. }
            | $crate::Request::ConfirmTotpEnrollment { .. }
            | $crate::Request::DisableTotp { .. }
            | $crate::Request::CreatePairingCode { .. }
    };
}

/// The remote UI administration requests (M2b: settings, users, passkeys, devices, sessions, pairing, audit).
#[macro_export]
macro_rules! remote_admin_requests {
    () => {
        $crate::Request::GetRemoteUiSettings
            | $crate::Request::UpdateRemoteUiSettings { .. }
            | $crate::Request::RegenerateRemoteCertificate
            | $crate::Request::ListRemoteUsers
            | $crate::Request::CreateRemoteUser { .. }
            | $crate::Request::UpdateRemoteUser { .. }
            | $crate::Request::DeleteRemoteUser { .. }
            | $crate::Request::UnlockRemoteUser { .. }
            | $crate::Request::RenamePasskey { .. }
            | $crate::Request::DeletePasskey { .. }
            | $crate::Request::ListRemoteDevices { .. }
            | $crate::Request::UpdateRemoteDevice { .. }
            | $crate::Request::RevokeRemoteDevice { .. }
            | $crate::Request::ListRemoteSessions { .. }
            | $crate::Request::RevokeRemoteSession { .. }
            | $crate::Request::RevokeAllRemoteSessions { .. }
            | $crate::Request::ListPairingCodes
            | $crate::Request::CancelPairingCode { .. }
            | $crate::Request::ListRemoteAudit { .. }
            | $crate::remote_auth_requests!()
    };
}

/// The desktop requests (M4b: installation, updates, desktop settings, default apps, first run).
#[macro_export]
macro_rules! desktop_requests {
    () => {
        $crate::Request::GetInstallation
            | $crate::Request::GetUpdateStatus
            | $crate::Request::SetUpdateSettings { .. }
            | $crate::Request::CheckForUpdates
            | $crate::Request::DownloadUpdate
            | $crate::Request::InstallUpdate
            | $crate::Request::GetDesktopSettings
            | $crate::Request::SetDesktopSettings { .. }
            | $crate::Request::GetDefaultApps
            | $crate::Request::OpenDefaultAppsSettings
            | $crate::Request::GetFirstRun
            | $crate::Request::CompleteFirstRun
    };
}

/// The import, export, backup and restore requests and their jobs (M4c).
#[macro_export]
macro_rules! portability_requests {
    () => {
        $crate::Request::FindImportSources
            | $crate::Request::PreviewImport { .. }
            | $crate::Request::StartImport { .. }
            | $crate::Request::StartExport { .. }
            | $crate::Request::CreateBackup { .. }
            | $crate::Request::InspectBackup { .. }
            | $crate::Request::VerifyBackup { .. }
            | $crate::Request::PreviewRestore { .. }
            | $crate::Request::StartRestore { .. }
            | $crate::Request::ListPortabilityJobs
            | $crate::Request::GetPortabilityJob { .. }
            | $crate::Request::CancelPortabilityJob { .. }
    };
}

/// The connection log and the app log (administration; ADR 0190).
#[macro_export]
macro_rules! connlog_admin_requests {
    () => {
        $crate::Request::GetConnectionLog { .. }
            | $crate::Request::ClearConnectionLog { .. }
            | $crate::Request::ExportConnectionLog { .. }
            | $crate::Request::GetConnectionLogSettings
            | $crate::Request::SetConnectionLogSettings { .. }
            | $crate::Request::GetAppLog
    };
}

/// The plug-in administration requests on installed plug-ins (switching, grants, logs, settings, placements).
#[macro_export]
macro_rules! plugin_admin_requests {
    () => {
        $crate::Request::EnablePlugin { .. }
            | $crate::Request::DisablePlugin { .. }
            | $crate::Request::UninstallPlugin { .. }
            | $crate::Request::UpdatePluginGrants { .. }
            | $crate::Request::ListPluginLogs { .. }
            | $crate::Request::ClearPluginLogs { .. }
            | $crate::Request::GetPluginSettings { .. }
            | $crate::Request::UpdatePluginSettings { .. }
            | $crate::Request::PreviewPluginPlacements { .. }
    };
}

/// The PIM requests on sources, calendars and address books (M3a).
#[macro_export]
macro_rules! pim_source_requests {
    () => {
        $crate::Request::ListPimSources
            | $crate::Request::AddPimSource { .. }
            | $crate::Request::UpdatePimSource { .. }
            | $crate::Request::RemovePimSource { .. }
            | $crate::Request::TestPimSource { .. }
            | $crate::Request::SyncPimSources { .. }
            | $crate::Request::ListCalendars { .. }
            | $crate::Request::CreateCalendar { .. }
            | $crate::Request::UpdateCalendar { .. }
            | $crate::Request::DeleteCalendar { .. }
            | $crate::Request::ListAddressBooks { .. }
            | $crate::Request::CreateAddressBook { .. }
            | $crate::Request::UpdateAddressBook { .. }
            | $crate::Request::DeleteAddressBook { .. }
            | $crate::Request::ResolvePimConflict { .. }
    };
}

/// The PIM requests on events and tasks (M3a).
#[macro_export]
macro_rules! pim_event_task_requests {
    () => {
        $crate::Request::ListEventInstances { .. }
            | $crate::Request::GetEvent { .. }
            | $crate::Request::CreateEvent { .. }
            | $crate::Request::UpdateEvent { .. }
            | $crate::Request::DeleteEvent { .. }
            | $crate::Request::MoveEvent { .. }
            | $crate::Request::FindFreeTime { .. }
            | $crate::Request::ListTasks { .. }
            | $crate::Request::GetTask { .. }
            | $crate::Request::CreateTask { .. }
            | $crate::Request::UpdateTask { .. }
            | $crate::Request::CompleteTask { .. }
            | $crate::Request::MoveTask { .. }
            | $crate::Request::DeleteTask { .. }
    };
}

/// The PIM requests on contacts and contact groups (M3a).
#[macro_export]
macro_rules! pim_contact_requests {
    () => {
        $crate::Request::ListContacts { .. }
            | $crate::Request::GetContact { .. }
            | $crate::Request::CreateContact { .. }
            | $crate::Request::UpdateContact { .. }
            | $crate::Request::DeleteContacts { .. }
            | $crate::Request::MoveContact { .. }
            | $crate::Request::SearchContacts { .. }
            | $crate::Request::FindDuplicateContacts { .. }
            | $crate::Request::MergeContacts { .. }
            | $crate::Request::UnlinkContact { .. }
            | $crate::Request::GetContactPhoto { .. }
            | $crate::Request::ListContactGroups { .. }
            | $crate::Request::CreateContactGroup { .. }
            | $crate::Request::UpdateContactGroup { .. }
            | $crate::Request::DeleteContactGroup { .. }
    };
}

#[cfg(test)]
mod tests {
    use crate::Request;

    /// Every group matches its own requests (a pattern that matches
    /// nothing would not compile; this checks a sample of each).
    #[test]
    #[expect(
        clippy::unnested_or_patterns,
        reason = "remote_admin_requests nests the remote_auth_requests group"
    )]
    fn groups_match_their_requests() {
        assert!(matches!(Request::ListMcpClients, mcp_admin_requests!()));
        assert!(matches!(Request::ListRemoteUsers, remote_admin_requests!()));
        assert!(matches!(
            Request::DisableTotp {
                user_id: "u".into()
            },
            remote_admin_requests!()
        ));
        assert!(matches!(
            Request::DisableTotp {
                user_id: "u".into()
            },
            remote_auth_requests!()
        ));
        assert!(matches!(Request::GetInstallation, desktop_requests!()));
        assert!(matches!(
            Request::FindImportSources,
            portability_requests!()
        ));
        assert!(matches!(Request::ListPimSources, pim_source_requests!()));
        assert!(!matches!(Request::ListPimSources, pim_contact_requests!()));
        assert!(!matches!(Request::Ping, pim_event_task_requests!()));
        assert!(!matches!(Request::Ping, config_merge_requests!()));
        assert!(!matches!(Request::Ping, plugin_admin_requests!()));
    }
}
