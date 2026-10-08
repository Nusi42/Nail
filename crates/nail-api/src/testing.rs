//! Sample values of the contract types for tests (feature `testing`; the
//! crates enable it in their dev-dependencies): the one definition of the
//! accounts, folders and messages every crate's tests build (wave Q2,
//! ADR 0171). Tests change what they care about with struct update syntax
//! (`MessageSummary { subject: "Hi".into(), ..summary("m1", "a1", "inbox") }`).

use crate::accounts::{
    AccountConfig, AuthConfig, ImapOptions, IncomingServer, ServerEndpoint, SyncPolicy, TlsMode,
};
use crate::agents::{
    ActionAttribution, ActionKind, ActionTarget, ActionTargetKind, NewConfirmation,
};
use crate::domain::{Account, AccountKind, Flags, Folder, FolderRole, MessageSummary};
use crate::pim::{PimTime, Reminder, ReminderState, ReminderTarget};

/// A fixed instant tests start their clocks at (2026-09-21 14:13:20 UTC).
pub const T0: i64 = 1_790_000_000;

/// Noon of Tuesday 2026-09-29 (UTC): "now" of the rule and search tests.
pub const TUESDAY_NOON: i64 = 1_790_683_200;

/// The date every sample message carries.
pub const SAMPLE_DATE: &str = "2026-10-01T10:00:00Z";

/// An account named `Account <id>` with the address `<id>@example.org`.
pub fn account(id: &str, kind: AccountKind) -> Account {
    Account {
        id: id.into(),
        name: format!("Account {id}"),
        email: format!("{id}@example.org"),
        kind,
    }
}

/// The IMAP account `work` (`ada@example.org` at `imap.example.org:993`,
/// implicit TLS, no login), with defaults for everything else.
pub fn imap_account() -> AccountConfig {
    AccountConfig {
        id: "work".into(),
        name: "Work".into(),
        email: "ada@example.org".into(),
        enabled: true,
        incoming: IncomingServer::Imap {
            server: ServerEndpoint {
                host: "imap.example.org".into(),
                port: crate::accounts::default_port(
                    crate::accounts::MailProtocol::Imap,
                    TlsMode::Implicit,
                ),
                tls: TlsMode::Implicit,
                auth: AuthConfig::None,
            },
            options: ImapOptions::default(),
        },
        outgoing: None,
        identities: vec![],
        sync: SyncPolicy::default(),
        save_sent_copy: None,
        authserv_ids: vec![],
        sieve: None,
    }
}

/// A top-level folder of `account` at `path` (named after its last
/// segment), empty.
pub fn folder(id: &str, account: &str, path: &str, role: FolderRole) -> Folder {
    Folder {
        id: id.into(),
        account_id: account.into(),
        parent_id: None,
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        role,
        unread: 0,
        total: 0,
    }
}

/// A message `id` in `folder` of `account`: its subject is its id, no
/// sender or recipients, [`SAMPLE_DATE`], unread, nothing else set.
pub fn summary(id: &str, account: &str, folder: &str) -> MessageSummary {
    MessageSummary {
        id: id.into(),
        folder_id: folder.into(),
        account_id: account.into(),
        subject: id.to_owned(),
        from: None,
        to: Vec::new(),
        date: SAMPLE_DATE.to_owned(),
        flags: Flags::default(),
        has_attachments: false,
        preview: String::new(),
        thread_id: None,
        tags: Vec::new(),
        duplicates: Vec::new(),
        headers_only: false,
        gmail: None,
        exchange: None,
    }
}

