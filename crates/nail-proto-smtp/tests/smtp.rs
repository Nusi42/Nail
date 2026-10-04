//! SMTP client against the mock server of `nail-testkit-net`.
#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, SaslMechanism, SecretRef, TlsMode};
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, EngineError, Envelope, MemorySecretStore, OutgoingTransport,
    SecretStore,
};
use nail_proto_smtp::{
    SendOptions, SmtpAuth, SmtpConfig, SmtpCredentials, SmtpError, SmtpSession, SmtpTransport,
    TlsTrust, test_login,
};
use nail_testkit_net::{MockSmtpConfig, MockSmtpServer, MockTls};

const USER: &str = "sam@nail.example";
const PASS: &str = "secret";
const WAIT: Duration = Duration::from_secs(5);

fn plain_config(server: &MockSmtpServer) -> SmtpConfig {
    SmtpConfig::new("127.0.0.1")
        .with_port(server.port())
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5))
}

fn tls_config(server: &MockSmtpServer, tls: TlsMode) -> SmtpConfig {
    SmtpConfig::new("localhost")
        .with_port(server.port())
        .with_tls(tls)
        .with_trust(TlsTrust::CustomRoots(vec![
            server.ca_der().unwrap().to_vec(),
        ]))
        .with_timeout(Duration::from_secs(5))
}

fn envelope(to: &[&str]) -> Envelope {
    Envelope {
        from: USER.to_owned(),
        to: to.iter().map(|s| (*s).to_owned()).collect(),
    }
}

const MESSAGE: &[u8] = b"From: sam@nail.example\r\nTo: bob@example.org\r\nSubject: Hi\r\n\r\nHello\r\n.leading dot\r\n";

async fn server(config: MockSmtpConfig) -> MockSmtpServer {
    MockSmtpServer::start(config).await.unwrap()
}

async fn session(
    config: &SmtpConfig,
    credentials: &SmtpCredentials,
) -> Result<SmtpSession, SmtpError> {
    let mut session = SmtpSession::connect(config).await?;
    session.authenticate(credentials).await?;
    Ok(session)
}

