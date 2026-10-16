//! Folder management (ADR 0196) against the `demo-multi` mock servers:
//! create, rename, move and delete folders on IMAP ("work") and local
//! (POP3 "home") accounts; ids and messages stay on rename and move, paths
//! in rules follow; changes wait while offline; system folders, taken
//! names, Gmail and EWS accounts are refused.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{Multi, eventually, folder, folder_at, inbox_count, messages, ok, op_ids, request};
use nail_core::testing::error_of;

use nail_api::rules::{FolderRef, RuleAction, RuleTrigger};
use nail_api::{
    AccountId, ErrorCode, Event, Folder, FolderId, FolderRole, MoveTarget, OpState, Request,
    Response,
};
use nail_core::demo::{MULTI_EWS_ID, MULTI_GMAIL_ID, MULTI_HOME_ID, MULTI_WORK_ID};
use serde_json::json;

async fn setup() -> Multi {
    let f = Multi::start().await;
    eventually("both accounts synced", || async {
        inbox_count(&f.service, MULTI_WORK_ID).await == 15
            && inbox_count(&f.service, MULTI_HOME_ID).await == 3
    })
    .await;
    f
}

fn create(account: &str, parent: Option<&FolderId>, name: &str) -> Request {
    Request::CreateFolder {
        account_id: AccountId::new(account),
        parent_id: parent.cloned(),
        name: name.into(),
    }
}

/// The folder at `path` once the queued change reached the store.
async fn wait_for(f: &Multi, account: &str, path: &str) -> Folder {
    eventually(&format!("folder {path}"), || async {
        folder_at(&f.service, account, path).await.is_some()
    })
    .await;
    folder_at(&f.service, account, path).await.unwrap()
}

/// The mailbox names on the "work" mock server.
fn server_mailboxes(f: &Multi) -> Vec<String> {
    f.control("imap-mailboxes", &json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_owned())
        .collect()
}

/// Syncs the "work" account now.
async fn sync_work(f: &Multi) {
    ok(
        &f.service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_WORK_ID)),
        },
    )
    .await;
}

/// The first status in `state` of one of `ids`.
async fn op_status(
    events: &mut tokio::sync::broadcast::Receiver<Event>,
    ids: &[nail_api::OpId],
    state: OpState,
) -> nail_api::OpStatus {
    common::until_some("the op status", || {
        let mut seen = Vec::new();
        while let Ok(event) = events.try_recv() {
            seen.push(event);
        }
        async move {
            seen.into_iter().find_map(|e| match e {
                Event::OpStatus { status }
                    if ids.contains(&status.op_id) && status.state == state =>
                {
                    Some(status)
                }
                _ => None,
            })
        }
    })
    .await
}

/// The issue id of a refused request.
async fn refused(f: &Multi, request: Request) -> String {
    let (code, localized) = error_of(common::request(&f.service, request).await);
    assert_eq!(code, ErrorCode::InvalidRequest);
    localized.unwrap()
}

