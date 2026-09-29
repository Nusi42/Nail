//! The IMAP client against the in-process mock server from `nail-testkit`.

use std::time::Duration;

use nail_api::{ErrorCode, FolderRole};
use nail_proto_imap::{
    AuthMechanism, Credentials, FetchBatchLimits, ImapClient, ImapConfig, ImapError, Security,
    SyncState, TlsTrust, UidRange,
};
use nail_testkit::{MailStore, MockImapConfig, MockImapServer, MockTls, demo_corpus};

const TIMEOUT: Duration = Duration::from_secs(10);

async fn demo_server() -> MockImapServer {
    MockImapServer::start(MockImapConfig::default(), MailStore::demo())
        .await
        .unwrap()
}

fn config_for(server: &MockImapServer, security: Security) -> ImapConfig {
    let mut config = ImapConfig::new(server.host())
        .with_port(server.port())
        .with_security(security)
        .with_timeout(TIMEOUT);
    if let Some(ca) = server.ca_certificate_der() {
        config = config.with_tls_trust(TlsTrust::CustomRoots(vec![ca.to_vec()]));
    }
    config
}

fn credentials(server: &MockImapServer) -> Credentials {
    Credentials::new(&server.config().username, &server.config().password)
}

async fn login(server: &MockImapServer) -> ImapClient {
    ImapClient::connect(
        &config_for(server, Security::InsecurePlaintext),
        &credentials(server),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn lists_folders_with_roles() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    assert!(client.capabilities().has("SPECIAL-USE"));

    let folders = client.list_folders().await.unwrap();
    let summary: Vec<_> = folders.iter().map(|f| (f.path.as_str(), f.role)).collect();
    assert_eq!(
        summary,
        [
            ("INBOX", FolderRole::Inbox),
            ("Archive", FolderRole::Archive),
            ("Sent", FolderRole::Sent)
        ]
    );
    assert!(
        folders
            .iter()
            .all(|f| f.selectable && f.parent_path.is_none())
    );
    assert_eq!(folders[0].delimiter.as_deref(), Some("/"));
    client.logout().await.unwrap();
}

#[tokio::test]
async fn special_use_wins_and_names_are_decoded() {
    let store = MailStore::new();
    store.add_mailbox("Outgoing", FolderRole::Sent);
    store.add_mailbox("Entw&APw-rfe", FolderRole::Other);
    store.add_mailbox("Work", FolderRole::Other);
    store.add_mailbox("Work/Reports", FolderRole::Other);
    let server = MockImapServer::start(MockImapConfig::default(), store)
        .await
        .unwrap();
    let mut client = login(&server).await;
    let folders = client.list_folders().await.unwrap();
    let outgoing = folders.iter().find(|f| f.path == "Outgoing").unwrap();
    assert_eq!(outgoing.role, FolderRole::Sent);
    let drafts = folders.iter().find(|f| f.path == "Entw&APw-rfe").unwrap();
    assert_eq!(drafts.name, "Entwürfe");
    assert_eq!(
        drafts.role,
        FolderRole::Drafts,
        "name heuristic without SPECIAL-USE"
    );
    let reports = folders.iter().find(|f| f.path == "Work/Reports").unwrap();
    assert_eq!(reports.name, "Reports");
    assert_eq!(reports.parent_path.as_deref(), Some("Work"));
}

#[tokio::test]
async fn full_fetch_returns_raw_messages_and_flags() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    let corpus = demo_corpus();
    let inbox = &corpus[0];

    let status = client.examine("INBOX").await.unwrap();
    assert_eq!(status.exists as usize, inbox.messages.len());
    assert_eq!(status.uid_next as usize, inbox.messages.len() + 1);

    let messages = client.fetch_range(UidRange::ALL).await.unwrap();
    assert_eq!(messages.len(), inbox.messages.len());
    for (i, (fetched, expected)) in messages.iter().zip(&inbox.messages).enumerate() {
        assert_eq!(fetched.uid as usize, i + 1);
        assert_eq!(fetched.raw, expected.raw, "{}", expected.file_name);
        assert_eq!(
            fetched.flags,
            expected.api_flags(),
            "{}",
            expected.file_name
        );
        assert_eq!(
            fetched.size,
            Some(u32::try_from(expected.raw.len()).unwrap())
        );
    }
    let welcome = messages[0].envelope.as_ref().unwrap();
    assert_eq!(welcome.subject.as_deref(), Some("Welcome to Nail"));
    assert_eq!(welcome.from[0].email, "ada@lovelace.example");
    assert_eq!(welcome.from[0].name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(
        welcome.message_id.as_deref(),
        Some("<welcome-0001@lovelace.example>")
    );
    assert_eq!(
        messages[0].internal_date.as_deref(),
        Some("2026-09-28T08:15:00+02:00")
    );

    // Read-only: fetching with BODY.PEEK[] must not mark anything as seen.
    let again = client.fetch_range(UidRange::ALL).await.unwrap();
    assert_eq!(
        again.iter().filter(|m| m.flags.seen).count(),
        messages.iter().filter(|m| m.flags.seen).count()
    );
    let unseen_in_store = server
        .store()
        .mailbox("INBOX")
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.flags.is_empty())
        .count();
    assert!(unseen_in_store > 0);

    let partial = client.fetch_range(UidRange::new(2, 4)).await.unwrap();
    assert_eq!(partial.iter().map(|m| m.uid).collect::<Vec<_>>(), [2, 3, 4]);
}

