//! Login mechanisms (PLAIN, LOGIN, XOAUTH2, OAUTHBEARER, with and without
//! SASL-IR / LITERAL+) and the extensions switched on after login
//! (COMPRESS=DEFLATE, ID, ENABLE QRESYNC / CONDSTORE).

mod common;

use common::*;
use nail_api::ErrorCode;
use nail_proto_imap::{AuthMechanism, Credentials, ImapClient, ImapError, UidRange};
use nail_testkit::{MailStore, MockCapabilities, MockImapConfig, MockImapServer, MockTls};

async fn connect_with(
    server: &MockImapServer,
    mechanism: AuthMechanism,
    credentials: &Credentials,
) -> Result<ImapClient, ImapError> {
    ImapClient::connect(&config_for(server).with_auth(mechanism), credentials).await
}

/// The login command the server received.
fn auth_command(server: &MockImapServer) -> String {
    server
        .commands()
        .into_iter()
        .find(|c| c.starts_with("AUTHENTICATE") || c == "LOGIN")
        .unwrap_or_default()
}

fn token(server: &MockImapServer, token: &str) -> Credentials {
    Credentials::oauth2(&server.config().username, token)
}

#[tokio::test]
async fn xoauth2_and_oauthbearer_log_in_with_a_valid_token() {
    for sasl_ir in [true, false] {
        let server = server_with(MockCapabilities {
            sasl_ir,
            ..MockCapabilities::default()
        })
        .await;
        for (mechanism, expected) in [
            (AuthMechanism::XOAuth2, "AUTHENTICATE XOAUTH2"),
            (AuthMechanism::OAuthBearer, "AUTHENTICATE OAUTHBEARER"),
            // With a token, Auto prefers the standard mechanism.
            (AuthMechanism::Auto, "AUTHENTICATE OAUTHBEARER"),
        ] {
            server.clear_commands();
            let mut client = connect_with(&server, mechanism, &token(&server, TOKEN))
                .await
                .unwrap_or_else(|e| panic!("{mechanism:?} sasl_ir={sasl_ir}: {e}"));
            assert_eq!(auth_command(&server), expected);
            assert!(client.examine("INBOX").await.unwrap().exists > 0);
            client.logout().await.unwrap();
        }
    }
}

#[tokio::test]
async fn a_rejected_token_is_an_auth_failure_with_the_server_details() {
    let server = modern_server().await;
    for mechanism in [AuthMechanism::XOAuth2, AuthMechanism::OAuthBearer] {
        let err = connect_with(&server, mechanism, &token(&server, "expired"))
            .await
            .unwrap_err();
        assert!(matches!(err, ImapError::AuthFailed(_)), "{err:?}");
        assert_eq!(err.code(), ErrorCode::AuthFailed);
        // The JSON from the error challenge is kept for diagnostics.
        assert!(err.to_string().contains("401"), "{err}");
    }
    // A refreshed token works on the same server.
    server.set_oauth_token(Some("fresh"));
    connect_with(&server, AuthMechanism::Auto, &token(&server, "fresh"))
        .await
        .unwrap();
}

#[tokio::test]
async fn tokens_need_an_oauth_mechanism_on_the_server() {
    let server = MockImapServer::start(MockImapConfig::default(), MailStore::new())
        .await
        .unwrap();
    let err = connect_with(&server, AuthMechanism::Auto, &token(&server, TOKEN))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::AuthUnsupported(_)), "{err:?}");
    // A password with an OAuth mechanism is a configuration mistake.
    let err = connect_with(&server, AuthMechanism::XOAuth2, &credentials(&server))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::AuthUnsupported(_)), "{err:?}");
}

#[tokio::test]
async fn plain_without_sasl_ir_uses_a_continuation() {
    let server = bare_server().await;
    let client = connect_with(&server, AuthMechanism::Plain, &credentials(&server))
        .await
        .unwrap();
    assert_eq!(auth_command(&server), "AUTHENTICATE PLAIN");
    client.logout().await.unwrap();
}

#[tokio::test]
async fn eight_bit_passwords_are_sent_as_literals() {
    for literal_plus in [false, true] {
        let server = MockImapServer::start(
            MockImapConfig {
                password: "pässwörd".to_owned(),
                capabilities: MockCapabilities {
                    literal_plus,
                    ..MockCapabilities::imap4rev1()
                },
                ..MockImapConfig::default()
            },
            MailStore::new(),
        )
        .await
        .unwrap();
        let client = connect_with(&server, AuthMechanism::Login, &credentials(&server))
            .await
            .unwrap_or_else(|e| panic!("literal_plus={literal_plus}: {e}"));
        client.logout().await.unwrap();
        let err = connect_with(
            &server,
            AuthMechanism::Login,
            &Credentials::new(&server.config().username, "passwort"),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::AuthFailed);
    }
}

