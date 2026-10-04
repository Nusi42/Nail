//! IDLE on one connection, the NOOP polling fallback, and the
//! [`MailboxWatcher`] with re-issue, reconnect and fault injection.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::*;
use futures::StreamExt;
use futures::future::BoxFuture;
use nail_proto_imap::{
    CredentialProvider, Credentials, IdleEnd, ImapError, MailboxChange, MailboxWatcher,
    WatchConfig, WatchEvent, WatchMode,
};
use nail_testkit::{MockCapabilities, NO_FLAGS};

const WAIT: Duration = Duration::from_secs(10);

fn never() -> std::future::Pending<()> {
    std::future::pending()
}

#[tokio::test]
async fn idle_reports_new_mail_from_another_client() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();

    let store = server.store().clone();
    let (outcome, ()) = tokio::join!(client.idle(WAIT, never()), async {
        assert!(server.wait_for_idle(1, WAIT).await);
        store.add_message("INBOX", &message("pushed"), NO_FLAGS);
    });
    let outcome = outcome.unwrap();
    assert_eq!(outcome.end, IdleEnd::Changes);
    assert_eq!(outcome.changes, [MailboxChange::Exists(4)]);
    // IDLE ended cleanly: the connection works.
    assert_eq!(server.idle_sessions(), 0);
    client.noop().await.unwrap();
}

#[tokio::test]
async fn idle_reports_flag_changes_and_vanished_messages_with_qresync() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    let store = server.store().clone();
    let (outcome, ()) = tokio::join!(client.idle(WAIT, never()), async {
        assert!(server.wait_for_idle(1, WAIT).await);
        store.add_flags("INBOX", 3, &["\\Flagged"]);
    });
    let outcome = outcome.unwrap();
    match &outcome.changes[..] {
        [
            MailboxChange::Flags {
                seq: 3,
                uid: Some(3),
                flags,
                modseq: Some(_),
            },
        ] => assert!(flags.flags.flagged),
        other => panic!("{other:?}"),
    }

    let (outcome, ()) = tokio::join!(client.idle(WAIT, never()), async {
        assert!(server.wait_for_idle(1, WAIT).await);
        store.remove_message("INBOX", 2);
    });
    assert_eq!(outcome.unwrap().changes, [MailboxChange::Vanished(vec![2])]);
}

#[tokio::test]
async fn idle_without_qresync_reports_sequence_expunges() {
    let server = server_with(MockCapabilities {
        qresync: false,
        condstore: false,
        ..MockCapabilities::default()
    })
    .await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    let store = server.store().clone();
    let (outcome, ()) = tokio::join!(client.idle(WAIT, never()), async {
        assert!(server.wait_for_idle(1, WAIT).await);
        store.remove_message("INBOX", 2);
    });
    assert_eq!(outcome.unwrap().changes, [MailboxChange::Expunged(2)]);
}

#[tokio::test]
async fn idle_times_out_for_reissue_and_stops_on_request() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();

    for _ in 0..2 {
        let outcome = client
            .idle(Duration::from_millis(100), never())
            .await
            .unwrap();
        assert_eq!(outcome.end, IdleEnd::Timeout);
        assert!(outcome.changes.is_empty());
    }
    assert_eq!(server.command_count("IDLE"), 2);

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (outcome, ()) = tokio::join!(
        client.idle(WAIT, async {
            let _ = stop_rx.await;
        }),
        async {
            assert!(server.wait_for_idle(1, WAIT).await);
            stop_tx.send(()).unwrap();
        }
    );
    assert_eq!(outcome.unwrap().end, IdleEnd::Stopped);
    client.noop().await.unwrap();
}

#[tokio::test]
async fn idle_outlives_the_read_timeout() {
    let server = modern_server().await;
    let config = config_for(&server).with_timeout(Duration::from_millis(150));
    let mut client = nail_proto_imap::ImapClient::connect(&config, &credentials(&server))
        .await
        .unwrap();
    client.examine("INBOX").await.unwrap();
    // The server stays silent for longer than the read timeout.
    let outcome = client
        .idle(Duration::from_millis(600), never())
        .await
        .unwrap();
    assert_eq!(outcome.end, IdleEnd::Timeout);
    // ... and the read timeout is back afterwards.
    server.delay_responses(Some("NOOP"), Duration::from_secs(2));
    assert!(matches!(client.noop().await, Err(ImapError::Timeout(_))));
}

#[tokio::test]
async fn changes_seen_before_idle_return_at_once() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    server
        .store()
        .add_message("INBOX", &message("early"), NO_FLAGS);
    // NOOP picks up the change; IDLE hands it out without waiting.
    client.noop().await.unwrap();
    assert_eq!(client.pending_changes(), [MailboxChange::Exists(4)]);
    let outcome = client.idle(WAIT, never()).await.unwrap();
    assert_eq!(outcome.end, IdleEnd::Changes);
    assert_eq!(outcome.changes, [MailboxChange::Exists(4)]);
    assert_eq!(server.command_count("IDLE"), 0);
}

