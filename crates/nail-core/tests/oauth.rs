//! OAuth2 through the core: `StartOAuth` against the mock authorization
//! server, the tokens used by `TestAccount`, stored by `AddAccount` and used
//! by the IMAP engine (XOAUTH2 / OAUTHBEARER); `CancelOAuth`.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::oauth_provider;
use nail_core::testing::TempService;

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{
    AccountConfig, AuthConfig, EndpointSlot, ImapOptions, IncomingServer, SecretInput, SecretValue,
    ServerEndpoint, TlsMode,
};
use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::{AccountId, AccountState, ErrorCode, Event, OAuthFlowId, Request, Response};
use nail_core::{EngineTiming, Service, ServiceOptions};
use nail_testkit::{MailStore, MockImapConfig, MockImapServer};
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer};
use tokio::sync::broadcast;

const TIMEOUT: Duration = Duration::from_secs(20);

async fn finished(
    events: &mut broadcast::Receiver<Event>,
    flow: &OAuthFlowId,
) -> Option<ErrorCode> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("OAuthFinished in time")
            .unwrap();
        if let Event::OAuthFinished { flow_id, error, .. } = event
            && &flow_id == flow
        {
            return error;
        }
    }
}

async fn start(service: &Service) -> (OAuthFlowId, String) {
    match service
        .handle(Request::StartOAuth {
            provider: "mock".into(),
            login_hint: Some("sam@nail.example".into()),
            protocol: None,
        })
        .await
    {
        Response::OAuthStarted {
            flow_id,
            authorize_url,
            opened,
        } => {
            assert!(!opened, "tests never open a browser");
            (flow_id, authorize_url)
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn oauth_accounts_sign_in_test_save_and_sync() {
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    // OAuth mechanisms are offered; the accepted token is set after sign-in.
    let imap = MockImapServer::start(
        MockImapConfig {
            oauth_token: Some("not-issued-yet".into()),
            ..MockImapConfig::default()
        },
        MailStore::demo(),
    )
    .await
    .unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let temp = TempService::open_with(
        &[("accounts.kdl", &oauth_provider("mock", &oauth, "mail"))],
        |data, config| {
            let mut options = ServiceOptions::new(data, config);
            options.secrets = Some(secrets.clone());
            options.timing = EngineTiming::fast();
            options
        },
    );
    let service = temp.service.clone();
    let mut events = service.subscribe();

    // Sign in: the mock plays the browser.
    let (flow, url) = start(&service).await;
    assert!(url.starts_with(&oauth.authorize_url()), "{url}");
    MockOAuthServer::approve(&url).await.unwrap();
    assert_eq!(finished(&mut events, &flow).await, None);
    let token = oauth.issued().last().unwrap().access_token.clone();
    imap.set_oauth_token(Some(&token));

    let account = AccountConfig {
        id: AccountId::new("gmailish"),
        name: "OAuth".into(),
        email: "sam@nail.example".into(),
        enabled: true,
        incoming: IncomingServer::Imap {
            server: ServerEndpoint {
                host: imap.host().into(),
                port: imap.port(),
                tls: TlsMode::Plaintext,
                auth: AuthConfig::OAuth2 {
                    username: "sam@nail.example".into(),
                    provider: "mock".into(),
                    secret: None,
                    mechanism: None,
                },
            },
            options: ImapOptions::default(),
        },
        outgoing: None,
        identities: vec![],
        sync: nail_api::accounts::SyncPolicy::default(),
        save_sent_copy: None,
        authserv_ids: Vec::new(),
        sieve: None,
    };
    let sign_in = vec![SecretInput {
        endpoint: EndpointSlot::Incoming,
        value: SecretValue::OAuth2 {
            flow_id: flow.clone(),
        },
    }];
    let Response::AccountTested { result } = service
        .handle(Request::TestAccount {
            config: Box::new(account.clone()),
            secrets: sign_in.clone(),
        })
        .await
    else {
        panic!()
    };
    assert!(result.incoming.ok(), "{result:?}");

    let saved = service
        .handle(Request::AddAccount {
            config: Box::new(account),
            secrets: sign_in.clone(),
        })
        .await;
    assert!(matches!(saved, Response::AccountSaved { .. }), "{saved:?}");
    // Only the refresh token is kept, in the keychain.
    let stored = secrets
        .get(&nail_api::SecretRef::parse("keychain:nail/gmailish/oauth2").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(Some(stored), oauth.issued().last().unwrap().refresh_token);
    // The sign-in was used up.
    assert!(matches!(
        service
            .handle(Request::TestAccount {
                config: Box::new(saved_config(&saved)),
                secrets: sign_in,
            })
            .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    // The engine logs in with the token.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let Response::AccountStatuses { statuses } =
            service.handle(Request::ListAccountStatuses).await
        else {
            panic!()
        };
        if statuses[0].state == AccountState::Online {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{statuses:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        imap.commands()
            .iter()
            .any(|c| c.starts_with("AUTHENTICATE OAUTHBEARER")
                || c.starts_with("AUTHENTICATE XOAUTH2")),
        "{:?}",
        imap.commands()
    );
    service.shutdown().await;
}

fn saved_config(response: &Response) -> AccountConfig {
    match response {
        Response::AccountSaved { config } => config.as_ref().clone(),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn sign_ins_can_be_cancelled_and_unknown_providers_fail() {
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    let provider = format!(
        "oauth-provider \"mock\" client-id=\"nail-test-client\" auth-url=\"{}\" token-url=\"{}\"\n",
        oauth.authorize_url(),
        oauth.token_url()
    );
    let temp = TempService::open_with(&[("accounts.kdl", &provider)], |data, config| {
        let mut options = ServiceOptions::new(data, config);
        options.secrets = Some(Arc::new(MemorySecretStore::new()));
        options
    });
    let service = temp.service.clone();
    let mut events = service.subscribe();
    let (flow, _) = start(&service).await;
    assert_eq!(
        service
            .handle(Request::CancelOAuth {
                flow_id: flow.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(
        finished(&mut events, &flow).await,
        Some(ErrorCode::Cancelled)
    );
    // Google is built in but has no client id here.
    assert!(matches!(
        service
            .handle(Request::StartOAuth {
                provider: "google".into(),
                login_hint: None,
                protocol: None,
            })
            .await,
        Response::Error {
            code: ErrorCode::Config,
            ..
        }
    ));
    assert!(matches!(
        service
            .handle(Request::CancelOAuth {
                flow_id: "oauth-99".into()
            })
            .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    service.shutdown().await;
}
