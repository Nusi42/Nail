//! POP3 client against the mock server of `nail-testkit-net`.
#![cfg(test)]

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::{SaslMechanism, TlsMode};
use nail_proto_pop3::{
    LeavePolicy, Pop3Client, Pop3Config, Pop3Credentials, Pop3Error, TlsTrust, plan_deletions,
    plan_downloads, test_login, vanished,
};
use nail_testkit_net::{MockPop3Config, MockPop3Server, MockTls, Pop3Message};

const USER: &str = "sam@nail.example";
const PASS: &str = "secret";

fn maildrop() -> Vec<Pop3Message> {
    vec![
        Pop3Message {
            uid: "uid-1".into(),
            data: b"Subject: one\r\n\r\nfirst\r\n.dotted line\r\nlast\r\n".to_vec(),
        },
        Pop3Message {
            uid: "uid-2".into(),
            data: b"Subject: two\r\nX-A: b\r\n\r\nbody 1\r\nbody 2\r\nbody 3\r\n".to_vec(),
        },
    ]
}

async fn server(config: MockPop3Config) -> MockPop3Server {
    MockPop3Server::start(config, maildrop()).await.unwrap()
}

fn plain(server: &MockPop3Server) -> Pop3Config {
    Pop3Config::new("127.0.0.1")
        .with_port(server.port())
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5))
}

fn tls(server: &MockPop3Server, mode: TlsMode) -> Pop3Config {
    Pop3Config::new("localhost")
        .with_port(server.port())
        .with_tls(mode)
        .with_trust(TlsTrust::CustomRoots(vec![
            server.ca_der().unwrap().to_vec(),
        ]))
        .with_timeout(Duration::from_secs(5))
}

async fn logged_in(
    config: &Pop3Config,
    credentials: &Pop3Credentials,
) -> Result<Pop3Client, Pop3Error> {
    let mut client = Pop3Client::connect(config).await?;
    client.login(credentials).await?;
    Ok(client)
}

#[tokio::test]
async fn reads_lists_and_deletes() {
    let server = server(MockPop3Config::default()).await;
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    assert!(!client.is_encrypted());
    assert!(client.supports_apop());
    assert!(client.capabilities().has("UIDL"));
    let (count, size) = client.stat().await.unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        size,
        maildrop().iter().map(|m| m.data.len() as u64).sum::<u64>()
    );
    let messages = client.messages().await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!((messages[0].number, messages[0].uid.as_str()), (1, "uid-1"));
    assert_eq!(messages[1].size, maildrop()[1].data.len() as u64);

    // Dot-stuffing is undone; bytes round-trip exactly.
    assert_eq!(client.retr(1).await.unwrap(), maildrop()[0].data);
    assert_eq!(
        client.top(2, 1).await.unwrap(),
        b"Subject: two\r\nX-A: b\r\n\r\nbody 1\r\n".to_vec()
    );
    client.dele(1).await.unwrap();
    let err = client.retr(1).await.unwrap_err();
    assert!(
        matches!(err, Pop3Error::Refused { ref command, .. } if command == "RETR"),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::Rejected);
    client.noop().await.unwrap();
    client.quit().await.unwrap();
    assert_eq!(server.uids(), ["uid-2"]);
    assert_eq!(server.deleted(), ["uid-1"]);
    // SASL PLAIN is preferred when offered; the password never appears.
    let commands = server.commands();
    assert!(commands.contains(&"AUTH PLAIN".to_owned()), "{commands:?}");
    assert!(!commands.iter().any(|c| c.contains(PASS)));
}

#[tokio::test]
async fn deletions_need_quit_and_rset_undoes_them() {
    let server = server(MockPop3Config::default()).await;
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    client.dele(1).await.unwrap();
    drop(client);
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    assert_eq!(client.stat().await.unwrap().0, 2);
    client.dele(2).await.unwrap();
    client.rset().await.unwrap();
    client.quit().await.unwrap();
    assert!(server.deleted().is_empty());
    assert_eq!(server.uids().len(), 2);
}