/// A fired reminder `id` of the occurrence of event `ev1` ("Standup" in
/// calendar `c1`) at 09:00 UTC on 2026-10-01, fired 15 minutes before.
pub fn reminder(id: &str) -> Reminder {
    Reminder {
        id: id.into(),
        target: ReminderTarget::Event {
            event_id: "ev1".into(),
            recurrence_id: Some(PimTime::utc("2026-10-01T09:00:00")),
        },
        calendar_id: "c1".into(),
        title: "Standup".into(),
        location: None,
        description: None,
        starts_at: Some("2026-10-01T09:00:00Z".into()),
        all_day: false,
        fire_at: "2026-10-01T08:45:00Z".into(),
        state: ReminderState::Fired,
        snoozed_until: None,
    }
}

/// A pending send of draft `d1` ("Hi", one recipient, account `work`)
/// that `requested_by` asks to be confirmed (no idempotency key, five
/// minutes to decide).
pub fn send_confirmation(requested_by: ActionAttribution) -> NewConfirmation {
    let summary = crate::agents::send_summary(&requested_by.name, "Hi", 1);
    NewConfirmation {
        requested_by,
        kind: ActionKind::Send,
        operation: "send_message".into(),
        summary,
        targets: vec![ActionTarget {
            kind: ActionTargetKind::Draft,
            id: "d1".into(),
            label: "Hi".into(),
        }],
        changes: vec![],
        account_ids: vec!["work".into()],
        payload: serde_json::json!({"draft_id": "d1"}),
        idempotency_key: None,
        timeout_secs: crate::agents::CONFIRMATION_TIMEOUT_SECS,
    }
}

/// The installation facts of a development build (x64, nothing installed).
pub fn dev_installation() -> crate::desktop::InstallationInfo {
    crate::desktop::InstallationInfo {
        version: "1.0.0".into(),
        kind: crate::desktop::InstallKind::Development,
        scope: None,
        architecture: crate::desktop::WindowsArch::X64,
        install_dir: None,
        updates_disabled: None,
        forced_channel: None,
        mapi_installed: false,
        webview_version: None,
    }
}

/// The [`crate::desktop::DesktopHost`] methods of a test host that does
/// nothing desktop-wide: a development build ([`dev_installation`]), no
/// updates, no settings, no default apps, no dev endpoint. A test host
/// writes `attach` and what it records, plus `inert_desktop_host!();`.
#[macro_export]
macro_rules! inert_desktop_host {
    () => {
        fn installation(&self) -> $crate::desktop::InstallationInfo {
            $crate::testing::dev_installation()
        }
        fn update_state(&self) -> $crate::desktop::HostUpdateState {
            $crate::desktop::HostUpdateState::default()
        }
        fn apply_update_settings(&self, _: &$crate::desktop::UpdateSettings) {}
        fn check_for_updates(&self) -> Result<(), $crate::ApiError> {
            Ok(())
        }
        fn download_update(&self) -> Result<(), $crate::ApiError> {
            Ok(())
        }
        fn install_update(&self) -> Result<(), $crate::ApiError> {
            Ok(())
        }
        fn start_with_windows(&self) -> Option<bool> {
            None
        }
        fn apply_desktop_settings(
            &self,
            _: &$crate::desktop::DesktopSettings,
        ) -> Result<(), $crate::ApiError> {
            Ok(())
        }
        fn dev_endpoint_url(&self) -> Option<String> {
            None
        }
        fn default_apps(&self) -> $crate::desktop::DefaultAppsStatus {
            $crate::desktop::DefaultAppsStatus::unregistered()
        }
        fn open_default_apps_settings(&self) -> Result<(), $crate::ApiError> {
            Ok(())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples() {
        let a = account("work", AccountKind::Imap);
        assert_eq!(
            (a.name.as_str(), a.email.as_str()),
            ("Account work", "work@example.org")
        );
        let f = folder("f1", "work", "Lists/rust", FolderRole::Other);
        assert_eq!((f.name.as_str(), f.parent_id), ("rust", None));
        let m = summary("m1", "work", "f1");
        assert_eq!((m.subject.as_str(), m.date.as_str()), ("m1", SAMPLE_DATE));
        assert!(!m.flags.seen);
    }
}