#[tokio::test]
async fn a_connection_dropped_mid_idle_is_connection_lost() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    let (outcome, dropped) = tokio::join!(client.idle(WAIT, never()), async {
        assert!(server.wait_for_idle(1, WAIT).await);
        server.drop_idle_connections()
    });
    assert_eq!(dropped, 1);
    assert!(
        matches!(outcome, Err(ImapError::ConnectionLost)),
        "{outcome:?}"
    );
    assert!(client.is_broken());
}

#[tokio::test]
async fn idle_needs_the_extension_and_a_mailbox() {
    let server = bare_server().await;
    let mut client = login(&server).await;
    assert!(matches!(
        client.idle(WAIT, never()).await,
        Err(ImapError::Protocol(_))
    ));
    client.examine("INBOX").await.unwrap();
    assert!(matches!(
        client.idle(WAIT, never()).await,
        Err(ImapError::Unsupported(_))
    ));
}

#[tokio::test]
async fn polling_reports_changes_without_idle() {
    let server = bare_server().await;
    let mut client = login(&server).await;
    client.examine("INBOX").await.unwrap();
    assert!(client.poll().await.unwrap().is_empty());
    server.store().add_flags("INBOX", 3, &["\\Seen"]);
    server
        .store()
        .add_message("INBOX", &message("polled"), NO_FLAGS);
    let changes = client.poll().await.unwrap();
    assert!(changes.contains(&MailboxChange::Exists(4)), "{changes:?}");
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, MailboxChange::Flags { seq: 3, flags, .. } if flags.flags.seen))
    );
}

fn fast_watch() -> WatchConfig {
    WatchConfig::new("INBOX")
        .with_poll_interval(Duration::from_millis(50))
        .with_reconnect(Duration::from_millis(20), Duration::from_millis(200))
}

async fn next(watcher: &mut MailboxWatcher) -> WatchEvent {
    tokio::time::timeout(WAIT, watcher.next_event())
        .await
        .expect("watcher event in time")
        .expect("watcher still running")
}

async fn expect_connected(watcher: &mut MailboxWatcher, mode: WatchMode) {
    match next(watcher).await {
        WatchEvent::Connected { mode: got, status } => {
            assert_eq!(got, mode);
            assert!(status.exists > 0);
        }
        other => panic!("{other:?}"),
    }
}

async fn expect_exists(watcher: &mut MailboxWatcher, count: u32) {
    match next(watcher).await {
        WatchEvent::Changed(changes) => {
            assert!(
                changes.contains(&MailboxChange::Exists(count)),
                "{changes:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_watcher_idles_reconnects_after_a_drop_and_keeps_reporting() {
    let server = modern_server().await;
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch(),
    );
    expect_connected(&mut watcher, WatchMode::Idle).await;
    assert!(server.wait_for_idle(1, WAIT).await);
    server.store().add_message("INBOX", &message("a"), NO_FLAGS);
    expect_exists(&mut watcher, 4).await;

    // The connection dies mid-IDLE: reported, then re-established.
    assert!(server.wait_for_idle(1, WAIT).await);
    assert_eq!(server.drop_idle_connections(), 1);
    match next(&mut watcher).await {
        WatchEvent::Disconnected { error, retry_in } => {
            assert!(matches!(error, ImapError::ConnectionLost), "{error:?}");
            assert_eq!(retry_in, Duration::from_millis(20));
        }
        other => panic!("{other:?}"),
    }
    expect_connected(&mut watcher, WatchMode::Idle).await;
    assert!(server.wait_for_idle(1, WAIT).await);
    server.store().add_message("INBOX", &message("b"), NO_FLAGS);
    expect_exists(&mut watcher, 5).await;

    assert!(server.connection_count() >= 2);
    watcher.stop().await;
    assert_eq!(server.command_count("LOGOUT"), 1);
}

#[tokio::test]
async fn the_watcher_reissues_idle() {
    let server = modern_server().await;
    let watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch().with_idle_reissue(Duration::from_millis(60)),
    );
    tokio::time::timeout(WAIT, async {
        while server.command_count("IDLE") < 3 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("IDLE re-issued");
    assert_eq!(
        server.connection_count(),
        1,
        "re-issue keeps the connection"
    );
    watcher.stop().await;
}

#[tokio::test]
async fn the_watcher_polls_when_idle_is_missing_or_disabled() {
    let server = bare_server().await;
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch(),
    );
    expect_connected(&mut watcher, WatchMode::Poll).await;
    server.store().add_message("INBOX", &message("a"), NO_FLAGS);
    expect_exists(&mut watcher, 4).await;
    assert!(server.command_count("NOOP") >= 1);
    watcher.stop().await;

    let server = modern_server().await;
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch().with_idle(false),
    );
    expect_connected(&mut watcher, WatchMode::Poll).await;
    // It is a stream, too.
    server.store().add_message("INBOX", &message("b"), NO_FLAGS);
    let event = tokio::time::timeout(WAIT, watcher.next()).await.unwrap();
    assert!(matches!(event, Some(WatchEvent::Changed(_))), "{event:?}");
    assert_eq!(server.command_count("IDLE"), 0);
    drop(watcher);
}

/// Hands out an expired token first and a valid one when asked to refresh.
struct RotatingTokens {
    username: String,
    calls: AtomicUsize,
    refreshes: AtomicUsize,
}

impl CredentialProvider for RotatingTokens {
    fn credentials(&self, refresh: bool) -> BoxFuture<'_, nail_proto_imap::Result<Credentials>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if refresh {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
        }
        let token = if refresh { TOKEN } else { "expired" };
        let credentials = Credentials::oauth2(&self.username, token);
        Box::pin(async move { Ok(credentials) })
    }
}

