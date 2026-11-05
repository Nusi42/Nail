//! Wave M3c (contract ADR 0072, integration ADR 0077): every Exchange
//! request is routed and served from the store (the engines run them
//! against the server: `tests/exchange.rs`); requests for other accounts
//! answer `invalid_request`; a configured Exchange account is listed as
//! one and its engine starts (and reports an unreachable server);
//! `TestAccount` signs in through the engine; the store keeps Exchange
//! state.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::failed_status;
use nail_core::testing::TempService;

use nail_api::exchange::{
    AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeMailboxSettings,
    InferenceClassification,
};
use nail_api::{AccountKind, AccountState, ErrorCode, Request, Response};

// Loopback ports nothing listens on: the engines start and stay offline
// (no network in tests).
const EXCHANGE: &str = r#"account "work" name="Work" email="ada@contoso.example" {
    exchange host="127.0.0.1" port=9 tls="plaintext" {
        auth "oauth2" provider="microsoft"
    }
}
account "corp" name="Corp" email="ada@corp.example" {
    exchange engine="ews" host="127.0.0.1" url="http://127.0.0.1:9/EWS/Exchange.asmx" tls="plaintext" {
        auth "password" username="ada@corp.example"
    }
}
account "mail" name="Mail" email="ada@mail.example" {
    imap host="127.0.0.1" port=9 tls="plaintext" {
        auth "password" username="ada"
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
async fn every_m3c_request_is_routed_and_served() {
    let (service, _data, _config) =
        TempService::quiet_with(&[("accounts.kdl", EXCHANGE)], |_| {}).into_parts();
    let requests = every_m3c_request();
    assert_eq!(requests.len(), 18);
    for request in requests {
        let name = serde_json::to_value(&request).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned();
        let response = service.handle(request).await;
        match name.as_str() {
            // Ids nobody has.
            "UpdateExchangeCategory"
            | "DeleteExchangeCategory"
            | "SetExchangeInboxRuleEnabled"
            | "DeleteExchangeInboxRule"
            | "ClassifyFocused" => {
                assert!(
                    matches!(
                        response,
                        Response::Error {
                            code: ErrorCode::NotFound,
                            ..
                        }
                    ),
                    "{name}: {response:?}"
                );
            }
            // The server is asked (and unreachable here).
            "ListRoomLists" | "FindRooms" => {
                let Response::Error { code, .. } = &response else {
                    panic!("{name}: {response:?}");
                };
                assert_ne!(*code, ErrorCode::Unsupported, "{name}");
            }
            _ => assert!(
                !matches!(response, Response::Error { .. }),
                "{name}: {response:?}"
            ),
        }
    }
    // Other accounts are refused.
    let other = service
        .handle(Request::GetExchangeStatus {
            account_id: "mail".into(),
        })
        .await;
    assert!(
        matches!(
            other,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ),
        "{other:?}"
    );
    // The queued category and the shown mailbox are there.
    match service
        .handle(Request::ListExchangeCategories {
            account_id: "work".into(),
        })
        .await
    {
        Response::ExchangeCategories { categories, .. } => {
            assert_eq!(categories.len(), 1);
            assert_eq!(categories[0].name, "Red");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn exchange_accounts_are_listed_and_their_engines_start() {
    let (service, _data, _config) =
        TempService::quiet_with(&[("accounts.kdl", EXCHANGE)], |o| o.start_engines = true)
            .into_parts();
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
    let status = failed_status(&service, "corp", 400)
        .await
        .expect("a status for the Exchange account");
    assert_ne!(status.state, AccountState::Online, "{status:?}");
    assert_ne!(status.error, Some(ErrorCode::Unsupported), "{status:?}");
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
async fn testing_an_exchange_account_signs_in_through_the_engine() {
    let (service, _data, _config) = TempService::quiet().into_parts();
    let config = nail_config::parse_accounts(EXCHANGE, "accounts.kdl")
        .unwrap()
        .accounts
        .remove(1);
    match service
        .handle(Request::TestAccount {
            config: Box::new(config),
            secrets: vec![nail_api::accounts::SecretInput {
                endpoint: nail_api::accounts::EndpointSlot::Incoming,
                value: nail_api::accounts::SecretValue::Password {
                    password: "secret".into(),
                },
            }],
        })
        .await
    {
        Response::AccountTested { result } => {
            // Nothing listens there: a network error, not "unsupported".
            assert!(result.incoming.error.is_some());
            assert_ne!(result.incoming.error, Some(ErrorCode::Unsupported));
            assert!(result.outgoing.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_store_keeps_exchange_state() {
    let (service, _data, _config) =
        TempService::quiet_with(&[("accounts.kdl", EXCHANGE)], |_| {}).into_parts();
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
            .is_empty(),
        "{:?}",
        store.exchange_mailboxes(&account, false).unwrap()
    );
}