#[tokio::test]
async fn sends_over_plaintext_with_plain_auth() {
    let server = server(MockSmtpConfig::default()).await;
    let mut session = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    assert!(!session.is_encrypted());
    assert_eq!(session.authenticated_as(), Some(USER));
    assert_eq!(session.capabilities().size_limit(), Some(10 * 1024 * 1024));
    let receipt = session
        .send(
            &envelope(&["bob@example.org", "carol@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap();
    assert!(receipt.response.starts_with("250 "), "{}", receipt.response);
    session.quit().await.unwrap();

    let mail = server.wait_for_messages(1, WAIT).await.unwrap().remove(0);
    assert_eq!(mail.mail_from, USER);
    assert_eq!(
        mail.recipient_addresses(),
        ["bob@example.org", "carol@example.org"]
    );
    // Dot-stuffing is undone by the server and no blank line is appended.
    assert_eq!(mail.data, MESSAGE);
    assert_eq!(mail.mail_params, [format!("SIZE={}", MESSAGE.len())]);
    assert_eq!(mail.mechanism.as_deref(), Some("PLAIN"));
    assert!(!mail.tls);
    // The EHLO name is an address literal, not the machine's host name.
    assert!(server.commands().iter().any(|c| c == "EHLO [127.0.0.1]"));
}

#[tokio::test]
async fn implicit_tls_and_starttls() {
    let implicit = server(MockSmtpConfig {
        tls: MockTls::Implicit,
        ..MockSmtpConfig::default()
    })
    .await;
    let mut s = session(
        &tls_config(&implicit, TlsMode::Implicit),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    assert!(s.is_encrypted());
    s.send(
        &envelope(&["bob@example.org"]),
        MESSAGE,
        SendOptions::default(),
    )
    .await
    .unwrap();
    assert!(implicit.wait_for_messages(1, WAIT).await.unwrap()[0].tls);

    let starttls = server(MockSmtpConfig {
        tls: MockTls::StartTls,
        ..MockSmtpConfig::default()
    })
    .await;
    let mut s = session(
        &tls_config(&starttls, TlsMode::Starttls),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    assert!(s.is_encrypted());
    // AUTH is only announced after STARTTLS; we saw it.
    assert!(s.capabilities().has_auth("PLAIN"));
    assert!(!s.capabilities().has("STARTTLS"));
    s.send(
        &envelope(&["bob@example.org"]),
        MESSAGE,
        SendOptions::default(),
    )
    .await
    .unwrap();
    let mail = starttls.wait_for_messages(1, WAIT).await.unwrap().remove(0);
    assert!(mail.tls);
    let commands = starttls.commands();
    let tls_at = commands.iter().position(|c| c == "STARTTLS").unwrap();
    let auth_at = commands.iter().position(|c| c.starts_with("AUTH")).unwrap();
    assert!(tls_at < auth_at, "{commands:?}");
}

#[tokio::test]
async fn starttls_is_never_downgraded() {
    let server = server(MockSmtpConfig::default()).await;
    let config = plain_config(&server).with_tls(TlsMode::Starttls);
    let err = SmtpSession::connect(&config).await.unwrap_err();
    assert_eq!(err, SmtpError::StartTlsUnavailable);
    assert_eq!(err.code(), ErrorCode::Unsupported);
    assert!(!server.commands().iter().any(|c| c.starts_with("AUTH")));
}

#[tokio::test]
async fn untrusted_certificate_is_a_tls_error() {
    let server = server(MockSmtpConfig {
        tls: MockTls::Implicit,
        ..MockSmtpConfig::default()
    })
    .await;
    let config =
        tls_config(&server, TlsMode::Implicit).with_trust(TlsTrust::CustomRoots(Vec::new()));
    let err = SmtpSession::connect(&config).await.unwrap_err();
    assert!(matches!(err, SmtpError::Tls(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Tls);
}

#[tokio::test]
async fn authentication_mechanisms_and_failures() {
    let server = server(MockSmtpConfig {
        oauth_tokens: vec![(USER.to_owned(), "good-token".to_owned())],
        ..MockSmtpConfig::default()
    })
    .await;
    let config = plain_config(&server);

    let err = session(&config, &SmtpCredentials::password(USER, "wrong"))
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::AuthFailed(_)), "{err:?}");
    assert!(!err.is_retryable());

    let login = SmtpCredentials::Password {
        username: USER.to_owned(),
        password: PASS.to_owned(),
        mechanism: Some(SaslMechanism::Login),
    };
    session(&config, &login).await.unwrap();

    session(&config, &SmtpCredentials::oauth2(USER, "good-token"))
        .await
        .unwrap();
    let bearer = SmtpCredentials::OAuth2 {
        username: USER.to_owned(),
        access_token: "good-token".to_owned(),
        mechanism: Some(SaslMechanism::Oauthbearer),
    };
    session(&config, &bearer).await.unwrap();

    // A rejected token goes through the error challenge to a clean failure.
    let err = session(&config, &SmtpCredentials::oauth2(USER, "stale-token"))
        .await
        .unwrap_err();
    match &err {
        SmtpError::AuthFailed(message) => assert!(message.contains("401"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(!format!("{err}").contains("stale-token"));

    let mechanisms: Vec<String> = server
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("AUTH"))
        .collect();
    assert_eq!(
        mechanisms,
        [
            "AUTH PLAIN",
            "AUTH LOGIN",
            "AUTH XOAUTH2",
            "AUTH OAUTHBEARER",
            "AUTH XOAUTH2"
        ]
    );
}

#[tokio::test]
async fn server_without_auth_or_mechanism() {
    let server = server(MockSmtpConfig {
        mechanisms: vec!["LOGIN".to_owned()],
        ..MockSmtpConfig::default()
    })
    .await;
    let err = session(&plain_config(&server), &SmtpCredentials::oauth2(USER, "t"))
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::AuthUnsupported(_)), "{err:?}");

    // Sending without credentials to a server that requires them.
    let mut s = SmtpSession::connect(&plain_config(&server)).await.unwrap();
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::AuthFailed);
}

#[tokio::test]
async fn size_limit_is_checked_before_sending() {
    let server = server(MockSmtpConfig {
        size_limit: Some(64),
        ..MockSmtpConfig::default()
    })
    .await;
    let mut s = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let big = vec![b'x'; 200];
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            &big,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::TooLarge(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::TooLarge);
    assert!(!err.is_retryable());
    assert!(!server.commands().iter().any(|c| c.starts_with("MAIL")));

    // The server's own check (no SIZE announced in EHLO) is mapped too.
    let unlimited = self::server(MockSmtpConfig {
        size_limit: None,
        ..MockSmtpConfig::default()
    })
    .await;
    unlimited.fail("MESSAGE", 552, "5.3.4 Message too big", Some(1));
    let mut s = session(
        &plain_config(&unlimited),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::TooLarge(_)), "{err:?}");
}

#[tokio::test]
async fn permanent_and_transient_refusals() {
    let server = server(MockSmtpConfig::default()).await;
    server.reject_recipient("nobody@example.org", 550, "5.1.1 No such user");
    let mut s = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let err = s
        .send(
            &envelope(&["bob@example.org", "nobody@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    match &err {
        SmtpError::Rejected { code, message } => {
            assert_eq!(*code, 550);
            assert!(message.contains("nobody@example.org"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let engine: EngineError = err.into();
    assert_eq!(
        (engine.code, engine.retryable),
        (ErrorCode::Rejected, false)
    );
    assert!(server.commands().contains(&"RSET".to_owned()));

    // The same session can go on after RSET.
    server.fail("MAIL", 451, "4.3.0 Try again later", Some(1));
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, SmtpError::Transient { code: 451, .. }),
        "{err:?}"
    );
    assert!(err.is_retryable());
    s.send(
        &envelope(&["bob@example.org"]),
        MESSAGE,
        SendOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(server.messages().len(), 1);
}

#[tokio::test]
async fn dsn_and_8bit_parameters() {
    let server = server(MockSmtpConfig::default()).await;
    let mut s = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let eight_bit = "Subject: Grüße\r\n\r\nÄpfel\r\n".as_bytes();
    s.send(
        &envelope(&["bob@example.org"]),
        eight_bit,
        SendOptions { dsn: true },
    )
    .await
    .unwrap();
    let mail = server.wait_for_messages(1, WAIT).await.unwrap().remove(0);
    assert!(
        mail.mail_params.contains(&"BODY=8BITMIME".to_owned()),
        "{:?}",
        mail.mail_params
    );
    assert!(mail.mail_params.contains(&"RET=HDRS".to_owned()));
    assert_eq!(
        mail.recipients[0].params,
        ["NOTIFY=FAILURE,DELAY", "ORCPT=rfc822;bob@example.org"]
    );
    assert_eq!(mail.data, eight_bit);

    let seven_bit_only = self::server(MockSmtpConfig {
        eight_bit_mime: false,
        dsn: false,
        ..MockSmtpConfig::default()
    })
    .await;
    let mut s = session(
        &plain_config(&seven_bit_only),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            eight_bit,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::Unsupported(_)), "{err:?}");
    // DSN requested but not offered: sent without DSN parameters.
    s.send(
        &envelope(&["bob@example.org"]),
        MESSAGE,
        SendOptions { dsn: true },
    )
    .await
    .unwrap();
    let mail = seven_bit_only
        .wait_for_messages(1, WAIT)
        .await
        .unwrap()
        .remove(0);
    assert!(mail.recipients[0].params.is_empty());
}

#[tokio::test]
async fn invalid_envelopes() {
    let server = server(MockSmtpConfig::default()).await;
    let mut s = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let err = s
        .send(&envelope(&[]), MESSAGE, SendOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, SmtpError::InvalidAddress(_)));
    let err = s
        .send(
            &envelope(&["not an address"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRequest);
    // The null sender is allowed (bounces).
    s.send(
        &Envelope {
            from: String::new(),
            to: vec!["bob@example.org".into()],
        },
        MESSAGE,
        SendOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        server.wait_for_messages(1, WAIT).await.unwrap()[0].mail_from,
        ""
    );
}

#[tokio::test]
async fn network_failures_are_retryable() {
    // Nothing listens on the port any more.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config = SmtpConfig::new("127.0.0.1")
        .with_port(port)
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5));
    let err = SmtpSession::connect(&config).await.unwrap_err();
    assert!(matches!(err, SmtpError::Connect { .. }), "{err:?}");
    assert!(err.is_retryable());

    // A server that accepts but never greets.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config
        .with_port(silent.local_addr().unwrap().port())
        .with_timeout(Duration::from_millis(300));
    let err = SmtpSession::connect(&config).await.unwrap_err();
    assert!(matches!(err, SmtpError::Timeout(_)), "{err:?}");
    assert!(err.is_retryable());
    drop(silent);

    // The connection drops during DATA.
    let server = server(MockSmtpConfig::default()).await;
    server.disconnect_on("DATA");
    let mut s = session(
        &plain_config(&server),
        &SmtpCredentials::password(USER, PASS),
    )
    .await
    .unwrap();
    let err = s
        .send(
            &envelope(&["bob@example.org"]),
            MESSAGE,
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(err.is_retryable(), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Network);

    // A 554 greeting.
    server.fail("CONNECT", 554, "5.3.2 Service unavailable", Some(1));
    let err = SmtpSession::connect(&plain_config(&server))
        .await
        .unwrap_err();
    assert!(
        matches!(err, SmtpError::Rejected { code: 554, .. }),
        "{err:?}"
    );
}

#[derive(Debug, Default)]
struct FakeTokens {
    calls: Mutex<Vec<bool>>,
    refreshed: AtomicUsize,
}

impl AccessTokenProvider for FakeTokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        _secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            assert_eq!(provider, "google");
            self.calls.lock().unwrap().push(force_refresh);
            if force_refresh {
                self.refreshed.fetch_add(1, Ordering::SeqCst);
                Ok("fresh-token".to_owned())
            } else {
                Ok("stale-token".to_owned())
            }
        })
    }
}

#[tokio::test]
async fn transport_refreshes_a_rejected_oauth_token_once() {
    let server = server(MockSmtpConfig {
        oauth_tokens: vec![(USER.to_owned(), "fresh-token".to_owned())],
        ..MockSmtpConfig::default()
    })
    .await;
    let tokens = Arc::new(FakeTokens::default());
    let auth = SmtpAuth::from_config(
        &"work".into(),
        &AuthConfig::OAuth2 {
            username: USER.to_owned(),
            provider: "google".to_owned(),
            secret: None,
            mechanism: None,
        },
        Arc::new(MemorySecretStore::new()),
        tokens.clone(),
    )
    .unwrap();
    let transport: Arc<dyn OutgoingTransport> = Arc::new(SmtpTransport::new(
        plain_config(&server),
        auth,
        SendOptions::default(),
    ));
    transport
        .send(&envelope(&["bob@example.org"]), MESSAGE)
        .await
        .unwrap();
    assert_eq!(*tokens.calls.lock().unwrap(), [false, true]);
    assert_eq!(server.messages().len(), 1);
    assert_eq!(server.connections(), 2);
}

#[tokio::test]
async fn transport_reads_passwords_from_the_secret_store() {
    let server = server(MockSmtpConfig::default()).await;
    let store = Arc::new(MemorySecretStore::new());
    let auth_config = AuthConfig::Password {
        username: USER.to_owned(),
        secret: None,
        mechanism: None,
    };
    let auth = SmtpAuth::from_config(
        &"work".into(),
        &auth_config,
        store.clone(),
        Arc::new(FakeTokens::default()),
    )
    .unwrap();
    let transport = SmtpTransport::new(plain_config(&server), auth, SendOptions::default());

    // No password stored yet: a permanent auth error, no connection made.
    let err = transport
        .send_message(&envelope(&["bob@example.org"]), MESSAGE)
        .await
        .unwrap_err();
    assert_eq!((err.code, err.retryable), (ErrorCode::AuthFailed, false));
    assert!(err.message.contains("keychain:nail/work/password"));
    assert_eq!(server.connections(), 0);

    store
        .set(
            &SecretRef::parse("keychain:nail/work/password").unwrap(),
            PASS,
        )
        .unwrap();
    transport
        .send_message(&envelope(&["bob@example.org"]), MESSAGE)
        .await
        .unwrap();
    assert_eq!(server.messages().len(), 1);

    let unsupported = SmtpAuth::from_config(
        &"work".into(),
        &AuthConfig::ClientCertificate {
            username: None,
            secret: None,
        },
        store,
        Arc::new(FakeTokens::default()),
    )
    .unwrap_err();
    assert_eq!(unsupported.code, ErrorCode::Unsupported);
    let with_password = SmtpTransport::new(
        plain_config(&server),
        SmtpAuth::Credentials(SmtpCredentials::password(USER, "hunter2")),
        SendOptions::default(),
    );
    assert!(!format!("{with_password:?}").contains("hunter2"));
}

#[tokio::test]
async fn test_login_reports_results() {
    let server = server(MockSmtpConfig::default()).await;
    let config = plain_config(&server);
    let ok = test_login(
        &config,
        &SmtpAuth::Credentials(SmtpCredentials::password(USER, PASS)),
    )
    .await;
    assert!(ok.ok(), "{ok:?}");
    assert!(ok.capabilities.iter().any(|c| c == "SIZE 10485760"));

    let bad = test_login(
        &config,
        &SmtpAuth::Credentials(SmtpCredentials::password(USER, "nope")),
    )
    .await;
    assert_eq!(bad.error, Some(ErrorCode::AuthFailed));
    assert!(!bad.capabilities.is_empty());
    assert!(!bad.message.unwrap_or_default().contains("nope"));

    drop(server);
    let down = test_login(&config, &SmtpAuth::None).await;
    assert_eq!(down.error, Some(ErrorCode::Network));
    assert!(down.capabilities.is_empty());
}
