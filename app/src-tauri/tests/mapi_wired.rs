//! M4b integration (WP115, ADR 0092): the DLL's side of Simple MAPI
//! (`nail_mapi::forward` with the real `PipeTransport`: private copies of
//! the files, the same-user check of the pipe server, frames) against the
//! shell's pipe server (`MapiServer`, first instance, current-user DACL)
//! and the real core (`Service::submit_mapi`): the program's message
//! becomes a saved draft with its files and a `ComposeDraft` launch
//! request; nothing is sent.

#![cfg(windows)]

use std::sync::Arc;
use std::time::Duration;

use nail_api::launch::LaunchRequest;
use nail_api::mapi::{
    MapiAttachment, MapiMessage, MapiRecipient, MapiRecipientClass, MapiResult, MapiUi,
};
use nail_api::{Request, Response};
use nail_app_lib::mapi_server::{MapiHandler, MapiServer};
use nail_app_lib::{AppArgs, open_service};
use nail_core::Service;
use nail_mapi::windows::client::PipeTransport;

const ACCOUNTS: &str = r#"account "work" email="ada@example.org" name="Work" enabled=#false {
    imap host="127.0.0.1" port=1 tls="plaintext"
    smtp host="127.0.0.1" port=1 tls="plaintext"
    identity "ada@example.org" name="Ada Lovelace"
}
"#;

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
}

fn service(with_identity: bool) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    if with_identity {
        std::fs::write(config.path().join("accounts.kdl"), ACCOUNTS).unwrap();
    }
    let args = AppArgs {
        data_dir: Some(data.path().into()),
        config_dir: Some(config.path().into()),
        seed: Some("demo".into()),
        ..AppArgs::default()
    };
    Fixture {
        service: open_service(&args).unwrap(),
        _data: data,
        _config: config,
    }
}

fn pipe_name(test: &str) -> String {
    format!(r"\\.\pipe\nail-mapi-test-{test}-{}", std::process::id())
}

fn start_server(service: &Service, name: &str) -> MapiServer {
    let service = service.clone();
    let handler: MapiHandler = Arc::new(move |message| {
        let service = service.clone();
        Box::pin(async move { service.submit_mapi(message).await })
    });
    let sid = nail_app_lib::win::current_user_sid().unwrap();
    MapiServer::start(
        name,
        &nail_app_lib::win::pipe_sddl(&sid),
        handler,
        &tokio::runtime::Handle::current(),
    )
    .unwrap()
}

fn transport(name: &str) -> PipeTransport {
    PipeTransport {
        pipe: name.to_owned(),
        // Never start an app from a test.
        app: None,
        connect_timeout: Duration::from_secs(5),
        response_timeout: Duration::from_secs(60),
    }
}

/// What the DLL does for one `MAPISendMailW` call, on a blocking thread.
async fn send_like_the_dll(
    name: &str,
    message: MapiMessage,
    staging: &std::path::Path,
) -> MapiResult {
    let transport = transport(name);
    let staging = staging.to_path_buf();
    tokio::task::spawn_blocking(move || nail_mapi::forward::forward(message, &transport, &staging))
        .await
        .unwrap()
}

fn message(file: &std::path::Path) -> MapiMessage {
    MapiMessage {
        subject: Some("Quarterly report".into()),
        body: Some("Here are the figures.".into()),
        recipients: vec![
            MapiRecipient {
                class: MapiRecipientClass::To,
                name: Some("Grace".into()),
                address: Some("SMTP:grace@navy.example".into()),
            },
            MapiRecipient {
                class: MapiRecipientClass::Cc,
                name: None,
                address: Some("<alan@example.org>".into()),
            },
        ],
        attachments: vec![MapiAttachment {
            path: file.to_string_lossy().into_owned(),
            file_name: Some("figures.csv".into()),
        }],
        // A program that asks for no window: Nail opens the composer anyway.
        ui: MapiUi::None,
        program: Some("EXCEL.EXE".into()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_programs_mail_becomes_a_draft_with_its_files_and_opens_the_composer() {
    let fixture = service(true);
    let service = &fixture.service;
    let name = pipe_name("draft");
    let _server = start_server(service, &name);
    let files = tempfile::tempdir().unwrap();
    let file = files.path().join("report-temp.csv");
    std::fs::write(&file, b"quarter,revenue\nQ3,42\n").unwrap();
    let staging = tempfile::tempdir().unwrap();

    let result = send_like_the_dll(&name, message(&file), staging.path()).await;
    assert_eq!(result, MapiResult::Success);
    // The caller's file is untouched; the DLL's private copy is gone.
    assert!(file.exists());
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);

    let Response::LaunchRequests { requests } = service.handle(Request::TakeLaunchRequests).await
    else {
        panic!("expected launch requests");
    };
    let [LaunchRequest::ComposeDraft { draft_id, program }] = requests.as_slice() else {
        panic!("expected one ComposeDraft: {requests:?}");
    };
    assert_eq!(program.as_deref(), Some("EXCEL.EXE"));
    let Response::Draft { draft } = service
        .handle(Request::GetDraft {
            draft_id: draft_id.clone(),
        })
        .await
    else {
        panic!("expected the draft");
    };
    assert_eq!(draft.subject, "Quarterly report");
    assert_eq!(draft.to[0].email, "grace@navy.example");
    assert_eq!(draft.cc[0].email, "alan@example.org");
    assert!(
        draft.text.starts_with("Here are the figures."),
        "{}",
        draft.text
    );
    assert_eq!(draft.attachments.len(), 1);
    assert_eq!(draft.attachments[0].filename, "figures.csv");
    assert_eq!(draft.attachments[0].size, 22);
    // Nothing was queued for sending.
    let Response::Outbox { entries } = service.handle(Request::ListOutbox).await else {
        panic!("expected the outbox");
    };
    assert!(entries.is_empty(), "{entries:?}");

    // The pipe serves the next program too (sequential clients).
    let again = send_like_the_dll(&name, message(&file), staging.path()).await;
    assert_eq!(again, MapiResult::Success);
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_core_s_answers_reach_the_program() {
    // No identity yet: the program is told to log on (set up an account).
    let fixture = service(false);
    let name = pipe_name("login");
    let _server = start_server(&fixture.service, &name);
    let files = tempfile::tempdir().unwrap();
    let file = files.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let staging = tempfile::tempdir().unwrap();
    assert_eq!(
        send_like_the_dll(&name, message(&file), staging.path()).await,
        MapiResult::LoginFailure
    );
    fixture.service.shutdown().await;

    // A file that disappears before the DLL copies it never reaches the app.
    let fixture = service(true);
    let name = pipe_name("missing");
    let _server = start_server(&fixture.service, &name);
    let gone = files.path().join("gone.txt");
    assert_eq!(
        send_like_the_dll(&name, message(&gone), staging.path()).await,
        MapiResult::AttachmentNotFound
    );
    let Response::LaunchRequests { requests } =
        fixture.service.handle(Request::TakeLaunchRequests).await
    else {
        panic!("expected launch requests");
    };
    assert!(requests.is_empty());
    fixture.service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_listening_fails_fast_without_starting_an_app() {
    let staging = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let file = files.path().join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let started = std::time::Instant::now();
    let result = send_like_the_dll(&pipe_name("nobody"), message(&file), staging.path()).await;
    assert_eq!(result, MapiResult::Failure);
    assert!(started.elapsed() < Duration::from_secs(10));
}