#[tokio::test]
async fn fetch_requires_examine() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    assert!(matches!(
        client.fetch_range(UidRange::ALL).await,
        Err(ImapError::Protocol(_))
    ));
}

#[tokio::test]
async fn incremental_fetch_only_returns_new_messages() {
    let server = demo_server().await;
    let mut client = login(&server).await;

    let first = client.fetch_new("Sent", None).await.unwrap();
    assert!(!first.uid_validity_changed);
    assert_eq!(first.messages.len(), 4);
    let state = first.next_state;
    assert_eq!(state.uid_next, 5);

    // Nothing new: `UID 5:*` matches the highest UID on the server; the client
    // must not return it again.
    let none = client.fetch_new("Sent", Some(state)).await.unwrap();
    assert!(none.messages.is_empty());
    assert_eq!(none.next_state, state);

    let uid = server
        .store()
        .append(
            "Sent",
            b"From: sam@nail.example\r\nSubject: New\r\n\r\nfresh",
            &["\\Seen"],
        )
        .unwrap();
    assert_eq!(uid, 5);
    let new = client.fetch_new("Sent", Some(state)).await.unwrap();
    assert!(!new.uid_validity_changed);
    assert_eq!(new.messages.len(), 1);
    assert_eq!(new.messages[0].uid, 5);
    assert!(new.messages[0].flags.seen);
    assert_eq!(new.next_state.uid_next, 6);

    // A stale state from before an expunge still only yields newer messages.
    assert!(server.store().expunge("Sent", 5));
    let after_expunge = client
        .fetch_new("Sent", Some(new.next_state))
        .await
        .unwrap();
    assert!(after_expunge.messages.is_empty());
    assert_eq!(after_expunge.next_state.uid_next, 6);
}

#[tokio::test]
async fn uid_validity_change_triggers_full_resync() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    let first = client.fetch_new("Archive", None).await.unwrap();
    let old = first.next_state;

    let new_validity = server.store().reset_uid_validity("Archive").unwrap();
    assert_ne!(new_validity, old.uid_validity);

    let resync = client.fetch_new("Archive", Some(old)).await.unwrap();
    assert!(resync.uid_validity_changed);
    assert_eq!(resync.messages.len(), first.messages.len());
    assert_eq!(resync.next_state.uid_validity, new_validity);
    assert_eq!(resync.status.uid_validity, new_validity);

    // Also detected when the old state claims more messages than exist now.
    let bogus = SyncState {
        uid_validity: 1,
        uid_next: 999,
    };
    let resync = client.fetch_new("Archive", Some(bogus)).await.unwrap();
    assert!(resync.uid_validity_changed);
    assert_eq!(resync.messages.len(), first.messages.len());
}

