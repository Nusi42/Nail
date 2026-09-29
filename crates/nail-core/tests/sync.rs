//! The sync engine against the in-process mock IMAP server.

use std::time::Duration;

use nail_api::{Account, AccountId, AccountKind, ErrorCode, Event, FolderRole, Request, Response};
use nail_core::{ImapAccount, Service, ServiceOptions};
use nail_proto_imap::{Credentials, ImapConfig, Security};
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
    let server = MockImapServer::start(MockImapConfig::default(), MailStore::demo())
        .await
        .unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.watch_config = false;
    let service = Service::open(options).unwrap();
    let account = Account {
        id: AccountId::new("imap-1"),
        name: "Mock".into(),
        email: server.config().username.clone(),
        kind: AccountKind::Imap,
    };
    let settings = ImapAccount {
        config: ImapConfig::new(server.host())
            .with_port(server.port())
            .with_security(Security::InsecurePlaintext)
            .with_timeout(TIMEOUT),
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
        .map(|f| (f.name, f.role, f.messages.len() as u32))
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
    assert_eq!(report.folders, expected.len() as u32);
    assert_eq!(report.new_messages, messages);
    assert_eq!(folder_totals(&f.service, &f.account).await, expected);

    let events = drain(&mut events);
    let total = expected.len() as u32 + 1;
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
    assert!(!changed.is_empty());
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
        .append("INBOX", raw.as_bytes(), &[] as &[&str])
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
    assert!(folder_totals(&f.service, &f.account).await.is_empty());
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
    let total = expected.len() as u32 + 1;
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