#[tokio::test]
async fn the_watcher_refreshes_rejected_tokens_and_backs_off() {
    let server = modern_server().await;
    let provider = Arc::new(RotatingTokens {
        username: server.config().username.clone(),
        calls: AtomicUsize::new(0),
        refreshes: AtomicUsize::new(0),
    });
    let credentials: Arc<dyn CredentialProvider> = provider.clone();
    let mut watcher = MailboxWatcher::spawn(config_for(&server), credentials, fast_watch());
    match next(&mut watcher).await {
        WatchEvent::Disconnected { error, .. } => {
            assert!(matches!(error, ImapError::AuthFailed(_)), "{error:?}");
        }
        other => panic!("{other:?}"),
    }
    expect_connected(&mut watcher, WatchMode::Idle).await;
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
    watcher.stop().await;
}

/// Review M1a: a server that accepts the login but drops the connection on
/// `IDLE` must not be reconnected to every `reconnect_min`.
#[tokio::test]
async fn a_server_that_drops_idle_is_backed_off_from() {
    let server = modern_server().await;
    server.disconnect_on(Some("IDLE"));
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch().with_healthy_after(Duration::from_secs(30)),
    );
    let mut delays = Vec::new();
    while delays.len() < 4 {
        match next(&mut watcher).await {
            WatchEvent::Connected { .. } => {}
            WatchEvent::Disconnected { retry_in, .. } => delays.push(retry_in.as_millis()),
            other @ WatchEvent::Changed(_) => panic!("{other:?}"),
        }
    }
    assert_eq!(delays, [20, 40, 80, 160]);
    watcher.stop().await;

    // A connection that stayed up long enough resets the backoff.
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        fast_watch().with_healthy_after(Duration::ZERO),
    );
    let mut delays = Vec::new();
    while delays.len() < 3 {
        if let WatchEvent::Disconnected { retry_in, .. } = next(&mut watcher).await {
            delays.push(retry_in.as_millis());
        }
    }
    assert_eq!(delays, [20, 20, 20]);
    watcher.stop().await;
}

#[tokio::test]
async fn the_watcher_backs_off_exponentially_while_the_server_is_down() {
    // A port where nobody listens (the server is gone). Windows retries a
    // refused connect for seconds, so the connect timeout is kept short.
    let server = modern_server().await;
    let config = config_for(&server).with_timeout(Duration::from_millis(100));
    server.shutdown().await;
    let mut watcher =
        MailboxWatcher::spawn(config, Arc::new(Credentials::new("sam", "x")), fast_watch());
    let mut delays = Vec::new();
    for _ in 0..5 {
        match next(&mut watcher).await {
            WatchEvent::Disconnected { error, retry_in } => {
                assert!(error.is_transient(), "{error:?}");
                delays.push(retry_in.as_millis());
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(delays, [20, 40, 80, 160, 200]);
    watcher.stop().await;
}

#[tokio::test]
async fn a_missing_mailbox_is_reported_and_retried() {
    let server = modern_server().await;
    let mut watcher = MailboxWatcher::spawn(
        config_for(&server),
        Arc::new(credentials(&server)),
        WatchConfig {
            mailbox: "Later".to_owned(),
            ..fast_watch()
        },
    );
    match next(&mut watcher).await {
        WatchEvent::Disconnected { error, .. } => {
            assert_eq!(error.code(), nail_api::ErrorCode::NotFound);
        }
        other => panic!("{other:?}"),
    }
    server
        .store()
        .add_mailbox("Later", nail_api::FolderRole::Other);
    server.store().add_message("Later", &message("x"), NO_FLAGS);
    // Skip further failures until the mailbox exists.
    loop {
        match next(&mut watcher).await {
            WatchEvent::Connected { .. } => break,
            WatchEvent::Disconnected { .. } => {}
            other @ WatchEvent::Changed(_) => panic!("{other:?}"),
        }
    }
    watcher.stop().await;
}
