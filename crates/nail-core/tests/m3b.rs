//! Wave M3b (contract ADR 0065, integration ADR 0070): every invitation,
//! scheduling, free/busy and Gmail label request is served (never
//! `unsupported` / `internal`); a configured Gmail account gets the Gmail
//! API engine; messages without iTIP parts carry no card. The flows
//! themselves are in `tests/scheduling.rs` and `tests/gmail.rs`.

use std::time::Duration;

use nail_api::gmail::GmailLabelSettings;
use nail_api::pim::PimTime;
use nail_api::scheduling::{CancelScope, InvitationResponse, InvitationTarget, SchedulingSettings};
use nail_api::{AccountKind, AccountState, Address, ErrorCode, Request, Response};
use nail_core::{Service, ServiceOptions};

fn setup_with(
    accounts_kdl: Option<&str>,
    start_engines: bool,
) -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    if let Some(text) = accounts_kdl {
        std::fs::write(config.path().join("accounts.kdl"), text).unwrap();
    }
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = start_engines;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    (service, data, config)
}

/// One request of every M3b kind.
fn every_m3b_request() -> Vec<Request> {
    let message = || InvitationTarget::Message {
        message_id: "m1".into(),
    };
    vec![
        Request::GetInvitation {
            message_id: "m1".into(),
        },
        Request::RespondToInvitation {
            target: message(),
            response: InvitationResponse::Accept,
            comment: Some("See you".into()),
            send: true,
            calendar_id: None,
        },
        Request::ProposeNewTime {
            target: InvitationTarget::Event {
                event_id: "ev-1".into(),
                recurrence_id: None,
            },
            start: PimTime::utc("2026-10-05T10:00:00"),
            end: PimTime::utc("2026-10-05T11:00:00"),
            comment: None,
        },
        Request::RefreshInvitation { target: message() },
        Request::ForwardInvitation {
            target: message(),
            to: vec![Address {
                name: None,
                email: "carol@example.org".into(),
            }],
            comment: None,
        },
        Request::ApplyInvitation {
            message_id: "m1".into(),
            calendar_id: None,
        },
        Request::SendInvitations {
            event_id: "ev-1".into(),
            recurrence_id: None,
            attendees: vec![],
            comment: None,
        },
        Request::CancelMeeting {
            event_id: "ev-1".into(),
            scope: CancelScope::Meeting,
            recurrence_id: None,
            attendees: vec![],
            comment: None,
        },
        Request::RespondToCounter {
            message_id: "m1".into(),
            accept: false,
            comment: None,
        },
        Request::QueryFreeBusy {
            attendees: vec!["bob@example.org".into()],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-06T00:00:00Z".into(),
            identity_id: None,
            ignore_event_id: None,
        },
        Request::GetSchedulingSettings,
        Request::SetSchedulingSettings {
            settings: SchedulingSettings::default(),
        },
        Request::ListGmailLabels {
            account_id: "gm".into(),
        },
        Request::CreateGmailLabel {
            account_id: "gm".into(),
            settings: GmailLabelSettings::named("Work"),
        },
        Request::UpdateGmailLabel {
            account_id: "gm".into(),
            label_id: "Label_1".into(),
            settings: GmailLabelSettings::named("Office"),
        },
        Request::DeleteGmailLabel {
            account_id: "gm".into(),
            label_id: "Label_1".into(),
        },
    ]
}

#[tokio::test]
async fn every_m3b_request_is_served() {
    let (service, _data, _config) = setup_with(Some(GMAIL), false);
    let requests = every_m3b_request();
    assert_eq!(requests.len(), 16);
    for request in requests {
        let name = serde_json::to_value(&request).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned();
        // Unknown messages / events / labels are refused for what they
        // are, never as unsupported or internal.
        if let Response::Error { code, message, .. } = service.handle(request).await {
            assert!(
                !matches!(code, ErrorCode::Unsupported | ErrorCode::Internal),
                "{name}: {code:?} {message}"
            );
        }
    }
}

const GMAIL: &str = r#"account "gm" name="Gmail" email="ada@gmail.example" {
    gmail {
        auth "oauth2" provider="google"
    }
}
"#;

#[tokio::test]
async fn gmail_accounts_are_listed_and_their_engine_needs_a_sign_in() {
    let (service, _data, _config) = setup_with(Some(GMAIL), true);
    service.start();
    match service.handle(Request::ListAccounts).await {
        Response::Accounts { accounts } => {
            let gmail = accounts.iter().find(|a| a.id.as_str() == "gm").unwrap();
            assert_eq!(gmail.kind, AccountKind::Gmail);
        }
        other => panic!("{other:?}"),
    }
    // The Gmail API engine runs; without a stored sign-in (no refresh
    // token, nothing is sent anywhere) it reports an authentication error.
    let mut status = None;
    for _ in 0..200 {
        if let Response::AccountStatuses { statuses } =
            service.handle(Request::ListAccountStatuses).await
        {
            status = statuses.into_iter().find(|s| s.account_id.as_str() == "gm");
            if status.as_ref().is_some_and(|s| s.error.is_some()) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = status.expect("a status for the Gmail account");
    assert_eq!(status.state, AccountState::AuthError, "{status:?}");
    assert_eq!(status.error, Some(ErrorCode::AuthFailed));
    service.shutdown().await;
}

#[tokio::test]
async fn messages_without_itip_parts_carry_no_card() {
    let (service, _data, _config) = setup_with(None, false);
    nail_core::demo::seed_demo(service.store(), service.blobs()).unwrap();
    let store = service.store();
    let account = store.list_accounts().unwrap()[0].id.clone();
    let folder = store.list_folders(&account).unwrap()[0].id.clone();
    let message = store.list_messages(&folder, 0, 1).unwrap().messages[0].clone();
    assert!(message.gmail.is_none());
    match service
        .handle(Request::GetMessage {
            id: message.id.clone(),
        })
        .await
    {
        Response::Message { message } => assert!(message.invitation.is_none()),
        other => panic!("{other:?}"),
    }
    // The store's scheduling settings start at their defaults.
    assert_eq!(
        store.scheduling_settings().unwrap(),
        SchedulingSettings::default()
    );
}
