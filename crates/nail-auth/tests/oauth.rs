//! OAuth2 flow, refresh and token management against the mock server of
//! `nail-testkit-net`.

use std::sync::Arc;
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::{OAuthProviderConfig, SecretRef};
use nail_api::engine::{AccessTokenProvider, EngineError, MemorySecretStore, SecretStore};
use nail_auth::{AuthError, CallbackPages, FlowOptions, ProviderRegistry, TokenManager};
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer};

const SUCCESS: &str = "<p>signed-in-page</p>";
const FAILURE: &str = "<p>failed-page</p>";

fn pages() -> CallbackPages {
    CallbackPages {
        success_html: SUCCESS.to_owned(),
        failure_html: FAILURE.to_owned(),
    }
}

fn options() -> FlowOptions {
    FlowOptions {
        login_hint: Some("ada@nail.example".to_owned()),
        ..FlowOptions::new(pages())
    }
}

fn http() -> reqwest::Client {
    nail_auth::http_client_builder(nail_auth::tls_config_with_roots(&[]).unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

fn secret() -> SecretRef {
    SecretRef::parse("keychain:nail/work/oauth2").unwrap()
}

struct Setup {
    server: MockOAuthServer,
    store: Arc<MemorySecretStore>,
    manager: Arc<TokenManager>,
}

async fn setup(config: MockOAuthConfig, client_secret: Option<SecretRef>) -> Setup {
    let server = MockOAuthServer::start(config).await.unwrap();
    // The built-in Google provider, pointed at the mock.
    let registry = ProviderRegistry::from_config(&[OAuthProviderConfig {
        id: "google".into(),
        client_id: "nail-test-client".into(),
        client_secret,
        auth_url: Some(server.authorize_url()),
        token_url: Some(server.token_url()),
        scopes: vec![],
    }])
    .unwrap();
    let store = Arc::new(MemorySecretStore::new());
    let manager = Arc::new(TokenManager::new(Arc::new(registry), store.clone(), http()));
    Setup {
        server,
        store,
        manager,
    }
}

/// Plays the browser with reqwest and returns the loopback page's status
/// and body.
async fn browse(authorize_url: &str) -> (u16, String) {
    let client = http();
    let redirect = client.get(authorize_url).send().await.unwrap();
    assert_eq!(redirect.status().as_u16(), 302);
    let location = redirect.headers()["location"].to_str().unwrap().to_owned();
    let page = client.get(&location).send().await.unwrap();
    (page.status().as_u16(), page.text().await.unwrap())
}

#[tokio::test]
async fn sign_in_store_and_refresh() {
    let s = setup(MockOAuthConfig::default(), None).await;
    let flow = s.manager.start_flow("google", options()).await.unwrap();
    assert!(flow.redirect_uri().starts_with("http://127.0.0.1:"));
    let url = url::Url::parse(flow.authorize_url()).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["login_hint"], "ada@nail.example");
    assert_eq!(query["scope"], "https://mail.google.com/");
    assert_eq!(query["code_challenge_method"], "S256");

    let (status, body) = browse(flow.authorize_url()).await;
    assert_eq!((status, body.as_str()), (200, SUCCESS));
    let tokens = flow.finish().await.unwrap();
    assert!(tokens.refresh_token.is_some());
    assert_eq!(
        s.server.user_of(&tokens.access_token).as_deref(),
        Some("ada@nail.example")
    );

    s.manager.store(&secret(), &tokens).await.unwrap();
    assert_eq!(s.store.get(&secret()).unwrap(), tokens.refresh_token);
    // Cached: no token request.
    let cached = s
        .manager
        .access_token("google", &secret(), false)
        .await
        .unwrap();
    assert_eq!(cached, tokens.access_token);
    assert_eq!(s.server.token_requests().len(), 1);
    // Forced refresh.
    let fresh = s
        .manager
        .access_token("google", &secret(), true)
        .await
        .unwrap();
    assert_ne!(fresh, tokens.access_token);
    let requests = s.server.token_requests();
    assert_eq!(
        requests
            .iter()
            .map(|r| r.grant_type.as_str())
            .collect::<Vec<_>>(),
        ["authorization_code", "refresh_token"]
    );
    // Public client: id in the body, no secret, no Basic auth.
    assert!(!requests[0].basic_auth);
    assert_eq!(requests[0].form["client_id"], "nail-test-client");
    assert!(!requests[0].form.contains_key("client_secret"));
    // The refresh token was not rotated, so it is kept.
    assert_eq!(s.store.get(&secret()).unwrap(), tokens.refresh_token);
    assert!(!format!("{:?}", s.manager).contains(&tokens.access_token));
}

#[tokio::test]
async fn denied_cancelled_and_timed_out_flows() {
    let s = setup(
        MockOAuthConfig {
            deny: true,
            ..MockOAuthConfig::default()
        },
        None,
    )
    .await;
    let flow = s.manager.start_flow("google", options()).await.unwrap();
    let (status, body) = browse(flow.authorize_url()).await;
    assert_eq!((status, body.as_str()), (200, FAILURE));
    let err = flow.finish().await.unwrap_err();
    assert_eq!(err, AuthError::Cancelled);
    assert_eq!(err.code(), ErrorCode::Cancelled);

    s.server.set_deny(false);
    let flow = s.manager.start_flow("google", options()).await.unwrap();
    let redirect = url::Url::parse(flow.redirect_uri()).unwrap();
    let canceller = flow.canceller();
    canceller.cancel();
    assert_eq!(flow.finish().await.unwrap_err(), AuthError::Cancelled);
    // The listener is closed after cancelling.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", redirect.port().unwrap()))
            .await
            .is_err()
    );

    let flow = s
        .manager
        .start_flow(
            "google",
            FlowOptions {
                timeout: Duration::from_millis(100),
                ..options()
            },
        )
        .await
        .unwrap();
    assert_eq!(flow.finish().await.unwrap_err(), AuthError::Timeout);
}