#[tokio::test]
async fn imap_folders_are_created_renamed_moved_and_deleted_keeping_their_mail() {
    let f = setup().await;
    let response = ok(&f.service, create(MULTI_WORK_ID, None, "Projects")).await;
    assert_eq!(op_ids(response).len(), 1);
    let projects = wait_for(&f, MULTI_WORK_ID, "Projects").await;
    assert_eq!(projects.role, FolderRole::Other);
    assert!(server_mailboxes(&f).contains(&"Projects".to_owned()));
    ok(
        &f.service,
        create(MULTI_WORK_ID, Some(&projects.id), "Acme"),
    )
    .await;
    let acme = wait_for(&f, MULTI_WORK_ID, "Projects/Acme").await;
    assert_eq!(acme.parent_id, Some(projects.id.clone()));

    // A message filed into it, and a rule moving mail there.
    let inbox = folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
        .await
        .unwrap();
    let message = messages(&f.service, &inbox.id).await[0].clone();
    ok(
        &f.service,
        Request::MoveMessages {
            message_ids: vec![message.id.clone()],
            target: MoveTarget::FolderId(acme.id.clone()),
        },
    )
    .await;
    eventually("the message is in Acme on the server", || async {
        f.server_subjects(MULTI_WORK_ID, "Projects/Acme").len() == 1
    })
    .await;
    let rule = common::subject_rule(
        "To Acme",
        vec![RuleTrigger::Manual],
        "acme",
        vec![RuleAction::Move {
            folder: FolderRef::Path {
                path: "Projects/Acme".into(),
                account_id: Some(AccountId::new(MULTI_WORK_ID)),
            },
        }],
    );
    ok(
        &f.service,
        Request::SaveRule {
            rule: Box::new(rule),
            position: None,
        },
    )
    .await;

    // Rename: same id, same message, the rule follows.
    ok(
        &f.service,
        Request::RenameFolder {
            folder_id: acme.id.clone(),
            name: "Acme Corp".into(),
        },
    )
    .await;
    let renamed = wait_for(&f, MULTI_WORK_ID, "Projects/Acme Corp").await;
    assert_eq!(renamed.id, acme.id);
    assert_eq!(renamed.name, "Acme Corp");
    assert!(server_mailboxes(&f).contains(&"Projects/Acme Corp".to_owned()));
    assert!(!server_mailboxes(&f).contains(&"Projects/Acme".to_owned()));
    let moved_mail = messages(&f.service, &renamed.id).await;
    assert_eq!(moved_mail.len(), 1);
    assert_eq!(moved_mail[0].subject, message.subject);

    // Move below Archive: the subtree goes along, ids stay.
    let archive = folder(&f.service, MULTI_WORK_ID, FolderRole::Archive)
        .await
        .unwrap();
    ok(
        &f.service,
        Request::MoveFolder {
            folder_id: projects.id.clone(),
            parent_id: Some(archive.id.clone()),
        },
    )
    .await;
    let child = wait_for(&f, MULTI_WORK_ID, "Archive/Projects/Acme Corp").await;
    assert_eq!(child.id, acme.id);
    assert_eq!(
        folder_at(&f.service, MULTI_WORK_ID, "Archive/Projects")
            .await
            .unwrap()
            .id,
        projects.id
    );
    let Response::Rules { rules } = request(&f.service, Request::ListRules).await else {
        panic!("rules expected")
    };
    assert!(
        matches!(&rules[0].actions[0], RuleAction::Move { folder: FolderRef::Path { path, .. } }
            if path == "Archive/Projects/Acme Corp"),
        "{:?}",
        rules[0].actions
    );

    // A sync keeps the ids and does not download the mail again.
    sync_work(&f).await;
    eventually("synced again", || async {
        messages(&f.service, &acme.id).await.len() == 1
    })
    .await;
    assert_eq!(messages(&f.service, &acme.id).await[0].id, moved_mail[0].id);

    // Delete: into Trash first, then for good.
    ok(
        &f.service,
        Request::DeleteFolder {
            folder_id: projects.id.clone(),
            permanent: false,
        },
    )
    .await;
    let trashed = wait_for(&f, MULTI_WORK_ID, "Trash/Projects").await;
    assert_eq!(trashed.id, projects.id);
    assert_eq!(
        refused(
            &f,
            Request::DeleteFolder {
                folder_id: projects.id.clone(),
                permanent: false,
            },
        )
        .await,
        "folder-issue-no-trash"
    );
    ok(
        &f.service,
        Request::DeleteFolder {
            folder_id: projects.id.clone(),
            permanent: true,
        },
    )
    .await;
    eventually("deleted for good", || async {
        folder_at(&f.service, MULTI_WORK_ID, "Trash/Projects")
            .await
            .is_none()
    })
    .await;
    assert!(
        folder_at(&f.service, MULTI_WORK_ID, "Trash/Projects/Acme Corp")
            .await
            .is_none()
    );
    assert!(
        !server_mailboxes(&f)
            .iter()
            .any(|m| m.starts_with("Trash/Projects"))
    );
}

#[tokio::test]
async fn changes_wait_while_offline_and_failures_are_reported() {
    let f = setup().await;
    let mut events = f.service.subscribe();
    f.control("imap-offline", &json!({ "offline": true }));
    let ids = op_ids(ok(&f.service, create(MULTI_WORK_ID, None, "Later")).await);
    // Nothing changes before the server confirmed.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        folder_at(&f.service, MULTI_WORK_ID, "Later")
            .await
            .is_none()
    );
    f.control("imap-offline", &json!({ "offline": false }));
    sync_work(&f).await;
    wait_for(&f, MULTI_WORK_ID, "Later").await;
    let done = op_status(&mut events, &ids, OpState::Done).await;
    assert_eq!(done.account_id.as_str(), MULTI_WORK_ID);

    // A name the server refuses (created there meanwhile) fails the op.
    f.control("imap-create-mailbox", &json!({ "mailbox": "Taken" }));
    let ids = op_ids(ok(&f.service, create(MULTI_WORK_ID, None, "Taken")).await);
    let failed = op_status(&mut events, &ids, OpState::Failed).await;
    assert_eq!(failed.error, Some(ErrorCode::Conflict));
    assert_eq!(
        failed.localized.map(|l| l.id).as_deref(),
        Some("folder-issue-name-taken")
    );
}