#[tokio::test]
async fn login_methods() {
    let server = server(MockPop3Config {
        sasl: vec!["PLAIN".into(), "XOAUTH2".into(), "OAUTHBEARER".into()],
        oauth_tokens: vec![(USER.into(), "good".into())],
        ..MockPop3Config::default()
    })
    .await;
    let config = plain(&server);
    for mechanism in [
        SaslMechanism::Login,
        SaslMechanism::Apop,
        SaslMechanism::Plain,
    ] {
        let credentials = Pop3Credentials::Password {
            username: USER.into(),
            password: PASS.into(),
            mechanism: Some(mechanism),
        };
        let client = logged_in(&config, &credentials).await.unwrap();
        client.quit().await.unwrap();
    }
    logged_in(&config, &Pop3Credentials::oauth2(USER, "good"))
        .await
        .unwrap();
    let bearer = Pop3Credentials::OAuth2 {
        username: USER.into(),
        access_token: "good".into(),
        mechanism: Some(SaslMechanism::Oauthbearer),
    };
    logged_in(&config, &bearer).await.unwrap();

    let commands = server.commands();
    for expected in [
        "USER sam@nail.example",
        "PASS",
        "APOP sam@nail.example",
        "AUTH PLAIN",
        "AUTH XOAUTH2",
        "AUTH OAUTHBEARER",
    ] {
        assert!(
            commands.iter().any(|c| c == expected),
            "{expected} missing from {commands:?}"
        );
    }
    assert_eq!(server.logins(), 5);
}

