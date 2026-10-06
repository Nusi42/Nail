//! Wave M2c (ADR 0043, wiring ADR 0048) on a store without servers:
//! settings, status, lists, sync policies, MCP settings, rules from
//! messages, and the local side of junk marks and snoozes. The server side
//! is tested against the mock servers in `m2c_wired.rs`.

#![cfg(test)]

use std::time::Duration;

use nail_api::accounts::{BodyPolicy, FolderMatch};
use nail_api::sieve::{SieveState, VacationSettings};
use nail_api::triage::{JunkSettings, NotificationKind, NotificationSettings, QuietHours, Weekday};
use nail_api::{
    AccountId, ErrorCode, Event, Folder, FolderRole, McpSettings, MessageId, Request, Response,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, seed_demo};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::broadcast;

/// A disabled IMAP account with ManageSieve (no engine runs, nothing
/// connects) and a POP3 account.
const ACCOUNTS: &str = r#"account "work" email="ada@example.org" enabled=#false {
    imap host="127.0.0.1" port=9 tls="plaintext"
    sieve script="nail"
}
account "home" email="ada@home.example" enabled=#false {
    pop3 host="127.0.0.1" port=9 tls="plaintext"
}
"#;

struct Fixture {
    service: Service,
    config_dir: TempDir,
    _data_dir: TempDir,
}

fn open() -> Fixture {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    std::fs::write(config_dir.path().join("accounts.kdl"), ACCOUNTS).unwrap();
    let mut options =
        nail_core::demo::demo_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    options.watch_config = false;
    let service = Service::open(options).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        config_dir,
        _data_dir: data_dir,
    }
}

fn work() -> AccountId {
    "work".into()
}

fn error(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("expected an error, got {other:?}"),
    }
}

/// The next event, skipping the progress of queued writes (local replays
/// of marks and snoozes finish asynchronously).
async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Event {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("no event within 10 s")
            .unwrap();
        if !matches!(event, Event::OpStatus { .. } | Event::MailChanged { .. }) {
            return event;
        }
    }
}

async fn demo_message(service: &Service) -> MessageId {
    let Response::Folders { folders } = service
        .handle(Request::ListFolders {
            account_id: DEMO_ACCOUNT_ID.into(),
        })
        .await
    else {
        panic!("no folders");
    };
    let inbox = folders
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap();
    let Response::Messages { messages, .. } = service
        .handle(Request::ListMessages {
            folder_id: inbox.id,
            offset: 0,
            limit: 1,
        })
        .await
    else {
        panic!("no messages");
    };
    messages[0].id.clone()
}

