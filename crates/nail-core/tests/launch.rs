//! Launch requests (ADR 0041): the shell queues what Windows asked to open,
//! the UI takes each request once, and `.eml` files are imported into the
//! local "Opened files" account.

use std::time::Duration;

use nail_api::launch::{LaunchRequest, MailtoLink, OPENED_FILES_ACCOUNT_ID};
use nail_api::{ErrorCode, Event, Request, Response};
use nail_core::demo::demo_options;
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;

fn open() -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    (Service::open(demo_options(options)).unwrap(), data, config)
}

#[tokio::test]
async fn launch_requests_are_handed_out_once_and_announced() {
    let (service, _d, _c) = open();
    let mut events = service.subscribe();
    let compose = LaunchRequest::Compose {
        mailto: MailtoLink {
            to: vec!["ada@example.org".into()],
            ..MailtoLink::default()
        },
    };
    service.push_launch_requests(vec![compose.clone()]);
    service.push_launch_requests(Vec::new());
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event, Event::LaunchRequested);
    assert_eq!(
        service.handle(Request::TakeLaunchRequests).await,
        Response::LaunchRequests {
            requests: vec![compose]
        }
    );
    assert_eq!(
        service.handle(Request::TakeLaunchRequests).await,
        Response::LaunchRequests { requests: vec![] }
    );
}

#[tokio::test]
async fn an_opened_eml_file_is_imported_and_readable() {
    let (service, _d, _c) = open();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Offer.eml");
    std::fs::write(
        &path,
        "From: Shop <news@shop.example>\r\nTo: ada@example.org\r\nSubject: Autumn offer\r\n\
         Message-ID: <offer@shop.example>\r\nDate: Tue, 29 Sep 2026 10:00:00 +0000\r\n\r\nHello Ada\r\n",
    )
    .unwrap();
    let request = Request::ImportMessageFile {
        path: path.display().to_string(),
    };
    let summary = match service.handle(request.clone()).await {
        Response::MessageImported { message } => *message,
        other => panic!("{other:?}"),
    };
    assert_eq!(summary.subject, "Autumn offer");
    assert_eq!(summary.account_id.as_str(), OPENED_FILES_ACCOUNT_ID);
    match service
        .handle(Request::GetMessage {
            id: summary.id.clone(),
        })
        .await
    {
        Response::Message { message } => assert!(message.text.contains("Hello Ada")),
        other => panic!("{other:?}"),
    }
    // Opening it again finds the same message.
    match service.handle(request).await {
        Response::MessageImported { message } => assert_eq!(message.id, summary.id),
        other => panic!("{other:?}"),
    }
    match service.handle(Request::ListAccounts).await {
        Response::Accounts { accounts } => {
            assert!(
                accounts
                    .iter()
                    .any(|a| a.id.as_str() == OPENED_FILES_ACCOUNT_ID)
            );
        }
        other => panic!("{other:?}"),
    }

    // Missing and non-mail files are refused.
    let missing = service
        .handle(Request::ImportMessageFile {
            path: dir.path().join("gone.eml").display().to_string(),
        })
        .await;
    match &missing {
        Response::Error {
            code: ErrorCode::NotFound,
            localized: Some(localized),
            ..
        } => {
            assert_eq!(localized.id, "message-file-unreadable");
            assert!(localized.args.contains_key("path"));
        }
        other => panic!("{other:?}"),
    }
    let empty = dir.path().join("empty.eml");
    std::fs::write(&empty, "").unwrap();
    let refused = service
        .handle(Request::ImportMessageFile {
            path: empty.display().to_string(),
        })
        .await;
    match &refused {
        Response::Error {
            code: ErrorCode::InvalidRequest,
            localized: Some(localized),
            ..
        } => assert_eq!(localized.id, "message-file-size"),
        other => panic!("{other:?}"),
    }
    let text = dir.path().join("notes.eml");
    std::fs::write(&text, b"\r\n\r\n").unwrap();
    match service
        .handle(Request::ImportMessageFile {
            path: text.display().to_string(),
        })
        .await
    {
        Response::Error {
            code: ErrorCode::InvalidRequest,
            localized: Some(localized),
            ..
        } => assert_eq!(localized.id, "message-file-not-mail"),
        other => panic!("{other:?}"),
    }
}
