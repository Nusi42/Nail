//! The sync engine against the in-process mock IMAP server.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::files_under;
use nail_core::testing::plain_options;

use std::time::Duration;

use nail_api::{Account, AccountId, AccountKind, ErrorCode, Event, FolderRole, Request, Response};
use nail_core::{ImapAccount, Service};
use nail_proto_imap::{Credentials, FetchBatchLimits, ImapConfig, Security};
use nail_testkit::{MailStore, MockImapConfig, MockImapServer, corpus_message, demo_corpus};
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::broadcast;

const TIMEOUT: Duration = Duration::from_secs(10);

struct Fixture {
    service: Service,
    server: MockImapServer,
    account: AccountId,
    _data_dir: TempDir,
    _config_dir: TempDir,
}

async fn setup(password: &str) -> Fixture {
    setup_with(password, |config| config).await
}

async fn setup_with(password: &str, tweak: impl FnOnce(ImapConfig) -> ImapConfig) -> Fixture {
    let server = MockImapServer::start(MockImapConfig::default(), MailStore::demo())
        .await
        .unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let service = Service::open(plain_options(data_dir.path(), config_dir.path())).unwrap();
    let account = Account {
        id: AccountId::new("imap-1"),
        name: "Mock".into(),
        email: server.config().username.clone(),
        kind: AccountKind::Imap,
    };
    let settings = ImapAccount {
        config: tweak(
            ImapConfig::new(server.host())
                .with_port(server.port())
                .with_security(Security::InsecurePlaintext)
                .with_timeout(TIMEOUT),
        ),
        credentials: Credentials::new(&server.config().username, password),
    };
    service
        .add_imap_account(account.clone(), settings)
        .await
        .unwrap();
    Fixture {
        service,
        server,
        account: account.id,
        _data_dir: data_dir,
        _config_dir: config_dir,
    }
}

fn drain(rx: &mut broadcast::Receiver<Event>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

async fn folder_totals(service: &Service, account: &AccountId) -> Vec<(String, FolderRole, u32)> {
    let Response::Folders { folders } = service
        .handle(Request::ListFolders {
            account_id: account.clone(),
        })
        .await
    else {
        panic!("expected folders");
    };
    let mut totals: Vec<_> = folders
        .into_iter()
        .map(|f| (f.path, f.role, f.total))
        .collect();
    totals.sort_by(|a, b| a.0.cmp(&b.0));
    totals
}

fn corpus_totals() -> Vec<(String, FolderRole, u32)> {
    let mut totals: Vec<_> = demo_corpus()
        .into_iter()
        .map(|f| (f.name, f.role, u32::try_from(f.messages.len()).unwrap()))
        .collect();
    totals.sort_by(|a, b| a.0.cmp(&b.0));
    totals
}

#[tokio::test]
async fn first_sync_mirrors_the_server() {
    let f = setup("secret").await;
    let mut events = f.service.subscribe();

    let report = f.service.sync_account(&f.account).await.unwrap();
    let expected = corpus_totals();
    let messages: u32 = expected.iter().map(|t| t.2).sum();
    assert_eq!(report.folders, u32::try_from(expected.len()).unwrap());
    assert_eq!(report.new_messages, messages);
    assert_eq!(folder_totals(&f.service, &f.account).await, expected);

    let events = drain(&mut events);
    let total = u32::try_from(expected.len()).unwrap() + 1;
    assert_eq!(
        events.last(),
        Some(&Event::SyncProgress {
            account_id: f.account.clone(),
            done: total,
            total
        })
    );
    let changed: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::MailChanged { .. }))
        .collect();
    assert!(!changed.is_empty(), "{changed:?}");
    assert!(!events.iter().any(|e| matches!(e, Event::SyncFailed { .. })));

    // Bodies are in the blob store: the reader works offline.
    let Response::Folders { folders } = f
        .service
        .handle(Request::ListFolders {
            account_id: f.account.clone(),
        })
        .await
    else {
        panic!()
    };
    let inbox = folders
        .iter()
        .find(|x| x.role == FolderRole::Inbox)
        .unwrap();
    let Response::Messages { messages, .. } = f
        .service
        .handle(Request::ListMessages {
            folder_id: inbox.id.clone(),
            offset: 0,
            limit: 100,
        })
        .await
    else {
        panic!()
    };
    let welcome = messages
        .iter()
        .find(|m| m.subject.contains("Welcome"))
        .expect("welcome message");
    assert!(welcome.flags.seen);
    f.server.shutdown().await;
    let Response::Message { message } = f
        .service
        .handle(Request::GetMessage {
            id: welcome.id.clone(),
        })
        .await
    else {
        panic!("expected message")
    };
    assert!(message.text.contains("Sam"));
}