#[tokio::test]
async fn sieve_status_and_vacation() {
    let fx = open();
    let s = &fx.service;
    let Response::SieveStatus { accounts } =
        s.handle(Request::GetSieveStatus { account_id: None }).await
    else {
        panic!("no status");
    };
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, work());
    // The account is disabled: nothing is uploaded.
    assert_eq!(accounts[0].state, SieveState::Disabled);
    assert_eq!(accounts[0].script_name, "nail");

    assert_eq!(
        s.handle(Request::GetVacation { account_id: work() }).await,
        Response::Vacation {
            account_id: work(),
            settings: VacationSettings::default(),
        }
    );
    let mut events = s.subscribe();
    let empty = VacationSettings {
        enabled: true,
        ..VacationSettings::default()
    };
    assert_eq!(
        error(
            s.handle(Request::UpdateVacation {
                account_id: work(),
                settings: empty,
            })
            .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("vacation-issue-empty-body".into())
        )
    );
    let away = VacationSettings {
        enabled: true,
        body: "Back on Monday.".into(),
        ..VacationSettings::default()
    };
    assert_eq!(
        s.handle(Request::UpdateVacation {
            account_id: work(),
            settings: away.clone(),
        })
        .await,
        Response::Vacation {
            account_id: work(),
            settings: away.clone(),
        }
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::SieveStatusChanged { account_id: work() }
    );
    assert_eq!(
        s.handle(Request::GetVacation { account_id: work() }).await,
        Response::Vacation {
            account_id: work(),
            settings: away,
        }
    );

    // The preview compiles the vacation reply into the script.
    let Response::SieveScript { script, .. } = s
        .handle(Request::PreviewSieveScript { account_id: work() })
        .await
    else {
        panic!("no preview");
    };
    assert!(script.contains("Back on Monday."), "{script}");

    // Not possible for the account.
    for request in [
        Request::SyncSieve { account_id: work() },
        Request::GetVacation {
            account_id: "home".into(),
        },
    ] {
        assert_eq!(error(s.handle(request).await).0, ErrorCode::Unsupported);
    }
    assert_eq!(
        error(
            s.handle(Request::GetSieveStatus {
                account_id: Some("nobody".into())
            })
            .await
        )
        .0,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn junk_snooze_and_notifications() {
    let fx = open();
    let s = &fx.service;
    let Response::JunkStatus { status } = s
        .handle(Request::GetJunkStatus { account_id: work() })
        .await
    else {
        panic!("no junk status");
    };
    assert_eq!(status.settings, JunkSettings::default());
    assert!(!status.ready && status.trained_junk == 0 && status.last_trained_at.is_none());

    let mut events = s.subscribe();
    let on = JunkSettings {
        enabled: true,
        min_training: 1,
        ..JunkSettings::default()
    };
    let Response::JunkStatus { status } = s
        .handle(Request::UpdateJunkSettings {
            account_id: work(),
            settings: on.clone(),
        })
        .await
    else {
        panic!("no junk status");
    };
    assert_eq!(status.settings, on);
    assert_eq!(
        next_event(&mut events).await,
        Event::JunkStatusChanged { account_id: work() }
    );
    assert_eq!(
        error(
            s.handle(Request::UpdateJunkSettings {
                account_id: work(),
                settings: JunkSettings { threshold: 5, ..on },
            })
            .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("junk-issue-threshold".into())
        )
    );
    assert!(matches!(
        s.handle(Request::ResetJunkClassifier { account_id: work() })
            .await,
        Response::JunkStatus { .. }
    ));
    assert_eq!(
        next_event(&mut events).await,
        Event::JunkStatusChanged { account_id: work() }
    );

    // A local account: marks and snoozes are local queue writes.
    let message = demo_message(s).await;
    assert!(matches!(
        s.handle(Request::MarkJunk {
            message_ids: vec![message.clone()],
            junk: true,
        })
        .await,
        Response::Queued { .. }
    ));
    let Response::Message { message: body } = s
        .handle(Request::GetMessage {
            id: message.clone(),
        })
        .await
    else {
        panic!("no message");
    };
    assert!(body.junk.is_some_and(|v| v.junk));
    let until = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
    let snoozed = s
        .handle(Request::SnoozeMessages {
            message_ids: vec![message.clone()],
            until,
            wake_folder_id: None,
        })
        .await;
    assert!(matches!(snoozed, Response::Queued { .. }), "{snoozed:?}");
    let Response::Snoozed { messages } = s.handle(Request::ListSnoozed { account_id: None }).await
    else {
        panic!("no list");
    };
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        s.handle(Request::UnsnoozeMessages {
            message_ids: vec![message.clone()],
        })
        .await,
        Response::Queued { .. }
    ));
    assert_eq!(
        error(
            s.handle(Request::MarkJunk {
                message_ids: vec!["gone".into()],
                junk: false,
            })
            .await
        )
        .0,
        ErrorCode::NotFound
    );
    assert_eq!(
        s.handle(Request::ListSnoozed { account_id: None }).await,
        Response::Snoozed { messages: vec![] }
    );
    assert_eq!(
        s.handle(Request::ListSnoozed {
            account_id: Some(AccountId::all())
        })
        .await,
        Response::Snoozed { messages: vec![] }
    );

    assert_eq!(
        s.handle(Request::GetNotificationSettings).await,
        Response::NotificationSettings {
            settings: NotificationSettings::default()
        }
    );
    let quiet = NotificationSettings {
        quiet_hours: Some(QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
            days: vec![Weekday::Sat, Weekday::Sun],
        }),
        ..NotificationSettings::default()
    };
    // Fresh events: the marks and snoozes above broadcast their own.
    let mut events = s.subscribe();
    assert_eq!(
        s.handle(Request::UpdateNotificationSettings {
            settings: quiet.clone()
        })
        .await,
        Response::NotificationSettings {
            settings: quiet.clone()
        }
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::NotificationSettingsChanged
    );
    assert_eq!(
        s.handle(Request::GetNotificationSettings).await,
        Response::NotificationSettings { settings: quiet }
    );
    assert_eq!(s.handle(Request::TestNotification).await, Response::Done);
    match next_event(&mut events).await {
        Event::NotificationRequested { notification } => {
            assert_eq!(notification.kind, NotificationKind::Test);
            assert_eq!(notification.title.id, "notification-test-title");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn folder_sync_policies_write_accounts_kdl() {
    let fx = open();
    let s = &fx.service;
    let inbox = Folder {
        id: "work-inbox".into(),
        account_id: work(),
        parent_id: None,
        name: "INBOX".into(),
        path: "INBOX".into(),
        role: FolderRole::Inbox,
        unread: 0,
        total: 0,
    };
    s.store().upsert_folder(&inbox).unwrap();
    let Response::FolderSyncPolicy { policy } = s
        .handle(Request::GetFolderSyncPolicy {
            folder_id: inbox.id.clone(),
        })
        .await
    else {
        panic!("no policy");
    };
    assert_eq!(policy.bodies, BodyPolicy::Full);
    assert!(policy.on_demand && policy.idle && policy.rule.is_none());

    let Response::FolderSyncPolicy { policy } = s
        .handle(Request::SetFolderSyncPolicy {
            folder_id: inbox.id.clone(),
            bodies: Some(BodyPolicy::HeadersOnly),
            enabled: None,
            idle: None,
        })
        .await
    else {
        panic!("no policy");
    };
    assert_eq!(policy.bodies, BodyPolicy::HeadersOnly);
    assert_eq!(
        policy.rule.map(|r| r.folder),
        Some(FolderMatch::Path("INBOX".into()))
    );
    let kdl = std::fs::read_to_string(fx.config_dir.path().join("accounts.kdl")).unwrap();
    assert!(kdl.contains("headers-only"), "{kdl}");
    assert!(kdl.contains("sieve script=\"nail\""), "{kdl}");
    assert_eq!(
        error(
            s.handle(Request::SetFolderSyncPolicy {
                folder_id: inbox.id.clone(),
                bodies: Some(BodyPolicy::Recent { days: 0 }),
                enabled: None,
                idle: None,
            })
            .await
        )
        .0,
        ErrorCode::InvalidRequest
    );

    // POP3 keeps full bodies and cannot fetch on demand.
    let Response::Folders { folders } = s
        .handle(Request::ListFolders {
            account_id: "home".into(),
        })
        .await
    else {
        panic!("no folders");
    };
    let Response::FolderSyncPolicy { policy } = s
        .handle(Request::GetFolderSyncPolicy {
            folder_id: folders[0].id.clone(),
        })
        .await
    else {
        panic!("no policy");
    };
    assert_eq!(policy.bodies, BodyPolicy::Full);
    assert!(!policy.on_demand);

    // Bodies: stored ones count as fetched, unknown ones as failed.
    let message = demo_message(s).await;
    assert_eq!(
        s.handle(Request::FetchMessageBodies {
            message_ids: vec![message.clone(), "gone".into()],
        })
        .await,
        Response::BodiesFetched {
            fetched: vec![message],
            failed: vec!["gone".into()],
        }
    );
}

#[tokio::test]
async fn mcp_settings_and_rules_from_messages() {
    let fx = open();
    let s = &fx.service;
    let Response::McpSettings { settings, info } = s.handle(Request::GetMcpSettings).await else {
        panic!("no MCP settings");
    };
    assert_eq!(settings, McpSettings::default());
    assert!(!info.running);
    let mut events = s.subscribe();
    let fixed = McpSettings { port: Some(4799) };
    assert!(matches!(
        s.handle(Request::UpdateMcpSettings {
            settings: fixed.clone()
        })
        .await,
        Response::McpSettings { settings, .. } if settings == fixed
    ));
    assert_eq!(next_event(&mut events).await, Event::McpSettingsChanged);
    assert!(matches!(
        s.handle(Request::GetMcpSettings).await,
        Response::McpSettings { settings, .. } if settings == fixed
    ));
    assert_eq!(
        error(
            s.handle(Request::UpdateMcpSettings {
                settings: McpSettings { port: Some(80) }
            })
            .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("mcp-settings-issue-port".into())
        )
    );

    let message = demo_message(s).await;
    let Response::RuleSuggested { rule } = s
        .handle(Request::RuleFromMessage {
            message_id: message,
        })
        .await
    else {
        panic!("no rule");
    };
    assert_eq!(rule.id.as_str().len(), 0);
    assert!(!rule.name.is_empty(), "{:?}", rule.name);
    assert_eq!(
        error(
            s.handle(Request::RuleFromMessage {
                message_id: "gone".into()
            })
            .await
        )
        .0,
        ErrorCode::NotFound
    );
}
