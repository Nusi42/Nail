//! `Service::handle` for every request, against a store seeded with the demo
//! corpus.

use std::time::Duration;

use nail_api::{
    AccountId, ErrorCode, Event, FolderId, FolderRole, MessageId, PROTOCOL_VERSION, Request,
    Response,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, seed_demo};
use nail_core::{MAX_PAGE_SIZE, Service, ServiceOptions};
use nail_testkit::demo_corpus;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::broadcast;

/// A theme with a syntax error (unclosed block) in line 2.
const BROKEN_THEME: &str = "theme \"Broken\" extends=\"default\" {\n    tokens {\n";

struct Fixture {
    service: Service,
    config_dir: TempDir,
    _data_dir: TempDir,
}

fn open(watch_config: bool, locale: Option<&str>) -> Fixture {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.watch_config = watch_config;
    options.locale = locale.map(str::to_owned);
    let service = Service::open(options).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        config_dir,
        _data_dir: data_dir,
    }
}

fn demo() -> AccountId {
    AccountId::new(DEMO_ACCOUNT_ID)
}

async fn folders(service: &Service) -> Vec<nail_api::Folder> {
    match service
        .handle(Request::ListFolders { account_id: demo() })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

async fn inbox(service: &Service) -> FolderId {
    folders(service)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap()
        .id
}

async fn page(service: &Service, folder_id: FolderId, offset: u32, limit: u32) -> Response {
    service
        .handle(Request::ListMessages {
            folder_id,
            offset,
            limit,
        })
        .await
}

fn error_code(response: Response) -> ErrorCode {
    match response {
        Response::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("no event within 10 s")
        .unwrap()
}

#[tokio::test]
async fn ping_reports_versions() {
    let f = open(false, None);
    assert_eq!(
        f.service.handle(Request::Ping).await,
        Response::Pong {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol: PROTOCOL_VERSION,
        }
    );
}

#[tokio::test]
async fn get_config_returns_defaults_and_locale() {
    let f = open(false, Some("de"));
    match f.service.handle(Request::GetConfig).await {
        Response::Config {
            theme,
            layout,
            locale,
        } => {
            assert_eq!(*theme, nail_config::default_theme());
            assert_eq!(*layout, nail_config::default_layout());
            assert_eq!(locale.as_deref(), Some("de"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(f.service.config_error_events().is_empty());
}

#[tokio::test]
async fn lists_accounts_and_folders_with_counts() {
    let f = open(false, None);
    match f.service.handle(Request::ListAccounts).await {
        Response::Accounts { accounts } => {
            assert_eq!(accounts.len(), 1);
            assert_eq!(accounts[0].id, demo());
        }
        other => panic!("unexpected {other:?}"),
    }

    let folders = folders(&f.service).await;
    let corpus = demo_corpus();
    let summary: Vec<_> = folders
        .iter()
        .map(|f| (f.path.clone(), f.role, f.total as usize))
        .collect();
    let mut expected: Vec<_> = corpus
        .iter()
        .map(|c| (c.name.clone(), c.role, c.messages.len()))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    let mut summary_sorted = summary.clone();
    summary_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(summary_sorted, expected);

    let inbox = folders
        .iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap();
    let corpus_inbox = corpus.iter().find(|c| c.name == "INBOX").unwrap();
    let unread = corpus_inbox
        .messages
        .iter()
        .filter(|m| !m.api_flags().seen)
        .count();
    assert_eq!(inbox.unread as usize, unread);
    assert!(unread > 0);

    assert_eq!(
        error_code(
            f.service
                .handle(Request::ListFolders {
                    account_id: AccountId::new("nope")
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn lists_messages_in_pages_newest_first() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let Response::Messages {
        folder_id,
        offset,
        total,
        messages: first,
    } = page(&f.service, inbox.clone(), 0, 5).await
    else {
        panic!("expected messages");
    };
    assert_eq!((folder_id, offset, first.len()), (inbox.clone(), 0, 5));
    assert!(total > 5);

    let Response::Messages {
        messages: rest,
        offset,
        ..
    } = page(&f.service, inbox.clone(), 5, 1000).await
    else {
        panic!("expected messages");
    };
    assert_eq!(offset, 5);
    assert_eq!(first.len() + rest.len(), total as usize);
    let ids: std::collections::HashSet<_> = first.iter().chain(&rest).map(|m| &m.id).collect();
    assert_eq!(ids.len(), total as usize);
    // Newest first; a message without a valid date sorts last.
    assert!(first[0].subject.starts_with("Reminder: Nightingale review"));
    assert!(rest.last().unwrap().subject.starts_with("Broken message"));

    let Response::Messages { messages, .. } = page(&f.service, inbox.clone(), 0, 0).await else {
        panic!("expected messages");
    };
    assert!(messages.is_empty());
    let Response::Messages { messages, .. } =
        page(&f.service, inbox.clone(), 0, MAX_PAGE_SIZE + 1).await
    else {
        panic!("expected messages");
    };
    assert_eq!(messages.len(), total as usize);

    assert_eq!(
        error_code(page(&f.service, FolderId::new("nope"), 0, 10).await),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn get_message_parses_the_stored_blob() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let Response::Messages { messages, .. } = page(&f.service, inbox, 0, 100).await else {
        panic!("expected messages");
    };
    let newsletter = messages
        .iter()
        .find(|m| m.subject.contains("garden digest"))
        .expect("newsletter in the inbox");

    let Response::Message { message } = f
        .service
        .handle(Request::GetMessage {
            id: newsletter.id.clone(),
        })
        .await
    else {
        panic!("expected a message");
    };
    assert_eq!(message.id, newsletter.id);
    assert_eq!(message.headers.subject, newsletter.subject);
    assert!(message.remote_content_blocked);
    let html = message.sanitized_html.expect("html body");
    assert!(html.contains("Autumn is here"));
    assert!(!html.contains("track.garden-weekly.example"));
    assert!(!html.contains("<script"));

    assert_eq!(
        error_code(
            f.service
                .handle(Request::GetMessage {
                    id: MessageId::new("nope")
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn blobs_are_served_by_hash() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let (first_uid, stored) = {
        let uids = f.service.store().folder_uids(&inbox).unwrap();
        let (uid, id) = uids[0].clone();
        (uid, f.service.store().get_message(&id).unwrap())
    };
    assert_eq!(first_uid, 1);
    let hash = stored.blob_hash.unwrap();
    let raw = f.service.blob(hash.as_str()).unwrap();
    assert!(raw.starts_with(b"From:") || raw.windows(5).any(|w| w == b"From:"));

    let err = nail_api::ApiError::from(f.service.blob("not-a-hash").unwrap_err());
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    let missing = "0".repeat(64);
    let err = nail_api::ApiError::from(f.service.blob(&missing).unwrap_err());
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn sync_now_on_a_local_account_finishes_at_once() {
    let f = open(false, None);
    let mut events = f.service.subscribe();
    assert_eq!(
        f.service
            .handle(Request::SyncNow {
                account_id: Some(demo())
            })
            .await,
        Response::SyncStarted
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::SyncProgress {
            account_id: demo(),
            done: 1,
            total: 1
        }
    );

    // All accounts: nothing is syncable, nothing happens.
    assert_eq!(
        f.service
            .handle(Request::SyncNow { account_id: None })
            .await,
        Response::SyncStarted
    );
    assert_eq!(
        error_code(
            f.service
                .handle(Request::SyncNow {
                    account_id: Some(AccountId::new("nope"))
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn invalid_config_at_start_falls_back_and_is_reported() {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    std::fs::write(config_dir.path().join("theme.kdl"), BROKEN_THEME).unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.watch_config = false;
    let service = Service::open(options).unwrap();

    let errors = service.config_error_events();
    assert_eq!(errors.len(), 1);
    assert!(matches!(&errors[0], Event::ConfigError { file, .. } if file == "theme.kdl"));
    let Response::Config { theme, .. } = service.handle(Request::GetConfig).await else {
        panic!("expected config");
    };
    assert_eq!(*theme, nail_config::default_theme());
}

#[tokio::test]
async fn config_changes_are_broadcast() {
    let f = open(true, None);
    let mut events = f.service.subscribe();
    let path = f.config_dir.path().join("theme.kdl");

    std::fs::write(
        &path,
        "theme \"Test\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#ff0066\"\n    }\n}\n",
    )
    .unwrap();
    match next_event(&mut events).await {
        Event::ConfigChanged { theme, .. } => {
            let dump = format!("{theme:?}");
            assert!(dump.contains("#ff0066"), "{dump}");
        }
        other => panic!("unexpected {other:?}"),
    }

    std::fs::write(&path, BROKEN_THEME).unwrap();
    match next_event(&mut events).await {
        Event::ConfigError { file, line, .. } => {
            assert_eq!(file, "theme.kdl");
            assert!(line >= 1);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn m1a_requests_are_unsupported_until_wired() {
    let fx = open(false, None);
    for request in [
        Request::GetAccountsConfig,
        Request::ListOutbox,
        Request::DiscoverAccount {
            email: "ada@example.org".into(),
        },
    ] {
        match fx.service.handle(request).await {
            Response::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unsupported);
                assert!(message.contains("not implemented"), "{message}");
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }
    let response = fx
        .service
        .handle(Request::DiscoverAccount {
            email: "ada@example.org".into(),
        })
        .await;
    let Response::Error { message, .. } = response else {
        panic!("expected an error");
    };
    assert!(message.starts_with("DiscoverAccount "), "{message}");
}