#[tokio::test]
async fn second_sync_fetches_only_new_mail() {
    let f = setup("secret").await;
    f.service.sync_account(&f.account).await.unwrap();

    let again = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(again.new_messages, 0);

    let raw = corpus_message("INBOX", "01-welcome.eml").unwrap();
    let raw = String::from_utf8(raw)
        .unwrap()
        .replace("Subject: ", "Subject: Again: ");
    f.server
        .store()
        .append::<&str>("INBOX", raw.as_bytes(), &[])
        .unwrap();
    let mut events = f.service.subscribe();
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.new_messages, 1);
    let events = drain(&mut events);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::MailChanged { .. }))
            .count(),
        1
    );

    let totals = folder_totals(&f.service, &f.account).await;
    let inbox = totals.iter().find(|t| t.0 == "INBOX").unwrap();
    let corpus_inbox = corpus_totals()
        .into_iter()
        .find(|t| t.0 == "INBOX")
        .unwrap();
    assert_eq!(inbox.2, corpus_inbox.2 + 1);
}

#[tokio::test]
async fn uidvalidity_change_refetches_the_folder() {
    let f = setup("secret").await;
    f.service.sync_account(&f.account).await.unwrap();
    let before = folder_totals(&f.service, &f.account).await;

    f.server.store().reset_uid_validity("Sent").unwrap();
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.reset_folders, 1);
    let sent = corpus_totals().into_iter().find(|t| t.0 == "Sent").unwrap();
    assert_eq!(report.new_messages, sent.2);
    assert_eq!(folder_totals(&f.service, &f.account).await, before);
}

#[tokio::test]
async fn new_server_folders_appear() {
    let f = setup("secret").await;
    f.service.sync_account(&f.account).await.unwrap();

    assert!(f.server.store().add_mailbox("Receipts", FolderRole::Other));
    let mut events = f.service.subscribe();
    f.service.sync_account(&f.account).await.unwrap();
    let totals = folder_totals(&f.service, &f.account).await;
    assert!(totals.iter().any(|t| t.0 == "Receipts" && t.2 == 0));
    assert!(
        drain(&mut events)
            .iter()
            .any(|e| matches!(e, Event::MailChanged { folder_ids } if !folder_ids.is_empty()))
    );
}

#[tokio::test]
async fn bad_credentials_report_sync_failed() {
    let f = setup("wrong").await;
    let mut events = f.service.subscribe();
    let err = f.service.sync_account(&f.account).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::AuthFailed);
    match drain(&mut events).as_slice() {
        [
            Event::SyncFailed {
                account_id, code, ..
            },
        ] => {
            assert_eq!(account_id, &f.account);
            assert_eq!(*code, ErrorCode::AuthFailed);
        }
        other => panic!("unexpected events {other:?}"),
    }
    assert_eq!(folder_totals(&f.service, &f.account).await.len(), 0);
}

#[tokio::test]
async fn sync_now_runs_in_the_background() {
    let f = setup("secret").await;
    let mut events = f.service.subscribe();
    assert_eq!(
        f.service
            .handle(Request::SyncNow { account_id: None })
            .await,
        Response::SyncStarted
    );
    let expected = corpus_totals();
    let total = u32::try_from(expected.len()).unwrap() + 1;
    loop {
        let event = tokio::time::timeout(TIMEOUT, events.recv())
            .await
            .expect("sync did not finish")
            .unwrap();
        match event {
            Event::SyncProgress { done, total: t, .. } if done == t => {
                assert_eq!(t, total);
                break;
            }
            Event::SyncFailed { message, .. } => panic!("sync failed: {message}"),
            _ => {}
        }
    }
    assert_eq!(folder_totals(&f.service, &f.account).await, expected);
}

#[tokio::test]
async fn concurrent_passes_are_rejected() {
    let f = setup("secret").await;
    let (a, b) = tokio::join!(
        f.service.sync_account(&f.account),
        f.service.sync_account(&f.account)
    );
    let outcomes = [a.is_ok(), b.is_ok()];
    assert!(outcomes.contains(&true), "{a:?} {b:?}");
    let failed = [a, b].into_iter().find_map(Result::err);
    if let Some(err) = failed {
        assert_eq!(err.code(), ErrorCode::Unavailable);
    }
    // The guard is released: a later pass runs.
    f.service.sync_account(&f.account).await.unwrap();
}

#[tokio::test]
async fn folders_gone_from_the_server_are_removed() {
    let f = setup("secret").await;
    f.service.sync_account(&f.account).await.unwrap();
    let stale = nail_api::Folder {
        id: nail_api::FolderId::new("stale"),
        account_id: f.account.clone(),
        parent_id: None,
        name: "Stale".into(),
        path: "Stale".into(),
        role: FolderRole::Other,
        unread: 0,
        total: 0,
    };
    f.service.store().upsert_folder(&stale).unwrap();

    let mut events = f.service.subscribe();
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.removed_folders, 1);
    assert!(
        !folder_totals(&f.service, &f.account)
            .await
            .iter()
            .any(|t| t.0 == "Stale")
    );
    assert!(
        drain(&mut events).iter().any(
            |e| matches!(e, Event::MailChanged { folder_ids } if folder_ids.contains(&stale.id))
        )
    );
}