#[tokio::test]
async fn login_failures() {
    let server = server(MockPop3Config::default()).await;
    let config = plain(&server);
    for mechanism in [None, Some(SaslMechanism::Login), Some(SaslMechanism::Apop)] {
        let credentials = Pop3Credentials::Password {
            username: USER.into(),
            password: "wrong".into(),
            mechanism,
        };
        let err = logged_in(&config, &credentials).await.unwrap_err();
        assert!(
            matches!(err, Pop3Error::AuthFailed(_)),
            "{mechanism:?}: {err:?}"
        );
        assert!(!err.is_retryable());
    }
    // The OAuth error challenge is answered and reported as auth failure.
    let err = logged_in(&config, &Pop3Credentials::oauth2(USER, "stale"))
        .await
        .unwrap_err();
    match err {
        Pop3Error::AuthFailed(message) => assert!(message.contains("401"), "{message}"),
        other => panic!("{other:?}"),
    }
    let err = logged_in(
        &config,
        &Pop3Credentials::Password {
            username: USER.into(),
            password: PASS.into(),
            mechanism: Some(SaslMechanism::ScramSha256),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Pop3Error::AuthUnsupported(_)));

    // A locked maildrop is temporary.
    server.fail("AUTH", "[IN-USE] maildrop locked", 1);
    let err = logged_in(&config, &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap_err();
    assert!(matches!(err, Pop3Error::Temporary(_)), "{err:?}");
    assert!(err.is_retryable());
}

#[tokio::test]
async fn long_oauth_tokens_use_a_continuation() {
    let token = "t".repeat(400);
    let server = server(MockPop3Config {
        oauth_tokens: vec![(USER.into(), token.clone())],
        ..MockPop3Config::default()
    })
    .await;
    logged_in(&plain(&server), &Pop3Credentials::oauth2(USER, &token))
        .await
        .unwrap();
    assert!(server.commands().contains(&"AUTH XOAUTH2".to_owned()));
}

#[tokio::test]
async fn implicit_tls_and_stls() {
    let implicit = server(MockPop3Config {
        tls: MockTls::Implicit,
        ..MockPop3Config::default()
    })
    .await;
    let mut client = logged_in(
        &tls(&implicit, TlsMode::Implicit),
        &Pop3Credentials::password(USER, PASS),
    )
    .await
    .unwrap();
    assert!(client.is_encrypted());
    assert_eq!(client.stat().await.unwrap().0, 2);

    let stls = server(MockPop3Config {
        tls: MockTls::StartTls,
        ..MockPop3Config::default()
    })
    .await;
    let client = logged_in(
        &tls(&stls, TlsMode::Starttls),
        &Pop3Credentials::password(USER, PASS),
    )
    .await
    .unwrap();
    assert!(client.is_encrypted());
    // Capabilities were asked again over TLS: STLS is gone, SASL appeared.
    assert!(!client.capabilities().has("STLS"));
    assert!(client.capabilities().has("SASL"));
    let commands = stls.commands();
    let tls_at = commands.iter().position(|c| c == "STLS").unwrap();
    let auth_at = commands.iter().position(|c| c.starts_with("AUTH")).unwrap();
    assert!(tls_at < auth_at);
}

#[tokio::test]
async fn stls_is_never_downgraded_and_certificates_are_checked() {
    let server_plain = server(MockPop3Config::default()).await;
    let err = Pop3Client::connect(&plain(&server_plain).with_tls(TlsMode::Starttls))
        .await
        .unwrap_err();
    assert_eq!(err, Pop3Error::StartTlsUnavailable);

    // Without CAPA the client still tries STLS and fails cleanly.
    let no_capa = server(MockPop3Config {
        capa: false,
        ..MockPop3Config::default()
    })
    .await;
    let err = Pop3Client::connect(&plain(&no_capa).with_tls(TlsMode::Starttls))
        .await
        .unwrap_err();
    assert_eq!(err, Pop3Error::StartTlsUnavailable);
    assert!(no_capa.commands().contains(&"STLS".to_owned()));

    let implicit = server(MockPop3Config {
        tls: MockTls::Implicit,
        ..MockPop3Config::default()
    })
    .await;
    let err = Pop3Client::connect(
        &tls(&implicit, TlsMode::Implicit).with_trust(TlsTrust::CustomRoots(Vec::new())),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Pop3Error::Tls(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Tls);
}

#[tokio::test]
async fn minimal_servers() {
    let server = server(MockPop3Config {
        capa: false,
        uidl: false,
        top: false,
        apop: false,
        sasl: vec![],
        ..MockPop3Config::default()
    })
    .await;
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    assert!(!client.capabilities().known);
    assert!(!client.supports_apop());
    assert_eq!(client.list().await.unwrap().len(), 2);
    assert_eq!(
        client.uidl().await.unwrap_err(),
        Pop3Error::Unsupported("UIDL".into())
    );
    assert!(matches!(
        client.top(1, 0).await,
        Err(Pop3Error::Refused { .. })
    ));

    let announced = self::server(MockPop3Config {
        uidl: false,
        top: false,
        ..MockPop3Config::default()
    })
    .await;
    let mut client = logged_in(&plain(&announced), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    assert_eq!(
        client.top(1, 0).await.unwrap_err(),
        Pop3Error::Unsupported("TOP".into())
    );
    assert_eq!(
        client.messages().await.unwrap_err().code(),
        ErrorCode::Unsupported
    );
}

#[tokio::test]
async fn connection_faults() {
    let server = server(MockPop3Config::default()).await;
    server.disconnect_on("LIST");
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    let err = client.list().await.unwrap_err();
    assert_eq!(err, Pop3Error::ConnectionLost);
    assert!(err.is_retryable());

    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Pop3Config::new("127.0.0.1")
        .with_port(silent.local_addr().unwrap().port())
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_millis(300));
    let err = Pop3Client::connect(&config).await.unwrap_err();
    assert!(matches!(err, Pop3Error::Timeout(_)), "{err:?}");

    let unused = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = Pop3Client::connect(
        &config
            .with_port(unused)
            .with_timeout(Duration::from_secs(10)),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Pop3Error::Connect { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Network);
}

#[tokio::test]
async fn full_sync_round_with_policy() {
    let server = server(MockPop3Config::default()).await;
    let now = 1_800_000_000;
    let mut known: HashMap<String, i64> = HashMap::new();

    // First round: download everything, keep it on the server.
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    let listing = client.messages().await.unwrap();
    let keys: HashSet<String> = known.keys().cloned().collect();
    for m in plan_downloads(&listing, &keys) {
        let data = client.retr(m.number).await.unwrap();
        assert!(!data.is_empty());
        known.insert(m.uid.clone(), now - 10 * 86_400);
    }
    assert_eq!(known.len(), 2);
    let policy = LeavePolicy {
        delete_after_days: Some(7),
        ..LeavePolicy::default()
    };
    server.add_message("uid-3", b"Subject: three\r\n\r\nnew\r\n".to_vec());
    for m in plan_deletions(&policy, &listing, &known, &HashSet::new(), now) {
        client.dele(m.number).await.unwrap();
    }
    client.quit().await.unwrap();
    assert_eq!(server.uids(), ["uid-3"]);

    // Second round: only the new message is downloaded; old UIDs vanished.
    let mut client = logged_in(&plain(&server), &Pop3Credentials::password(USER, PASS))
        .await
        .unwrap();
    let listing = client.messages().await.unwrap();
    let keys: HashSet<String> = known.keys().cloned().collect();
    let new: Vec<_> = plan_downloads(&listing, &keys)
        .into_iter()
        .map(|m| m.uid.clone())
        .collect();
    assert_eq!(new, ["uid-3"]);
    assert_eq!(vanished(known.keys(), &listing), ["uid-1", "uid-2"]);
    client.quit().await.unwrap();
}

#[tokio::test]
async fn test_login_reports_results() {
    let server = server(MockPop3Config::default()).await;
    let ok = test_login(&plain(&server), &Pop3Credentials::password(USER, PASS)).await;
    assert!(ok.ok(), "{ok:?}");
    assert!(ok.capabilities.iter().any(|c| c == "UIDL"));
    let bad = test_login(&plain(&server), &Pop3Credentials::password(USER, "nope")).await;
    assert_eq!(bad.error, Some(ErrorCode::AuthFailed));
    assert!(!bad.capabilities.is_empty());
    assert!(!bad.message.unwrap_or_default().contains("nope"));
    // Nothing was deleted by the test logins.
    assert_eq!(server.uids().len(), 2);
}
