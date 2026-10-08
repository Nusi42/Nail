//! A single bad message never fails a batch (ADR 0191): every failure kind
//! injected into the mock server, and the connection log of a session
//! (ADR 0190).

use std::collections::BTreeMap;

use nail_api::connlog::ConnectionLogScope;
use nail_connlog::ConnectionLogs;
use nail_proto_imap::{BodyFetch, FetchFailureKind, ImapClient, ImapError, ResyncState, SyncState};
use nail_testkit::{MessageFault, MockCapabilities, MockImapServer, NO_FLAGS};

use crate::common::{config_for, credentials, login, message, modern_server, server_with};

/// A server whose `INBOX` holds `n` messages (subjects `m1` …).
async fn server_with_messages(n: u32) -> MockImapServer {
    let server = modern_server().await;
    for i in 4..=n {
        server
            .store()
            .append("INBOX", &message(&format!("m{i}")), NO_FLAGS);
    }
    server
}

async fn fetch_all(client: &mut ImapClient) -> nail_proto_imap::BatchFetch {
    let plan = client.plan_fetch("INBOX", None).await.unwrap();
    let uids: Vec<u32> = plan.batches.concat();
    client
        .fetch_batch_resilient(&uids, BodyFetch::Full)
        .await
        .unwrap()
}

#[tokio::test]
async fn malformed_responses_are_read_leniently() {
    let server = server_with_messages(6).await;
    server.fault_message(2, Some(MessageFault::Malformed));
    let mut client = login(&server).await;
    let fetched = fetch_all(&mut client).await;
    assert_eq!(fetched.messages.len(), 6, "{:?}", fetched.failed);
    assert_eq!(fetched.failed, []);
    let two = fetched.messages.iter().find(|m| m.uid == 2).unwrap();
    assert!(two.raw.starts_with(b"From: Ada"));
    // The strict wrapper reads it too.
    assert_eq!(client.fetch_batch(&[2]).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_refused_message_is_isolated_and_reported() {
    let server = server_with_messages(9).await;
    server.fault_message(7, Some(MessageFault::Refuse));
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    // The plain batch fetch fails as a whole …
    assert!(matches!(
        client.fetch_batch(&[1, 2, 7]).await,
        Err(ImapError::No(_))
    ));
    // … the resilient one only that message.
    let fetched = fetch_all(&mut client).await;
    let uids: Vec<u32> = fetched.messages.iter().map(|m| m.uid).collect();
    assert_eq!(uids, [1, 2, 3, 4, 5, 6, 8, 9]);
    assert_eq!(fetched.failed.len(), 1);
    let failure = &fetched.failed[0];
    assert_eq!(failure.uid, 7);
    assert_eq!(failure.kind, FetchFailureKind::Refused);
    assert!(failure.detail.contains("SERVERBUG"), "{failure:?}");
    assert!(!client.is_broken());
    client.noop().await.unwrap();
}

#[tokio::test]
async fn withheld_and_unreadable_messages_are_reported() {
    let server = server_with_messages(5).await;
    server.fault_message(2, Some(MessageFault::Withhold));
    server.fault_message(4, Some(MessageFault::Garbage));
    let mut client = login(&server).await;
    let fetched = fetch_all(&mut client).await;
    let uids: Vec<u32> = fetched.messages.iter().map(|m| m.uid).collect();
    assert_eq!(uids, [1, 3, 5]);
    let failed: Vec<(u32, FetchFailureKind)> =
        fetched.failed.iter().map(|f| (f.uid, f.kind)).collect();
    assert_eq!(
        failed,
        [
            (2, FetchFailureKind::Withheld),
            (4, FetchFailureKind::Withheld)
        ]
    );
    // Expunged meanwhile is no failure.
    server.fault_message(2, None);
    server.store().expunge_uids("INBOX", &[4]);
    let fetched = client
        .fetch_batch_resilient(&[2, 4], BodyFetch::Full)
        .await
        .unwrap();
    assert_eq!(fetched.messages.len(), 1);
    assert_eq!(fetched.failed, []);
}

#[tokio::test]
async fn long_uid_sets_are_split() {
    let server = server_with_messages(10).await;
    server.limit_fetch_uids(Some(3));
    let mut client = login(&server).await;
    let fetched = fetch_all(&mut client).await;
    assert_eq!(fetched.messages.len(), 10);
    assert_eq!(fetched.failed, []);
}

#[tokio::test]
async fn many_failures_fail_the_batch() {
    let server = server_with_messages(10).await;
    for uid in 1..=10 {
        server.fault_message(uid, Some(MessageFault::Refuse));
    }
    let mut client = login(&server).await;
    client.plan_fetch("INBOX", None).await.unwrap();
    let err = client
        .fetch_batch_resilient(&(1..=10).collect::<Vec<_>>(), BodyFetch::Full)
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::Protocol(_)), "{err:?}");
}

#[tokio::test]
async fn select_parameters_the_server_refuses_are_dropped() {
    let server = modern_server().await;
    server.refuse_select_parameters(true);
    let mut client = login(&server).await;
    let plan = client.plan_fetch("INBOX", None).await.unwrap();
    assert_eq!(plan.message_count(), 3);
    // QRESYNC resync falls back to CONDSTORE / a full scan.
    let local = BTreeMap::new();
    let resync = client
        .resync_flags(
            "INBOX",
            ResyncState {
                uid_validity: plan.status.uid_validity,
                highest_modseq: Some(1),
            },
            &local,
        )
        .await
        .unwrap();
    assert!(!resync.uid_validity_changed);
    let next = client
        .plan_fetch(
            "INBOX",
            Some(SyncState {
                uid_validity: plan.status.uid_validity,
                uid_next: 1,
            }),
        )
        .await
        .unwrap();
    assert_eq!(next.message_count(), 3);
}

#[tokio::test]
async fn the_connection_log_shows_protocol_lines_only() {
    let server = server_with(MockCapabilities::default()).await;
    server.fault_message(3, Some(MessageFault::Refuse));
    let logs = ConnectionLogs::new();
    let scope = ConnectionLogScope::account(&"work".into());
    let config = config_for(&server).with_tap(logs.tap(&scope));
    let mut client = ImapClient::connect(&config, &credentials(&server))
        .await
        .unwrap();
    let fetched = fetch_all(&mut client).await;
    assert_eq!(fetched.failed.len(), 1);
    let failure = &fetched.failed[0];
    assert!(
        failure
            .server_text
            .as_deref()
            .is_some_and(|t| t.contains("NO [SERVERBUG]")),
        "{failure:?}"
    );
    assert!(
        failure
            .command
            .as_deref()
            .is_some_and(|c| c.contains("UID FETCH 3 ")),
        "{failure:?}"
    );
    client.logout().await.unwrap();
    let text = logs.text(&scope);
    let password = &server.config().password;
    assert!(!text.contains(password.as_str()), "{text}");
    assert!(
        text.contains("C: A0001 AUTHENTICATE PLAIN <redacted>"),
        "{text}"
    );
    assert!(text.contains("EXAMINE \"INBOX\""), "{text}");
    assert!(
        text.lines()
            .any(|l| l.contains(" FETCH (UID 1 ") && l.contains(" bytes}")),
        "{text}"
    );
    for subject in ["one", "two", "three"] {
        assert!(
            !text.contains(&format!("Subject: {subject}")),
            "{subject} in {text}"
        );
        assert!(!text.contains(&format!("Hello {subject}")), "{text}");
    }
    assert!(text.contains("closed after"), "{text}");
}