/// Regression: one folder the server refuses to open used to abort the
/// whole pass, so the folders after it were never synced.
#[tokio::test]
async fn a_failing_folder_does_not_stop_the_others() {
    let f = setup("secret").await;
    // "Sent" is synced first (shortest path); the server refuses to open it.
    f.server.deny_mailboxes(&["Sent"]);
    let mut events = f.service.subscribe();

    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.failed_folders.len(), 1);
    assert_eq!(report.failed_folders[0].path, "Sent");
    assert_eq!(report.failed_folders[0].code, ErrorCode::NotFound);
    let expected = corpus_totals();
    let sent = expected.iter().find(|t| t.0 == "Sent").unwrap().2;
    let all: u32 = expected.iter().map(|t| t.2).sum();
    assert_eq!(report.new_messages, all - sent);

    let totals = folder_totals(&f.service, &f.account).await;
    for (path, role, count) in &expected {
        let want = if path == "Sent" { 0 } else { *count };
        assert!(
            totals.contains(&(path.clone(), *role, want)),
            "{path}: {totals:?}"
        );
    }
    // The pass completes (full progress) and then reports the folder.
    let events = drain(&mut events);
    let total = u32::try_from(expected.len()).unwrap() + 1;
    let progress_end = events
        .iter()
        .position(|e| matches!(e, Event::SyncProgress { done, .. } if *done == total))
        .expect("the pass reached the end");
    match &events[progress_end + 1..] {
        [Event::SyncFailed { code, message, .. }] => {
            assert_eq!(*code, ErrorCode::NotFound);
            assert!(message.contains("Sent"), "{message}");
        }
        other => panic!("unexpected events {other:?}"),
    }

    // Once the server allows it again, the folder catches up.
    f.server.deny_mailboxes(&[]);
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert!(
        report.failed_folders.is_empty(),
        "{:?}",
        report.failed_folders
    );
    assert_eq!(report.new_messages, sent);
    assert_eq!(folder_totals(&f.service, &f.account).await, expected);
}

/// Regression: a folder used to be fetched completely into memory and
/// checkpointed only at the end, so a connection lost midway discarded
/// everything and the next pass started the folder from UID 1 again.
#[tokio::test]
async fn an_interrupted_folder_resumes_after_the_last_stored_batch() {
    let f = setup_with("secret", |config| {
        config.with_fetch_batch(FetchBatchLimits {
            max_messages: 1,
            max_bytes: u64::MAX,
        })
    })
    .await;
    let expected = corpus_totals();
    let sent = expected.iter().find(|t| t.0 == "Sent").unwrap().2;
    assert!(sent >= 3, "the test needs at least three sent messages");
    let all: u32 = expected.iter().map(|t| t.2).sum();

    // Two body batches (one message each) of "Sent" succeed, then the
    // connection drops.
    f.server.disconnect_after("BODY.PEEK[]", 2);
    let err = f.service.sync_account(&f.account).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Network);
    let totals = folder_totals(&f.service, &f.account).await;
    let stored_sent = totals.iter().find(|t| t.0 == "Sent").unwrap().2;
    assert_eq!(stored_sent, 2, "completed batches are kept");

    // The next pass fetches only what is missing.
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.new_messages, all - 2);
    assert_eq!(folder_totals(&f.service, &f.account).await, expected);
}

/// Regression: folders deleted on the server lost their message rows, but
/// their raw mail stayed in the blob store forever.
#[tokio::test]
async fn raw_mail_of_deleted_folders_is_removed() {
    let f = setup("secret").await;
    // A message only the "Receipts" folder has.
    assert!(f.server.store().add_mailbox("Receipts", FolderRole::Other));
    let raw = b"From: shop@example.org\r\nSubject: Receipt 42\r\n\r\nThanks!\r\n";
    f.server
        .store()
        .append::<&str>("Receipts", raw, &[])
        .unwrap();
    f.service.sync_account(&f.account).await.unwrap();
    let hash = nail_store::BlobHash::of(raw);
    assert!(f.service.blobs().contains(&hash));
    let blob_count =
        |dir: &std::path::Path| files_under(dir).into_iter().filter(|p| p.is_file()).count();
    let before = blob_count(f.service.blobs().root());

    assert!(f.server.store().delete_mailbox("Receipts"));
    let report = f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(report.removed_folders, 1);
    assert!(!f.service.blobs().contains(&hash), "the blob is collected");
    assert_eq!(blob_count(f.service.blobs().root()), before - 1);

    // Mail still referenced elsewhere stays readable after a UIDVALIDITY
    // reset (its blobs are dropped and stored again).
    f.server.store().reset_uid_validity("Sent").unwrap();
    f.service.sync_account(&f.account).await.unwrap();
    assert_eq!(blob_count(f.service.blobs().root()), before - 1);
}
