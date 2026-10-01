//! Wave M3c contract (ADR 0072): every Exchange request is routed and
//! answered `unsupported` until the integration (WP95) wires the engines;
//! a configured Exchange account is listed as one and its engine reports
//! itself offline with `unsupported`; the store keeps Exchange state.
//! WP95 replaces these expectations with the served behaviour.

use std::time::Duration;

use nail_api::exchange::{
    AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeMailboxSettings,
    InferenceClassification,
};
use nail_api::{AccountKind, AccountState, ErrorCode, Request, Response};
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

const EXCHANGE: &str = r#"account "work" name="Work" email="ada@contoso.example" {
    exchange {
        auth "oauth2" provider="microsoft"
    }
}
account "corp" name="Corp" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" {
        auth "windows"
    }
}
"#;

/// One request of every M3c kind.
fn every_m3c_request() -> Vec<Request> {
    let account = || nail_api::AccountId::new("work");
    vec![
        Request::GetExchangeStatus {
            account_id: account(),
        },
        Request::ListExchangeMailboxes {
            account_id: account(),
        },
        Request::AddExchangeMailbox {
            account_id: account(),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        },
        Request::UpdateExchangeMailbox {
            account_id: account(),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        },
        Request::RemoveExchangeMailbox {
            account_id: account(),
            address: "support@contoso.example".into(),
        },
        Request::ListExchangeCategories {
            account_id: account(),
        },
        Request::CreateExchangeCategory {
            account_id: account(),
            settings: ExchangeCategorySettings::named("Red"),
        },
        Request::UpdateExchangeCategory {
            account_id: account(),
            category_id: "c1".into(),
            settings: ExchangeCategorySettings::named("Blue"),
        },
        Request::DeleteExchangeCategory {
            account_id: account(),
            category_id: "c1".into(),
        },
        Request::ClassifyFocused {
            message_ids: vec!["m1".into()],
            classification: InferenceClassification::Other,
            always: false,
        },
        Request::GetAutomaticReplies {
            account_id: account(),
        },
        Request::SetAutomaticReplies {
            account_id: account(),
            settings: AutomaticRepliesSettings::default(),
        },
        Request::GetExchangeRules {
            account_id: account(),
        },
        Request::SyncExchangeRules {
            account_id: account(),
        },
        Request::SetExchangeInboxRuleEnabled {
            account_id: account(),
            server_rule_id: "r1".into(),
            enabled: false,
        },
        Request::DeleteExchangeInboxRule {
            account_id: account(),
            server_rule_id: "r1".into(),
        },
        Request::ListRoomLists {
            account_id: account(),
            refresh: false,
        },
        Request::FindRooms {
            account_id: account(),
            room_list: None,
            from: "2026-10-05T08:00:00Z".into(),
            to: "2026-10-05T09:00:00Z".into(),
            min_capacity: Some(4),
            refresh: false,
        },
    ]
}

#[tokio::test]
async fn every_m3c_request_is_routed_and_unsupported_until_wp95() {
    let (service, _data, _config) = setup_with(Some(EXCHANGE), false);
    let requests = every_m3c_request();
    assert_eq!(requests.len(), 18);
    for request in requests {
        let name = serde_json::to_value(&request).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned();
        match service.handle(request).await {
            Response::Error { code, message, .. } => {
                assert_eq!(code, ErrorCode::Unsupported, "{name}: {message}");
                assert!(message.contains(&name), "{name}: {message}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn exchange_accounts_are_listed_and_their_engine_is_not_wired() {
    let (service, _data, _config) = setup_with(Some(EXCHANGE), true);
    service.start();
    match service.handle(Request::ListAccounts).await {
        Response::Accounts { accounts } => {
            for id in ["work", "corp"] {
                let account = accounts.iter().find(|a| a.id.as_str() == id).unwrap();
                assert_eq!(account.kind, AccountKind::Exchange);
            }
        }
        other => panic!("{other:?}"),
    }
    let mut status = None;
    for _ in 0..200 {
        if let Response::AccountStatuses { statuses } =
            service.handle(Request::ListAccountStatuses).await
        {
            status = statuses
                .into_iter()
                .find(|s| s.account_id.as_str() == "corp");
            if status.as_ref().is_some_and(|s| s.error.is_some()) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = status.expect("a status for the Exchange account");
    assert_eq!(status.state, AccountState::Offline, "{status:?}");
    assert_eq!(status.error, Some(ErrorCode::Unsupported));
    // No folders until the server's hierarchy is synced.
    match service
        .handle(Request::ListFolders {
            account_id: "work".into(),
        })
        .await
    {
        Response::Folders { folders } => assert!(folders.is_empty(), "{folders:?}"),
        other => panic!("{other:?}"),
    }
    service.shutdown().await;
}

#[tokio::test]
async fn testing_an_exchange_account_reports_unsupported() {
    let (service, _data, _config) = setup_with(None, false);
    let config = nail_config::parse_accounts(EXCHANGE, "accounts.kdl")
        .unwrap()
        .accounts
        .remove(0);
    match service
        .handle(Request::TestAccount {
            config: Box::new(config),
            secrets: vec![],
        })
        .await
    {
        Response::AccountTested { result } => {
            assert_eq!(result.incoming.error, Some(ErrorCode::Unsupported));
            assert!(result.outgoing.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_store_keeps_exchange_state() {
    let (service, _data, _config) = setup_with(Some(EXCHANGE), false);
    let store = service.store();
    let account = nail_api::AccountId::new("work");
    let source = store.ensure_exchange_pim_source(&account, 1).unwrap();
    assert_eq!(source.as_str(), "exchange:work");
    store
        .set_exchange_hierarchy_cursor(&account, None, Some("delta"), 2)
        .unwrap();
    assert_eq!(
        store
            .exchange_hierarchy_cursor(&account, None)
            .unwrap()
            .as_deref(),
        Some("delta")
    );
    assert!(
        store
            .exchange_mailboxes(&account, false)
            .unwrap()
            .is_empty()
    );
}