#[tokio::test]
async fn all_login_mechanisms_work() {
    let server = demo_server().await;
    for auth in [
        AuthMechanism::Auto,
        AuthMechanism::Login,
        AuthMechanism::Plain,
    ] {
        let config = config_for(&server, Security::InsecurePlaintext).with_auth(auth);
        let mut client = ImapClient::connect(&config, &credentials(&server))
            .await
            .unwrap();
        client.noop().await.unwrap();
        client.logout().await.unwrap();
    }
}

#[tokio::test]
async fn bad_credentials_are_reported() {
    let server = demo_server().await;
    for auth in [AuthMechanism::Login, AuthMechanism::Plain] {
        let config = config_for(&server, Security::InsecurePlaintext).with_auth(auth);
        let err = ImapClient::connect(&config, &Credentials::new("sam@nail.example", "wrong"))
            .await
            .unwrap_err();
        assert!(matches!(err, ImapError::AuthFailed(_)), "{auth:?}: {err:?}");
        assert_eq!(err.code(), ErrorCode::AuthFailed);
    }
}

#[tokio::test]
async fn unknown_folder_is_refused() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    let err = client.examine("Nope").await.unwrap_err();
    assert!(matches!(err, ImapError::No(_)), "{err:?}");
    // The connection stays usable.
    client.examine("INBOX").await.unwrap();
}

#[tokio::test]
async fn server_disconnect_during_fetch_is_an_error() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    server.disconnect_on(Some("UID FETCH"));
    let err = client.fetch_range(UidRange::ALL).await.unwrap_err();
    assert!(matches!(err, ImapError::ConnectionLost), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Network);
    assert!(err.is_transient());
}

#[tokio::test]
async fn server_shutdown_is_connection_lost() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    server.shutdown().await;
    let err = client.noop().await.unwrap_err();
    assert!(
        matches!(err, ImapError::ConnectionLost | ImapError::Io(_)),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::Network);
}

#[tokio::test]
async fn implicit_tls_with_trusted_ca() {
    let server = MockImapServer::start(
        MockImapConfig {
            tls: MockTls::Implicit,
            ..MockImapConfig::default()
        },
        MailStore::demo(),
    )
    .await
    .unwrap();
    let mut client = ImapClient::connect(
        &config_for(&server, Security::ImplicitTls),
        &credentials(&server),
    )
    .await
    .unwrap();
    let status = client.examine("INBOX").await.unwrap();
    assert!(status.exists > 0);
    client.logout().await.unwrap();

    // The same server is rejected when its CA is not trusted.
    let untrusted =
        config_for(&server, Security::ImplicitTls).with_tls_trust(TlsTrust::WebPkiRoots);
    let err = ImapClient::connect(&untrusted, &credentials(&server))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::Tls(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Network);
}

#[tokio::test]
async fn starttls_upgrades_before_login() {
    let server = MockImapServer::start(
        MockImapConfig {
            tls: MockTls::StartTls,
            ..MockImapConfig::default()
        },
        MailStore::demo(),
    )
    .await
    .unwrap();
    let mut client = ImapClient::connect(
        &config_for(&server, Security::StartTls),
        &credentials(&server),
    )
    .await
    .unwrap();
    // AUTH=PLAIN is only advertised after STARTTLS on this server.
    assert!(client.capabilities().has_auth("PLAIN"));
    assert_eq!(client.list_folders().await.unwrap().len(), 3);

    // Without TLS the server refuses to authenticate.
    let err = ImapClient::connect(
        &config_for(&server, Security::InsecurePlaintext),
        &credentials(&server),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ImapError::AuthUnsupported(_)), "{err:?}");
}

#[tokio::test]
async fn starttls_never_falls_back_to_plaintext() {
    let server = demo_server().await; // no STARTTLS offered
    let err = ImapClient::connect(
        &config_for(&server, Security::StartTls),
        &credentials(&server),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ImapError::StartTlsUnavailable), "{err:?}");
}

