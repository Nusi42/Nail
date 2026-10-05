//! Mail actions on conversations (P3, ADR 0164). The contract's stub
//! resolves conversation targets per action and forwards to the
//! per-message writes; WP-actions-core extends this file (one batch, undo,
//! every stored copy, engines, remote narrowing).

#![cfg(test)]

use nail_api::mail_actions::MailActionKind;
use nail_api::{
    AccountId, ErrorCode, Folder, FolderRole, MailAction, MessageTarget, Request, Response,
    ThreadSummary,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, demo_options, seed_demo};
use nail_core::{Service, ServiceOptions};

const SUBJECT: &str = "Project Nightingale kickoff";

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
}

fn setup() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(demo_options(options)).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        _data: data,
        _config: config,
    }
}

fn folder(f: &Fixture, role: FolderRole) -> Folder {
    f.service
        .store()
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
        .into_iter()
        .find(|folder| folder.role == role)
        .unwrap()
}

async fn nightingale(f: &Fixture, folder: &Folder) -> ThreadSummary {
    match f
        .service
        .handle(Request::ListThreads {
            folder_id: folder.id.clone(),
            offset: 0,
            limit: 100,
        })
        .await
    {
        Response::Threads { threads, .. } => {
            threads.into_iter().find(|t| t.subject == SUBJECT).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn reach_picks_the_listed_messages_or_the_whole_conversation() {
    let f = setup();
    let inbox = folder(&f, FolderRole::Inbox);
    let row = nightingale(&f, &inbox).await;
    let target = vec![MessageTarget::conversation(
        row.id.clone(),
        Some(inbox.id.clone()),
    )];
    let resolve = |action| Request::ResolveMailTargets {
        targets: target.clone(),
        action,
    };
    let count = |response: Response| match response {
        Response::MailTargets { resolution } => {
            assert_eq!(resolution.conversations, 1);
            resolution.message_ids.len()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(
        count(f.service.handle(resolve(MailAction::Archive)).await),
        4,
        "archive: the four in the Inbox, not the replies in Sent"
    );
    assert_eq!(
        count(
            f.service
                .handle(resolve(MailAction::MarkRead { read: false }))
                .await
        ),
        7,
        "read state: every message the row counts"
    );
    match f
        .service
        .handle(resolve(MailAction::Flag { flagged: true }))
        .await
    {
        Response::MailTargets { resolution } => {
            assert_eq!(resolution.message_ids, vec![row.latest_message_id.clone()]);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn archiving_a_conversation_leaves_the_users_replies_in_sent() {
    let f = setup();
    let inbox = folder(&f, FolderRole::Inbox);
    let sent = folder(&f, FolderRole::Sent);
    let row = nightingale(&f, &inbox).await;
    let result = match f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![MessageTarget::conversation(
                row.id.clone(),
                Some(inbox.id.clone()),
            )],
            action: MailAction::Archive,
        })
        .await
    {
        Response::MailActionQueued { result } => result,
        other => panic!("{other:?}"),
    };
    assert_eq!(result.kind, MailActionKind::Archive);
    assert_eq!(result.message_ids.len(), 4);
    assert_eq!(result.conversations, 1);
    let store = f.service.store();
    for id in &result.message_ids {
        let moved = store.get_message(id).unwrap().summary;
        assert_ne!(moved.folder_id, inbox.id);
    }
    let in_sent = store
        .list_messages(&sent.id, 0, 1000)
        .unwrap()
        .messages
        .into_iter()
        .filter(|m| m.thread_id.as_ref() == Some(&row.id))
        .count();
    assert_eq!(in_sent, 3, "the replies stay in Sent");
}

#[tokio::test]
async fn delete_permanently_only_from_trash_or_junk() {
    let f = setup();
    let inbox = folder(&f, FolderRole::Inbox);
    let row = nightingale(&f, &inbox).await;
    match f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![MessageTarget::conversation(row.id, Some(inbox.id))],
            action: MailAction::DeletePermanently,
        })
        .await
    {
        Response::Error {
            code, localized, ..
        } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert_eq!(
                localized.map(|l| l.id),
                Some("mail-action-issue-not-in-trash".to_owned())
            );
        }
        other => panic!("{other:?}"),
    }
    match f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![],
            action: MailAction::Archive,
        })
        .await
    {
        Response::Error { localized, .. } => assert_eq!(
            localized.map(|l| l.id),
            Some("mail-action-issue-no-targets".to_owned())
        ),
        other => panic!("{other:?}"),
    }
}