#[tokio::test]
async fn a_modern_server_gets_compression_id_and_qresync() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    assert!(client.is_compressed());
    assert_eq!(server.compressed_sessions(), 1);
    assert!(client.qresync_enabled());
    assert!(client.condstore_enabled());

    // ID: ours reached the server, the server's came back.
    let ids = server.client_ids();
    assert_eq!(ids.len(), 1);
    assert!(ids[0].contains(&("name".to_owned(), Some("Nail".to_owned()))));
    let server_id = client.server_id().unwrap();
    assert_eq!(server_id["name"], "Nail mock IMAP");

    // Everything still works through the compressed stream, bodies included.
    let status = client.examine("INBOX").await.unwrap();
    assert!(status.highest_modseq.is_some());
    let messages = client.fetch_range(UidRange::ALL).await.unwrap();
    assert_eq!(messages.len(), 3);
    assert!(messages.iter().all(|m| m.modseq.is_some()));
    assert!(String::from_utf8_lossy(&messages[0].raw).contains("Hello one."));
    client.logout().await.unwrap();
}

#[tokio::test]
async fn compression_works_over_tls_with_large_bodies() {
    let store = MailStore::new();
    let big: String = (0..20_000)
        .map(|i| format!("line {i} of a long mail\r\n"))
        .collect();
    let raw = format!("Subject: big\r\n\r\n{big}");
    store.append("INBOX", raw.as_bytes(), &[] as &[&str]);
    let server = MockImapServer::start(
        MockImapConfig {
            tls: MockTls::Implicit,
            ..MockImapConfig::default()
        },
        store,
    )
    .await
    .unwrap();
    let mut client = login(&server).await;
    assert!(client.is_compressed());
    let fetched = client.fetch_new("INBOX", None).await.unwrap();
    assert_eq!(fetched.messages.len(), 1);
    assert_eq!(fetched.messages[0].raw, raw.as_bytes());
    client.logout().await.unwrap();
}

#[tokio::test]
async fn extensions_can_be_switched_off_or_missing() {
    let server = modern_server().await;
    let client = ImapClient::connect(
        &config_for(&server)
            .with_compress(false)
            .with_client_id(None),
        &credentials(&server),
    )
    .await
    .unwrap();
    assert!(!client.is_compressed());
    assert!(client.server_id().is_none());
    assert_eq!(server.compressed_sessions(), 0);
    assert!(server.client_ids().is_empty());
    assert!(
        !server
            .commands()
            .iter()
            .any(|c| c == "COMPRESS" || c == "ID")
    );

    // CONDSTORE without QRESYNC: only CONDSTORE is enabled.
    let server = server_with(MockCapabilities {
        qresync: false,
        ..MockCapabilities::default()
    })
    .await;
    let client = login(&server).await;
    assert!(client.condstore_enabled() && !client.qresync_enabled());

    // A bare server: nothing to switch on, and nothing is sent.
    let server = bare_server().await;
    let mut client = login(&server).await;
    assert!(!client.is_compressed() && !client.condstore_enabled());
    assert_eq!(client.examine("INBOX").await.unwrap().highest_modseq, None);
    let messages = client.fetch_range(UidRange::ALL).await.unwrap();
    assert!(messages.iter().all(|m| m.modseq.is_none()));
    let commands = server.commands();
    for unexpected in ["COMPRESS", "ID", "ENABLE"] {
        assert!(!commands.iter().any(|c| c == unexpected), "{commands:?}");
    }
}

#[tokio::test]
async fn a_refused_compress_is_not_fatal() {
    let server = modern_server().await;
    server.fail_on(Some("COMPRESS"));
    let mut client = login(&server).await;
    assert!(!client.is_compressed());
    assert!(client.examine("INBOX").await.is_ok());
}

#[tokio::test]
async fn a_slow_server_is_fine_within_the_timeout_and_times_out_beyond() {
    let server = modern_server().await;
    let config = config_for(&server).with_timeout(std::time::Duration::from_millis(400));
    let mut client = ImapClient::connect(&config, &credentials(&server))
        .await
        .unwrap();
    server.delay_responses(Some("NOOP"), std::time::Duration::from_millis(100));
    client.noop().await.unwrap();

    server.delay_responses(Some("NOOP"), std::time::Duration::from_secs(2));
    let err = client.noop().await.unwrap_err();
    assert!(matches!(err, ImapError::Timeout(_)), "{err:?}");
    // The connection is out of step now and refuses further commands.
    assert!(client.is_broken());
    assert!(matches!(
        client.examine("INBOX").await,
        Err(ImapError::ConnectionLost)
    ));
    server.clear_delay();
}