#[tokio::test]
async fn connection_refused_and_timeouts() {
    // A port nobody listens on (bind, then drop).
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config = ImapConfig::new("127.0.0.1")
        .with_port(port)
        .with_security(Security::InsecurePlaintext)
        .with_timeout(TIMEOUT);
    let err = ImapClient::connect(&config, &Credentials::new("a", "b"))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::Connect { .. }), "{err:?}");

    // A server that accepts but never greets.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    let hold = tokio::spawn(async move {
        let (_socket, _) = silent.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let config = config
        .with_port(port)
        .with_timeout(Duration::from_millis(300));
    let err = ImapClient::connect(&config, &Credentials::new("a", "b"))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::Timeout(_)), "{err:?}");
    hold.abort();
}

#[tokio::test]
async fn refused_fetch_and_search_are_errors_not_empty_results() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();

    server.fail_on(Some("UID FETCH"));
    let err = client.fetch_range(UidRange::ALL).await.unwrap_err();
    assert!(matches!(err, ImapError::No(_)), "{err:?}");

    server.fail_on(Some("UID SEARCH"));
    let err = client.fetch_range(UidRange::ALL).await.unwrap_err();
    assert!(matches!(err, ImapError::No(_)), "{err:?}");

    server.fail_on(None);
    assert!(!client.fetch_range(UidRange::ALL).await.unwrap().is_empty());
}

#[tokio::test]
async fn disconnect_during_list_is_an_error() {
    let server = demo_server().await;
    let mut client = login(&server).await;
    server.disconnect_on(Some("LIST"));
    let err = client.list_folders().await.unwrap_err();
    assert!(matches!(err, ImapError::ConnectionLost), "{err:?}");
}

#[tokio::test]
async fn plans_are_batched_by_size_and_fetched_batch_by_batch() {
    let store = MailStore::new();
    store.add_mailbox("INBOX", FolderRole::Inbox);
    let big = format!("Subject: big\r\n\r\n{}", "x".repeat(4000));
    for raw in [
        "Subject: a\r\n\r\na",
        big.as_str(),
        "Subject: b\r\n\r\nb",
        "Subject: c\r\n\r\nc",
    ] {
        store
            .append("INBOX", raw.as_bytes(), &[] as &[&str])
            .unwrap();
    }
    let server = MockImapServer::start(MockImapConfig::default(), store)
        .await
        .unwrap();
    let config =
        config_for(&server, Security::InsecurePlaintext).with_fetch_batch(FetchBatchLimits {
            max_messages: 2,
            max_bytes: 1000,
        });
    let mut client = ImapClient::connect(&config, &credentials(&server))
        .await
        .unwrap();

    let plan = client.plan_fetch("INBOX", None).await.unwrap();
    // The 4 kB message exceeds the byte limit and gets a batch of its own.
    assert_eq!(plan.batches, vec![vec![1], vec![2], vec![3, 4]]);
    assert_eq!(plan.message_count(), 4);
    assert_eq!(plan.final_state.uid_next, 5);
    assert_eq!(plan.state_after(1).uid_next, 3);

    let second = client.fetch_batch(&plan.batches[1]).await.unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].raw, big.as_bytes());

    // Resuming from a checkpoint only plans what is left.
    let resumed = client
        .plan_fetch("INBOX", Some(plan.state_after(1)))
        .await
        .unwrap();
    assert_eq!(resumed.batches, vec![vec![3, 4]]);
    let rest = client.fetch_batch(&resumed.batches[0]).await.unwrap();
    assert_eq!(rest.iter().map(|m| m.uid).collect::<Vec<_>>(), [3, 4]);

    // Nothing new after the final state.
    let done = client
        .plan_fetch("INBOX", Some(plan.final_state))
        .await
        .unwrap();
    assert!(done.batches.is_empty());
    assert_eq!(done.final_state, plan.final_state);
}