#[tokio::test]
async fn local_folders_change_at_once() {
    let f = setup().await;
    ok(&f.service, create(MULTI_HOME_ID, None, "Receipts")).await;
    let receipts = wait_for(&f, MULTI_HOME_ID, "Receipts").await;
    ok(
        &f.service,
        create(MULTI_HOME_ID, Some(&receipts.id), "2026"),
    )
    .await;
    let year = wait_for(&f, MULTI_HOME_ID, "Receipts/2026").await;
    ok(
        &f.service,
        Request::RenameFolder {
            folder_id: receipts.id.clone(),
            name: "Bills".into(),
        },
    )
    .await;
    let moved = wait_for(&f, MULTI_HOME_ID, "Bills/2026").await;
    assert_eq!(moved.id, year.id);
}

#[tokio::test]
async fn system_folders_taken_names_and_other_accounts_are_refused() {
    let f = setup().await;
    let inbox = folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
        .await
        .unwrap();
    let rename_inbox = Request::RenameFolder {
        folder_id: inbox.id.clone(),
        name: "Post".into(),
    };
    assert_eq!(
        refused(&f, rename_inbox).await,
        "folder-issue-system-folder"
    );
    assert_eq!(
        refused(&f, create(MULTI_WORK_ID, None, "  ")).await,
        "folder-issue-missing-name"
    );
    assert_eq!(
        refused(&f, create(MULTI_WORK_ID, None, "a/b")).await,
        "folder-issue-invalid-name"
    );
    assert_eq!(
        refused(&f, create(MULTI_WORK_ID, None, "archive")).await,
        "folder-issue-name-taken"
    );
    let home_inbox = folder(&f.service, MULTI_HOME_ID, FolderRole::Inbox)
        .await
        .unwrap();
    assert_eq!(
        refused(&f, create(MULTI_WORK_ID, Some(&home_inbox.id), "x")).await,
        "folder-issue-other-account"
    );
    assert_eq!(
        refused(&f, create(MULTI_GMAIL_ID, None, "x")).await,
        "folder-issue-gmail-labels"
    );
    assert_eq!(
        refused(&f, create(MULTI_EWS_ID, None, "x")).await,
        "folder-issue-unsupported"
    );
    // Moving a folder into its own subfolder.
    ok(&f.service, create(MULTI_WORK_ID, None, "Top")).await;
    let top = wait_for(&f, MULTI_WORK_ID, "Top").await;
    ok(&f.service, create(MULTI_WORK_ID, Some(&top.id), "Sub")).await;
    let sub = wait_for(&f, MULTI_WORK_ID, "Top/Sub").await;
    assert_eq!(
        refused(
            &f,
            Request::MoveFolder {
                folder_id: top.id.clone(),
                parent_id: Some(sub.id.clone()),
            },
        )
        .await,
        "folder-issue-into-itself"
    );
    // The same name and place: nothing to do.
    let same = Request::RenameFolder {
        folder_id: top.id.clone(),
        name: "Top".into(),
    };
    assert_eq!(op_ids(ok(&f.service, same).await), []);
}

#[tokio::test]
async fn a_new_folder_at_a_renamed_folders_old_path_is_a_folder_of_its_own() {
    let f = setup().await;
    ok(&f.service, create(MULTI_WORK_ID, None, "Old")).await;
    let old = wait_for(&f, MULTI_WORK_ID, "Old").await;
    let raw =
        "From: a@example.org\r\nSubject: First\r\nMessage-ID: <first@example.org>\r\n\r\nx\r\n";
    f.control("imap-deliver", &json!({ "mailbox": "Old", "raw": raw }));
    sync_work(&f).await;
    eventually("the first message", || async {
        messages(&f.service, &old.id).await.len() == 1
    })
    .await;
    ok(
        &f.service,
        Request::RenameFolder {
            folder_id: old.id.clone(),
            name: "New".into(),
        },
    )
    .await;
    wait_for(&f, MULTI_WORK_ID, "New").await;
    // Another mail program creates "Old" again and files mail in it.
    f.control("imap-create-mailbox", &json!({ "mailbox": "Old" }));
    let raw =
        "From: b@example.org\r\nSubject: Second\r\nMessage-ID: <second@example.org>\r\n\r\ny\r\n";
    f.control("imap-deliver", &json!({ "mailbox": "Old", "raw": raw }));
    sync_work(&f).await;
    let again = wait_for(&f, MULTI_WORK_ID, "Old").await;
    assert_ne!(again.id, old.id);
    eventually("the second message", || async {
        messages(&f.service, &again.id).await.len() == 1
    })
    .await;
    let first = messages(&f.service, &old.id).await;
    let second = messages(&f.service, &again.id).await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].subject, "First");
    assert_eq!(second[0].subject, "Second");
    assert_ne!(first[0].id, second[0].id);
}