#[tokio::test]
async fn foreign_requests_do_not_end_the_flow() {
    let s = setup(MockOAuthConfig::default(), None).await;
    let flow = s.manager.start_flow("google", options()).await.unwrap();
    let client = http();
    let base = flow.redirect_uri().to_owned();
    for path in [
        "favicon.ico",
        "?code=evil&state=wrong",
        "?error=access_denied&state=wrong",
        "?code=x",
    ] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 404, "{path}");
    }
    assert_eq!(
        MockOAuthServer::approve(flow.authorize_url())
            .await
            .unwrap(),
        200
    );
    let tokens = flow.finish().await.unwrap();
    assert!(s.server.user_of(&tokens.access_token).is_some());
}

async fn signed_in(s: &Setup) {
    let flow = s.manager.start_flow("google", options()).await.unwrap();
    MockOAuthServer::approve(flow.authorize_url())
        .await
        .unwrap();
    let tokens = flow.finish().await.unwrap();
    s.manager.store(&secret(), &tokens).await.unwrap();
}

#[tokio::test]
async fn revoked_refresh_tokens_are_auth_failures() {
    let s = setup(MockOAuthConfig::default(), None).await;
    signed_in(&s).await;
    s.server.revoke_all();
    let err: EngineError = s
        .manager
        .access_token("google", &secret(), true)
        .await
        .unwrap_err();
    assert_eq!((err.code, err.retryable), (ErrorCode::AuthFailed, false));
    assert!(err.message.contains("invalid_grant"), "{}", err.message);

    let other = SecretRef::parse("keychain:nail/other/oauth2").unwrap();
    let err = s
        .manager
        .access_token("google", &other, false)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert!(err.message.contains("keychain:nail/other/oauth2"));
}

#[tokio::test]
async fn short_lived_tokens_refresh_and_rotation_is_stored() {
    let s = setup(
        MockOAuthConfig {
            access_token_lifetime: 30,
            rotate_refresh_tokens: true,
            ..MockOAuthConfig::default()
        },
        None,
    )
    .await;
    signed_in(&s).await;
    let first_refresh = s.store.get(&secret()).unwrap().unwrap();
    // 30 s is inside the expiry margin: every call refreshes.
    let a = s
        .manager
        .access_token("google", &secret(), false)
        .await
        .unwrap();
    let b = s
        .manager
        .access_token("google", &secret(), false)
        .await
        .unwrap();
    assert_ne!(a, b);
    let rotated = s.store.get(&secret()).unwrap().unwrap();
    assert_ne!(rotated, first_refresh);
    assert_eq!(s.server.token_requests().len(), 3);
}

#[tokio::test]
async fn concurrent_requests_share_one_refresh() {
    let s = setup(MockOAuthConfig::default(), None).await;
    signed_in(&s).await;
    // A fresh manager (e.g. after a restart) has only the refresh token.
    let registry = s.manager.registry().clone();
    let manager = Arc::new(TokenManager::new(
        Arc::new(registry),
        s.store.clone(),
        http(),
    ));
    let mut tasks = Vec::new();
    for _ in 0..5 {
        let manager = Arc::clone(&manager);
        tasks.push(tokio::spawn(async move {
            manager
                .access_token("google", &secret(), false)
                .await
                .unwrap()
        }));
    }
    let mut tokens = Vec::new();
    for task in tasks {
        tokens.push(task.await.unwrap());
    }
    tokens.dedup();
    assert_eq!(tokens.len(), 1);
    assert_eq!(
        s.server
            .token_requests()
            .iter()
            .filter(|r| r.grant_type == "refresh_token")
            .count(),
        1
    );
}

#[tokio::test]
async fn client_secrets_come_from_the_keychain() {
    let secret_ref = SecretRef::parse("keychain:nail/oauth/google-client-secret").unwrap();
    let s = setup(
        MockOAuthConfig {
            client_secret: Some("shh".into()),
            ..MockOAuthConfig::default()
        },
        Some(secret_ref.clone()),
    )
    .await;
    let err = s.manager.start_flow("google", options()).await.unwrap_err();
    assert!(
        matches!(err, AuthError::Config(ref m) if m.contains("client secret")),
        "{err:?}"
    );

    s.store.set(&secret_ref, "shh").unwrap();
    signed_in(&s).await;
    let request = &s.server.token_requests()[0];
    assert_eq!(
        request.form.get("client_secret").map(String::as_str),
        Some("shh")
    );

    // A wrong secret is refused by the server.
    s.store.set(&secret_ref, "wrong").unwrap();
    let err = s
        .manager
        .access_token("google", &secret(), true)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert!(err.message.contains("invalid_client"), "{}", err.message);
    assert!(!err.message.contains("wrong"));
}

#[tokio::test]
async fn unreachable_token_endpoint_is_retryable() {
    let s = setup(MockOAuthConfig::default(), None).await;
    signed_in(&s).await;
    let registry = ProviderRegistry::from_config(&[OAuthProviderConfig {
        id: "google".into(),
        client_id: "nail-test-client".into(),
        client_secret: None,
        auth_url: Some(s.server.authorize_url()),
        token_url: Some("http://127.0.0.1:9/token".into()),
        scopes: vec![],
    }])
    .unwrap();
    let manager = TokenManager::new(Arc::new(registry), s.store.clone(), http());
    let err = manager
        .access_token("google", &secret(), true)
        .await
        .unwrap_err();
    assert_eq!((err.code, err.retryable), (ErrorCode::Network, true));

    let err = manager
        .access_token("microsoft", &secret(), true)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Config);
}
