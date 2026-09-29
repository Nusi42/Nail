//! The OAuth2 path end to end, as the core wires it: sign in with the
//! `nail-auth` token manager against the mock OAuth server, then send with
//! XOAUTH2 through `SmtpTransport` to the mock SMTP server, which accepts
//! exactly the tokens the OAuth server issued.

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{AuthConfig, OAuthProviderConfig, SecretKind, SecretRef, TlsMode};
use nail_api::engine::{Envelope, MemorySecretStore, OutgoingTransport};
use nail_auth::{CallbackPages, FlowOptions, ProviderRegistry, TokenManager};
use nail_proto_smtp::{SendOptions, SmtpAuth, SmtpConfig, SmtpTransport};
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer, MockSmtpConfig, MockSmtpServer};

#[tokio::test]
async fn sign_in_then_send_with_xoauth2_and_refresh() {
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    let smtp = MockSmtpServer::start(MockSmtpConfig::default())
        .await
        .unwrap();
    let registry = ProviderRegistry::from_config(&[OAuthProviderConfig {
        id: "google".into(),
        client_id: "nail-test-client".into(),
        client_secret: None,
        auth_url: Some(oauth.authorize_url()),
        token_url: Some(oauth.token_url()),
        scopes: vec![],
    }])
    .unwrap();
    let http = nail_auth::http_client_builder(nail_auth::tls_config_with_roots(&[]).unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let store = Arc::new(MemorySecretStore::new());
    let tokens = Arc::new(TokenManager::new(Arc::new(registry), store.clone(), http));

    // Account setup: sign in and keep the refresh token.
    let pages = CallbackPages {
        success_html: "ok".into(),
        failure_html: "no".into(),
    };
    let flow = tokens
        .start_flow("google", FlowOptions::new(pages))
        .await
        .unwrap();
    MockOAuthServer::approve(flow.authorize_url())
        .await
        .unwrap();
    let signed_in = flow.finish().await.unwrap();
    let secret = SecretRef::default_for(&"work".into(), SecretKind::OAuth2);
    tokens.store(&secret, &signed_in).await.unwrap();

    // The SMTP server trusts the first access token only.
    let user = oauth.user_of(&signed_in.access_token).unwrap();
    smtp.add_oauth_token(&user, &signed_in.access_token);

    let auth = SmtpAuth::from_config(
        &"work".into(),
        &AuthConfig::OAuth2 {
            username: user.clone(),
            provider: "google".into(),
            secret: None,
            mechanism: None,
        },
        store.clone(),
        tokens.clone(),
    )
    .unwrap();
    let config = SmtpConfig::new("127.0.0.1")
        .with_port(smtp.port())
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5));
    let transport: Arc<dyn OutgoingTransport> =
        Arc::new(SmtpTransport::new(config, auth, SendOptions::default()));
    let envelope = Envelope {
        from: user.clone(),
        to: vec!["bob@example.org".into()],
    };
    transport
        .send(&envelope, b"Subject: one\r\n\r\nfirst\r\n")
        .await
        .unwrap();
    assert_eq!(smtp.messages()[0].mechanism.as_deref(), Some("XOAUTH2"));

    // The server stops trusting the old token (as after expiry); the
    // transport gets a refreshed one and retries once.
    let smtp2 = MockSmtpServer::start(MockSmtpConfig::default())
        .await
        .unwrap();
    let issued_before = oauth.issued().len();
    let refreshed_config = SmtpConfig::new("127.0.0.1")
        .with_port(smtp2.port())
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5));
    // Pre-register the token the next refresh will issue (the mock numbers
    // its tokens sequentially: `access-<n>`).
    let current: usize = signed_in
        .access_token
        .strip_prefix("access-")
        .unwrap()
        .parse()
        .unwrap();
    smtp2.add_oauth_token(&user, &format!("access-{}", current + 1));
    let auth = SmtpAuth::from_config(
        &"work".into(),
        &AuthConfig::OAuth2 {
            username: user.clone(),
            provider: "google".into(),
            secret: None,
            mechanism: None,
        },
        store,
        tokens,
    )
    .unwrap();
    SmtpTransport::new(refreshed_config, auth, SendOptions::default())
        .send_message(&envelope, b"Subject: two\r\n\r\nsecond\r\n")
        .await
        .unwrap();
    assert_eq!(oauth.issued().len(), issued_before + 1);
    assert_eq!(smtp2.messages().len(), 1);
    assert_eq!(smtp2.connections(), 2);
}
